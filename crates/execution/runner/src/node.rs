//! Unstable Node types config.

use base_common_consensus::UnstablePrimitives;
use base_engine_tree::UnstableEngineValidatorBuilder;
use base_execution_chainspec::UnstableChainSpec;
use base_execution_payload_builder::config::{UnstableDAConfig, GasLimitConfig};
use base_execution_rpc::eth::UnstableEthApiBuilder;
use base_node_core::{
    UnstableConsensusBuilder, UnstableEngineApiBuilder, UnstableEngineTypes, UnstableExecutorBuilder,
    UnstableNetworkBuilder, UnstableNodeComponentBuilder, UnstableNodeTypes, UnstablePayloadValidatorBuilder,
    UnstableStorage,
    args::RollupArgs,
    node::{UnstablePayloadBuilder, UnstablePoolBuilder},
};
use reth_node_builder::{
    Node, NodeAdapter, NodeComponentsBuilder,
    components::{BasicPayloadServiceBuilder, ComponentsBuilder},
    node::{FullNodeTypes, NodeTypes},
};
use reth_provider::providers::ProviderFactoryBuilder;
use reth_rpc_api::eth::RpcTypes;

use crate::{UnstableAddOns, UnstableAddOnsBuilder};

/// Type configuration for a regular Unstable node.
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct UnstableNode {
    /// Additional Unstable args
    pub args: RollupArgs,
    /// Data availability configuration for the payload builder.
    ///
    /// Used to throttle the size of the data availability payloads (configured by the batcher via
    /// the `miner_` api).
    ///
    /// By default no throttling is applied.
    pub da_config: UnstableDAConfig,
    /// Gas limit configuration for the payload builder.
    /// Used to control the gas limit of the blocks produced by the payload builder (configured by the
    /// batcher via the `miner_` api)
    pub gas_limit_config: GasLimitConfig,
}

impl UnstableNode {
    /// Creates a new instance of the Unstable node type.
    pub fn new(args: RollupArgs) -> Self {
        Self {
            args,
            da_config: UnstableDAConfig::default(),
            gas_limit_config: GasLimitConfig::default(),
        }
    }

    /// Configure the data availability configuration for the payload builder.
    pub fn with_da_config(mut self, da_config: UnstableDAConfig) -> Self {
        self.da_config = da_config;
        self
    }

    /// Configure the gas limit configuration for the payload builder.
    pub fn with_gas_limit_config(mut self, gas_limit_config: GasLimitConfig) -> Self {
        self.gas_limit_config = gas_limit_config;
        self
    }

    /// Returns the components for the given [`RollupArgs`].
    pub fn components<Node>(&self) -> UnstableNodeComponentBuilder<Node>
    where
        Node: FullNodeTypes<Types: UnstableNodeTypes>,
    {
        let RollupArgs {
            disable_txpool_gossip,
            compute_pending_block,
            discovery_v4,
            base_protocol,
            max_inflight_delegated_slots,
            ..
        } = self.args;
        ComponentsBuilder::default()
            .node_types::<Node>()
            .pool(
                UnstablePoolBuilder::default()
                    .with_max_inflight_delegated_slots(max_inflight_delegated_slots),
            )
            .executor(UnstableExecutorBuilder::default())
            .payload(BasicPayloadServiceBuilder::new(
                UnstablePayloadBuilder::new(compute_pending_block)
                    .with_da_config(self.da_config.clone())
                    .with_gas_limit_config(self.gas_limit_config.clone()),
            ))
            .network(UnstableNetworkBuilder::new(disable_txpool_gossip, !discovery_v4, base_protocol))
            .consensus(UnstableConsensusBuilder::default())
    }

    /// Returns [`UnstableAddOnsBuilder`] with configured arguments.
    pub fn add_ons_builder<NetworkT: RpcTypes>(&self) -> UnstableAddOnsBuilder<NetworkT> {
        UnstableAddOnsBuilder::default()
            .with_sequencer(self.args.sequencer.clone())
            .with_sequencer_headers(self.args.sequencer_headers.clone())
            .with_da_config(self.da_config.clone())
            .with_gas_limit_config(self.gas_limit_config.clone())
            .with_min_suggested_priority_fee(self.args.min_suggested_priority_fee)
    }

    /// Instantiates the [`ProviderFactoryBuilder`] for a Unstable node.
    ///
    /// # Open a `ProviderFactory` in read-only mode from a datadir
    ///
    /// See also: [`ProviderFactoryBuilder`] and
    /// [`ReadOnlyConfig`](reth_provider::providers::ReadOnlyConfig).
    ///
    /// ```no_run
    /// use base_execution_chainspec::UnstableChainSpec;
    /// use base_node_runner::UnstableNode;
    /// use reth_provider::providers::ReadOnlyConfig;
    /// use std::sync::Arc;
    ///
    /// let runtime = reth_tasks::Runtime::test();
    /// let factory = UnstableNode::provider_factory_builder()
    ///     .open_read_only(
    ///         Arc::new(UnstableChainSpec::mainnet()),
    ///         ReadOnlyConfig::from_datadir("datadir").no_watch(),
    ///         runtime,
    ///     )
    ///     .unwrap();
    /// ```
    ///
    /// # Open a `ProviderFactory` manually with all required components
    ///
    /// ```no_run
    /// use base_execution_chainspec::UnstableChainSpecBuilder;
    /// use base_node_runner::UnstableNode;
    /// use reth_db::mdbx::DatabaseArguments;
    /// use reth_provider::providers::ReadOnlyConfig;
    ///
    /// let runtime = reth_tasks::Runtime::test();
    /// let factory = UnstableNode::provider_factory_builder()
    ///     .open_read_only(
    ///         UnstableChainSpecBuilder::base_mainnet().build().into(),
    ///         ReadOnlyConfig {
    ///             db_dir: "db".into(),
    ///             db_args: DatabaseArguments::default(),
    ///             static_files_dir: "db/static_files".into(),
    ///             rocksdb_dir: "db/rocksdb".into(),
    ///             watch_static_files: false,
    ///         },
    ///         runtime,
    ///     )
    ///     .unwrap();
    /// ```
    pub fn provider_factory_builder() -> ProviderFactoryBuilder<Self> {
        ProviderFactoryBuilder::default()
    }
}

impl<N> Node<N> for UnstableNode
where
    N: FullNodeTypes<Types: UnstableNodeTypes>,
{
    type ComponentsBuilder = ComponentsBuilder<
        N,
        UnstablePoolBuilder,
        BasicPayloadServiceBuilder<UnstablePayloadBuilder>,
        UnstableNetworkBuilder,
        UnstableExecutorBuilder,
        UnstableConsensusBuilder,
    >;

    type AddOns = UnstableAddOns<
        NodeAdapter<N, <Self::ComponentsBuilder as NodeComponentsBuilder<N>>::Components>,
        UnstableEthApiBuilder,
        UnstablePayloadValidatorBuilder,
        UnstableEngineApiBuilder<UnstablePayloadValidatorBuilder>,
        UnstableEngineValidatorBuilder<UnstablePayloadValidatorBuilder>,
    >;

    fn components_builder(&self) -> Self::ComponentsBuilder {
        Self::components(self)
    }

    fn add_ons(&self) -> Self::AddOns {
        self.add_ons_builder().build()
    }
}

impl NodeTypes for UnstableNode {
    type Primitives = UnstablePrimitives;
    type ChainSpec = UnstableChainSpec;
    type Storage = UnstableStorage;
    type Payload = UnstableEngineTypes;
}
