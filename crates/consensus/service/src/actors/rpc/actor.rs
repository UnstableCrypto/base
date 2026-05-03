//! RPC Server Actor

use std::sync::Arc;

use async_trait::async_trait;
use base_consensus_gossip::P2pRpcRequest;
use base_consensus_rpc::{
    AdminApiServer, AdminRpc, BaseP2PApiServer, DevEngineApiServer, DevEngineRpc, EngineRpcClient,
    HealthzApiServer, HealthzRpc, L1WatcherQueries, NetworkAdminQuery, P2pRpc, RollupNodeApiServer,
    RollupRpc, RpcBuilder, SequencerAdminAPIClient, WsRPC, WsServer,
};
use base_consensus_safedb::SafeDBReader;
use base_health::EthHealthCheckLayer;
use derive_more::Constructor;
use http::StatusCode;
use jsonrpsee::{
    RpcModule,
    server::{Server, ServerHandle, middleware::http::ProxyGetRequestLayer},
};
use tokio::sync::mpsc;
use tokio_util::sync::{CancellationToken, WaitForCancellationFuture};
use tower_http::timeout::TimeoutLayer;

use crate::{NodeActor, RpcActorError, actors::CancellableContext};

/// An actor that handles the RPC server for the rollup node.
#[derive(Constructor, Debug)]
pub struct RpcActor<EngineRpcClient_, SequencerAdminApiClient_>
where
    EngineRpcClient_: EngineRpcClient,
    SequencerAdminApiClient_: SequencerAdminAPIClient,
{
    /// A launcher for the rpc.
    config: RpcBuilder,

    engine_rpc_client: EngineRpcClient_,
    sequencer_admin_rpc_client: Option<SequencerAdminApiClient_>,
    safe_db_reader: Arc<dyn SafeDBReader>,
}

/// The communication context used by the RPC actor.
#[derive(Debug)]
pub struct RpcContext {
    /// The network p2p rpc sender.
    pub p2p_network: Option<mpsc::Sender<P2pRpcRequest>>,
    /// The network admin rpc sender.
    pub network_admin: Option<mpsc::Sender<NetworkAdminQuery>>,
    /// The l1 watcher queries sender.
    pub l1_watcher_queries: mpsc::Sender<L1WatcherQueries>,
    /// The cancellation token, shared between all tasks.
    pub cancellation: CancellationToken,
}

impl CancellableContext for RpcContext {
    fn cancelled(&self) -> WaitForCancellationFuture<'_> {
        self.cancellation.cancelled()
    }
}

/// Launches the jsonrpsee [`Server`].
///
/// If the RPC server is disabled, this will return `Ok(None)`.
///
/// ## Errors
///
/// - [`std::io::Error`] if the server fails to start.
async fn launch(
    config: &RpcBuilder,
    module: RpcModule<()>,
) -> Result<ServerHandle, std::io::Error> {
    let middleware = tower::ServiceBuilder::new()
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, config.http_timeout))
        .layer(tower::limit::ConcurrencyLimitLayer::new(config.max_concurrent_requests.get()))
        .layer(tower::load_shed::LoadShedLayer::new())
        .layer(EthHealthCheckLayer)
        .layer(
            ProxyGetRequestLayer::new([("/healthz", "healthz")])
                .expect("Critical: Failed to build GET method proxy"),
        );
    let server = Server::builder().set_http_middleware(middleware).build(config.socket).await?;

    if let Ok(addr) = server.local_addr() {
        info!(target: "rpc", addr = ?addr, "RPC server bound to address");
    } else {
        error!(target: "rpc", "Failed to get local address for RPC server");
    }

    Ok(server.start(module))
}

#[async_trait]
impl<EngineRpcClient_, SequencerAdminApiClient_> NodeActor
    for RpcActor<EngineRpcClient_, SequencerAdminApiClient_>
