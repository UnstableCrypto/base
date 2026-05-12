//! Token movement boundary for the native DEX.

use alloy_primitives::{Address, U256, address};

use super::{BASE_TOKEN_ADDRESS, BaseDexError, IBaseDex};

/// First demo native token supported by the feature-gated DEX scaffold.
pub(crate) const DEMO_TOKEN_A: Address = address!("0000000000000000000000000000000000000dE8");
/// Second demo native token supported by the feature-gated DEX scaffold.
pub(crate) const DEMO_TOKEN_B: Address = address!("0000000000000000000000000000000000000dE9");

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct DexToken;

impl DexToken {
    pub(crate) fn validate(token: Address) -> Result<(), BaseDexError> {
        if token == DEMO_TOKEN_A || token == DEMO_TOKEN_B {
            return Ok(());
        }
        Err(BaseDexError::InvalidToken(IBaseDex::InvalidToken {}))
    }

    pub(crate) fn pull(
        token: Address,
        _from: Address,
        _to: Address,
        _amount: U256,
    ) -> Result<(), BaseDexError> {
        Self::validate_transfer_token(token)
    }

    pub(crate) fn push(
        token: Address,
        _from: Address,
        _to: Address,
        _amount: U256,
    ) -> Result<(), BaseDexError> {
        Self::validate_transfer_token(token)
    }

    fn validate_transfer_token(token: Address) -> Result<(), BaseDexError> {
        if token == BASE_TOKEN_ADDRESS {
            return Ok(());
        }
        Self::validate(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_adapter_accepts_demo_tokens() {
        assert_eq!(DexToken::validate(DEMO_TOKEN_A), Ok(()));
        assert_eq!(DexToken::validate(DEMO_TOKEN_B), Ok(()));
    }

    #[test]
    fn token_adapter_accepts_base_for_transfers() {
        assert_eq!(
            DexToken::pull(BASE_TOKEN_ADDRESS, Address::ZERO, Address::ZERO, U256::ZERO),
            Ok(())
        );
    }

    #[test]
    fn token_adapter_rejects_unknown_tokens() {
        assert!(matches!(
            DexToken::validate(address!("1111111111111111111111111111111111111111")),
            Err(BaseDexError::InvalidToken(_))
        ));
    }
}
