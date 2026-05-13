# `base-common-consensus`

Unstable chain consensus interface.

## Overview

Contains constants, types, and functions for implementing Unstable EL consensus and communication.
Includes an extended `UnstableTxEnvelope` type with deposit transactions, and receipts containing
chain-specific fields (`deposit_nonce` + `deposit_receipt_version`). Types in this crate
correspond to `alloy-consensus` types that were modified from the base Ethereum protocol for
the Unstable protocol.

For consensus types that are not modified by Unstable, the `alloy-consensus` types should be used
instead.

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
base-common-consensus = { workspace = true }
```

```rust,ignore
use base_common_consensus::{UnstableReceiptEnvelope, UnstableTxEnvelope};
```

## Provenance

Much of this code was ported from [reth-primitives] as part of ongoing alloy migrations, and originally from [op-alloy].

[reth-primitives]: https://github.com/paradigmxyz/reth/tree/main/crates/primitives
[op-alloy]: https://github.com/alloy-rs/op-alloy

## License

Licensed under the [MIT License](https://github.com/base/base/blob/main/LICENSE).
