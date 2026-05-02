# TEE Provers

TEE provers produce enclave-backed proof material for checkpoint proposals and disputes.

Nitro remains the default production path. Intel TDX support is enabled only when operators provide
TDX prover endpoints and TDX attestation proving configuration. Both platforms expose the shared
`prover_prove`, `enclave_signerPublicKey`, and `enclave_signerAttestation` RPC surface. TDX provers
also expose `enclave_attestationKind == "tdx"` so the registrar can reject endpoints discovered
through the wrong fleet.

Proposal proof bytes stay unchanged across platforms: `proposer(20) || signature(65)`. The
platform-specific attestation path only affects signer registration and image-hash derivation.

For TDX rollout commands, image hash extraction, canary validation, and rollback, see
[TDX Deployment](./tdx-deployment).
