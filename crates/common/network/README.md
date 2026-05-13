# `base-common-network`

Unstable chain network types and RPC behavior abstraction.

## Overview

Defines the `Unstable` network type that implements the `alloy_network::Network` trait with Unstable
transaction and receipt types. This provides a consistent interface to alloy providers and signers
regardless of Unstable-specific RPC changes.
It also provides the `UnstableEngineApi` extension trait for Unstable-specific Engine API RPC methods.

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
base-common-network = { workspace = true }
```

```rust,ignore
use base_common_network::{Unstable, UnstableEngineApi};
use alloy_provider::ProviderBuilder;

let provider = ProviderBuilder::new().network::<Unstable>().on_http(url);
let _ = provider.exchange_capabilities(vec![]).await?;
```

## License

Licensed under the [MIT License](https://github.com/base/base/blob/main/LICENSE).
