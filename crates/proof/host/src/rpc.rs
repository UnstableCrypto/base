use std::{fmt, time::Duration};

use base_proof_primitives::{ProofRequest, ProofResult, ProverApiServer, ProverBackend};
use jsonrpsee::core::{RpcResult, async_trait};
use tracing::warn;

use crate::ProverService;

/// Error returned by a proof request handler before JSON-RPC conversion.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ProverRpcError {
    code: i32,
    message: String,
}

impl ProverRpcError {
    /// Creates a proof request error with an explicit JSON-RPC error code.
    pub fn new(code: i32, error: impl fmt::Display) -> Self {
        Self { code, message: error.to_string() }
    }

    /// Convert an internal error into a JSON-RPC error object.
    pub fn rpc_err(code: i32, err: impl fmt::Display) -> jsonrpsee::types::ErrorObjectOwned {
        jsonrpsee::types::ErrorObjectOwned::owned(code, err.to_string(), None::<()>)
    }

    /// Returns the JSON-RPC error code.
    pub const fn code(&self) -> i32 {
        self.code
    }

    /// Converts this error into a JSON-RPC error object.
    pub fn into_rpc_error(self) -> jsonrpsee::types::ErrorObjectOwned {
        Self::rpc_err(self.code, self.message)
    }
}

/// A backend capable of serving one `prover_prove` request.
#[async_trait]
pub trait ProverRequestHandler: Send + Sync {
    /// Execute a proof request.
    async fn prove_block(&self, request: ProofRequest) -> Result<ProofResult, ProverRpcError>;
}

#[async_trait]
impl<B> ProverRequestHandler for ProverService<B>
where
    B: ProverBackend,
{
    async fn prove_block(&self, request: ProofRequest) -> Result<ProofResult, ProverRpcError> {
        self.prove_block(request).await.map_err(|error| ProverRpcError::new(-32000, error))
    }
}

/// Shared JSON-RPC handler for `prover_*` methods.
pub struct ProverRpc<H> {
    handler: H,
    proof_request_timeout: Duration,
}

impl<H> ProverRpc<H> {
    /// Creates a new proving RPC handler.
    pub const fn new(handler: H, proof_request_timeout: Duration) -> Self {
        Self { handler, proof_request_timeout }
    }

    /// Returns the configured proof request timeout.
    pub const fn proof_request_timeout(&self) -> Duration {
        self.proof_request_timeout
    }
}

impl<H> fmt::Debug for ProverRpc<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProverRpc")
            .field("proof_request_timeout", &self.proof_request_timeout)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl<H> ProverApiServer for ProverRpc<H>
where
    H: ProverRequestHandler + 'static,
{
    async fn prove(&self, request: ProofRequest) -> RpcResult<ProofResult> {
        let l2_block = request.claimed_l2_block_number;
        let timeout = self.proof_request_timeout;

        match tokio::time::timeout(timeout, self.handler.prove_block(request)).await {
            Ok(result) => result.map_err(ProverRpcError::into_rpc_error),
            Err(_elapsed) => {
                warn!(l2_block, timeout_secs = timeout.as_secs(), "proof request timed out");
                Err(ProverRpcError::rpc_err(
                    -32000,
                    format!(
                        "proof request timed out after {}s for L2 block {l2_block}",
                        timeout.as_secs()
                    ),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopHandler;

    #[async_trait]
    impl ProverRequestHandler for NoopHandler {
        async fn prove_block(&self, _request: ProofRequest) -> Result<ProofResult, ProverRpcError> {
            Err(ProverRpcError::new(-32042, "handler failed"))
        }
    }

    #[tokio::test]
    async fn prover_rpc_maps_handler_error_code() {
        let rpc = ProverRpc::new(NoopHandler, Duration::from_secs(1));
        let err = ProverApiServer::prove(&rpc, ProofRequest::default()).await.unwrap_err();

        assert_eq!(err.code(), -32042);
        assert!(err.message().contains("handler failed"));
    }
}
