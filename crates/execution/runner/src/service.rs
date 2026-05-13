//! Trait for customizing the payload service used by the node.

use base_node_core::{
    UnstableConsensusBuilder, UnstableExecutorBuilder, UnstableNetworkBuilder,
    node::{UnstablePayloadBuilder, UnstablePoolBuilder},
};
use reth_node_builder::{
    NodeComponentsBuilder,
    components::{BasicPayloadServiceBuilder, ComponentsBuilder},
};

use crate::{
    node::UnstableNode,
    types::{UnstableComponentsBuilder, UnstableNodeTypes},
};

/// Trait for customizing the payload service used by the node.
///
/// Implementors provide a custom [`NodeComponentsBuilder`] that wires in their
/// payload service. The default implementation uses reth's standard Unstable payload builder.
///
/// The produced components must have the same concrete `Components` type as the default
/// so that hooks (RPC, `ExEx`, node-started) remain type-compatible.
pub trait PayloadServiceBuilder: Send + 'static {
    /// The component builder type this produces.
    type ComponentsBuilder: NodeComponentsBuilder<
            UnstableNodeTypes,
            Components = <UnstableComponentsBuilder as NodeComponentsBuilder<UnstableNodeTypes>>::Components,
        >;

    /// Build components using the given [`UnstableNode`] configuration.
    fn build_components(self, base_node: &UnstableNode) -> Self::ComponentsBuilder;
}

/// Default payload service using the standard Unstable payload builder.
#[derive(Debug, Default)]
pub struct DefaultPayloadServiceBuilder;

impl PayloadServiceBuilder for DefaultPayloadServiceBuilder {
    type ComponentsBuilder = ComponentsBuilder<
        UnstableNodeTypes,
        UnstablePoolBuilder,
        BasicPayloadServiceBuilder<UnstablePayloadBuilder>,
        UnstableNetworkBuilder,
        UnstableExecutorBuilder,
        UnstableConsensusBuilder,
    >;

    fn build_components(self, base_node: &UnstableNode) -> Self::ComponentsBuilder {
        base_node.components()
    }
}
