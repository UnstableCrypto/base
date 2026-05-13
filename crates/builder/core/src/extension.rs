//! Builder API RPC extension for registering the `base_insertValidatedTransaction` endpoint.

use base_execution_txpool::{BuilderApiImpl, BuilderApiServer};
use base_node_runner::{UnstableNodeExtension, UnstableRpcContext, FromExtensionConfig, NodeHooks};

/// Extension that registers the Builder API RPC module (`base_insertValidatedTransaction`).
#[derive(Debug, Default)]
pub struct BuilderApiExtension;

impl UnstableNodeExtension for BuilderApiExtension {
    fn apply(self: Box<Self>, builder: NodeHooks) -> NodeHooks {
        builder.add_rpc_module(move |ctx: &mut UnstableRpcContext<'_>| {
            let api = BuilderApiImpl::new(ctx.pool().clone());
            ctx.modules.merge_configured(api.into_rpc())?;
            Ok(())
        })
    }
}

impl FromExtensionConfig for BuilderApiExtension {
    type Config = ();

    fn from_config(_config: Self::Config) -> Self {
        Self
    }
}
