# TDX TEE Prover

Host-side JSON-RPC prover server for Intel TDX TEE proof backends.

The crate exposes the shared prover RPC namespace used by other TEE backends,
collects TDX quotes for signer registration, and signs `ProofJournal` bytes
with the TDX guest signer.

`enclave_signerAttestation` returns encoded `TdxSignerAttestation` payloads:
each payload includes the signer public key, the raw TDX quote, and the quote
timestamp committed into `TDREPORT.REPORTDATA`. TDX attestations currently
reject `user_data` and `nonce` parameters because the runtime does not bind
those challenge fields into report data.
