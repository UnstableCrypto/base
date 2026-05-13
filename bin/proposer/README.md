# `base-proposer-bin`

TEE-based output proposer binary for Unstable.

Parses CLI arguments, builds a validated configuration, and delegates to
`base_proposer::ProposerService::run()` for the full service lifecycle.
