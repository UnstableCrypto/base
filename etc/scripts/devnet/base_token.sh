#!/usr/bin/env bash
# End-to-end smoke test for the BaseToken (plan-1) precompile family via `cast`.
# Mirrors b20.sh but exercises the 0xBA5E... stack: factory → createToken → mint → transfer.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMMON_SH="$SCRIPT_DIR/common.sh"
if [[ -f "$COMMON_SH" ]]; then
    # shellcheck source=/dev/null
    source "$COMMON_SH"
fi

RPC_URL="${1:-${RPC_URL:-${L2_BUILDER_RPC_URL:-http://localhost:7545}}}"
PRIVATE_KEY="${PRIVATE_KEY:-${SEQUENCER_KEY:-${ANVIL_ACCOUNT_5_KEY:-0x8b3a350cf5c34c9194ca85829a2df0ec3153be0318b5e2d3348e872092edffba}}}"
ADMIN="${ADMIN:-${SEQUENCER_ADDR:-${ANVIL_ACCOUNT_5_ADDR:-0x9965507D1a55bcC2695C58ba16FB37d819B0A4dc}}}"
RECIPIENT_ONE="${RECIPIENT_ONE:-${BATCHER_ADDR:-${ANVIL_ACCOUNT_6_ADDR:-0x976EA74026E726554dB657fA54763abd0C3a0aa9}}}"
RECIPIENT_TWO="${RECIPIENT_TWO:-${PROPOSER_ADDR:-${ANVIL_ACCOUNT_7_ADDR:-0x14dC79964da2C08b23698B3D3cc7Ca32193d9955}}}"

# Plan-1 precompile addresses (0xBA5E... prefix, sibling to B20's 0x8453).
BASE_TOKEN_FACTORY_ADDRESS="${BASE_TOKEN_FACTORY_ADDRESS:-0xBA5E000000000000000000000000000000000001}"
BASE_TOKEN_POLICY_REGISTRY_ADDRESS="${BASE_TOKEN_POLICY_REGISTRY_ADDRESS:-0xBA5E000000000000000000000000000000000403}"

TOKEN_NAME="${TOKEN_NAME:-DevToken}"
TOKEN_SYMBOL="${TOKEN_SYMBOL:-DTK}"
DECIMALS="${DECIMALS:-18}"
SALT="${SALT:-$(cast keccak "base-token-$(date +%s)-$$")}"

# Feature bitmap. Bits (must match plan_1::base_token::Feature):
#   1<<0 Mint   1<<1 Burn   1<<2 Pause   1<<3 Permit   1<<4 Memo   1<<5 Policy
# Default: Mint | Burn | Pause = 0x07. Override via FEATURES env var.
FEATURES="${FEATURES:-7}"

MINT_AMOUNT="${MINT_AMOUNT:-1000000000}"
TRANSFER_ONE="${TRANSFER_ONE:-100000000}"
TRANSFER_TWO="${TRANSFER_TWO:-25000000}"
GAS_LIMIT="${GAS_LIMIT:-10000000}"
BERYL_BLOCK="${BERYL_BLOCK:-${L2_BASE_BERYL_BLOCK:-3}}"
BERYL_WAIT_SECONDS="${BERYL_WAIT_SECONDS:-120}"

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "missing required command: $1" >&2
        exit 1
    }
}

send_tx() {
    local from_key="$1"
    local raw_tx
    local tx_hash
    shift

    raw_tx="$(
        cast mktx \
            --rpc-url "$RPC_URL" \
            --private-key "$from_key" \
            --gas-limit "$GAS_LIMIT" \
            "$@"
    )"

    tx_hash="$(cast rpc --rpc-url "$RPC_URL" eth_sendRawTransaction "$raw_tx" | jq -r .)"

    cast receipt \
        --rpc-url "$RPC_URL" \
        --json \
        "$tx_hash" |
        jq -r '"tx=\(.transactionHash) block=\(.blockNumber) status=\(.status)"'
}

balance_of() {
    local token="$1"
    local account="$2"
    cast call --rpc-url "$RPC_URL" "$token" "balanceOf(address)(uint256)" "$account"
}

wait_for_block() {
    local target_block="$1"
    local label="$2"
    local current_block

    for _ in $(seq 1 "$BERYL_WAIT_SECONDS"); do
        current_block="$(cast block-number --rpc-url "$RPC_URL" 2>/dev/null || true)"
        if [[ "$current_block" =~ ^[0-9]+$ && "$current_block" -ge "$target_block" ]]; then
            echo "$label active at block $current_block"
            return
        fi
        sleep 1
    done

    echo "timed out waiting for $label block $target_block; latest block: ${current_block:-<unknown>}" >&2
    exit 1
}

