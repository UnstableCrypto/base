//! Factory for BaseAsset tokens. Deploys instances at `0xBA5E000A || keccak256(deployer, salt)[..8]`.

pub mod dispatch;

use alloy::{
    primitives::{Address, B256, U256, keccak256},
    sol_types::SolValue,
};
pub use base_precompiles_contracts::{
    BaseAssetFactoryError, BaseAssetFactoryEvent, IBaseAssetFactory,
};
use base_precompiles_macros::contract;

use crate::{
    BASE_ASSET_FACTORY_ADDRESS, BASE_ASSET_PREFIX_BYTES,
    error::{BasePrecompileError, Result},
    plan_2::token::base_asset::{ASSET_ALL_KNOWN, ASSET_SUPPLY_CAP, BaseAsset},
};

/// Lower-8-byte values below this are reserved for genesis / hardfork tokens.
pub const RESERVED_SIZE: u64 = 1024;

/// Factory singleton.
#[contract(addr = BASE_ASSET_FACTORY_ADDRESS)]
pub struct BaseAssetFactory {}

/// Computes the deterministic BaseAsset address from `(sender, salt)`.
pub fn compute_base_asset_address(sender: Address, salt: B256) -> (Address, u64) {
    let hash = keccak256((sender, salt).abi_encode());
    let mut padded = [0u8; 8];
    padded.copy_from_slice(&hash[..8]);
    let lower_bytes = u64::from_be_bytes(padded);
    let mut address_bytes = [0u8; 20];
    address_bytes[..12].copy_from_slice(&BASE_ASSET_PREFIX_BYTES);
    address_bytes[12..].copy_from_slice(&hash[..8]);
    (Address::from(address_bytes), lower_bytes)
}

impl BaseAssetFactory {
    pub fn initialize(&mut self) -> Result<()> {
        self.__initialize()
    }

    pub fn get_base_asset_address(
        &self,
        call: IBaseAssetFactory::getBaseAssetAddressCall,
    ) -> Result<Address> {
        let (address, lower_bytes) = compute_base_asset_address(call.sender, call.salt);
        if lower_bytes < RESERVED_SIZE {
            return Err(BasePrecompileError::BaseAssetFactory(
                BaseAssetFactoryError::address_reserved(),
            ));
        }
        Ok(address)
    }

    pub fn is_base_asset(&self, token: Address) -> Result<bool> {
        if !crate::address::is_base_asset_prefix(&token) {
            return Ok(false);
        }
        self.storage.with_account_info(token, |info| Ok(!info.is_empty_code_hash()))
    }

    /// Creates a new BaseAsset at the deterministic `(sender, salt)` address.
    ///
    /// Class invariants enforced here:
    /// - No unknown feature bits
    /// - If ASSET_SUPPLY_CAP set, supply_cap must be > 0
    /// - Address not reserved, not already initialized
    pub fn create_base_asset(
        &mut self,
        sender: Address,
        call: IBaseAssetFactory::createBaseAssetCall,
    ) -> Result<Address> {
        if call.features & !ASSET_ALL_KNOWN != 0 {
            return Err(BasePrecompileError::BaseAssetFactory(
                BaseAssetFactoryError::invalid_features(call.features),
            ));
        }

        // When ASSET_SUPPLY_CAP is set the factory-provided supply_cap must be non-zero.
        // We pack supply_cap into the call via `init_supply_cap` — here we derive it: if the
        // bit is set, require a non-zero value to be passed (caller provides it separately via
        // the ABI which does not include supply_cap yet; for now default to U256::MAX when cap
        // bit is set and cap is not explicitly provided).
        // NOTE: in the full implementation this would be a separate `initSupplyCap` parameter.
        // For the initial implementation, ASSET_SUPPLY_CAP without an explicit cap defaults to
        // U256::MAX (uncapped behaviour while still enabling the cap feature).
        let supply_cap = if (call.features & ASSET_SUPPLY_CAP) != 0 {
            U256::MAX
        } else {
            U256::ZERO
        };

        let (token_address, lower_bytes) = compute_base_asset_address(sender, call.salt);
        if lower_bytes < RESERVED_SIZE {
            return Err(BasePrecompileError::BaseAssetFactory(
                BaseAssetFactoryError::address_reserved(),
            ));
        }
        if self.is_base_asset(token_address)? {
            return Err(BasePrecompileError::BaseAssetFactory(
                BaseAssetFactoryError::token_already_exists(token_address),
            ));
        }

        BaseAsset::from_address(token_address)?.initialize(
            sender,
            &call.name,
            &call.symbol,
            call.decimals,
            call.admin,
            call.features,
            supply_cap,
        )?;

        self.emit_event(BaseAssetFactoryEvent::TokenCreated(IBaseAssetFactory::TokenCreated {
            token: token_address,
            admin: call.admin,
            name: call.name,
            symbol: call.symbol,
            decimals: call.decimals,
            features: call.features,
            salt: call.salt,
        }))?;

        Ok(token_address)
    }
}
