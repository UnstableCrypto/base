//! Factory for BaseSecurity tokens. Deploys at `0xBA5E000B || keccak256(deployer, salt)[..8]`.
//!
//! Class invariants enforced here:
//! - Non-default policy_id (not ALLOW_ALL=1, not REJECT_ALL=0)
//! - Non-zero supply_cap
//! - No unknown feature bits

pub mod dispatch;

use alloy::{
    primitives::{Address, B256, U256, keccak256},
    sol_types::SolValue,
};
pub use base_precompiles_contracts::{
    BaseSecurityFactoryError, BaseSecurityFactoryEvent, IBaseSecurityFactory,
};
use base_precompiles_macros::contract;

use crate::{
    BASE_SECURITY_FACTORY_ADDRESS, BASE_SECURITY_PREFIX_BYTES,
    error::{BasePrecompileError, Result},
    plan_2::{
        token::base_security::{SECURITY_ALL_KNOWN, SECURITY_HOLDER_LIMIT, BaseSecurity},
        policy_registry::{ALLOW_ALL_POLICY_ID, REJECT_ALL_POLICY_ID, Base2PolicyRegistry},
    },
};

pub const RESERVED_SIZE: u64 = 1024;

#[contract(addr = BASE_SECURITY_FACTORY_ADDRESS)]
pub struct BaseSecurityFactory {}

pub fn compute_base_security_address(sender: Address, salt: B256) -> (Address, u64) {
    let hash = keccak256((sender, salt).abi_encode());
    let mut padded = [0u8; 8];
    padded.copy_from_slice(&hash[..8]);
    let lower_bytes = u64::from_be_bytes(padded);
    let mut address_bytes = [0u8; 20];
    address_bytes[..12].copy_from_slice(&BASE_SECURITY_PREFIX_BYTES);
    address_bytes[12..].copy_from_slice(&hash[..8]);
    (Address::from(address_bytes), lower_bytes)
}

impl BaseSecurityFactory {
    pub fn initialize(&mut self) -> Result<()> {
        self.__initialize()
    }

    pub fn get_base_security_address(
        &self,
        call: IBaseSecurityFactory::getBaseSecurityAddressCall,
    ) -> Result<Address> {
        let (address, lower_bytes) = compute_base_security_address(call.sender, call.salt);
        if lower_bytes < RESERVED_SIZE {
            return Err(BasePrecompileError::BaseSecurityFactory(
                BaseSecurityFactoryError::address_reserved(),
            ));
        }
        Ok(address)
    }

    pub fn is_base_security(&self, token: Address) -> Result<bool> {
        if !crate::address::is_base_security_prefix(&token) {
            return Ok(false);
        }
        self.storage.with_account_info(token, |info| Ok(!info.is_empty_code_hash()))
    }

    pub fn create_base_security(
        &mut self,
        sender: Address,
        call: IBaseSecurityFactory::createBaseSecurityCall,
    ) -> Result<Address> {
        // Invariant: no unknown feature bits.
        if call.features & !SECURITY_ALL_KNOWN != 0 {
            return Err(BasePrecompileError::BaseSecurityFactory(
                BaseSecurityFactoryError::invalid_features(call.features),
            ));
        }

        // Invariant: non-default policy_id.
        if matches!(call.policyId, ALLOW_ALL_POLICY_ID | REJECT_ALL_POLICY_ID) {
            return Err(BasePrecompileError::BaseSecurityFactory(
                BaseSecurityFactoryError::invalid_policy_id(),
            ));
        }
        if !Base2PolicyRegistry::new().policy_exists_internal(call.policyId)? {
            return Err(BasePrecompileError::BaseSecurityFactory(
                BaseSecurityFactoryError::invalid_policy_id(),
            ));
        }

        // Invariant: non-zero supply_cap.
        if call.supplyCap == U256::ZERO {
            return Err(BasePrecompileError::BaseSecurityFactory(
                BaseSecurityFactoryError::invalid_supply_cap(),
            ));
        }

        let (token_address, lower_bytes) = compute_base_security_address(sender, call.salt);
        if lower_bytes < RESERVED_SIZE {
            return Err(BasePrecompileError::BaseSecurityFactory(
                BaseSecurityFactoryError::address_reserved(),
            ));
        }
        if self.is_base_security(token_address)? {
            return Err(BasePrecompileError::BaseSecurityFactory(
                BaseSecurityFactoryError::token_already_exists(token_address),
            ));
        }

        // holder_limit is 0 unless the bit is set (factory should also accept it from a param,
        // but for now we default to 0 meaning unlimited when the bit is set).
        let holder_limit = 0u64;

        BaseSecurity::from_address(token_address)?.initialize(
            sender,
            &call.name,
            &call.symbol,
            call.decimals,
            call.admin,
            call.policyId,
            call.supplyCap,
            call.features,
            holder_limit,
        )?;

        self.emit_event(BaseSecurityFactoryEvent::TokenCreated(
            IBaseSecurityFactory::TokenCreated {
                token: token_address,
                admin: call.admin,
                name: call.name,
                symbol: call.symbol,
                decimals: call.decimals,
                policyId: call.policyId,
                supplyCap: call.supplyCap,
                features: call.features,
                salt: call.salt,
            },
        ))?;

        Ok(token_address)
    }
}
