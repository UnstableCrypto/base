//! Factory for BaseStablecoin tokens. Deploys at `0xBA5E000C || keccak256(deployer, salt)[..8]`.
//!
//! Class invariants enforced here:
//! - Valid 3-letter uppercase ISO 4217 currency code
//! - Non-default policy_id (not ALLOW_ALL=1, not REJECT_ALL=0)
//! - No unknown feature bits

pub mod dispatch;

use alloy::{
    primitives::{Address, B256, keccak256},
    sol_types::SolValue,
};
pub use base_precompiles_contracts::{
    BaseStablecoinFactoryError, BaseStablecoinFactoryEvent, IBaseStablecoinFactory,
};
use base_precompiles_macros::contract;

use crate::{
    BASE_STABLECOIN_FACTORY_ADDRESS, BASE_STABLECOIN_PREFIX_BYTES,
    error::{BasePrecompileError, Result},
    plan_2::{
        stablecoin::{STABLECOIN_ALL_KNOWN, BaseStablecoin},
        policy_registry::{ALLOW_ALL_POLICY_ID, REJECT_ALL_POLICY_ID, Base2PolicyRegistry},
    },
};

pub const RESERVED_SIZE: u64 = 1024;

#[contract(addr = BASE_STABLECOIN_FACTORY_ADDRESS)]
pub struct BaseStablecoinFactory {}

pub fn compute_base_stablecoin_address(sender: Address, salt: B256) -> (Address, u64) {
    let hash = keccak256((sender, salt).abi_encode());
    let mut padded = [0u8; 8];
    padded.copy_from_slice(&hash[..8]);
    let lower_bytes = u64::from_be_bytes(padded);
    let mut address_bytes = [0u8; 20];
    address_bytes[..12].copy_from_slice(&BASE_STABLECOIN_PREFIX_BYTES);
    address_bytes[12..].copy_from_slice(&hash[..8]);
    (Address::from(address_bytes), lower_bytes)
}

/// Validates that `currency` is a 3-byte uppercase ASCII string (ISO 4217 format).
pub fn is_valid_currency(currency: &str) -> bool {
    let bytes = currency.as_bytes();
    bytes.len() == 3 && bytes.iter().all(|b| b.is_ascii_uppercase())
}

impl BaseStablecoinFactory {
    pub fn initialize(&mut self) -> Result<()> {
        self.__initialize()
    }

    pub fn get_base_stablecoin_address(
        &self,
        call: IBaseStablecoinFactory::getBaseStablecoinAddressCall,
    ) -> Result<Address> {
        let (address, lower_bytes) = compute_base_stablecoin_address(call.sender, call.salt);
        if lower_bytes < RESERVED_SIZE {
            return Err(BasePrecompileError::BaseStablecoinFactory(
                BaseStablecoinFactoryError::address_reserved(),
            ));
        }
        Ok(address)
    }

    pub fn is_base_stablecoin(&self, token: Address) -> Result<bool> {
        if !crate::address::is_base_stablecoin_prefix(&token) {
            return Ok(false);
        }
        self.storage.with_account_info(token, |info| Ok(!info.is_empty_code_hash()))
    }

    pub fn create_base_stablecoin(
        &mut self,
        sender: Address,
        call: IBaseStablecoinFactory::createBaseStablecoinCall,
    ) -> Result<Address> {
        // Invariant: valid ISO 4217 currency code.
        if !is_valid_currency(&call.currency) {
            return Err(BasePrecompileError::BaseStablecoinFactory(
                BaseStablecoinFactoryError::invalid_currency(),
            ));
        }

        // Invariant: no unknown feature bits.
        if call.features & !STABLECOIN_ALL_KNOWN != 0 {
            return Err(BasePrecompileError::BaseStablecoinFactory(
                BaseStablecoinFactoryError::invalid_features(call.features),
            ));
        }

        // Invariant: non-default policy_id.
        if matches!(call.policyId, ALLOW_ALL_POLICY_ID | REJECT_ALL_POLICY_ID) {
            return Err(BasePrecompileError::BaseStablecoinFactory(
                BaseStablecoinFactoryError::invalid_policy_id(),
            ));
        }
        if !Base2PolicyRegistry::new().policy_exists_internal(call.policyId)? {
            return Err(BasePrecompileError::BaseStablecoinFactory(
                BaseStablecoinFactoryError::invalid_policy_id(),
            ));
        }

        let (token_address, lower_bytes) = compute_base_stablecoin_address(sender, call.salt);
        if lower_bytes < RESERVED_SIZE {
            return Err(BasePrecompileError::BaseStablecoinFactory(
                BaseStablecoinFactoryError::address_reserved(),
            ));
        }
        if self.is_base_stablecoin(token_address)? {
            return Err(BasePrecompileError::BaseStablecoinFactory(
                BaseStablecoinFactoryError::token_already_exists(token_address),
            ));
        }

        BaseStablecoin::from_address(token_address)?.initialize(
            sender,
            &call.name,
            &call.symbol,
            call.decimals,
            call.admin,
            &call.currency,
            call.policyId,
            call.features,
        )?;

        self.emit_event(BaseStablecoinFactoryEvent::TokenCreated(
            IBaseStablecoinFactory::TokenCreated {
                token: token_address,
                admin: call.admin,
                name: call.name,
                symbol: call.symbol,
                decimals: call.decimals,
                currency: call.currency,
                policyId: call.policyId,
                features: call.features,
                salt: call.salt,
            },
        ))?;

        Ok(token_address)
    }
}