assert_call_equals() {
    local address="$1"
    local call="$2"
    local expected="$3"
    local actual

    actual="$(cast call --rpc-url "$RPC_URL" "$address" "$call")"
    if [[ "$actual" != "$expected" ]]; then
        echo "expected $call to return $expected, got $actual" >&2
        exit 1
    fi
    echo "$call: $actual"
}

require_cmd cast
require_cmd jq

if [[ ! "$BERYL_BLOCK" =~ ^[0-9]+$ ]]; then
    echo "BERYL_BLOCK must be a non-negative integer, got: $BERYL_BLOCK" >&2
    exit 1
fi

echo "RPC: $RPC_URL"
echo "factory: $BASE_TOKEN_FACTORY_ADDRESS"
echo "registry: $BASE_TOKEN_POLICY_REGISTRY_ADDRESS"
echo "admin: $ADMIN"
echo "features: $FEATURES (Mint|Burn|Pause by default)"
echo "salt: $SALT"
echo "beryl_block: $BERYL_BLOCK"

echo
echo "waiting for Beryl activation"
wait_for_block "$BERYL_BLOCK" "Beryl"

# 1. Predict the token address before deployment.
TOKEN_ADDRESS="$(
    cast call \
        --rpc-url "$RPC_URL" \
        "$BASE_TOKEN_FACTORY_ADDRESS" \
        "getTokenAddress(address,bytes32)(address)" \
        "$ADMIN" \
        "$SALT"
)"

echo
echo "addresses"
echo "factory: $BASE_TOKEN_FACTORY_ADDRESS"
echo "predicted token: $TOKEN_ADDRESS"

# 2. Deploy via the factory.
echo
echo "creating BaseToken"
send_tx "$PRIVATE_KEY" \
    "$BASE_TOKEN_FACTORY_ADDRESS" \
    "createToken(string,string,uint8,address,uint64,bytes32)" \
    "$TOKEN_NAME" \
    "$TOKEN_SYMBOL" \
    "$DECIMALS" \
    "$ADMIN" \
    "$FEATURES" \
    "$SALT"

# 3. Verify metadata reads back through the per-token precompile.
echo
echo "verifying token metadata"
assert_call_equals "$TOKEN_ADDRESS" "name()(string)" "\"$TOKEN_NAME\""
assert_call_equals "$TOKEN_ADDRESS" "symbol()(string)" "\"$TOKEN_SYMBOL\""
echo "decimals(): $(cast call --rpc-url "$RPC_URL" "$TOKEN_ADDRESS" "decimals()(uint8)")"
echo "features(): $(cast call --rpc-url "$RPC_URL" "$TOKEN_ADDRESS" "features()(uint64)")"

# 4. Grant ISSUER_ROLE to admin so they can mint.
ISSUER_ROLE="$(cast call --rpc-url "$RPC_URL" "$TOKEN_ADDRESS" "ISSUER_ROLE()(bytes32)")"
echo
echo "granting ISSUER_ROLE ($ISSUER_ROLE) to admin"
send_tx "$PRIVATE_KEY" "$TOKEN_ADDRESS" "grantRole(bytes32,address)" "$ISSUER_ROLE" "$ADMIN"

# 5. Mint to admin.
echo
echo "minting $MINT_AMOUNT to admin"
send_tx "$PRIVATE_KEY" "$TOKEN_ADDRESS" "mint(address,uint256)" "$ADMIN" "$MINT_AMOUNT"

# 6. Transfer admin -> recipient_one and admin -> recipient_two.
echo
echo "transferring admin -> recipient_one ($TRANSFER_ONE)"
send_tx "$PRIVATE_KEY" "$TOKEN_ADDRESS" "transfer(address,uint256)" "$RECIPIENT_ONE" "$TRANSFER_ONE"

echo "transferring admin -> recipient_two ($TRANSFER_TWO)"
send_tx "$PRIVATE_KEY" "$TOKEN_ADDRESS" "transfer(address,uint256)" "$RECIPIENT_TWO" "$TRANSFER_TWO"

# 7. Final balances.
echo
echo "final balances"
echo "totalSupply: $(cast call --rpc-url "$RPC_URL" "$TOKEN_ADDRESS" "totalSupply()(uint256)")"
echo "admin:         $(balance_of "$TOKEN_ADDRESS" "$ADMIN")"
echo "recipient_one: $(balance_of "$TOKEN_ADDRESS" "$RECIPIENT_ONE")"
echo "recipient_two: $(balance_of "$TOKEN_ADDRESS" "$RECIPIENT_TWO")"

echo
echo "BaseToken smoke test passed at $TOKEN_ADDRESS"
