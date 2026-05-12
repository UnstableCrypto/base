//! Native DEX precompile implementation.

use alloy_primitives::{Address, U256};
use tracing::warn;

mod abi;
pub use abi::{BASE_DEX_ADDRESS, IBaseDex};
use abi::{BASE_TOKEN_ADDRESS, BaseDexError, IBaseDexCalls};

mod dispatch;
pub use dispatch::BaseDexPrecompile;

mod gas;
use gas::DexGasMeter;

mod math;
use math::{ConstantProduct, MINIMUM_LIQUIDITY};

mod storage;
use storage::{DexStorage, DexStorageError, PoolState};

mod token;
use token::DexToken;

#[derive(Debug)]
struct BaseDex<'a> {
    storage: DexStorage<'a>,
}

impl<'a> BaseDex<'a> {
    const fn new(storage: DexStorage<'a>) -> Self {
        Self { storage }
    }

    const fn base_token(&self) -> Address {
        BASE_TOKEN_ADDRESS
    }

    fn get_pool(&mut self, token: Address) -> Result<PoolState, BaseDexError> {
        self.storage.pool(token).map_err(|error| Self::storage_error("pool", error))
    }

    fn quote_exact_input(
        &mut self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<U256, BaseDexError> {
        Self::validate_path(token_in, token_out)?;
        self.validate_swap_tokens(token_in, token_out)?;

        if token_in == self.base_token() {
            let pool = self.get_pool(token_out)?;
            return ConstantProduct::amount_out(
                amount_in,
                U256::from(pool.reserve_base),
                U256::from(pool.reserve_token),
            );
        }

        if token_out == self.base_token() {
            let pool = self.get_pool(token_in)?;
            return ConstantProduct::amount_out(
                amount_in,
                U256::from(pool.reserve_token),
                U256::from(pool.reserve_base),
            );
        }

        let base_out = self.quote_exact_input(token_in, self.base_token(), amount_in)?;
        self.quote_exact_input(self.base_token(), token_out, base_out)
    }

    fn add_liquidity(
        &mut self,
        caller: Address,
        token: Address,
        amount_token: U256,
        amount_base: U256,
        to: Address,
    ) -> Result<U256, BaseDexError> {
        if token == self.base_token() {
            return Err(BaseDexError::InvalidToken(IBaseDex::InvalidToken {}));
        }

        DexToken::validate(token)?;
        DexToken::pull(token, caller, BASE_DEX_ADDRESS, amount_token)?;
        DexToken::pull(self.base_token(), caller, BASE_DEX_ADDRESS, amount_base)?;

        let mut pool = self.get_pool(token)?;
        let (liquidity, supply_delta) = if pool.total_lp_supply.is_zero() {
            let liquidity = ConstantProduct::initial_liquidity(amount_token, amount_base)?;
            let supply_delta = liquidity
                .checked_add(MINIMUM_LIQUIDITY)
                .ok_or(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?;
            (liquidity, supply_delta)
        } else {
            let liquidity = ConstantProduct::minted_liquidity(
                amount_token,
                amount_base,
                U256::from(pool.reserve_token),
                U256::from(pool.reserve_base),
                pool.total_lp_supply,
            )?;
            (liquidity, liquidity)
        };

        pool.reserve_token = Self::u128_amount(
            U256::from(pool.reserve_token)
                .checked_add(amount_token)
                .ok_or(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?,
        )?;
        pool.reserve_base = Self::u128_amount(
            U256::from(pool.reserve_base)
                .checked_add(amount_base)
                .ok_or(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?,
        )?;
        pool.total_lp_supply = pool
            .total_lp_supply
            .checked_add(supply_delta)
            .ok_or(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?;

        let lp_balance = self
            .storage
            .lp_balance(token, to)
            .map_err(|error| Self::storage_error("lp_balance", error))?
            .checked_add(liquidity)
            .ok_or(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?;
        self.storage
            .write_lp_balance(token, to, lp_balance)
            .map_err(|error| Self::storage_error("write_lp_balance", error))?;
        self.storage
            .write_pool(token, pool)
            .map_err(|error| Self::storage_error("write_pool", error))?;
        self.storage.emit(IBaseDex::Mint {
            sender: caller,
            token,
            amountToken: amount_token,
            amountBase: amount_base,
            liquidity,
            to,
        });

        Ok(liquidity)
    }

    fn remove_liquidity(
        &mut self,
        caller: Address,
        token: Address,
        liquidity: U256,
        to: Address,
    ) -> Result<(U256, U256), BaseDexError> {
        if token == self.base_token() {
            return Err(BaseDexError::InvalidToken(IBaseDex::InvalidToken {}));
        }
        if liquidity.is_zero() {
            return Err(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}));
        }

        DexToken::validate(token)?;

        let mut pool = self.get_pool(token)?;
        let lp_balance = self
            .storage
            .lp_balance(token, caller)
            .map_err(|error| Self::storage_error("lp_balance", error))?;
        if lp_balance < liquidity {
            return Err(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}));
        }

        let (amount_token, amount_base) = ConstantProduct::burn_amounts(
            liquidity,
            U256::from(pool.reserve_token),
            U256::from(pool.reserve_base),
            pool.total_lp_supply,
        )?;

        DexToken::push(token, BASE_DEX_ADDRESS, to, amount_token)?;
        DexToken::push(self.base_token(), BASE_DEX_ADDRESS, to, amount_base)?;

        pool.reserve_token = Self::u128_amount(
            U256::from(pool.reserve_token)
                .checked_sub(amount_token)
                .ok_or(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}))?,
        )?;
        pool.reserve_base = Self::u128_amount(
            U256::from(pool.reserve_base)
                .checked_sub(amount_base)
                .ok_or(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}))?,
        )?;
        pool.total_lp_supply = pool
            .total_lp_supply
            .checked_sub(liquidity)
            .ok_or(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}))?;

        let updated_lp_balance = lp_balance
            .checked_sub(liquidity)
            .ok_or(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}))?;
        self.storage
            .write_lp_balance(token, caller, updated_lp_balance)
            .map_err(|error| Self::storage_error("write_lp_balance", error))?;
        self.storage
            .write_pool(token, pool)
            .map_err(|error| Self::storage_error("write_pool", error))?;
        self.storage.emit(IBaseDex::Burn {
            sender: caller,
            token,
            amountToken: amount_token,
            amountBase: amount_base,
            liquidity,
            to,
        });

        Ok((amount_token, amount_base))
    }

    fn swap_exact_tokens_for_tokens(
        &mut self,
        caller: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        min_amount_out: U256,
        to: Address,
    ) -> Result<U256, BaseDexError> {
        let (amount_out, intermediate_base_out) =
            self.quote_exact_input_with_intermediate(token_in, token_out, amount_in)?;
        if amount_out < min_amount_out {
            return Err(BaseDexError::InsufficientOutputAmount(
                IBaseDex::InsufficientOutputAmount {},
            ));
        }

        DexToken::pull(token_in, caller, BASE_DEX_ADDRESS, amount_in)?;
        DexToken::push(token_out, BASE_DEX_ADDRESS, to, amount_out)?;
        self.apply_swap(token_in, token_out, amount_in, amount_out, intermediate_base_out)?;
        self.storage.emit(IBaseDex::Swap {
            sender: caller,
            tokenIn: token_in,
            tokenOut: token_out,
            amountIn: amount_in,
            amountOut: amount_out,
            to,
        });

        Ok(amount_out)
    }

    fn quote_exact_input_with_intermediate(
        &mut self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<(U256, Option<U256>), BaseDexError> {
        Self::validate_path(token_in, token_out)?;
        self.validate_swap_tokens(token_in, token_out)?;

        if token_in == self.base_token() || token_out == self.base_token() {
            return self
                .quote_exact_input(token_in, token_out, amount_in)
                .map(|amount_out| (amount_out, None));
        }

        let base_out = self.quote_exact_input(token_in, self.base_token(), amount_in)?;
        let amount_out = self.quote_exact_input(self.base_token(), token_out, base_out)?;
        Ok((amount_out, Some(base_out)))
    }

    fn apply_swap(
        &mut self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        amount_out: U256,
        intermediate_base_out: Option<U256>,
    ) -> Result<(), BaseDexError> {
        if token_in == self.base_token() {
            return self.apply_base_to_token_swap(token_out, amount_in, amount_out);
        }

        if token_out == self.base_token() {
            return self.apply_token_to_base_swap(token_in, amount_in, amount_out);
        }

        let base_out = intermediate_base_out
            .ok_or(BaseDexError::InvalidSwapPath(IBaseDex::InvalidSwapPath {}))?;
        self.apply_token_to_base_swap(token_in, amount_in, base_out)?;
        self.apply_base_to_token_swap(token_out, base_out, amount_out)
    }

    fn apply_token_to_base_swap(
        &mut self,
        token: Address,
        amount_token_in: U256,
        amount_base_out: U256,
    ) -> Result<(), BaseDexError> {
        let mut pool = self.get_pool(token)?;
        pool.reserve_token = Self::u128_amount(
            U256::from(pool.reserve_token)
                .checked_add(amount_token_in)
                .ok_or(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?,
        )?;
        pool.reserve_base = Self::u128_amount(
            U256::from(pool.reserve_base)
                .checked_sub(amount_base_out)
                .ok_or(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}))?,
        )?;
        self.storage
            .write_pool(token, pool)
            .map_err(|error| Self::storage_error("write_pool", error))
    }

    fn apply_base_to_token_swap(
        &mut self,
        token: Address,
        amount_base_in: U256,
        amount_token_out: U256,
    ) -> Result<(), BaseDexError> {
        let mut pool = self.get_pool(token)?;
        pool.reserve_base = Self::u128_amount(
            U256::from(pool.reserve_base)
                .checked_add(amount_base_in)
                .ok_or(BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))?,
        )?;
        pool.reserve_token = Self::u128_amount(
            U256::from(pool.reserve_token)
                .checked_sub(amount_token_out)
                .ok_or(BaseDexError::InsufficientLiquidity(IBaseDex::InsufficientLiquidity {}))?,
        )?;
        self.storage
            .write_pool(token, pool)
            .map_err(|error| Self::storage_error("write_pool", error))
    }

    fn validate_path(token_in: Address, token_out: Address) -> Result<(), BaseDexError> {
        if token_in == BASE_TOKEN_ADDRESS && token_out == BASE_TOKEN_ADDRESS {
            return Err(BaseDexError::InvalidSwapPath(IBaseDex::InvalidSwapPath {}));
        }
        if token_in == token_out {
            return Err(BaseDexError::IdenticalTokens(IBaseDex::IdenticalTokens {}));
        }
        Ok(())
    }

    fn validate_swap_tokens(
        &self,
        token_in: Address,
        token_out: Address,
    ) -> Result<(), BaseDexError> {
        if token_in != self.base_token() {
            DexToken::validate(token_in)?;
        }
        if token_out != self.base_token() {
            DexToken::validate(token_out)?;
        }
        Ok(())
    }

    fn u128_amount(amount: U256) -> Result<u128, BaseDexError> {
        amount.try_into().map_err(|_| BaseDexError::InvalidAmount(IBaseDex::InvalidAmount {}))
    }

    fn storage_error(operation: &'static str, error: DexStorageError) -> BaseDexError {
        warn!(operation, error = %error, "native DEX storage operation failed");
        BaseDexError::InvalidToken(IBaseDex::InvalidToken {})
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::address;

    use super::*;

    #[test]
    fn validate_path_rejects_base_to_base_as_invalid_path() {
        assert!(matches!(
            BaseDex::validate_path(BASE_TOKEN_ADDRESS, BASE_TOKEN_ADDRESS),
            Err(BaseDexError::InvalidSwapPath(_))
        ));
    }

    #[test]
    fn validate_path_rejects_identical_non_base_tokens() {
        let token = address!("0000000000000000000000000000000000000dE8");

        assert!(matches!(
            BaseDex::validate_path(token, token),
            Err(BaseDexError::IdenticalTokens(_))
        ));
    }
}
