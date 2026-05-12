#![cfg(feature = "native-dex")]

//! End-user RPC demonstration for the feature-gated native DEX.

use std::time::Duration;

use alloy_consensus::{SignableTransaction, TxReceipt};
use alloy_eips::{BlockNumberOrTag, eip2718::Encodable2718};
use alloy_network::{ReceiptResponse, TransactionBuilder};
use alloy_primitives::{Address, B256, Bytes, U256, address};
use alloy_provider::{Provider, RootProvider};
use alloy_rpc_types_eth::TransactionInput;
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{SolCall, sol};
use base_common_network::Base;
use base_common_rpc_types::{BaseTransactionReceipt, BaseTransactionRequest};
use devnet::{
    DevnetBuilder,
    config::{ANVIL_ACCOUNT_1, ANVIL_ACCOUNT_2},
};
use eyre::{Result, WrapErr};
use tokio::time::{sleep, timeout};

const L1_CHAIN_ID: u64 = 1337;
const L2_CHAIN_ID: u64 = 84538453;
const TX_RECEIPT_TIMEOUT: Duration = Duration::from_secs(60);
const BLOCK_PRODUCTION_TIMEOUT: Duration = Duration::from_secs(20);
const BASE_DEX_ADDRESS: Address = address!("0000000000000000000000000000000000000dE7");
const DEMO_TOKEN_A: Address = address!("0000000000000000000000000000000000000dE8");
const DEMO_TOKEN_B: Address = address!("0000000000000000000000000000000000000dE9");

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
        function swapExactTokensForTokens(
            address tokenIn,
            address tokenOut,
            uint256 amountIn,
            uint256 minAmountOut,
            address to
        ) external returns (uint256 amountOut);

        event Mint(address indexed sender, address indexed token, uint256 amountToken, uint256 amountBase, uint256 liquidity, address indexed to);
        event Swap(address indexed sender, address indexed tokenIn, address indexed tokenOut, uint256 amountIn, uint256 amountOut, address to);
    }
}

