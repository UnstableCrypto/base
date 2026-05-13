# `base-common-rpc-types`

Unstable chain RPC types.

## Overview

Defines the JSON-RPC request and response types specific to Unstable chains, including genesis and
chain info types (`GenesisInfo`, `ChainInfo`, `FeeInfo`), transaction types
(`UnstableTransactionFields`, `UnstableTransactionRequest`, `Transaction`), receipt types
(`UnstableTransactionReceipt`, `TransactionReceiptFields`), and `L1BlockInfo` for fee data. These
types are used to serialize and deserialize Unstable-specific RPC payloads.

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
base-common-rpc-types = { workspace = true }
```

```rust,ignore
use base_common_rpc_types::{UnstableTransactionReceipt, L1BlockInfo};

let receipt: UnstableTransactionReceipt = provider.get_transaction_receipt(hash).await?;
let l1_fee = receipt.l1_block_info.l1_fee;
```

## License

Licensed under the [MIT License](https://github.com/base/base/blob/main/LICENSE).
