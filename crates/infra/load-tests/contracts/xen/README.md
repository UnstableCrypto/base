# XEN-shape contracts

Three minimal contracts used by the `xen` load-test payload. They mirror the
trie-load shape of XEN's `bulkClaimRank` (one tx → N CREATE2 deploys + N
unique storage writes) without any of XEN's economics, ranks, or hardfork-tied
initialization.

## Files

- `State.sol` — single mapping `ranks[address] => uint256`; one `claimRank` method.
- `Worker.sol` — calls `State.claimRank` once in its constructor. Deployed via CREATE2 by `Multicall`.
- `Multicall.sol` — loops `count` CREATE2 deployments of `Worker`. Per-iteration salt is `keccak256(abi.encodePacked(baseSalt, i))`.

## Compiling

This directory is a self-contained Foundry project. Forge resolves solc 0.8.28
via the bundled `foundry.toml`:

```bash
cd crates/infra/load-tests/contracts/xen
forge build
```

After compilation, the deployment bytecode for each contract is at
`out/<Contract>.sol/<Contract>.json` under `.bytecode.object`. To re-embed
into the Rust payload, copy each into the `&'static [u8]` constants in
`crates/infra/load-tests/src/workload/payloads/xen.rs`:

```bash
jq -r '.bytecode.object' out/State.sol/State.json
jq -r '.bytecode.object' out/Worker.sol/Worker.json
jq -r '.bytecode.object' out/Multicall.sol/Multicall.json
```

Strip the leading `0x` before pasting into `hex!(...)`.

## Deploying

Follow the `LooperPayload` pattern: deploy the contracts manually (e.g. via
`forge create`, `cast send --create`, or a small one-shot script), then pass
the resulting addresses to the load test via the YAML `transactions` block:

```yaml
sender_count: 100
target_gps: 30000000
transactions:
  - weight: 100
    type: xen
    multicall: "0x..."  # Multicall address
    state: "0x..."      # State address
    term: 1000
    proxies_per_tx: 10
```

## Notes

- `Worker` is a plain contract, not an EIP-1167 minimal proxy. Real XEN uses
  proxies for cheaper deploys, but the trie work — one new account-trie entry
  per worker — is the same either way.
- `Multicall.bulkClaimRank` reverts if any inner CREATE2 fails (e.g. salt
  collision). With a fresh `baseSalt` per tx and a `proxies_per_tx` count
  well below `2^32`, collisions are vanishingly rare in practice.
