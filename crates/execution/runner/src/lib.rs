#![doc = include_str!("../README.md")]
#![doc(
    html_logo_url = "https://avatars.githubusercontent.com/u/16627100?s=200&v=4",
    html_favicon_url = "https://avatars.githubusercontent.com/u/16627100?s=200&v=4",
    issue_tracker_base_url = "https://github.com/base/base/issues/"
)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]

mod builder;
pub use builder::{UnstableNodeAdapter, UnstableRpcContext, NodeHooks, RethNodeBuilder};

mod extension;
pub use extension::{UnstableNodeExtension, FromExtensionConfig};

mod runner;
pub use runner::UnstableNodeRunner;

mod service;
pub use service::{DefaultPayloadServiceBuilder, PayloadServiceBuilder};

mod types;
pub use types::{UnstableComponentsBuilder, UnstableNodeBuilder, UnstableNodeTypes, UnstableProvider};

mod node;
pub use node::UnstableNode;

mod add_ons;
pub use add_ons::{UnstableAddOns, UnstableAddOnsBuilder};

#[cfg(feature = "test-utils")]
pub mod test_utils;
