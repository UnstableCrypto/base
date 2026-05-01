use std::{fmt, net::SocketAddr, sync::Arc, time::Duration};

use base_health::{HealthzApiServer, HealthzRpc};
use base_proof_host::{ProverConfig, ProverRpc, ProverRpcError, ProverService};
use base_proof_primitives::{EnclaveApiServer, ProverApiServer};
use base_proof_tee_tdx_runtime::{TdxQuoteProvider, TdxRuntime};
use jsonrpsee::{
    RpcModule,
    core::{RpcResult, async_trait},
    server::{Server, ServerHandle, middleware::http::ProxyGetRequestLayer},
};
use tracing::info;

use crate::{TdxBackend, TdxSignerAttestation};

/// JSON-RPC attestation kind returned by TDX prover servers.
pub const TDX_ATTESTATION_KIND: &str = "tdx";

/// Host-side TDX prover server exposing the shared JSON-RPC interface.
pub struct TdxProverServer<P> {
    runtime: Arc<TdxRuntime<P>>,
    service: ProverService<TdxBackend<P>>,
    proof_request_timeout: Duration,
}

impl<P> TdxProverServer<P>
where
    P: TdxQuoteProvider + fmt::Debug + 'static,
{
    /// Create a server with the given prover config, TDX runtime, and proof timeout.
    pub fn new(
        config: ProverConfig,
        runtime: Arc<TdxRuntime<P>>,
        proof_request_timeout: Duration,
    ) -> Self {
        let backend = TdxBackend::new(Arc::clone(&runtime));
        Self { runtime, service: ProverService::new(config, backend), proof_request_timeout }
    }

    /// Start the JSON-RPC HTTP server on the given address.
    pub async fn run(self, addr: SocketAddr) -> eyre::Result<ServerHandle> {
        let middleware = tower::ServiceBuilder::new()
            .layer(ProxyGetRequestLayer::new([("/healthz", "healthz")])?);
        let server = Server::builder().set_http_middleware(middleware).build(addr).await?;
        let addr = server.local_addr()?;
        info!(addr = %addr, "tdx rpc server started");

        Ok(server.start(self.into_rpc_module()?))
    }

    /// Build the JSON-RPC module served by this TDX prover.
    pub fn into_rpc_module(self) -> eyre::Result<RpcModule<()>> {
        let mut module = RpcModule::new(());
        module.merge(HealthzRpc::new(env!("CARGO_PKG_VERSION")).into_rpc())?;
        module.merge(ProverRpc::new(self.service, self.proof_request_timeout).into_rpc())?;
        module.merge(TdxSignerRpc { runtime: self.runtime }.into_rpc())?;

        Ok(module)
    }
}

impl<P> fmt::Debug for TdxProverServer<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TdxProverServer").finish_non_exhaustive()
    }
}

/// Inner RPC handler for `enclave_*` methods.
pub struct TdxSignerRpc<P> {
    /// TDX runtime used for signer and quote collection calls.
    pub runtime: Arc<TdxRuntime<P>>,
}

impl<P> fmt::Debug for TdxSignerRpc<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TdxSignerRpc").finish_non_exhaustive()
    }
}

