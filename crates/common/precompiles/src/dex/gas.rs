//! Gas schedule for the native DEX precompile.

use alloy_primitives::Address;
use revm::{context_interface::cfg::gas, precompile::PrecompileError};

use super::{IBaseDex, IBaseDexCalls, abi::BASE_TOKEN_ADDRESS};

const INPUT_PER_WORD_COST: u64 = 6;
const SELECTOR_DISPATCH_COST: u64 = 100;
const ARITHMETIC_COST: u64 = 500;
const STORAGE_ACCOUNT_TOUCH_COST: u64 = gas::COLD_ACCOUNT_ACCESS_COST;
const STORAGE_READ_COST: u64 = gas::COLD_SLOAD_COST;
const STORAGE_WRITE_COST: u64 = gas::COLD_SLOAD_COST + gas::SSTORE_SET;

/// Narrow gas meter for the feature-gated DEX scaffold.
///
/// This intentionally prices the current method-level storage footprint rather than trying to
/// replicate Tempo's full `StorageCtx` abstraction. Once token adapters and dynamic storage
/// accounting are added, this meter should move closer to the storage operation boundary so cold
/// versus warm SLOAD/SSTORE costs can be calculated from the actual journal results.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DexGasMeter {
    limit: u64,
    used: u64,
}

impl DexGasMeter {
    pub(crate) const fn new(limit: u64) -> Self {
        Self { limit, used: 0 }
    }

    pub(crate) const fn charge_calldata(&mut self, calldata: &[u8]) -> Result<(), PrecompileError> {
        self.charge(Self::input_cost(calldata.len()))
    }

    pub(crate) fn charge_call(&mut self, call: &IBaseDexCalls) -> Result<(), PrecompileError> {
        self.charge(Self::call_cost(call))
    }

    pub(crate) const fn used(&self) -> u64 {
        self.used
    }

    const fn charge(&mut self, amount: u64) -> Result<(), PrecompileError> {
        let Some(used) = self.used.checked_add(amount) else {
            return Err(PrecompileError::OutOfGas);
        };
        if used > self.limit {
            return Err(PrecompileError::OutOfGas);
        }
        self.used = used;
        Ok(())
    }

    const fn input_cost(calldata_len: usize) -> u64 {
        calldata_len.div_ceil(32) as u64 * INPUT_PER_WORD_COST
    }

    fn call_cost(call: &IBaseDexCalls) -> u64 {
        match call {
            IBaseDexCalls::BASE_TOKEN(_) => SELECTOR_DISPATCH_COST,
            IBaseDexCalls::getPool(_) => SELECTOR_DISPATCH_COST + pool_read_cost(),
            IBaseDexCalls::quoteExactInput(call) => {
                SELECTOR_DISPATCH_COST + ARITHMETIC_COST + Self::quote_pool_access_cost(call)
            }
            IBaseDexCalls::addLiquidity(_) => {
                SELECTOR_DISPATCH_COST + lp_update_cost() + log_cost(4, 96)
            }
            IBaseDexCalls::removeLiquidity(_) => {
                SELECTOR_DISPATCH_COST + lp_update_cost() + log_cost(3, 128)
            }
            IBaseDexCalls::swapExactTokensForTokens(call) => {
                SELECTOR_DISPATCH_COST
                    + ARITHMETIC_COST
                    + Self::swap_pool_access_cost(call)
                    + log_cost(4, 96)
            }
        }
    }

    fn quote_pool_access_cost(call: &IBaseDex::quoteExactInputCall) -> u64 {
        if is_single_hop_swap(call.tokenIn, call.tokenOut) {
            return pool_read_cost();
        }
        pool_read_cost() * 2
    }

    fn swap_pool_access_cost(call: &IBaseDex::swapExactTokensForTokensCall) -> u64 {
        if is_single_hop_swap(call.tokenIn, call.tokenOut) {
            return pool_read_cost() * 2 + pool_write_cost();
        }
        pool_read_cost() * 5 + pool_write_cost() * 2
    }
}

fn is_single_hop_swap(token_in: Address, token_out: Address) -> bool {
    token_in == BASE_TOKEN_ADDRESS || token_out == BASE_TOKEN_ADDRESS
}

const fn pool_read_cost() -> u64 {
    slot_hash_cost(2) * 3 + STORAGE_READ_COST * 3
}

