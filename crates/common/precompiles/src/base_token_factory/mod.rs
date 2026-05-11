//! Factory precompile for the BaseToken family. Sibling to [`B20Factory`](crate::b20_factory::B20Factory).
//!
//! Computes deterministic addresses at `0xBA5E_PREFIX || keccak256(deployer, salt)[..8]`,
//! then calls into [`BaseToken::initialize`] to bind the metadata + immutable
//! `FeatureSet` bitmap. Lower-8-byte values < `RESERVED_SIZE` are reserved for
//! genesis / hardfork-deployed tokens.

pub mod dispatch;

use alloy::{
    primitives::{Address, B256, keccak256},
    sol_types::SolValue,
};
pub use base_precompiles_contracts::{
    BaseTokenFactoryError, BaseTokenFactoryEvent, IBaseTokenFactory,
};
use base_precompiles_macros::contract;

use crate::{
    BASE_TOKEN_FACTORY_ADDRESS, BASE_TOKEN_PREFIX_BYTES, BaseBAddressExt,
    base_token::{BaseToken, Feature},
    error::{BasePrecompileError, Result},
};

/// Lower-8-byte value below this is reserved (cannot be created via `createToken`).
pub const RESERVED_SIZE: u64 = 1024;

/// Factory singleton precompile.
#[contract(addr = BASE_TOKEN_FACTORY_ADDRESS)]
pub struct BaseTokenFactory {}

/// Computes the deterministic BaseToken address from `(sender, salt)`. Returns the
/// address and the lower-8-byte numeric value (used for the reserved-range check).
pub fn compute_base_token_address(sender: Address, salt: B256) -> (Address, u64) {
    let hash = keccak256((sender, salt).abi_encode());
    let mut padded = [0u8; 8];
    padded.copy_from_slice(&hash[..8]);
    let lower_bytes = u64::from_be_bytes(padded);
    let mut address_bytes = [0u8; 20];
    address_bytes[..12].copy_from_slice(&BASE_TOKEN_PREFIX_BYTES);
    address_bytes[12..].copy_from_slice(&hash[..8]);
    (Address::from(address_bytes), lower_bytes)
}

impl BaseTokenFactory {
    /// One-shot init.
    pub fn initialize(&mut self) -> Result<()> {
        self.__initialize()
    }

    /// View-only address derivation, mirroring `compute_base_token_address` but reverting
    /// when the derived address is in the reserved range.
    pub fn get_token_address(
        &self,
        call: IBaseTokenFactory::getTokenAddressCall,
    ) -> Result<Address> {
        let (address, lower_bytes) = compute_base_token_address(call.sender, call.salt);
        if lower_bytes < RESERVED_SIZE {
            return Err(BasePrecompileError::BaseTokenFactory(
                BaseTokenFactoryError::address_reserved(),
            ));
        }
        Ok(address)
    }

    /// Returns `true` if `token` is a deployed BaseToken precompile (correct prefix +
    /// initialized).
    pub fn is_base_token(&self, token: Address) -> Result<bool> {
        if !token.is_base_token() {
            return Ok(false);
        }
        self.storage.with_account_info(token, |info| Ok(!info.is_empty_code_hash()))
    }

    /// Deploys a new BaseToken at `(sender, salt)`. Validates that the derived address
    /// is unused and outside the reserved range, that `features` only sets bits known
    /// to the active binary, then calls into `BaseToken::initialize`.
    pub fn create_token(
        &mut self,
        sender: Address,
        call: IBaseTokenFactory::createTokenCall,
    ) -> Result<Address> {
        if call.features & !Feature::ALL_KNOWN != 0 {
            return Err(BaseTokenFactoryError::invalid_features(call.features).into());
        }

        let (token_address, lower_bytes) = compute_base_token_address(sender, call.salt);
        if lower_bytes < RESERVED_SIZE {
            return Err(BasePrecompileError::BaseTokenFactory(
                BaseTokenFactoryError::address_reserved(),
            ));
        }
        if self.is_base_token(token_address)? {
            return Err(BasePrecompileError::BaseTokenFactory(
                BaseTokenFactoryError::token_already_exists(token_address),
            ));
        }

        BaseToken::from_address(token_address)?.initialize(
            sender,
            &call.name,
            &call.symbol,
            call.decimals,
            call.admin,
            call.features,
        )?;

        self.emit_event(BaseTokenFactoryEvent::TokenCreated(IBaseTokenFactory::TokenCreated {
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
