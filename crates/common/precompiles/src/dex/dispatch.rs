//! Dispatch for the native DEX precompile.

use alloy_evm::precompiles::{DynPrecompile, PrecompileInput};
use alloy_primitives::{Address, Bytes};
use alloy_sol_types::{SolCall, SolInterface};
use revm::precompile::{PrecompileId, PrecompileOutput, PrecompileResult};

use super::{BASE_DEX_ADDRESS, BaseDex, BaseDexError, DexStorage, IBaseDex, IBaseDexCalls};

/// Native DEX stateful precompile.
#[derive(Debug, Default, Clone, Copy)]
pub struct BaseDexPrecompile;

impl BaseDexPrecompile {
    /// Returns this precompile as a stateful dynamic precompile.
    pub fn precompile() -> DynPrecompile {
        DynPrecompile::new_stateful(PrecompileId::Custom("BaseDex".into()), Self::run)
    }

    fn run(input: PrecompileInput<'_>) -> PrecompileResult {
        if !input.is_direct_call() {
            return Ok(Self::revert(BaseDexError::DelegateCallNotAllowed(
                IBaseDex::DelegateCallNotAllowed {},
            )));
        }

        let storage = DexStorage::new(BASE_DEX_ADDRESS, input.internals);
        Self::dispatch(storage, input.data, input.caller, input.is_static)
    }

    fn dispatch(
        storage: DexStorage<'_>,
        calldata: &[u8],
        caller: Address,
        is_static: bool,
    ) -> PrecompileResult {
        let call = match IBaseDexCalls::abi_decode(calldata) {
            Ok(call) => call,
            Err(_) => return Ok(PrecompileOutput::new_reverted(0, Bytes::new())),
        };

        if let Some(output) = Self::static_revert(&call, is_static) {
            return Ok(output);
        }

        let mut dex = BaseDex::new(storage);
        match call {
            IBaseDexCalls::BASE_TOKEN(_) => {
                Ok(Self::success(IBaseDex::BASE_TOKENCall::abi_encode_returns(&dex.base_token())))
            }
            IBaseDexCalls::getPool(call) => Self::get_pool(&mut dex, call.token),
            IBaseDexCalls::quoteExactInput(call) => {
                Self::quote_exact_input(&mut dex, call.tokenIn, call.tokenOut, call.amountIn)
            }
            IBaseDexCalls::addLiquidity(call) => Self::add_liquidity(
                &mut dex,
                caller,
                call.token,
                call.amountToken,
                call.amountBase,
                call.to,
            ),
            IBaseDexCalls::removeLiquidity(call) => {
                Self::remove_liquidity(&mut dex, caller, call.token, call.liquidity, call.to)
            }
            IBaseDexCalls::swapExactTokensForTokens(call) => Self::swap_exact_tokens_for_tokens(
                &mut dex,
                caller,
                call.tokenIn,
                call.tokenOut,
                call.amountIn,
                call.minAmountOut,
                call.to,
            ),
        }
    }

    fn get_pool(dex: &mut BaseDex<'_>, token: Address) -> PrecompileResult {
        match dex.get_pool(token) {
            Ok(pool) => {
                Ok(Self::success(IBaseDex::getPoolCall::abi_encode_returns(&IBaseDex::Pool {
                    reserveToken: pool.reserve_token,
                    reserveBase: pool.reserve_base,
                    totalSupply: pool.total_lp_supply,
                })))
            }
            Err(error) => Ok(Self::revert(error)),
        }
    }

    fn quote_exact_input(
        dex: &mut BaseDex<'_>,
        token_in: Address,
        token_out: Address,
        amount_in: alloy_primitives::U256,
    ) -> PrecompileResult {
        match dex.quote_exact_input(token_in, token_out, amount_in) {
            Ok(amount_out) => {
                Ok(Self::success(IBaseDex::quoteExactInputCall::abi_encode_returns(&amount_out)))
            }
            Err(error) => Ok(Self::revert(error)),
        }
    }

    fn add_liquidity(
        dex: &mut BaseDex<'_>,
        caller: Address,
        token: Address,
        amount_token: alloy_primitives::U256,
        amount_base: alloy_primitives::U256,
        to: Address,
    ) -> PrecompileResult {
        match dex.add_liquidity(caller, token, amount_token, amount_base, to) {
            Ok(liquidity) => {
                Ok(Self::success(IBaseDex::addLiquidityCall::abi_encode_returns(&liquidity)))
            }
            Err(error) => Ok(Self::revert(error)),
        }
    }

    fn remove_liquidity(
        dex: &mut BaseDex<'_>,
        caller: Address,
        token: Address,
        liquidity: alloy_primitives::U256,
        to: Address,
    ) -> PrecompileResult {
        match dex.remove_liquidity(caller, token, liquidity, to) {
            Ok((amount_token, amount_base)) => {
                Ok(Self::success(IBaseDex::removeLiquidityCall::abi_encode_returns(
                    &IBaseDex::removeLiquidityReturn {
                        amountToken: amount_token,
                        amountBase: amount_base,
                    },
                )))
            }
            Err(error) => Ok(Self::revert(error)),
        }
    }

