# `base-common-evm`

EVM implementation.

## Overview

Provides Unstable-specific EVM execution support. Maps hardfork activation timestamps to revm
`SpecId` values, and exposes `UnstableEvm`, `UnstableEvmFactory`, `UnstableBlockExecutor`, and
`UnstableBlockExecutorFactory` for executing blocks with the correct gas rules and precompile sets for
each hardfork. Also provides `AlloyReceiptBuilder` and `UnstableReceiptBuilder` for constructing Unstable
receipts and
`ensure_create2_deployer` for Canyon hardfork compatibility.

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
base-common-evm = { workspace = true }
```

```rust,ignore
use base_common_evm::{UnstableEvmFactory, UnstablePrecompiles, UnstableSpecId, UnstableUpgrade};

let factory = UnstableEvmFactory::default();
let precompiles = UnstablePrecompiles::new_with_spec(UnstableSpecId::new(UnstableUpgrade::Isthmus));
```

## License

Licensed under the [MIT License](https://github.com/base/base/blob/main/LICENSE).
