# Performance Autopilot Journal

This file records each autonomous performance run.

Entry format

## YYYY-MM-DD HH:MM UTC
Focus:
Hypothesis:
Commands:
Results:
Next:

## 2026-04-26 14:01 UTC
Focus: bootstrap performance autopilot
Hypothesis: the repo already contains enough service hotspots, metrics, load tests, and benchmark hooks to start an autonomous optimization loop without guessing blindly.
Commands:
- inspected consensus, batcher, zk service, and shared benchmarking infrastructure
- created dedicated worktree `/home/refcell/dev/base-perf-autopilot`
Results:
- consensus hotspots identified around sequencer build/seal, engine request processing, derivation, provider RPC/cache behavior, and gossip paths
- batcher hotspots identified around driver scheduling, encoder/compression, blob packing, recent-tx startup scan, and source polling/catchup
- zk hotspots identified around witness generation, status polling, repeated GetProof syncs, session round-trips, and proxy rate limiting
- reusable benchmarking infrastructure exists in `crates/infra/load-tests` plus several criterion benches elsewhere in the repo
Next:
- start with the narrowest measurable improvement in one hotspot area, likely batcher encoder/submission or zk service polling behavior