where
    EngineRpcClient_: EngineRpcClient + 'static,
    SequencerAdminApiClient_: SequencerAdminAPIClient + 'static,
{
    type Error = RpcActorError;
    type StartData = RpcContext;

    async fn start(
        mut self,
        RpcContext {
            cancellation,
            p2p_network,
            l1_watcher_queries,
            network_admin,
        }: Self::StartData,
    ) -> Result<(), Self::Error> {
        let mut modules = RpcModule::new(());

        modules.merge(HealthzApiServer::into_rpc(HealthzRpc {}))?;

        // Build the p2p rpc module.
        if let Some(p2p_network) = p2p_network {
            modules.merge(P2pRpc::new(p2p_network).into_rpc())?;
        }

        if self.config.admin_enabled() {
            // Build the admin rpc module.
            if let Some(network_admin) = network_admin {
                modules.merge(
                    AdminRpc::new(self.sequencer_admin_rpc_client, network_admin).into_rpc(),
                )?;
            }
        }

        // Create context for communication between actors.
        let rollup_rpc = RollupRpc::new(
            self.engine_rpc_client.clone(),
            l1_watcher_queries,
            Arc::clone(&self.safe_db_reader),
        );
        modules.merge(rollup_rpc.into_rpc())?;

        // Add development RPC module for engine state introspection if enabled
        if self.config.dev_enabled() {
            let dev_rpc = DevEngineRpc::new(self.engine_rpc_client.clone());
            modules.merge(dev_rpc.into_rpc())?;
        }

        if self.config.ws_enabled() {
            modules.merge(WsRPC::new(self.engine_rpc_client.clone()).into_rpc())?;
        }

        let restarts = self.config.restart_count();

        let mut handle = launch(&self.config, modules.clone()).await?;

        for _ in 0..=restarts {
            tokio::select! {
                _ = handle.clone().stopped() => {
                    match launch(&self.config, modules.clone()).await {
                        Ok(h) => handle = h,
                        Err(err) => {
                            error!(target: "rpc", ?err, "Failed to launch rpc server");
                            cancellation.cancel();
                            return Err(RpcActorError::ServerStopped);
                        }
                    }
                }
                _ = cancellation.cancelled() => {
                    // The cancellation token has been triggered, so we should stop the server.
                    handle.stop().map_err(|_| RpcActorError::StopFailed)?;
                    // Since the RPC Server didn't originate the error, we should return Ok.
                    return Ok(());
                }
            }
        }

        // Stop the node if there has already been 3 rpc restarts.
        cancellation.cancel();
        return Err(RpcActorError::ServerStopped);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{SocketAddr, TcpListener},
        num::NonZeroUsize,
        sync::Arc,
        time::Duration,
    };

    use super::*;
    use alloy_eips::BlockNumberOrTag;
    use alloy_primitives::B256;
    use base_common_genesis::RollupConfig;
    use base_consensus_engine::EngineState;
    use base_consensus_rpc::{SequencerAdminAPIClient, SequencerAdminAPIError};
    use base_consensus_safedb::{SafeDBError, SafeHeadResponse};
    use base_protocol::{L2BlockInfo, OutputRoot};
    use jsonrpsee::{
        core::{ClientError, client::ClientT},
        http_client::HttpClientBuilder,
        rpc_params,
        types::ErrorCode,
    };
    use tokio::{sync::watch, time::sleep};

    #[derive(Clone, Debug)]
    struct TestEngineRpcClient;

    #[async_trait]
    impl EngineRpcClient for TestEngineRpcClient {
        async fn get_config(&self) -> jsonrpsee::core::RpcResult<RollupConfig> {
            Ok(RollupConfig::default())
        }

        async fn get_state(&self) -> jsonrpsee::core::RpcResult<EngineState> {
            Ok(EngineState::default())
        }

        async fn output_at_block(
            &self,
            _: BlockNumberOrTag,
        ) -> jsonrpsee::core::RpcResult<(L2BlockInfo, OutputRoot, EngineState)> {
            Ok((
                L2BlockInfo::default(),
                OutputRoot::from_parts(B256::ZERO, B256::ZERO, B256::ZERO),
                EngineState::default(),
            ))
        }

        async fn dev_get_task_queue_length(&self) -> jsonrpsee::core::RpcResult<usize> {
            Ok(0)
        }

        async fn dev_subscribe_to_engine_queue_length(
            &self,
        ) -> jsonrpsee::core::RpcResult<watch::Receiver<usize>> {
            let (_, rx) = watch::channel(0);
            Ok(rx)
        }

        async fn dev_subscribe_to_engine_state(
            &self,
        ) -> jsonrpsee::core::RpcResult<watch::Receiver<EngineState>> {
            let (_, rx) = watch::channel(EngineState::default());
            Ok(rx)
        }
    }

    #[derive(Debug)]
    struct TestSequencerAdminClient;

    #[async_trait]
    impl SequencerAdminAPIClient for TestSequencerAdminClient {
        async fn is_sequencer_active(&self) -> Result<bool, SequencerAdminAPIError> {
            Ok(false)
        }

        async fn is_conductor_enabled(&self) -> Result<bool, SequencerAdminAPIError> {
            Ok(false)
        }

        async fn is_recovery_mode(&self) -> Result<bool, SequencerAdminAPIError> {
            Ok(false)
        }

        async fn start_sequencer(&self, _: B256) -> Result<(), SequencerAdminAPIError> {
            Ok(())
        }

        async fn stop_sequencer(&self) -> Result<B256, SequencerAdminAPIError> {
            Ok(B256::ZERO)
        }

        async fn set_recovery_mode(&self, _: bool) -> Result<(), SequencerAdminAPIError> {
            Ok(())
        }

        async fn override_leader(&self) -> Result<(), SequencerAdminAPIError> {
            Ok(())
        }

        async fn reset_derivation_pipeline(&self) -> Result<(), SequencerAdminAPIError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct TestSafeDBReader;

    #[async_trait]
    impl SafeDBReader for TestSafeDBReader {
        async fn safe_head_at_l1(&self, _: u64) -> Result<SafeHeadResponse, SafeDBError> {
            Err(SafeDBError::Disabled)
        }
    }

    #[tokio::test]
    async fn test_launch_no_modules() {
        let launcher = RpcBuilder {
            socket: SocketAddr::from(([127, 0, 0, 1], 8080)),
            no_restart: false,
            enable_admin: false,
            admin_persistence: None,
            ws_enabled: false,
            dev_enabled: false,
            http_timeout: Duration::from_secs(60),
            max_concurrent_requests: NonZeroUsize::new(1024).expect("nonzero"),
        };
        let result = launch(&launcher, RpcModule::new(())).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_launch_with_modules() {
        let launcher = RpcBuilder {
            socket: SocketAddr::from(([127, 0, 0, 1], 8081)),
            no_restart: false,
            enable_admin: false,
            admin_persistence: None,
            ws_enabled: false,
            dev_enabled: false,
            http_timeout: Duration::from_secs(60),
            max_concurrent_requests: NonZeroUsize::new(1024).expect("nonzero"),
        };
        let mut modules = RpcModule::new(());

        modules.merge(RpcModule::new(())).expect("module merge");
        modules.merge(RpcModule::new(())).expect("module merge");
        modules.merge(RpcModule::new(())).expect("module merge");

        let result = launch(&launcher, modules).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_admin_rpc_disabled_even_with_admin_channels() {
        let result = admin_sequencer_active(false).await;

        let Err(ClientError::Call(error)) = result else {
            panic!("expected method not found, got {result:?}");
        };

        assert_eq!(error.code(), ErrorCode::MethodNotFound.code());
    }

    #[tokio::test]
    async fn test_admin_rpc_enabled_with_admin_channels() {
        let result = admin_sequencer_active(true).await.expect("admin rpc request succeeds");

        assert!(!result);
    }

    async fn admin_sequencer_active(enable_admin: bool) -> Result<bool, ClientError> {
        let socket = unused_loopback_addr();
        let config = RpcBuilder {
            socket,
            no_restart: true,
            enable_admin,
            admin_persistence: None,
            ws_enabled: false,
            dev_enabled: false,
            http_timeout: Duration::from_secs(60),
            max_concurrent_requests: NonZeroUsize::new(1024).expect("nonzero"),
        };
        let actor = RpcActor::new(
            config,
            TestEngineRpcClient,
            Some(TestSequencerAdminClient),
            Arc::new(TestSafeDBReader),
        );
        let cancellation = CancellationToken::new();
        let (network_admin, _network_admin_rx) = mpsc::channel(1);
        let (l1_watcher_queries, _l1_watcher_queries_rx) = mpsc::channel(1);
        let context = RpcContext {
            p2p_network: None,
            network_admin: Some(network_admin),
            l1_watcher_queries,
            cancellation: cancellation.clone(),
        };

        let task = tokio::spawn(actor.start(context));
        let client =
            HttpClientBuilder::default().build(format!("http://{socket}")).expect("rpc client");
        let result = request_admin_sequencer_active(&client).await;

        cancellation.cancel();
        task.await.expect("rpc actor task join").expect("rpc actor stops");

        result
    }

    async fn request_admin_sequencer_active(
        client: &jsonrpsee::http_client::HttpClient,
    ) -> Result<bool, ClientError> {
        for attempt in 0..50 {
            let result = ClientT::request(client, "admin_sequencerActive", rpc_params![]).await;

            if !matches!(result, Err(ClientError::Transport(_))) || attempt == 49 {
                return result;
            }

            sleep(Duration::from_millis(10)).await;
        }

        unreachable!("request loop returns on the final attempt");
    }

    fn unused_loopback_addr() -> SocketAddr {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind port");
        listener.local_addr().expect("local address")
    }
}
