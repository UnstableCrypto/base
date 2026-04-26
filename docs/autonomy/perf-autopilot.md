# Autonomous Performance Autopilot

Goal: continuously improve performance of the Base services in this repository through recurring research, targeted benchmarking, small safe code changes, and PR-driven delivery.

The autopilot runs in the dedicated worktree at `/home/refcell/dev/base-perf-autopilot` on branch `automation/perf-autopilot`.

Scope

The current highest-value service areas are:

1. `crates/consensus/service` and adjacent consensus crates
   - likely hot paths: sequencer build/seal loop, engine request queue, derivation stepping, provider RPC/cache behavior, gossip validation/publish
   - existing observability: service, engine, derive, gossip, and provider metrics behind the `metrics` feature

2. `crates/batcher/service`, `crates/batcher/core`, `crates/batcher/encoder`, and `crates/batcher/source`
   - likely hot paths: driver loop backlog handling, encoding/compression, blob/calldata submission packing, recent-tx startup scan, source polling/catchup
   - existing observability: encoder/core metrics, queue depth, submission outcomes, compression ratios, in-flight counts

3. `crates/proof/zk/service`
   - likely hot paths: witness generation, L1-head calculation, status polling, repeated `GetProof` sync calls, session/state round-trips, SNARK stage-2 aggregation, proxy rate limiting
   - existing observability: request counters, latency histograms, witness generation duration, proof request duration, stuck request counters

4. Shared benchmarking and load testing
   - `crates/infra/load-tests`
   - existing benches in `crates/builder/core`, `crates/builder/publish`, `crates/proof/mpt`, and `crates/client/flashblocks-node`

Operating rules

- Work in small increments.
- Prefer measurement before modification.
- Reuse existing benchmarks first. If coverage is missing, add focused benchmark or timing instrumentation before changing logic.
- Favor changes that improve durable capability: benchmark harnesses, metrics, profiling hooks, caching, batching, concurrency limits, backpressure, and algorithmic simplification.
- Keep commits reviewable and conventional, usually `perf: ...`, `bench: ...`, `docs: ...`, or `refactor: ...`.
- Never push directly to `main`.
- Use PRs for meaningful code changes.
- When uncertain, prefer report-only progress rather than speculative edits.

Per-run workflow

1. Pull latest changes for the branch and inspect the working tree.
2. Read `docs/autonomy/perf-journal.md` to avoid repeating work.
3. Choose one concrete focus area based on the strongest combination of expected impact and measurability.
4. Do external research as needed on Rust async performance, tokio scheduling, libp2p/discv5 tuning, RPC batching/caching, compression, proof-service polling, or relevant protocol/client techniques.
5. Run or extend the narrowest benchmark that can validate the hypothesis.
6. If a code change is justified, implement the smallest testable improvement.
7. Re-run the relevant benchmark or load test and capture before/after numbers.
8. Append a concise timestamped entry to `docs/autonomy/perf-journal.md` with hypothesis, commands, results, and next step.
9. Commit substantive progress. If a meaningful code delta exists and no PR is open for this branch, open or update a PR.

Useful commands

Repo root: `/home/refcell/dev/base-perf-autopilot`

Consensus
- `cargo test -p base-consensus-node -- --list`
- `cargo test -p base-consensus-node actors::sequencer:: -- --nocapture --test-threads=1`
- `cargo test -p base-consensus-node engine_request_processor -- --nocapture --test-threads=1`
- `cargo build -p base-consensus-node --features metrics`

Batcher
- `cargo test -p base-batcher-encoder -- --nocapture`
- `cargo test -p base-batcher-core --features test-utils -- --nocapture`
- `cargo test -p base-batcher-service -- --nocapture`
- `cargo test -p base-batcher-source -- --nocapture`
- `cargo run -p base-batcher-bin -- --help`

ZK service
- `cargo test -p base-zk-service`
- `cargo nextest run --run-ignored all -p base-zk-service --test mock_backend_e2e`
- `cargo nextest run --run-ignored all -p base-zk-service --test snark_groth16_e2e`

Existing benches
- `cargo bench -p base-builder-core --bench state_root`
- `cargo bench -p base-builder-publish --bench publisher`
- `cargo bench -p base-proof-mpt --bench trie_node`
- `cargo bench -p base-flashblocks-node --bench pending_state`
- `cargo bench -p base-flashblocks-node --bench sender_recovery`

Load tests
- `just --justfile crates/infra/load-tests/Justfile devnet`
- `cargo run -p base-load-tests --bin base-load-test -- crates/infra/load-tests/examples/devnet.yaml`

Definition of done for a single run

A run is successful if it produces at least one of the following:

- a benchmark result with clear numbers
- a new or improved benchmark harness
- a metrics or profiling improvement
- a small code change with measured benefit
- a research note that materially changes the next experiment

Avoid repeating the same experiment unless the setup changed or the prior result was inconclusive.
