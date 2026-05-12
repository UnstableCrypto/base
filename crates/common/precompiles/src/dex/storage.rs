//! Storage layout for the native DEX precompile.

use alloc::string::ToString;

use alloy_evm::EvmInternals;
use alloy_primitives::{Address, Bytes, U256, keccak256};
use alloy_sol_types::{SolEvent, SolValue};
use revm::{
    bytecode::Bytecode,
    primitives::{Log, StorageKey, StorageValue},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PoolState {
    pub reserve_token: u128,
    pub reserve_base: u128,
    pub total_lp_supply: U256,
}

#[derive(Debug)]
pub(crate) struct DexStorage<'a> {
    address: Address,
    internals: EvmInternals<'a>,
}

impl<'a> DexStorage<'a> {
    pub(crate) const fn new(address: Address, internals: EvmInternals<'a>) -> Self {
        Self { address, internals }
    }

    pub(crate) fn pool(&mut self, token: Address) -> Result<PoolState, String> {
        let slot = Self::pool_slot(token);
        Ok(PoolState {
            reserve_token: self
                .read(slot)?
                .try_into()
                .map_err(|_| "reserve token overflow".to_string())?,
            reserve_base: self
                .read(slot + U256::from(1))?
                .try_into()
                .map_err(|_| "reserve base overflow".to_string())?,
            total_lp_supply: self.read(slot + U256::from(2))?,
        })
    }

    pub(crate) fn write_pool(&mut self, token: Address, pool: PoolState) -> Result<(), String> {
        self.ensure_storage_account()?;
        let slot = Self::pool_slot(token);
        self.write(slot, U256::from(pool.reserve_token))?;
        self.write(slot + U256::from(1), U256::from(pool.reserve_base))?;
        self.write(slot + U256::from(2), pool.total_lp_supply)
    }

    pub(crate) fn lp_balance(&mut self, token: Address, user: Address) -> Result<U256, String> {
        self.read(Self::lp_balance_slot(token, user))
    }

    pub(crate) fn write_lp_balance(
        &mut self,
        token: Address,
        user: Address,
        balance: U256,
    ) -> Result<(), String> {
        self.ensure_storage_account()?;
        self.write(Self::lp_balance_slot(token, user), balance)
    }

    pub(crate) fn emit(&mut self, event: impl SolEvent) {
        self.internals.log(Log { address: self.address, data: event.encode_log_data() });
    }

    pub(crate) fn pool_slot(token: Address) -> U256 {
        U256::from_be_bytes(keccak256((token, U256::ZERO).abi_encode()).0)
    }

    pub(crate) fn lp_balance_slot(token: Address, user: Address) -> U256 {
        U256::from_be_bytes(keccak256((token, user, U256::from(1)).abi_encode()).0)
    }

    fn read(&mut self, slot: U256) -> Result<U256, String> {
        self.internals
            .sload(self.address, StorageKey::from(slot))
            .map(|value| value.data)
            .map_err(|error| error.to_string())
    }

    fn write(&mut self, slot: U256, value: U256) -> Result<(), String> {
        self.internals
            .sstore(self.address, StorageKey::from(slot), StorageValue::from(value))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn ensure_storage_account(&mut self) -> Result<(), String> {
        // Keep the DEX account non-empty so storage writes survive EIP-161 empty-account pruning.
        self.internals
            .set_code(self.address, Bytecode::new_legacy(Bytes::from_static(&[0x00])))
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{Address, U256, address, hex};

    use super::*;

    const TOKEN: Address = address!("1111111111111111111111111111111111111111");
    const USER: Address = address!("2222222222222222222222222222222222222222");

    #[test]
    fn pool_slot_is_stable() {
        assert_eq!(
            DexStorage::pool_slot(TOKEN),
            U256::from_be_bytes(hex!(
                "f043c50fe795c69f30b8ff78b84032dc53a9d87ca283ae10a1dacfbb648e83ef"
            ))
        );
    }

    #[test]
    fn lp_balance_slot_is_stable_and_separate_from_pool() {
        let pool_slot = DexStorage::pool_slot(TOKEN);
        let lp_slot = DexStorage::lp_balance_slot(TOKEN, USER);

        assert_ne!(pool_slot, lp_slot);
        assert_ne!(pool_slot + U256::from(1), lp_slot);
        assert_ne!(pool_slot + U256::from(2), lp_slot);
        assert_eq!(
            lp_slot,
            U256::from_be_bytes(hex!(
                "34bbbd721609e5bb442b61609b7b532682b16796bedbe1f30aaeefe6aab8c77c"
            ))
        );
    }
}
