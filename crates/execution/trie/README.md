# `base-execution-trie`

Trie implementation for Unstable.

## Overview

Manages Merkle Patricia Trie proof storage for the fault-proof window. The `UnstableProofsStore`
traits and storage backends accumulate per-block state diffs and trie node preimages, making them
available for proof generation without re-executing blocks. Provides cursor interfaces for
navigating account and storage tries, a pruner for removing data outside the retention window, and
an initialization job for syncing historical proofs at startup.

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
base-execution-trie = { workspace = true }
```

```rust,ignore
use base_execution_trie::{UnstableProofStoragePruner, MdbxProofsStorage};

let storage = MdbxProofsStorage::open(db_path)?;
let pruner = UnstableProofStoragePruner::new(storage.clone(), retention_blocks);
```

## License

Licensed under the [MIT License](https://github.com/base/base/blob/main/LICENSE).
