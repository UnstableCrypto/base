# TDX Prover Binary

JSON-RPC server binary for Intel TDX TEE proof backends.

The binary contains CLI glue only. TDX signer, quote, proof, and RPC behavior
is implemented in `base-proof-tee-tdx-prover` and `base-proof-tee-tdx-runtime`.