const fn pool_write_cost() -> u64 {
    slot_hash_cost(2) * 3 + STORAGE_ACCOUNT_TOUCH_COST + STORAGE_WRITE_COST * 3
}

const fn lp_update_cost() -> u64 {
    pool_read_cost()
        + slot_hash_cost(3)
        + STORAGE_READ_COST
        + STORAGE_WRITE_COST
        + pool_write_cost()
}

const fn slot_hash_cost(words: u64) -> u64 {
    gas::KECCAK256 + gas::KECCAK256WORD * words
}

const fn log_cost(topics: u64, data_bytes: u64) -> u64 {
    gas::LOG + gas::LOGTOPIC * topics + gas::LOGDATA * data_bytes
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{U256, address};

    use super::*;
    use crate::dex::IBaseDex;

    #[test]
    fn input_cost_rounds_up_to_words() {
        assert_eq!(DexGasMeter::input_cost(0), 0);
        assert_eq!(DexGasMeter::input_cost(1), INPUT_PER_WORD_COST);
        assert_eq!(DexGasMeter::input_cost(32), INPUT_PER_WORD_COST);
        assert_eq!(DexGasMeter::input_cost(33), INPUT_PER_WORD_COST * 2);
    }

    #[test]
    fn mutating_calls_cost_more_than_views() {
        let token = address!("0000000000000000000000000000000000000dE8");
        let view = IBaseDexCalls::getPool(IBaseDex::getPoolCall { token });
        let add = IBaseDexCalls::addLiquidity(IBaseDex::addLiquidityCall {
            token,
            amountToken: U256::from(1),
            amountBase: U256::from(1),
            to: token,
        });

        assert!(DexGasMeter::call_cost(&add) > DexGasMeter::call_cost(&view));
    }

    #[test]
    fn swap_cost_tracks_single_and_two_hop_paths() {
        let token_a = address!("0000000000000000000000000000000000000dE8");
        let token_b = address!("0000000000000000000000000000000000000dE9");
        let two_hop =
            IBaseDexCalls::swapExactTokensForTokens(IBaseDex::swapExactTokensForTokensCall {
                tokenIn: token_a,
                tokenOut: token_b,
                amountIn: U256::from(1),
                minAmountOut: U256::ZERO,
                to: token_b,
            });
        let token_to_base =
            IBaseDexCalls::swapExactTokensForTokens(IBaseDex::swapExactTokensForTokensCall {
                tokenIn: token_a,
                tokenOut: BASE_TOKEN_ADDRESS,
                amountIn: U256::from(1),
                minAmountOut: U256::ZERO,
                to: token_b,
            });
        let base_to_token =
            IBaseDexCalls::swapExactTokensForTokens(IBaseDex::swapExactTokensForTokensCall {
                tokenIn: BASE_TOKEN_ADDRESS,
                tokenOut: token_b,
                amountIn: U256::from(1),
                minAmountOut: U256::ZERO,
                to: token_b,
            });

        assert_eq!(DexGasMeter::call_cost(&token_to_base), DexGasMeter::call_cost(&base_to_token));
        assert!(DexGasMeter::call_cost(&token_to_base) < DexGasMeter::call_cost(&two_hop));
    }

    #[test]
    fn quote_cost_tracks_single_and_two_hop_paths() {
        let token_a = address!("0000000000000000000000000000000000000dE8");
        let token_b = address!("0000000000000000000000000000000000000dE9");
        let two_hop = IBaseDexCalls::quoteExactInput(IBaseDex::quoteExactInputCall {
            tokenIn: token_a,
            tokenOut: token_b,
            amountIn: U256::from(1),
        });
        let single_hop = IBaseDexCalls::quoteExactInput(IBaseDex::quoteExactInputCall {
            tokenIn: token_a,
            tokenOut: BASE_TOKEN_ADDRESS,
            amountIn: U256::from(1),
        });

        assert!(DexGasMeter::call_cost(&single_hop) < DexGasMeter::call_cost(&two_hop));
    }

    #[test]
    fn charge_fails_when_limit_is_exceeded() {
        let mut meter = DexGasMeter::new(1);

        assert_eq!(meter.charge(2), Err(PrecompileError::OutOfGas));
        assert_eq!(meter.used(), 0);
    }
}
