//! Constant-product AMM math for the native DEX.

use alloy_primitives::{U256, uint};

use super::{BaseDexError, IBaseDex};

pub(crate) const FEE_NUMERATOR: U256 = uint!(997_U256);
pub(crate) const FEE_DENOMINATOR: U256 = uint!(1000_U256);
pub(crate) const MINIMUM_LIQUIDITY: U256 = uint!(1000_U256);

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ConstantProduct;

impl ConstantProduct {
    pub(crate) fn amount_out(
        amount_in: U256,
        reserve_in: U256,
        reserve_out: U256,
    ) -> Result<U256, BaseDexError> {
        if amount_in.is_zero() {
            return Err(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}));
        }
        if reserve_in.is_zero() || reserve_out.is_zero() {
            return Err(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}));
        }

        let amount_in_with_fee = amount_in
            .checked_mul(FEE_NUMERATOR)
            .ok_or_else(|| BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?;
        let numerator = amount_in_with_fee
            .checked_mul(reserve_out)
            .ok_or_else(|| BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?;
        let denominator = reserve_in
            .checked_mul(FEE_DENOMINATOR)
            .and_then(|value| value.checked_add(amount_in_with_fee))
            .ok_or_else(|| BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?;

        let amount_out = numerator / denominator;
        if amount_out.is_zero() {
            return Err(BaseDexError::InsufficientOutputAmount(
                IBaseDex::InsufficientOutputAmount {},
            ));
        }
        Ok(amount_out)
    }

    pub(crate) fn initial_liquidity(
        amount_token: U256,
        amount_base: U256,
    ) -> Result<U256, BaseDexError> {
        if amount_token.is_zero() || amount_base.is_zero() {
            return Err(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}));
        }

        let product = amount_token
            .checked_mul(amount_base)
            .ok_or_else(|| BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?;
        Self::sqrt(product)
            .checked_sub(MINIMUM_LIQUIDITY)
            .filter(|liquidity| !liquidity.is_zero())
            .ok_or_else(|| BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}))
    }

    pub(crate) fn minted_liquidity(
        amount_token: U256,
        amount_base: U256,
        reserve_token: U256,
        reserve_base: U256,
        total_supply: U256,
    ) -> Result<U256, BaseDexError> {
        if amount_token.is_zero() || amount_base.is_zero() {
            return Err(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}));
        }
        if reserve_token.is_zero() || reserve_base.is_zero() || total_supply.is_zero() {
            return Err(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}));
        }

        let token_liquidity = amount_token
            .checked_mul(total_supply)
            .and_then(|value| value.checked_div(reserve_token))
            .ok_or_else(|| BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?;
        let base_liquidity = amount_base
            .checked_mul(total_supply)
            .and_then(|value| value.checked_div(reserve_base))
            .ok_or_else(|| BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?;
        let liquidity = token_liquidity.min(base_liquidity);

        if liquidity.is_zero() {
            return Err(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}));
        }
        Ok(liquidity)
    }

    pub(crate) fn burn_amounts(
        liquidity: U256,
        reserve_token: U256,
        reserve_base: U256,
        total_supply: U256,
    ) -> Result<(U256, U256), BaseDexError> {
        if liquidity.is_zero() {
            return Err(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}));
        }
        if reserve_token.is_zero() || reserve_base.is_zero() || total_supply.is_zero() {
            return Err(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}));
        }

        let amount_token = liquidity
            .checked_mul(reserve_token)
            .and_then(|value| value.checked_div(total_supply))
            .ok_or_else(|| BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?;
        let amount_base = liquidity
            .checked_mul(reserve_base)
            .and_then(|value| value.checked_div(total_supply))
            .ok_or_else(|| BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?;

        if amount_token.is_zero() || amount_base.is_zero() {
            return Err(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}));
        }
        Ok((amount_token, amount_base))
    }

    fn sqrt(value: U256) -> U256 {
        if value <= U256::from(3) {
            return if value.is_zero() { U256::ZERO } else { U256::from(1) };
        }

        let mut root = value;
        let mut candidate = value / U256::from(2) + U256::from(1);
        while candidate < root {
            root = candidate;
            candidate = (value / candidate + candidate) / U256::from(2);
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{U256, uint};

    use super::*;

    #[test]
    fn amount_out_matches_constant_product_fee() {
        let amount_in = uint!(1000_U256);
        let reserve_in = uint!(100_000_U256);
        let reserve_out = uint!(100_000_U256);

        let expected = amount_in
            .checked_mul(FEE_NUMERATOR)
            .and_then(|amount_in_with_fee| {
                amount_in_with_fee
                    .checked_mul(reserve_out)
                    .zip(
                        reserve_in
                            .checked_mul(FEE_DENOMINATOR)
                            .and_then(|denominator| denominator.checked_add(amount_in_with_fee)),
                    )
                    .map(|(numerator, denominator)| numerator / denominator)
            })
            .expect("test values do not overflow");

        let actual = ConstantProduct::amount_out(amount_in, reserve_in, reserve_out).unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn amount_out_rejects_zero_input() {
        assert!(matches!(
            ConstantProduct::amount_out(U256::ZERO, U256::from(1), U256::from(1)),
            Err(BaseDexError::InvalidAmount(_))
        ));
    }

    #[test]
    fn amount_out_rejects_empty_reserves() {
        assert!(matches!(
            ConstantProduct::amount_out(U256::from(1), U256::ZERO, U256::from(1)),
            Err(BaseDexError::InsufficientLiquidity(_))
        ));
    }

    #[test]
    fn initial_liquidity_subtracts_minimum_liquidity() {
        assert_eq!(
            ConstantProduct::initial_liquidity(U256::from(1_000_000), U256::from(1_000_000)),
            Ok(U256::from(999_000))
        );
    }

    #[test]
    fn burn_amounts_are_pro_rata() {
        assert_eq!(
            ConstantProduct::burn_amounts(
                U256::from(10),
                U256::from(1_000),
                U256::from(2_000),
                U256::from(100),
            ),
            Ok((U256::from(100), U256::from(200)))
        );
    }
}
