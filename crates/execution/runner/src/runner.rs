//! Contains the [`UnstableNodeRunner`], which is responsible for configuring and launching a Unstable node.

use std::fmt;

use base_execution_payload_builder::config::UnstableDAConfig;
use base_node_core::args::RollupArgs;
use eyre::Result;
use reth_node_builder::{Node, NodeHandle, NodeHandleFor};
use reth_provider::providers::BlockchainProvider;
use tracing::info;

use crate::{
    UnstableNodeBuilder, UnstableNodeExtension, FromExtensionConfig, NodeHooks,
    node::UnstableNode,
    service::{DefaultPayloadServiceBuilder, PayloadServiceBuilder},
};

type StartedCallback = Box<dyn FnOnce() -> Result<()> + Send + 'static>;

/// Wraps the Unstable node configuration and orchestrates builder wiring.
pub struct UnstableNodeRunner<SB: PayloadServiceBuilder = DefaultPayloadServiceBuilder> {
    /// Rollup-specific arguments forwarded to the Unstable node implementation.
    rollup_args: RollupArgs,
    /// Registered builder extensions.
    extensions: Vec<Box<dyn UnstableNodeExtension>>,
    /// Payload service builder.
    service_builder: SB,
    /// Shared DA configuration for the node and metering extension.
    da_config: Option<UnstableDAConfig>,
    /// Binary-owned callbacks to run after the node has started.
    started_callbacks: Vec<StartedCallback>,
}

impl UnstableNodeRunner<DefaultPayloadServiceBuilder> {
    /// Creates a new launcher using the provided rollup arguments.
    pub fn new(rollup_args: RollupArgs) -> Self {
        Self {
            rollup_args,
            extensions: Vec::new(),
            service_builder: DefaultPayloadServiceBuilder,
            da_config: None,
            started_callbacks: Vec::new(),
        }
    }
}

impl<SB: PayloadServiceBuilder> fmt::Debug for UnstableNodeRunner<SB> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnstableNodeRunner")
            .field("rollup_args", &self.rollup_args)
            .field("extensions", &self.extensions.len())
            .field("da_config", &self.da_config)
            .field("started_callbacks", &self.started_callbacks.len())
            .finish()
    }
}

impl<SB: PayloadServiceBuilder> UnstableNodeRunner<SB> {
    /// Sets the shared DA configuration.
    pub fn with_da_config(mut self, da_config: UnstableDAConfig) -> Self {
        self.da_config = Some(da_config);
        self
    }

    /// Swap the payload service builder.
    pub fn with_service_builder<SB2: PayloadServiceBuilder>(self, sb: SB2) -> UnstableNodeRunner<SB2> {
        UnstableNodeRunner {
            rollup_args: self.rollup_args,
            extensions: self.extensions,
            service_builder: sb,
            da_config: self.da_config,
            started_callbacks: self.started_callbacks,
        }
    }

    /// Registers a new builder extension.
    pub fn install_ext<T: FromExtensionConfig + 'static>(&mut self, config: T::Config) {
        self.extensions.push(Box::new(T::from_config(config)));
    }

    /// Registers a callback to run after the node has started.
    pub fn add_started_callback<F>(&mut self, callback: F)
    where
        F: FnOnce() -> Result<()> + Send + 'static,
    {
        self.started_callbacks.push(Box::new(callback));
    }

    /// Applies all Unstable-specific wiring to the supplied builder, launches the node, and waits for
    /// shutdown.
    pub async fn run(self, builder: UnstableNodeBuilder) -> Result<()> {
        let Self { rollup_args, extensions, service_builder, da_config, started_callbacks } = self;
        let NodeHandle { node: _node, node_exit_future } = Self::launch_node(
            rollup_args,
            extensions,
            service_builder,
            da_config,
            started_callbacks,
            builder,
        )
        .await?;
        node_exit_future.await?;
        Ok(())
    }

    async fn launch_node(
        rollup_args: RollupArgs,
        extensions: Vec<Box<dyn UnstableNodeExtension>>,
        service_builder: SB,
        da_config: Option<UnstableDAConfig>,
        started_callbacks: Vec<StartedCallback>,
        builder: UnstableNodeBuilder,
    ) -> Result<NodeHandleFor<UnstableNode>> {
        info!(target: "base-runner", "starting custom Unstable node");

        let mut base_node = UnstableNode::new(rollup_args);
        if let Some(da_config) = da_config {
            base_node = base_node.with_da_config(da_config);
        }
        let components = service_builder.build_components(&base_node);

        let builder = builder
            .with_types_and_provider::<UnstableNode, BlockchainProvider<_>>()
            .with_components(components)
            .with_add_ons(base_node.add_ons())
            .on_component_initialized(move |_ctx| Ok(()));

        let hooks = extensions.into_iter().fold(NodeHooks::new(), |hooks, ext| ext.apply(hooks));
        let hooks = started_callbacks
            .into_iter()
            .fold(hooks, |hooks, callback| hooks.add_node_started_hook(move |_| callback()));

        hooks.apply_to(builder).launch().await
    }
}