#[tokio::test]
async fn native_dex_swap_through_user_rpc_flow() -> Result<()> {
    let devnet = DevnetBuilder::new()
        .with_l1_chain_id(L1_CHAIN_ID)
        .with_l2_chain_id(L2_CHAIN_ID)
        .with_beryl_at(0)
        .build()
        .await?;

    let provider = devnet.l2_builder_provider()?;
    wait_for_l2_block(&provider, 2).await?;

    let liquidity_provider = signer(&ANVIL_ACCOUNT_1.private_key)?;
    let swapper = signer(&ANVIL_ACCOUNT_2.private_key)?;
    let base_token = call_base_token(&provider, liquidity_provider.address()).await?;
    assert_eq!(base_token, Address::ZERO);

    let empty_pool = call_get_pool(&provider, liquidity_provider.address(), DEMO_TOKEN_A).await?;
    assert_eq!(empty_pool.reserveToken, 0);
    assert_eq!(empty_pool.reserveBase, 0);
    assert_eq!(empty_pool.totalSupply, U256::ZERO);

    let mut liquidity_nonce = provider.get_transaction_count(liquidity_provider.address()).await?;
    let seed_token_a = IBaseDex::addLiquidityCall {
        token: DEMO_TOKEN_A,
        amountToken: U256::from(1_000_000u64),
        amountBase: U256::from(1_000_000u64),
        to: liquidity_provider.address(),
    };
    let seed_token_a_receipt = send_dex_transaction(
        &provider,
        &liquidity_provider,
        liquidity_nonce,
        seed_token_a.abi_encode(),
    )
    .await
    .wrap_err("seed token A liquidity")?;
    assert_mint_receipt(
        &seed_token_a_receipt,
        DEMO_TOKEN_A,
        U256::from(1_000_000u64),
        U256::from(1_000_000u64),
    )?;
    liquidity_nonce += 1;

    let seed_token_b = IBaseDex::addLiquidityCall {
        token: DEMO_TOKEN_B,
        amountToken: U256::from(2_000_000u64),
        amountBase: U256::from(1_000_000u64),
        to: liquidity_provider.address(),
    };
    let seed_token_b_receipt = send_dex_transaction(
        &provider,
        &liquidity_provider,
        liquidity_nonce,
        seed_token_b.abi_encode(),
    )
    .await
    .wrap_err("seed token B liquidity")?;
    assert_mint_receipt(
        &seed_token_b_receipt,
        DEMO_TOKEN_B,
        U256::from(2_000_000u64),
        U256::from(1_000_000u64),
    )?;

    let pool_a_before =
        call_get_pool(&provider, liquidity_provider.address(), DEMO_TOKEN_A).await?;
    let pool_b_before =
        call_get_pool(&provider, liquidity_provider.address(), DEMO_TOKEN_B).await?;
    assert_eq!(pool_a_before.reserveToken, 1_000_000);
    assert_eq!(pool_a_before.reserveBase, 1_000_000);
    assert_eq!(pool_b_before.reserveToken, 2_000_000);
    assert_eq!(pool_b_before.reserveBase, 1_000_000);

    let amount_in = U256::from(10_000u64);
    let quoted_amount_out =
        call_quote_exact_input(&provider, swapper.address(), DEMO_TOKEN_A, DEMO_TOKEN_B, amount_in)
            .await?;
    assert!(quoted_amount_out > U256::ZERO);

    let min_amount_out = quoted_amount_out - U256::from(1u64);
    let swap = IBaseDex::swapExactTokensForTokensCall {
        tokenIn: DEMO_TOKEN_A,
        tokenOut: DEMO_TOKEN_B,
        amountIn: amount_in,
        minAmountOut: min_amount_out,
        to: swapper.address(),
    };
    let swap_calldata = swap.abi_encode();
    let gas = provider
        .estimate_gas(dex_request(swapper.address(), swap_calldata.clone()))
        .await
        .wrap_err("estimate swap gas")?;
    assert!(gas > 0);

    let swap_nonce = provider.get_transaction_count(swapper.address()).await?;
    let swap_receipt = send_dex_transaction(&provider, &swapper, swap_nonce, swap_calldata)
        .await
        .wrap_err("swap token A for token B")?;
    assert_swap_receipt(&swap_receipt, DEMO_TOKEN_A, DEMO_TOKEN_B, amount_in, quoted_amount_out)?;

    let pool_a_after = call_get_pool(&provider, swapper.address(), DEMO_TOKEN_A).await?;
    let pool_b_after = call_get_pool(&provider, swapper.address(), DEMO_TOKEN_B).await?;

    assert_eq!(pool_a_after.reserveToken, pool_a_before.reserveToken + 10_000);
    assert!(pool_a_after.reserveBase < pool_a_before.reserveBase);
    assert!(pool_b_after.reserveBase > pool_b_before.reserveBase);
    assert!(pool_b_after.reserveToken < pool_b_before.reserveToken);

    Ok(())
}

fn signer(private_key: &B256) -> Result<PrivateKeySigner> {
    let private_key_hex = format!("0x{}", hex::encode(private_key.as_slice()));
    Ok(private_key_hex.parse()?)
}

