# `base-txpool`

Transaction pool for Unstable.

## Overview

Extends Reth's transaction pool with Unstable-specific validation and ordering for the Unstable node.
`UnstableTransactionValidator` enforces L1 data fee checks and Unstable-specific validity rules.
`UnstableOrdering` and `TimestampOrdering` provide customizable transaction prioritization strategies.
Also includes a `Consumer` for processing mempool events, a `Forwarder` for relaying transactions,
and a `BuilderApiImpl` for builder-specific pool management.

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
base-txpool = { workspace = true }
```

```rust,ignore
use base_txpool::{UnstableOrdering, UnstableTransactionPool, UnstableTransactionValidator};

let pool = Pool::new(
    UnstableTransactionValidator::new(client, evm),
    UnstableOrdering::default(),
    config,
);
```

## License

Licensed under the [MIT License](https://github.com/base/base/blob/main/LICENSE).
