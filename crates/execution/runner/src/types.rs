//! Type aliases for the Unstable node builder.

use base_execution_chainspec::UnstableChainSpec;
use reth_db::DatabaseEnv;
use reth_node_builder::{
    FullNodeTypesAdapter, Node, NodeBuilder, NodeTypesWithDBAdapter, WithLaunchContext,
};
use reth_provider::providers::BlockchainProvider;

use crate::node::UnstableNode;

/// Alias for the Unstable node type adapter used by the runner.
pub type UnstableNodeTypes = FullNodeTypesAdapter<UnstableNode, DatabaseEnv, UnstableProvider>;
/// Internal alias for the Unstable node components builder (default payload service).
pub type UnstableComponentsBuilder = <UnstableNode as Node<UnstableNodeTypes>>::ComponentsBuilder;
/// Internal alias for the Unstable node add-ons (all generics resolved).
pub(crate) type ConcreteUnstableAddOns = <UnstableNode as Node<UnstableNodeTypes>>::AddOns;

/// A [`BlockchainProvider`] instance.
pub type UnstableProvider = BlockchainProvider<NodeTypesWithDBAdapter<UnstableNode, DatabaseEnv>>;

/// Convenience alias for the Unstable node builder type.
pub type UnstableNodeBuilder = WithLaunchContext<NodeBuilder<DatabaseEnv, UnstableChainSpec>>;