    fn swap_exact_tokens_for_tokens(
        dex: &mut BaseDex<'_>,
        caller: Address,
        token_in: Address,
        token_out: Address,
        amount_in: alloy_primitives::U256,
        min_amount_out: alloy_primitives::U256,
        to: Address,
    ) -> PrecompileResult {
        match dex.swap_exact_tokens_for_tokens(
            caller,
            token_in,
            token_out,
            amount_in,
            min_amount_out,
            to,
        ) {
            Ok(amount_out) => Ok(Self::success(
                IBaseDex::swapExactTokensForTokensCall::abi_encode_returns(&amount_out),
            )),
            Err(error) => Ok(Self::revert(error)),
        }
    }

    fn static_revert(call: &IBaseDexCalls, is_static: bool) -> Option<PrecompileOutput> {
        if is_static && Self::is_mutating(call) {
            return Some(Self::revert(BaseDexError::StaticCallNotAllowed(
                IBaseDex::StaticCallNotAllowed {},
            )));
        }
        None
    }

    const fn is_mutating(call: &IBaseDexCalls) -> bool {
        matches!(
            call,
            IBaseDexCalls::addLiquidity(_)
                | IBaseDexCalls::removeLiquidity(_)
                | IBaseDexCalls::swapExactTokensForTokens(_)
        )
    }

    fn success(bytes: impl Into<Bytes>) -> PrecompileOutput {
        PrecompileOutput::new(0, bytes.into())
    }

    fn revert(error: BaseDexError) -> PrecompileOutput {
        PrecompileOutput::new_reverted(0, error.abi_encode().into())
    }
}

#[cfg(test)]
mod tests {
    use alloy_evm::{
        EvmInternals,
        eth::EthEvmContext,
        precompiles::{Precompile, PrecompileInput},
    };
    use alloy_primitives::{Address, U256, address};
    use alloy_sol_types::SolCall;
    use revm::database::EmptyDB;

    use super::*;

    #[test]
    fn static_mutating_call_reverts_with_generated_error() {
        let call = IBaseDexCalls::addLiquidity(IBaseDex::addLiquidityCall {
            token: address!("1111111111111111111111111111111111111111"),
            amountToken: U256::from(1),
            amountBase: U256::from(1),
            to: address!("2222222222222222222222222222222222222222"),
        });

        let output = BaseDexPrecompile::static_revert(&call, true).unwrap();

        assert!(output.reverted);
        assert_eq!(
            output.bytes.as_ref(),
            BaseDexError::StaticCallNotAllowed(IBaseDex::StaticCallNotAllowed {})
                .abi_encode()
                .as_slice()
        );
    }

    #[test]
    fn view_call_is_not_rejected_in_static_context() {
        let call = IBaseDexCalls::BASE_TOKEN(IBaseDex::BASE_TOKENCall {});

        assert!(BaseDexPrecompile::static_revert(&call, true).is_none());
    }

    #[test]
    fn add_liquidity_reaches_stubbed_token_boundary() {
        let calldata = IBaseDex::addLiquidityCall {
            token: address!("1111111111111111111111111111111111111111"),
            amountToken: U256::from(1),
            amountBase: U256::from(1),
            to: address!("2222222222222222222222222222222222222222"),
        }
        .abi_encode();
        let mut context = EthEvmContext::new(EmptyDB::default(), Default::default());
        let result = BaseDexPrecompile::precompile()
            .call(PrecompileInput {
                data: &calldata,
                gas: u64::MAX,
                caller: Address::ZERO,
                value: U256::ZERO,
                target_address: BASE_DEX_ADDRESS,
                is_static: false,
                bytecode_address: BASE_DEX_ADDRESS,
                internals: EvmInternals::from_context(&mut context),
            })
            .unwrap();

        assert!(result.reverted);
        assert_eq!(
            result.bytes.as_ref(),
            BaseDexError::InvalidToken(IBaseDex::InvalidToken {}).abi_encode().as_slice()
        );
    }

    #[test]
    fn successful_liquidity_call_updates_pool_storage() {
        let token = address!("0000000000000000000000000000000000000dE8");
        let caller = address!("1111111111111111111111111111111111111111");
        let add_liquidity = IBaseDex::addLiquidityCall {
            token,
            amountToken: U256::from(1_000_000),
            amountBase: U256::from(1_000_000),
            to: caller,
        }
        .abi_encode();
        let get_pool = IBaseDex::getPoolCall { token }.abi_encode();
        let mut context = EthEvmContext::new(EmptyDB::default(), Default::default());
        let add_result = BaseDexPrecompile::precompile()
            .call(PrecompileInput {
                data: &add_liquidity,
                gas: u64::MAX,
                caller,
                value: U256::ZERO,
                target_address: BASE_DEX_ADDRESS,
                is_static: false,
                bytecode_address: BASE_DEX_ADDRESS,
                internals: EvmInternals::from_context(&mut context),
            })
            .unwrap();

        assert!(!add_result.reverted);

        let get_result = BaseDexPrecompile::precompile()
            .call(PrecompileInput {
                data: &get_pool,
                gas: u64::MAX,
                caller,
                value: U256::ZERO,
                target_address: BASE_DEX_ADDRESS,
                is_static: true,
                bytecode_address: BASE_DEX_ADDRESS,
                internals: EvmInternals::from_context(&mut context),
            })
            .unwrap();
        let pool = IBaseDex::getPoolCall::abi_decode_returns(&get_result.bytes).unwrap();

        assert_eq!(pool.reserveToken, 1_000_000);
        assert_eq!(pool.reserveBase, 1_000_000);
    }
}
