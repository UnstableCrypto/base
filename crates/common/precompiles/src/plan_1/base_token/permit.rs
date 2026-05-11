//! EIP-2612 `permit(...)` / `nonces(...)` / `DOMAIN_SEPARATOR()`.
//!
//! Gated by `Feature::Permit` at dispatch. The domain separator is computed dynamically
//! from the token name + chain id + token address so that permits cannot be replayed
//! across chains or across tokens.

use std::sync::LazyLock;

use alloy::{
    primitives::{B256, U256, keccak256},
    sol_types::SolValue,
};
use base_precompiles_contracts::{BaseTokenError, BaseTokenEvent, IBaseToken};

use crate::{
    base_token::BaseToken,
    error::{BasePrecompileError, Result},
    storage::Handler,
};

/// EIP-2612 permit typehash.
pub static PERMIT_TYPEHASH: LazyLock<B256> = LazyLock::new(|| {
    keccak256(b"Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)")
});

/// EIP-712 domain typehash.
pub static EIP712_DOMAIN_TYPEHASH: LazyLock<B256> = LazyLock::new(|| {
    keccak256(b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")
});

/// EIP-712 version hash (`keccak256("1")`).
pub static VERSION_HASH: LazyLock<B256> = LazyLock::new(|| keccak256(b"1"));

impl BaseToken {
    /// EIP-2612 `nonces(owner)`.
    pub fn nonces(&self, call: IBaseToken::noncesCall) -> Result<U256> {
        self.permit_nonces[call.owner].read()
    }

    /// EIP-712 domain separator, computed from token name + chain id + token address.
    pub fn domain_separator(&self) -> Result<B256> {
        let name = self.name()?;
        let name_hash = self.storage.keccak256(name.as_bytes())?;
        let chain_id = U256::from(self.storage.chain_id());
        let encoded = (*EIP712_DOMAIN_TYPEHASH, name_hash, *VERSION_HASH, chain_id, self.address)
            .abi_encode();
        self.storage.keccak256(&encoded)
    }

    /// EIP-2612 `permit`. Allowed even when the token is paused.
    pub fn permit(&mut self, call: IBaseToken::permitCall) -> Result<()> {
        if self.storage.timestamp() > call.deadline {
            return Err(BaseTokenError::permit_expired().into());
        }

        let nonce = self.permit_nonces[call.owner].read()?;
        let struct_hash = self.storage.keccak256(
            &(*PERMIT_TYPEHASH, call.owner, call.spender, call.value, nonce, call.deadline)
                .abi_encode(),
        )?;
        let domain_separator = self.domain_separator()?;
        let digest = self.storage.keccak256(
            &[&[0x19, 0x01], domain_separator.as_slice(), struct_hash.as_slice()].concat(),
        )?;

        let recovered = self
            .storage
            .recover_signer(digest, call.v, call.r, call.s)?
            .ok_or(BaseTokenError::invalid_signature())?;
        if recovered != call.owner {
            return Err(BaseTokenError::invalid_signature().into());
        }

        self.permit_nonces[call.owner].write(
            nonce.checked_add(U256::from(1)).ok_or(BasePrecompileError::under_overflow())?,
        )?;
        self.allowances[call.owner][call.spender].write(call.value)?;
        self.emit_event(BaseTokenEvent::Approval(IBaseToken::Approval {
            owner: call.owner,
            spender: call.spender,
            amount: call.value,
        }))
    }
}
