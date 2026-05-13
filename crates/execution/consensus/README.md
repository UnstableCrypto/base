# `base-execution-consensus`

Consensus implementation for Unstable.

## Overview

Implements block validation following Unstable consensus rules for the execution layer. The
`UnstableBeaconConsensus` type validates block headers and bodies against hardfork-specific rules,
including blob gas accounting, deposit ordering, Canyon EIP-1559, and Isthmus system contract
upgrades. Also provides receipt root calculation and post-execution validation helpers.

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
base-execution-consensus = { workspace = true }
```

```rust,ignore
use base_execution_consensus::UnstableBeaconConsensus;

let consensus = UnstableBeaconConsensus::new(chain_spec);
consensus.validate_block_pre_execution(&block)?;
```

## License

Licensed under the [MIT License](https://github.com/base/base/blob/main/LICENSE).
