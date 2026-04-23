//! XEN-shape payload: stresses the storage trie and drives elevated account
//! creation by mirroring XEN's `bulkClaimRank` shape. One transaction loops
//! `proxies_per_tx` CREATE2 deployments of a `Worker` contract, and each
//! worker writes a per-msg.sender storage slot in an underlying `State`
//! contract — N new account-trie entries plus N unique storage-trie writes
//! per transaction, with no merging in `HashedPostState`.
//!
//! Sources live in `crates/infra/load-tests/contracts/xen/`. See the README
//! there for the exact compile + re-embed steps.
//!
//! Compiled with solc 0.8.28, optimizer enabled (200 runs), `evm_version` paris.

use alloy_network::TransactionBuilder;
use alloy_primitives::{Address, Bytes, U256, hex};
use alloy_rpc_types::TransactionRequest;
use alloy_sol_types::{SolCall, sol};

use super::Payload;
use crate::workload::SeededRng;

sol! {
    interface IXenMulticall {
        function bulkClaimRank(address state, uint256 term, uint256 baseSalt, uint256 count) external;
    }
}

/// Embedded bytecodes for the XEN-shape contracts.
///
/// Compiled with:
/// ```text
/// cd crates/infra/load-tests/contracts/xen && forge build
/// ```
/// (forge resolves solc 0.8.28 itself via the bundled `foundry.toml`.)
#[derive(Debug, Clone, Copy)]
pub struct XenContracts;

impl XenContracts {
    /// Deployment bytecode for `State.sol`.
    pub const STATE_BYTECODE: &'static [u8] = &hex!(
        "6080604052348015600f57600080fd5b506101018061001f6000396000f3fe6080604052348015600f57600080fd5b506004361060325760003560e01c806325691de41460375780639ff054df146066575b600080fd5b605460423660046085565b60006020819052908152604090205481565b60405190815260200160405180910390f35b6083607136600460b3565b33600090815260208190526040902055565b005b600060208284031215609657600080fd5b81356001600160a01b038116811460ac57600080fd5b9392505050565b60006020828403121560c457600080fd5b503591905056fea26469706673582212209da2b8a502c840944301ded5a8b3325c92869ed09b0846260cf14e1595bf4b6c64736f6c634300081c0033"
    );

    /// Deployment bytecode for `Worker.sol`.
    ///
    /// Informational only: `MULTICALL_BYTECODE` already embeds the worker's
    /// creation code via `type(Worker).creationCode`. Kept here so users can
    /// independently verify or deploy a standalone worker.
    pub const WORKER_BYTECODE: &'static [u8] = &hex!(
        "6080604052348015600f57600080fd5b5060405161010f38038061010f833981016040819052602c91608a565b604051639ff054df60e01b8152600481018290526001600160a01b03831690639ff054df90602401600060405180830381600087803b158015606d57600080fd5b505af11580156080573d6000803e3d6000fd5b50505050505060c2565b60008060408385031215609c57600080fd5b82516001600160a01b038116811460b257600080fd5b6020939093015192949293505050565b603f806100d06000396000f3fe6080604052600080fdfea2646970667358221220fc04d189326c845d2f4f1389fdb2d824259e477b8cbee153fc31a76994f28ece64736f6c634300081c0033"
    );

    /// Deployment bytecode for `Multicall.sol`.
    pub const MULTICALL_BYTECODE: &'static [u8] = &hex!(
        "6080604052348015600f57600080fd5b506102f88061001f6000396000f3fe608060405234801561001057600080fd5b506004361061002b5760003560e01c8063d9eac7f714610030575b600080fd5b61004361003e36600461011f565b610045565b005b60006040518060200161005790610112565b601f1982820381018352601f9091011660408181526001600160a01b0388166020830152810186905260600160408051601f19818403018152908290526100a19291602001610196565b604051602081830303815290604052905060005b8281101561010a576040805160208101869052908101829052600090606001604051602081830303815290604052805190602001209050808351602085016000f58061010057600080fd5b50506001016100b5565b505050505050565b61010f806101b483390190565b6000806000806080858703121561013557600080fd5b84356001600160a01b038116811461014c57600080fd5b966020860135965060408601359560600135945092505050565b6000815160005b81811015610187576020818501810151868301520161016d565b50600093019283525090919050565b60006101ab6101a58386610166565b84610166565b94935050505056fe6080604052348015600f57600080fd5b5060405161010f38038061010f833981016040819052602c91608a565b604051639ff054df60e01b8152600481018290526001600160a01b03831690639ff054df90602401600060405180830381600087803b158015606d57600080fd5b505af11580156080573d6000803e3d6000fd5b50505050505060c2565b60008060408385031215609c57600080fd5b82516001600160a01b038116811460b257600080fd5b6020939093015192949293505050565b603f806100d06000396000f3fe6080604052600080fdfea2646970667358221220fc04d189326c845d2f4f1389fdb2d824259e477b8cbee153fc31a76994f28ece64736f6c634300081c0033a26469706673582212205ae8c736d7bf26d069401dc6fa9b39b4c7c8cb0a49547fdc9f0f27af8cb3295064736f6c634300081c0033"
    );

    /// Returns `State`'s deployment bytecode as a `Bytes`.
    pub const fn state_deployment_bytecode() -> Bytes {
        Bytes::from_static(Self::STATE_BYTECODE)
    }

    /// Returns `Worker`'s deployment bytecode as a `Bytes`.
    pub const fn worker_deployment_bytecode() -> Bytes {
        Bytes::from_static(Self::WORKER_BYTECODE)
    }

    /// Returns `Multicall`'s deployment bytecode as a `Bytes`.
    pub const fn multicall_deployment_bytecode() -> Bytes {
        Bytes::from_static(Self::MULTICALL_BYTECODE)
    }
}