#[async_trait]
impl<P> EnclaveApiServer for TdxSignerRpc<P>
where
    P: TdxQuoteProvider + fmt::Debug + 'static,
{
    async fn signer_public_key(&self) -> RpcResult<Vec<Vec<u8>>> {
        Ok(vec![self.runtime.signer_public_key().to_vec()])
    }

    async fn signer_attestation(
        &self,
        user_data: Option<Vec<u8>>,
        nonce: Option<Vec<u8>>,
    ) -> RpcResult<Vec<Vec<u8>>> {
        if user_data.is_some() {
            return Err(ProverRpcError::rpc_err(
                -32602,
                "TDX signer attestations do not support user_data challenge binding",
            ));
        }
        if nonce.is_some() {
            return Err(ProverRpcError::rpc_err(
                -32602,
                "TDX signer attestations do not support nonce challenge binding",
            ));
        }

        let signer_public_key = self.runtime.signer_public_key();
        let quote =
            self.runtime.signer_quote().map_err(|error| ProverRpcError::rpc_err(-32001, error))?;
        let attestation = TdxSignerAttestation::new(
            signer_public_key.to_vec().into(),
            quote.quote,
            quote.quote_timestamp_millis,
        )
        .encode();
        Ok(vec![attestation])
    }

    async fn attestation_kind(&self) -> RpcResult<String> {
        Ok(TDX_ATTESTATION_KIND.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use base_proof_host::{ProverRequestHandler, ProverRpcError};
    use base_proof_primitives::{EnclaveApiServer, ProofRequest, ProofResult, ProverApiServer};
    use base_proof_tee_tdx_runtime::TdxSigner;
    use jsonrpsee::{core::client::ClientT, http_client::HttpClientBuilder, rpc_params};

    use super::*;
    use crate::MeasuredMockTdxQuoteProvider;

    const TEST_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    struct FailingProverHandler;

    #[async_trait]
    impl ProverRequestHandler for FailingProverHandler {
        async fn prove_block(&self, _request: ProofRequest) -> Result<ProofResult, ProverRpcError> {
            Err(ProverRpcError::new(-32042, "mock proof failure"))
        }
    }

    fn test_rpc() -> TdxSignerRpc<MeasuredMockTdxQuoteProvider> {
        let signer = TdxSigner::from_hex(TEST_KEY).unwrap();
        let runtime = TdxRuntime::new(signer, MeasuredMockTdxQuoteProvider::local_mock());
        TdxSignerRpc { runtime: Arc::new(runtime) }
    }

    #[tokio::test]
    async fn signer_public_key_serves_tdx_signer_key() {
        let rpc = test_rpc();
        let result = EnclaveApiServer::signer_public_key(&rpc).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 65);
        assert_eq!(result[0][0], 0x04);
    }

    #[tokio::test]
    async fn signer_attestation_serves_self_contained_tdx_payload() {
        let rpc = test_rpc();
        let result = EnclaveApiServer::signer_attestation(&rpc, None, None).await.unwrap();

        assert_eq!(result.len(), 1);
        let attestation = TdxSignerAttestation::decode(&result[0]).unwrap();
        let quote = base_proof_tee_tdx_verifier::TdxQuote::parse(&attestation.quote).unwrap();
        assert_eq!(attestation.signer_public_key, rpc.runtime.signer_public_key().to_vec());
        assert_eq!(
            quote.report_data_suffix(),
            base_proof_tee_tdx_verifier::TdxVerifier::timestamp_report_data_suffix(
                attestation.quote_timestamp_millis
            )
        );
    }

    #[tokio::test]
    async fn signer_attestation_rejects_user_data() {
        let rpc = test_rpc();
        let err = EnclaveApiServer::signer_attestation(&rpc, Some(vec![1, 2, 3]), None)
            .await
            .unwrap_err();

        assert_eq!(err.code(), -32602);
        assert!(err.message().contains("user_data"));
    }

    #[tokio::test]
    async fn signer_attestation_rejects_nonce() {
        let rpc = test_rpc();
        let err = EnclaveApiServer::signer_attestation(&rpc, None, Some(vec![1, 2, 3]))
            .await
            .unwrap_err();

        assert_eq!(err.code(), -32602);
        assert!(err.message().contains("nonce"));
    }

    #[tokio::test]
    async fn attestation_kind_serves_tdx() {
        let rpc = test_rpc();
        let result = EnclaveApiServer::attestation_kind(&rpc).await.unwrap();

        assert_eq!(result, TDX_ATTESTATION_KIND);
    }

    #[tokio::test]
    async fn mock_prover_rpc_serves_prove_method() {
        let rpc = ProverRpc::new(FailingProverHandler, Duration::from_secs(1));

        let result = ProverApiServer::prove(&rpc, ProofRequest::default()).await;

        let err = result.unwrap_err();
        assert_eq!(err.code(), -32042);
        assert!(err.message().contains("mock proof failure"));
    }

    #[tokio::test]
    async fn local_mock_server_serves_json_rpc_methods() {
        let signer_rpc = test_rpc();
        let mut module = RpcModule::new(());
        module.merge(HealthzRpc::new(env!("CARGO_PKG_VERSION")).into_rpc()).unwrap();
        module
            .merge(ProverRpc::new(FailingProverHandler, Duration::from_secs(1)).into_rpc())
            .unwrap();
        module.merge(signer_rpc.into_rpc()).unwrap();
        let server =
            Server::builder().build("127.0.0.1:0".parse::<SocketAddr>().unwrap()).await.unwrap();
        let addr = server.local_addr().unwrap();
        let handle = server.start(module);
        let client = HttpClientBuilder::default().build(format!("http://{addr}")).unwrap();

        let kind: String = client.request("enclave_attestationKind", rpc_params![]).await.unwrap();
        let public_keys: Vec<Vec<u8>> =
            client.request("enclave_signerPublicKey", rpc_params![]).await.unwrap();
        let attestations: Vec<Vec<u8>> = client
            .request("enclave_signerAttestation", rpc_params![None::<Vec<u8>>, None::<Vec<u8>>])
            .await
            .unwrap();
        let proof_result = client
            .request::<ProofResult, _>("prover_prove", rpc_params![ProofRequest::default()])
            .await;

        handle.stop().unwrap();

        assert_eq!(kind, TDX_ATTESTATION_KIND);
        assert_eq!(public_keys.len(), 1);
        assert_eq!(public_keys[0].len(), 65);
        assert_eq!(attestations.len(), 1);
        let attestation = TdxSignerAttestation::decode(&attestations[0]).unwrap();
        assert_eq!(attestation.signer_public_key, public_keys[0]);
        assert!(base_proof_tee_tdx_verifier::TdxQuote::parse(&attestation.quote).is_ok());
        let err = proof_result.unwrap_err();
        assert!(err.to_string().contains("mock proof failure"));
        assert!(!err.to_string().contains("Method not found"));
    }

    #[tokio::test]
    async fn healthz_returns_version() {
        let rpc = HealthzRpc::new(env!("CARGO_PKG_VERSION"));
        let result = HealthzApiServer::healthz(&rpc).await.unwrap();
        assert_eq!(result.version, env!("CARGO_PKG_VERSION"));
    }
}
