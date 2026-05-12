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
use alloy_sol_types::SolCall;
use base_common_network::Base;
use base_common_precompiles::{BASE_DEX_ADDRESS, IBaseDex};
use base_common_rpc_types::{BaseTransactionReceipt, BaseTransactionRequest};
use devnet::{
    DevnetBuilder,
    config::{ANVIL_ACCOUNT_1, ANVIL_ACCOUNT_2},
};
use eyre::{Result, WrapErr};
use tokio::time::{sleep, timeout};

const L1_CHAIN_ID: u64 = 1337;
const L2_CHAIN_ID: u64 = 84538453;
const TX_BASE_GAS: u64 = 21_000;
const TX_STANDARD_TOKEN_COST: u64 = 4;
const TX_NON_ZERO_BYTE_MULTIPLIER_ISTANBUL: u64 = 4;
const TX_FLOOR_COST_PER_TOKEN: u64 = 10;
const ESTIMATE_CALL_STIPEND_GAS: u64 = 2_300;
const ESTIMATE_GAS_ERROR_RATIO: f64 = 0.015;
const DEX_INPUT_PER_WORD_COST: u64 = 6;
const DEX_SELECTOR_DISPATCH_COST: u64 = 100;
const DEX_ARITHMETIC_COST: u64 = 500;
const DEX_STORAGE_ACCOUNT_TOUCH_COST: u64 = 2_600;
const DEX_STORAGE_READ_COST: u64 = 2_100;
const DEX_STORAGE_WRITE_COST: u64 = 22_100;
const DEX_KECCAK_BASE_COST: u64 = 30;
const DEX_KECCAK_WORD_COST: u64 = 6;
const DEX_LOG_BASE_COST: u64 = 375;
const DEX_LOG_TOPIC_COST: u64 = 375;
const DEX_LOG_DATA_COST: u64 = 8;
const DEX_FEE_NUMERATOR: U256 = U256::from_limbs([997, 0, 0, 0]);
const DEX_FEE_DENOMINATOR: U256 = U256::from_limbs([1_000, 0, 0, 0]);
const DEX_MINIMUM_LIQUIDITY: U256 = U256::from_limbs([1_000, 0, 0, 0]);
const TX_RECEIPT_TIMEOUT: Duration = Duration::from_secs(60);
const BLOCK_PRODUCTION_TIMEOUT: Duration = Duration::from_secs(20);
const DEMO_TOKEN_A: Address = address!("0000000000000000000000000000000000000dE8");
const DEMO_TOKEN_B: Address = address!("0000000000000000000000000000000000000dE9");

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
    let seed_token_a_calldata = seed_token_a.abi_encode();
    let seed_token_a_receipt = send_dex_transaction(
        &provider,
        &liquidity_provider,
        liquidity_nonce,
        seed_token_a_calldata.clone(),
    )
    .await
    .wrap_err("seed token A liquidity")?;
    assert_mint_receipt(
        &seed_token_a_receipt,
        DEMO_TOKEN_A,
        U256::from(1_000_000u64),
        U256::from(1_000_000u64),
        expected_initial_liquidity(U256::from(1_000_000u64), U256::from(1_000_000u64)),
    )?;
    assert_exact_dex_gas(
        &seed_token_a_receipt,
        &seed_token_a_calldata,
        dex_add_liquidity_call_gas(),
    );
    liquidity_nonce += 1;

    let seed_token_b = IBaseDex::addLiquidityCall {
        token: DEMO_TOKEN_B,
        amountToken: U256::from(2_000_000u64),
        amountBase: U256::from(1_000_000u64),
        to: liquidity_provider.address(),
    };
    let seed_token_b_calldata = seed_token_b.abi_encode();
    let seed_token_b_receipt = send_dex_transaction(
        &provider,
        &liquidity_provider,
        liquidity_nonce,
        seed_token_b_calldata.clone(),
    )
    .await
    .wrap_err("seed token B liquidity")?;
    assert_mint_receipt(
        &seed_token_b_receipt,
        DEMO_TOKEN_B,
        U256::from(2_000_000u64),
        U256::from(1_000_000u64),
        expected_initial_liquidity(U256::from(2_000_000u64), U256::from(1_000_000u64)),
    )?;
    assert_exact_dex_gas(
        &seed_token_b_receipt,
        &seed_token_b_calldata,
        dex_add_liquidity_call_gas(),
    );

    let pool_a_before =
        call_get_pool(&provider, liquidity_provider.address(), DEMO_TOKEN_A).await?;
    let pool_b_before =
        call_get_pool(&provider, liquidity_provider.address(), DEMO_TOKEN_B).await?;
    assert_eq!(pool_a_before.reserveToken, 1_000_000);
    assert_eq!(pool_a_before.reserveBase, 1_000_000);
    assert_eq!(
        pool_a_before.totalSupply,
        expected_initial_total_supply(U256::from(1_000_000u64), U256::from(1_000_000u64))
    );
    assert_eq!(pool_b_before.reserveToken, 2_000_000);
    assert_eq!(pool_b_before.reserveBase, 1_000_000);
    assert_eq!(
        pool_b_before.totalSupply,
        expected_initial_total_supply(U256::from(2_000_000u64), U256::from(1_000_000u64))
    );

    let amount_in = U256::from(10_000u64);
    let expected_base_out = expected_amount_out(
        amount_in,
        U256::from(pool_a_before.reserveToken),
        U256::from(pool_a_before.reserveBase),
    );
    let expected_amount_out = expected_amount_out(
        expected_base_out,
        U256::from(pool_b_before.reserveBase),
        U256::from(pool_b_before.reserveToken),
    );
    let quoted_amount_out =
        call_quote_exact_input(&provider, swapper.address(), DEMO_TOKEN_A, DEMO_TOKEN_B, amount_in)
            .await?;
    assert_eq!(quoted_amount_out, expected_amount_out);

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
    let expected_swap_gas = expected_dex_transaction_gas(&swap_calldata, dex_swap_call_gas());
    assert_eq!(gas, expected_reth_estimate_gas(expected_swap_gas));

    let swap_nonce = provider.get_transaction_count(swapper.address()).await?;
    let swap_receipt = send_dex_transaction(&provider, &swapper, swap_nonce, swap_calldata.clone())
        .await
        .wrap_err("swap token A for token B")?;
    assert_swap_receipt(&swap_receipt, DEMO_TOKEN_A, DEMO_TOKEN_B, amount_in, quoted_amount_out)?;
    assert_exact_dex_gas(&swap_receipt, &swap_calldata, dex_swap_call_gas());

    let pool_a_after = call_get_pool(&provider, swapper.address(), DEMO_TOKEN_A).await?;
    let pool_b_after = call_get_pool(&provider, swapper.address(), DEMO_TOKEN_B).await?;

    assert_eq!(pool_a_after.reserveToken, pool_a_before.reserveToken + u128_amount(amount_in));
    assert_eq!(
        pool_a_after.reserveBase,
        pool_a_before.reserveBase - u128_amount(expected_base_out)
    );
    assert_eq!(
        pool_b_after.reserveBase,
        pool_b_before.reserveBase + u128_amount(expected_base_out)
    );
    assert_eq!(
        pool_b_after.reserveToken,
        pool_b_before.reserveToken - u128_amount(expected_amount_out)
    );

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
    liquidity: U256,
) -> Result<()> {
    let logs = receipt.inner.inner.logs();
    assert_eq!(logs.len(), 1);
    let mint = logs[0].log_decode_validate::<IBaseDex::Mint>()?;

    assert_eq!(mint.address(), BASE_DEX_ADDRESS);
    assert_eq!(mint.inner.data.token, token);
    assert_eq!(mint.inner.data.amountToken, amount_token);
    assert_eq!(mint.inner.data.amountBase, amount_base);
    assert_eq!(mint.inner.data.liquidity, liquidity);

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

fn assert_exact_dex_gas(receipt: &BaseTransactionReceipt, calldata: &[u8], call_gas: u64) {
    assert_eq!(receipt.gas_used(), expected_dex_transaction_gas(calldata, call_gas));
}

fn expected_dex_transaction_gas(calldata: &[u8], call_gas: u64) -> u64 {
    let precompile_gas = native_dex_input_gas(calldata.len()) + call_gas;
    let total_gas = intrinsic_transaction_gas(calldata) + precompile_gas;
    total_gas.max(eip7623_floor_gas(calldata))
}

fn expected_reth_estimate_gas(minimum_gas_limit: u64) -> u64 {
    let mut highest_gas_limit = (minimum_gas_limit + ESTIMATE_CALL_STIPEND_GAS) * 64 / 63;
    let mut lowest_gas_limit = minimum_gas_limit.saturating_sub(1);
    let mut mid_gas_limit = (minimum_gas_limit * 3)
        .min(((highest_gas_limit as u128 + lowest_gas_limit as u128) / 2) as u64);

    while lowest_gas_limit + 1 < highest_gas_limit {
        let ratio = (highest_gas_limit - lowest_gas_limit) as f64 / highest_gas_limit as f64;
        if ratio < ESTIMATE_GAS_ERROR_RATIO {
            break;
        }

        if mid_gas_limit >= minimum_gas_limit {
            highest_gas_limit = mid_gas_limit;
        } else {
            lowest_gas_limit = mid_gas_limit;
        }
        mid_gas_limit = ((highest_gas_limit as u128 + lowest_gas_limit as u128) / 2) as u64;
    }

    highest_gas_limit
}

fn intrinsic_transaction_gas(calldata: &[u8]) -> u64 {
    TX_BASE_GAS + calldata_token_count(calldata) * TX_STANDARD_TOKEN_COST
}

fn eip7623_floor_gas(calldata: &[u8]) -> u64 {
    TX_BASE_GAS + calldata_token_count(calldata) * TX_FLOOR_COST_PER_TOKEN
}

fn calldata_token_count(calldata: &[u8]) -> u64 {
    let zero_bytes = calldata.iter().filter(|byte| **byte == 0).count() as u64;
    let non_zero_bytes = calldata.len() as u64 - zero_bytes;
    zero_bytes + non_zero_bytes * TX_NON_ZERO_BYTE_MULTIPLIER_ISTANBUL
}

const fn native_dex_input_gas(calldata_len: usize) -> u64 {
    calldata_len.div_ceil(32) as u64 * DEX_INPUT_PER_WORD_COST
}

const fn dex_add_liquidity_call_gas() -> u64 {
    DEX_SELECTOR_DISPATCH_COST + dex_lp_update_gas() + dex_log_gas(4, 96)
}

const fn dex_swap_call_gas() -> u64 {
    DEX_SELECTOR_DISPATCH_COST
        + DEX_ARITHMETIC_COST
        + dex_pool_read_gas() * 5
        + dex_pool_write_gas() * 2
        + dex_log_gas(4, 96)
}

const fn dex_lp_update_gas() -> u64 {
    dex_pool_read_gas()
        + dex_slot_hash_gas(3)
        + DEX_STORAGE_READ_COST
        + DEX_STORAGE_WRITE_COST
        + dex_pool_write_gas()
}

const fn dex_pool_read_gas() -> u64 {
    dex_slot_hash_gas(2) * 3 + DEX_STORAGE_READ_COST * 3
}

const fn dex_pool_write_gas() -> u64 {
    dex_slot_hash_gas(2) * 3 + DEX_STORAGE_ACCOUNT_TOUCH_COST + DEX_STORAGE_WRITE_COST * 3
}

const fn dex_slot_hash_gas(words: u64) -> u64 {
    DEX_KECCAK_BASE_COST + DEX_KECCAK_WORD_COST * words
}

const fn dex_log_gas(topics: u64, data_bytes: u64) -> u64 {
    DEX_LOG_BASE_COST + DEX_LOG_TOPIC_COST * topics + DEX_LOG_DATA_COST * data_bytes
}

fn expected_amount_out(amount_in: U256, reserve_in: U256, reserve_out: U256) -> U256 {
    let amount_in_with_fee = amount_in * DEX_FEE_NUMERATOR;
    let numerator = amount_in_with_fee * reserve_out;
    let denominator = reserve_in * DEX_FEE_DENOMINATOR + amount_in_with_fee;
    numerator / denominator
}

fn expected_initial_liquidity(amount_token: U256, amount_base: U256) -> U256 {
    integer_sqrt(amount_token * amount_base) - DEX_MINIMUM_LIQUIDITY
}

fn expected_initial_total_supply(amount_token: U256, amount_base: U256) -> U256 {
    integer_sqrt(amount_token * amount_base)
}

fn integer_sqrt(value: U256) -> U256 {
    if value <= U256::from(3u64) {
        return if value.is_zero() { U256::ZERO } else { U256::from(1u64) };
    }

    let mut root = value;
    let mut candidate = value / U256::from(2u64) + U256::from(1u64);
    while candidate < root {
        root = candidate;
        candidate = (value / candidate + candidate) / U256::from(2u64);
    }
    root
}

fn u128_amount(amount: U256) -> u128 {
    amount.try_into().expect("demo amount fits u128")
}