/// Per-worker gas estimate used both for the runtime gas-limit and the
/// scenario-level average-gas estimator. ~25k for the worker deploy + ~5k
/// for the per-worker storage write + ~5k overhead.
pub const XEN_GAS_PER_PROXY: u64 = 35_000;

/// Per-tx fixed overhead added on top of `proxies_per_tx * XEN_GAS_PER_PROXY`.
pub const XEN_GAS_BASE: u64 = 50_000;

/// Generates `Multicall.bulkClaimRank` transactions.
#[derive(Debug, Clone)]
pub struct XenPayload {
    multicall: Address,
    state: Address,
    term: u64,
    proxies_per_tx: u32,
}

impl XenPayload {
    /// Creates a new XEN payload.
    pub const fn new(multicall: Address, state: Address, term: u64, proxies_per_tx: u32) -> Self {
        Self { multicall, state, term, proxies_per_tx }
    }

    /// Returns the gas limit used per generated transaction.
    pub const fn gas_limit(&self) -> u64 {
        XEN_GAS_BASE + (self.proxies_per_tx as u64 * XEN_GAS_PER_PROXY)
    }
}

impl Payload for XenPayload {
    fn name(&self) -> &'static str {
        "xen"
    }

    fn generate(&self, rng: &mut SeededRng, _from: Address, _to: Address) -> TransactionRequest {
        let base_salt: u64 = rng.random();
        let call = IXenMulticall::bulkClaimRankCall {
            state: self.state,
            term: U256::from(self.term),
            baseSalt: U256::from(base_salt),
            count: U256::from(self.proxies_per_tx),
        };

        TransactionRequest::default()
            .with_to(self.multicall)
            .with_input(Bytes::from(call.abi_encode()))
            .with_gas_limit(self.gas_limit())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytecodes_are_non_empty() {
        assert!(!XenContracts::STATE_BYTECODE.is_empty());
        assert!(!XenContracts::WORKER_BYTECODE.is_empty());
        assert!(!XenContracts::MULTICALL_BYTECODE.is_empty());
    }

    #[test]
    fn gas_limit_scales_linearly() {
        let p = XenPayload::new(Address::repeat_byte(1), Address::repeat_byte(2), 1000, 10);
        assert_eq!(p.gas_limit(), 50_000 + 10 * 35_000);

        let p2 = XenPayload::new(Address::repeat_byte(1), Address::repeat_byte(2), 1000, 0);
        assert_eq!(p2.gas_limit(), 50_000);
    }

    #[test]
    fn generate_produces_well_formed_calldata() {
        let multicall = Address::repeat_byte(0xaa);
        let state = Address::repeat_byte(0xbb);
        let payload = XenPayload::new(multicall, state, 1000, 7);

        let mut rng = SeededRng::new(42);
        let tx = payload.generate(&mut rng, Address::ZERO, Address::ZERO);

        assert_eq!(tx.to.and_then(|k| k.to().copied()), Some(multicall));
        assert_eq!(tx.gas, Some(50_000 + 7 * 35_000));

        let input = tx.input.input().expect("calldata present").clone();
        // selector (4) + 4 * 32-byte args = 132 bytes.
        assert_eq!(input.len(), 4 + 4 * 32);

        let decoded = IXenMulticall::bulkClaimRankCall::abi_decode(&input).expect("decodable");
        assert_eq!(decoded.state, state);
        assert_eq!(decoded.term, U256::from(1000u64));
        assert_eq!(decoded.count, U256::from(7u64));
    }
}