async fn wait_for_l2_block(provider: &RootProvider<Base>, min_block: u64) -> Result<()> {
    timeout(BLOCK_PRODUCTION_TIMEOUT, async {
        loop {
            let block = provider.get_block_number().await?;
            if block >= min_block {
                return Ok::<_, eyre::Error>(());
            }
            sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .wrap_err("L2 block production timed out")?
}

async fn call_base_token(provider: &RootProvider<Base>, from: Address) -> Result<Address> {
    let calldata = IBaseDex::BASE_TOKENCall {}.abi_encode();
    let result =
        provider.call(dex_request(from, calldata)).block(BlockNumberOrTag::Latest.into()).await?;
    Ok(IBaseDex::BASE_TOKENCall::abi_decode_returns(&result)?)
}

async fn call_get_pool(
    provider: &RootProvider<Base>,
    from: Address,
    token: Address,
) -> Result<IBaseDex::Pool> {
    let calldata = IBaseDex::getPoolCall { token }.abi_encode();
    let result =
        provider.call(dex_request(from, calldata)).block(BlockNumberOrTag::Latest.into()).await?;
    Ok(IBaseDex::getPoolCall::abi_decode_returns(&result)?)
}

async fn call_quote_exact_input(
    provider: &RootProvider<Base>,
    from: Address,
    token_in: Address,
    token_out: Address,
    amount_in: U256,
) -> Result<U256> {
    let calldata = IBaseDex::quoteExactInputCall {
        tokenIn: token_in,
        tokenOut: token_out,
        amountIn: amount_in,
    }
    .abi_encode();
    let result =
        provider.call(dex_request(from, calldata)).block(BlockNumberOrTag::Latest.into()).await?;
    Ok(IBaseDex::quoteExactInputCall::abi_decode_returns(&result)?)
}

async fn send_dex_transaction(
    provider: &RootProvider<Base>,
    signer: &PrivateKeySigner,
    nonce: u64,
    calldata: Vec<u8>,
) -> Result<BaseTransactionReceipt> {
    let request = dex_request(signer.address(), calldata)
        .transaction_type(2)
        .with_gas_limit(1_000_000)
        .with_max_fee_per_gas(1_000_000_000)
        .with_max_priority_fee_per_gas(1_000_000)
        .with_chain_id(L2_CHAIN_ID)
        .with_nonce(nonce);

    let tx = request
        .build_typed_tx()
        .map_err(|request| eyre::eyre!("invalid DEX transaction request: {request:?}"))?;
    let signature = signer.sign_hash_sync(&tx.signature_hash())?;
    let signed_tx = tx.into_signed(signature);
    let tx_hash = *signed_tx.hash();
    let raw_tx: Bytes = signed_tx.encoded_2718().into();
    let pending_tx = provider.send_raw_transaction(&raw_tx).await?;
    assert_eq!(*pending_tx.tx_hash(), tx_hash);

    timeout(TX_RECEIPT_TIMEOUT, async {
        loop {
            if let Some(receipt) = provider.get_transaction_receipt(tx_hash).await? {
                assert!(receipt.status(), "DEX transaction {tx_hash} must succeed");
                assert_eq!(receipt.inner.to, Some(BASE_DEX_ADDRESS));
                return Ok::<_, eyre::Error>(receipt);
            }
            sleep(Duration::from_secs(1)).await;
        }
    })
    .await
    .wrap_err("DEX transaction receipt timed out")?
}

fn dex_request(from: Address, calldata: Vec<u8>) -> BaseTransactionRequest {
    BaseTransactionRequest::default()
        .from(from)
        .to(BASE_DEX_ADDRESS)
        .value(U256::ZERO)
        .input(TransactionInput::new(Bytes::from(calldata)))
}

fn assert_mint_receipt(
    receipt: &BaseTransactionReceipt,
    token: Address,
    amount_token: U256,
    amount_base: U256,
) -> Result<()> {
    let logs = receipt.inner.inner.logs();
    assert_eq!(logs.len(), 1);
    let mint = logs[0].log_decode_validate::<IBaseDex::Mint>()?;

    assert_eq!(mint.address(), BASE_DEX_ADDRESS);
    assert_eq!(mint.inner.data.token, token);
    assert_eq!(mint.inner.data.amountToken, amount_token);
    assert_eq!(mint.inner.data.amountBase, amount_base);
    assert!(mint.inner.data.liquidity > U256::ZERO);

    Ok(())
}

fn assert_swap_receipt(
    receipt: &BaseTransactionReceipt,
    token_in: Address,
    token_out: Address,
    amount_in: U256,
    amount_out: U256,
) -> Result<()> {
    let logs = receipt.inner.inner.logs();
    assert_eq!(logs.len(), 1);
    let swap = logs[0].log_decode_validate::<IBaseDex::Swap>()?;

    assert_eq!(swap.address(), BASE_DEX_ADDRESS);
    assert_eq!(swap.inner.data.tokenIn, token_in);
    assert_eq!(swap.inner.data.tokenOut, token_out);
    assert_eq!(swap.inner.data.amountIn, amount_in);
    assert_eq!(swap.inner.data.amountOut, amount_out);

    Ok(())
}
