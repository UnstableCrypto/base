//! Hook accumulator for the node builder.
//!
//! [`NodeHooks`] collects RPC, node-started, and `ExEx` hooks that extensions install. These hooks
//! are applied to a configured reth builder via [`NodeHooks::apply_to`] just before launch.

use std::fmt;

use eyre::Result;
use futures::future::BoxFuture;
use reth_exex::ExExContext;
use reth_node_builder::{
    NodeAdapter, NodeBuilderWithComponents, NodeComponentsBuilder, WithLaunchContext,
    node::FullNode,
    rpc::{RethRpcAddOns, RpcContext},
};

use crate::types::{UnstableComponentsBuilder, UnstableNodeTypes, ConcreteUnstableAddOns};

/// Alias for the default Unstable components type.
type UnstableComponents = <UnstableComponentsBuilder as NodeComponentsBuilder<UnstableNodeTypes>>::Components;

/// Convenience alias for the Unstable node adapter type used by the reth builder.
///
/// Because `Components` depends only on pool, network, executor, and consensus builders (not the
/// payload service builder), this type is identical regardless of which payload service is used.
pub type UnstableNodeAdapter = NodeAdapter<UnstableNodeTypes, UnstableComponents>;

/// Convenience alias for the Unstable Eth API type exposed by the reth RPC add-ons.
type UnstableEthApi = <ConcreteUnstableAddOns as RethRpcAddOns<UnstableNodeAdapter>>::EthApi;

/// Convenience alias for the full Unstable node handle produced after launch.
type UnstableFullNode = FullNode<UnstableNodeAdapter, ConcreteUnstableAddOns>;

/// Alias for the RPC context used by Unstable extensions.
pub type UnstableRpcContext<'a> = RpcContext<'a, UnstableNodeAdapter, UnstableEthApi>;

/// Hook type for extending RPC modules.
type RpcModuleHook = Box<dyn FnOnce(&mut UnstableRpcContext<'_>) -> Result<()> + Send + 'static>;

/// Hook type for extending add-ons.
type AddOnsHook = Box<dyn FnOnce(ConcreteUnstableAddOns) -> ConcreteUnstableAddOns>;

/// Hook type for node-started callbacks.
type NodeStartedHook = Box<dyn FnOnce(UnstableFullNode) -> Result<()> + Send + 'static>;

/// Type-erased `ExEx` factory.
type BoxExExFactory = Box<
    dyn FnOnce(
            ExExContext<UnstableNodeAdapter>,
        ) -> BoxFuture<'static, eyre::Result<BoxFuture<'static, eyre::Result<()>>>>
        + Send
        + 'static,
>;

/// A type alias for any configured builder whose components match the canonical Unstable types.
///
/// This is generic over the `NodeComponentsBuilder` (`CB`) so that both the default payload and
/// the flashblocks payload service can be used interchangeably.
pub type RethNodeBuilder<CB> =
    WithLaunchContext<NodeBuilderWithComponents<UnstableNodeTypes, CB, ConcreteUnstableAddOns>>;

/// Pure hook accumulator for the Unstable node builder.
///
/// Extensions call [`add_rpc_module`](Self::add_rpc_module),
/// [`add_node_started_hook`](Self::add_node_started_hook), and
/// [`install_exex`](Self::install_exex) to register hooks. The runner then calls
/// [`apply_to`](Self::apply_to) to drain all hooks onto the concrete configured builder.
///
/// After applying hooks, call [`.launch()`](RethNodeBuilder::launch) on the configured builder.
pub struct NodeHooks {
    rpc_hooks: Vec<RpcModuleHook>,
    node_started_hooks: Vec<NodeStartedHook>,
    add_ons_hooks: Vec<AddOnsHook>,
    exex_hooks: Vec<(String, BoxExExFactory)>,
}

impl NodeHooks {
    /// Create a new, empty `NodeHooks`.
    pub fn new() -> Self {
        Self {
            rpc_hooks: Vec::new(),
            node_started_hooks: Vec::new(),
            exex_hooks: Vec::new(),
            add_ons_hooks: Vec::new(),
        }
    }

    /// Applies all accumulated hooks to the given configured builder.
    ///
    /// This is generic over `CB` so that it works with any payload service whose component
    /// builder produces the same concrete `Components` type as the default payload builder.
    pub fn apply_to<CB>(self, mut builder: RethNodeBuilder<CB>) -> RethNodeBuilder<CB>
    where
        CB: NodeComponentsBuilder<UnstableNodeTypes, Components = UnstableComponents>,
    {
        let Self { rpc_hooks, node_started_hooks, exex_hooks, add_ons_hooks } = self;

        // Install ExEx hooks
        for (id, factory) in exex_hooks {
            builder =
                builder.install_exex(id, move |ctx: ExExContext<UnstableNodeAdapter>| factory(ctx));
        }

        for hook in add_ons_hooks {
            builder = builder.map_add_ons(hook);
        }

        // Install RPC hooks
        if !rpc_hooks.is_empty() {
            builder = builder.extend_rpc_modules(move |mut ctx: UnstableRpcContext<'_>| {
                for hook in rpc_hooks {
                    hook(&mut ctx)?;
                }
                Ok(())
            });
        }

        // Install node-started hooks
        if !node_started_hooks.is_empty() {
            builder = builder.on_node_started(move |full_node: UnstableFullNode| {
                for hook in node_started_hooks {
                    hook(full_node.clone())?;
                }
                Ok(())
            });
        }

        builder
    }

    /// Adds an RPC hook that will run when RPC modules are configured.
    pub fn add_rpc_module<F>(mut self, hook: F) -> Self
    where
        F: FnOnce(&mut UnstableRpcContext<'_>) -> Result<()> + Send + 'static,
    {
        self.rpc_hooks.push(Box::new(hook));
        self
    }

    /// Adds an add-ons hook that will run when the add-ons are configured.
    pub fn add_add_ons_hook<F>(mut self, hook: F) -> Self
    where
        F: FnOnce(ConcreteUnstableAddOns) -> ConcreteUnstableAddOns + Send + 'static,
    {
        self.add_ons_hooks.push(Box::new(hook));
        self
    }

    /// Adds a node-started hook that will run after the node has started.
    pub fn add_node_started_hook<F>(mut self, hook: F) -> Self
    where
        F: FnOnce(UnstableFullNode) -> Result<()> + Send + 'static,
    {
        self.node_started_hooks.push(Box::new(hook));
        self
    }

    /// Installs an `ExEx` extension with the given name and closure.
    pub fn install_exex<F, R, E>(mut self, exex_id: impl Into<String>, exex: F) -> Self
    where
        F: FnOnce(ExExContext<UnstableNodeAdapter>) -> R + Send + 'static,
        R: Future<Output = eyre::Result<E>> + Send,
        E: Future<Output = eyre::Result<()>> + Send + 'static,
    {
        let factory: BoxExExFactory = Box::new(move |ctx| {
            Box::pin(async move {
                let inner = exex(ctx).await?;
                Ok(Box::pin(inner) as BoxFuture<'static, eyre::Result<()>>)
            })
        });
        self.exex_hooks.push((exex_id.into(), factory));
        self
    }
}

impl Default for NodeHooks {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for NodeHooks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeHooks").finish_non_exhaustive()
    }
}
