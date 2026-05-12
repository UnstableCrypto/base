//! ABI types for the native DEX precompile.

use alloy_primitives::{Address, address};
use alloy_sol_types::sol;

/// Native DEX singleton precompile address.
pub const BASE_DEX_ADDRESS: Address = address!("0000000000000000000000000000000000000dE7");

pub(crate) const BASE_TOKEN_ADDRESS: Address = Address::ZERO;

sol! {
    #[derive(Debug, PartialEq, Eq)]
    interface IBaseDex {
        struct Pool {
            uint128 reserveToken;
            uint128 reserveBase;
            uint256 totalSupply;
        }

        function BASE_TOKEN() external view returns (address);
        function getPool(address token) external view returns (Pool memory);
        function quoteExactInput(address tokenIn, address tokenOut, uint256 amountIn)
            external view returns (uint256 amountOut);

        function addLiquidity(address token, uint256 amountToken, uint256 amountBase, address to)
            external returns (uint256 liquidity);
        function removeLiquidity(address token, uint256 liquidity, address to)
            external returns (uint256 amountToken, uint256 amountBase);
        function swapExactTokensForTokens(
            address tokenIn,
            address tokenOut,
            uint256 amountIn,
            uint256 minAmountOut,
            address to
        ) external returns (uint256 amountOut);

        event Mint(address indexed sender, address indexed token, uint256 amountToken, uint256 amountBase, uint256 liquidity, address indexed to);
        event Burn(address indexed sender, address indexed token, uint256 amountToken, uint256 amountBase, uint256 liquidity, address to);
        event Swap(address indexed sender, address indexed tokenIn, address indexed tokenOut, uint256 amountIn, uint256 amountOut, address to);

        error IdenticalTokens();
        error InvalidToken();
        error InvalidAmount();
        error InsufficientLiquidity();
        error InsufficientOutputAmount();
        error InvalidSwapPath();
        error StaticCallNotAllowed();
        error DelegateCallNotAllowed();
    }
}

pub(crate) use IBaseDex::{IBaseDexCalls, IBaseDexErrors as BaseDexError};

#[cfg(test)]
mod tests {
    use alloy_primitives::{U256, address};
    use alloy_sol_types::{SolCall, SolError, SolInterface};

    use super::*;

    #[test]
    fn decodes_quote_exact_input_call() {
        let token_in = address!("1111111111111111111111111111111111111111");
        let token_out = address!("2222222222222222222222222222222222222222");
        let calldata = IBaseDex::quoteExactInputCall {
            tokenIn: token_in,
            tokenOut: token_out,
            amountIn: U256::from(123),
        }
        .abi_encode();

        assert_eq!(
            IBaseDexCalls::abi_decode(&calldata),
            Ok(IBaseDexCalls::quoteExactInput(IBaseDex::quoteExactInputCall {
                tokenIn: token_in,
                tokenOut: token_out,
                amountIn: U256::from(123),
            }))
        );
    }

    #[test]
    fn encodes_custom_error_selector() {
        assert_eq!(
            BaseDexError::InvalidToken(IBaseDex::InvalidToken {}).abi_encode().as_slice(),
            &IBaseDex::InvalidToken::SELECTOR
        );
    }
}
