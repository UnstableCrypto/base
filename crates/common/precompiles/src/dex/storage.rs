//! Storage layout for the native DEX precompile.

use alloc::string::{String, ToString};
use core::fmt;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DexStorageError {
    Database(String),
    ReserveTokenOverflow,
    ReserveBaseOverflow,
}

impl fmt::Display for DexStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::ReserveTokenOverflow => f.write_str("reserve token overflow"),
            Self::ReserveBaseOverflow => f.write_str("reserve base overflow"),
        }
    }
}

#[derive(Debug)]
pub(crate) struct DexStorage<'a> {
    address: Address,
    internals: EvmInternals<'a>,
    storage_account_initialized: bool,
}

impl<'a> DexStorage<'a> {
    pub(crate) const fn new(address: Address, internals: EvmInternals<'a>) -> Self {
        Self { address, internals, storage_account_initialized: false }
    }

    pub(crate) fn pool(&mut self, token: Address) -> Result<PoolState, DexStorageError> {
        Ok(PoolState {
            reserve_token: self
                .read(Self::pool_reserve_token_slot(token))?
                .try_into()
                .map_err(|_| DexStorageError::ReserveTokenOverflow)?,
            reserve_base: self
                .read(Self::pool_reserve_base_slot(token))?
                .try_into()
                .map_err(|_| DexStorageError::ReserveBaseOverflow)?,
            total_lp_supply: self.read(Self::pool_total_supply_slot(token))?,
        })
    }

    pub(crate) fn write_pool(
        &mut self,
        token: Address,
        pool: PoolState,
    ) -> Result<(), DexStorageError> {
        self.ensure_storage_account()?;
        self.write(Self::pool_reserve_token_slot(token), U256::from(pool.reserve_token))?;
        self.write(Self::pool_reserve_base_slot(token), U256::from(pool.reserve_base))?;
        self.write(Self::pool_total_supply_slot(token), pool.total_lp_supply)
    }

    pub(crate) fn lp_balance(
        &mut self,
        token: Address,
        user: Address,
    ) -> Result<U256, DexStorageError> {
        self.read(Self::lp_balance_slot(token, user))
    }

    pub(crate) fn write_lp_balance(
        &mut self,
        token: Address,
        user: Address,
        balance: U256,
    ) -> Result<(), DexStorageError> {
        self.ensure_storage_account()?;
        self.write(Self::lp_balance_slot(token, user), balance)
    }

    pub(crate) fn emit(&mut self, event: impl SolEvent) {
        self.internals.log(Log { address: self.address, data: event.encode_log_data() });
    }

    pub(crate) fn pool_reserve_token_slot(token: Address) -> U256 {
        U256::from_be_bytes(keccak256((token, U256::ZERO).abi_encode()).0)
    }

    pub(crate) fn pool_reserve_base_slot(token: Address) -> U256 {
        U256::from_be_bytes(keccak256((token, U256::from(1)).abi_encode()).0)
    }

    pub(crate) fn pool_total_supply_slot(token: Address) -> U256 {
        U256::from_be_bytes(keccak256((token, U256::from(2)).abi_encode()).0)
    }

    pub(crate) fn lp_balance_slot(token: Address, user: Address) -> U256 {
        U256::from_be_bytes(keccak256((token, user, U256::from(1)).abi_encode()).0)
    }

    fn read(&mut self, slot: U256) -> Result<U256, DexStorageError> {
        self.internals
            .sload(self.address, StorageKey::from(slot))
            .map(|value| value.data)
            .map_err(|error| DexStorageError::Database(error.to_string()))
    }

    fn write(&mut self, slot: U256, value: U256) -> Result<(), DexStorageError> {
        self.internals
            .sstore(self.address, StorageKey::from(slot), StorageValue::from(value))
            .map(|_| ())
            .map_err(|error| DexStorageError::Database(error.to_string()))
    }

    fn ensure_storage_account(&mut self) -> Result<(), DexStorageError> {
        if self.storage_account_initialized {
            return Ok(());
        }

        // Keep the DEX account non-empty so storage writes survive EIP-161 empty-account pruning.
        self.internals
            .set_code(self.address, Bytecode::new_legacy(Bytes::from_static(&[0x00])))
            .map_err(|error| DexStorageError::Database(error.to_string()))?;
        self.storage_account_initialized = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{Address, U256, address, hex};

    use super::*;

    const TOKEN: Address = address!("1111111111111111111111111111111111111111");
    const USER: Address = address!("2222222222222222222222222222222222222222");

    #[test]
    fn pool_slots_are_stable_and_independent() {
        assert_eq!(
            DexStorage::pool_reserve_token_slot(TOKEN),
            U256::from_be_bytes(hex!(
                "f043c50fe795c69f30b8ff78b84032dc53a9d87ca283ae10a1dacfbb648e83ef"
            ))
        );

        let reserve_token_slot = DexStorage::pool_reserve_token_slot(TOKEN);
        let reserve_base_slot = DexStorage::pool_reserve_base_slot(TOKEN);
        let total_supply_slot = DexStorage::pool_total_supply_slot(TOKEN);

        assert_ne!(reserve_token_slot + U256::from(1), reserve_base_slot);
        assert_ne!(reserve_token_slot + U256::from(2), total_supply_slot);
        assert_ne!(reserve_base_slot, total_supply_slot);
    }

    #[test]
    fn lp_balance_slot_is_stable_and_separate_from_pool() {
        let reserve_token_slot = DexStorage::pool_reserve_token_slot(TOKEN);
        let reserve_base_slot = DexStorage::pool_reserve_base_slot(TOKEN);
        let total_supply_slot = DexStorage::pool_total_supply_slot(TOKEN);
        let lp_slot = DexStorage::lp_balance_slot(TOKEN, USER);

        assert_ne!(reserve_token_slot, lp_slot);
        assert_ne!(reserve_base_slot, lp_slot);
        assert_ne!(total_supply_slot, lp_slot);
        assert_eq!(
            lp_slot,
            U256::from_be_bytes(hex!(
                "34bbbd721609e5bb442b61609b7b532682b16796bedbe1f30aaeefe6aab8c77c"
            ))
        );
    }
}
