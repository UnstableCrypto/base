//! `p384verify` precompile (secp384r1 ECDSA signature verification).
//!
//! This precompile verifies an ECDSA signature over the secp384r1 (NIST P-384)
//! elliptic curve. The primary motivator is on-chain verification of AWS Nitro
//! Enclave attestations, which use ECDSA over secp384r1 (`ES384`) inside their
//! `COSE_Sign1` envelope. Without a native primitive, callers must emulate
//! big-integer arithmetic via `MODEXP`, which becomes economically infeasible
//! after the Fusaka `MODEXP` reprice (EIP-7883).
//!
//! Status: prototype. The address, gas cost, and activation hardfork are
//! placeholders. See the project P/PS for the pricing methodology
//! (native benchmark, parity with comparable precompiles, prover-budget ceiling)
//! that needs to be run before mainnet.
//!
//! ## Wire format
//!
//! | signed message hash |  r  |  s  | public key x | public key y |
//! | :-----------------: | :-: | :-: | :----------: | :----------: |
//! |          32         |  48 | 48  |      48      |      48      |
//!
//! Total: 224 bytes. The message MUST already be the SHA-384 (or other 32-byte
//! pre-hash) of the original signed payload; this precompile does not hash.
//! Output mirrors `p256verify`: a 32-byte word with the low byte set to `0x01`
//! when the signature is valid, empty bytes otherwise.

use alloy_primitives::{Address, address};
use revm::precompile::{Precompile, PrecompileError, PrecompileId, PrecompileOutput, PrecompileResult};
use revm::primitives::{B256, Bytes};

/// Placeholder precompile address.
///
/// TODO: finalise an address before any hardfork activation. RIP-7212 used
/// `0x100` for `p256verify`; an analogous allocation through the RIP/EIP
/// process is the right path here. `0x111` is intentionally unallocated.
pub const P384VERIFY_ADDRESS: Address = address!("0x0000000000000000000000000000000000000111");

/// Placeholder gas cost.
///
/// Triangulation status (project P/PS, after Succinct response):
///
/// 1. Native execution: ~4x p256verify (our criterion bench is 3.73x;
///    Succinct independently measured ~4x with RustCrypto). Implies
///    ~27,600 gas at p256verify-Osaka parity.
/// 2. Parity with `p256verify` (6,900 gas Osaka): same ~27,600.
/// 3. Prover ceiling: SP1 has a custom circuit for p256 curve ops giving
///    a ~7.5x improvement vs pure RISC-V. No equivalent p384 circuit
///    exists today, so proving p384 in pure RISC-V costs roughly
///    4 x 7.5 = 30x p256verify on the SP1 prover. Pricing at the native
///    ratio (~27,600) underprices prover work by 7.5x — a DoS vector
///    against the prover.
///
/// Therefore: until a custom p384 circuit ships, gas must reflect the
/// pure-RISC-V proving cost (~30x p256verify ≈ 207,000 gas). The
/// current placeholder is set to 250,000 to give modest headroom over
/// that bound. Still <2% of the EIP-7825 per-tx cap.
///
/// Two paths reduce this:
///   - Custom p384 circuit in SP1 (LOE outstanding from Succinct).
///     Reprices down to ~27,600 once landed.
///   - Per-block prover-budget confirmation that p384 calls at this
///     price stay under the prover-throughput ceiling.
///
/// TODO: lock the number once circuit availability is decided.
pub const P384VERIFY_BASE_GAS_FEE: u64 = 250_000;

/// Length of the `p384verify` calldata: msg(32) || r(48) || s(48) || pub_x(48) || pub_y(48).
pub const P384VERIFY_INPUT_LEN: usize = 32 + 48 + 48 + 48 + 48;

/// Length of a single P-384 field element in bytes.
const FIELD_LEN: usize = 48;

/// `p384verify` precompile.
pub const P384VERIFY: Precompile = Precompile::new(
    PrecompileId::Custom(std::borrow::Cow::Borrowed("p384verify")),
    P384VERIFY_ADDRESS,
    p384_verify,
);

/// Verify an ECDSA signature over secp384r1.
///
/// The signed message MUST already be a 32-byte hash. The precompile does not
/// perform any hashing of its own.
pub fn p384_verify(input: &[u8], gas_limit: u64) -> PrecompileResult {
    if P384VERIFY_BASE_GAS_FEE > gas_limit {
        return Err(PrecompileError::OutOfGas);
    }
    let result =
        if verify_impl(input) { B256::with_last_byte(1).into() } else { Bytes::new() };
    Ok(PrecompileOutput::new(P384VERIFY_BASE_GAS_FEE, result))
}

/// Returns true when the input is a valid encoding of a verifying P-384 ECDSA
/// signature, false otherwise. Any structural error (wrong length, malformed
/// signature, invalid public key) maps to `false`, matching the `p256verify`
/// "empty on invalid" semantics.
pub fn verify_impl(input: &[u8]) -> bool {
    use p384::ecdsa::signature::hazmat::PrehashVerifier;
    use p384::ecdsa::{Signature, VerifyingKey};
    use p384::EncodedPoint;

    if input.len() != P384VERIFY_INPUT_LEN {
        return false;
    }

    // Parse fields. Layout: msg(32) | r(48) | s(48) | pub_x(48) | pub_y(48).
    let msg: &[u8; 32] = input[..32].try_into().expect("32 bytes by length check");
    let sig_bytes: &[u8] = &input[32..32 + 2 * FIELD_LEN];
    let pub_x: &[u8; FIELD_LEN] =
        input[128..176].try_into().expect("48 bytes by length check");
    let pub_y: &[u8; FIELD_LEN] =
        input[176..224].try_into().expect("48 bytes by length check");

    let Ok(signature) = Signature::from_slice(sig_bytes) else {
        return false;
    };
    let encoded_point = EncodedPoint::from_affine_coordinates(pub_x.into(), pub_y.into(), false);
    let Ok(public_key) = VerifyingKey::from_encoded_point(&encoded_point) else {
        return false;
    };

    public_key.verify_prehash(msg, &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use p384::ecdsa::{Signature, SigningKey, signature::hazmat::PrehashSigner};
    use p384::elliptic_curve::rand_core::OsRng;

    use super::*;

    /// Build a known-valid 224-byte calldata payload by signing a fixed digest
    /// with a freshly generated keypair. Returns the encoded input alongside
    /// the digest used so tests can corrupt individual fields and re-check.
    fn make_valid_input() -> (Vec<u8>, [u8; 32]) {
        let signing_key = SigningKey::random(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let msg = [0x42u8; 32];
        let sig: Signature = signing_key.sign_prehash(&msg).expect("sign");

        let pub_point = verifying_key.to_encoded_point(false);
        let pub_x = pub_point.x().expect("x present");
        let pub_y = pub_point.y().expect("y present");
        let sig_bytes = sig.to_bytes();

        let mut input = Vec::with_capacity(P384VERIFY_INPUT_LEN);
        input.extend_from_slice(&msg);
        input.extend_from_slice(&sig_bytes);
        input.extend_from_slice(pub_x);
        input.extend_from_slice(pub_y);
        assert_eq!(input.len(), P384VERIFY_INPUT_LEN);
        (input, msg)
    }

    #[test]
    fn ok_returns_one_word() {
        let (input, _) = make_valid_input();
        let outcome = p384_verify(&input, P384VERIFY_BASE_GAS_FEE).expect("ok");
        assert_eq!(outcome.gas_used, P384VERIFY_BASE_GAS_FEE);
        let expected: Bytes = B256::with_last_byte(1).into();
        assert_eq!(outcome.bytes, expected);
    }

    #[test]
    fn wrong_message_returns_empty() {
        let (mut input, _) = make_valid_input();
        input[0] ^= 0xFF;
        let outcome = p384_verify(&input, P384VERIFY_BASE_GAS_FEE).expect("ok");
        assert_eq!(outcome.bytes, Bytes::new());
    }

    #[test]
    fn wrong_signature_returns_empty() {
        let (mut input, _) = make_valid_input();
        // Flip a bit in r.
        input[32] ^= 0x01;
        let outcome = p384_verify(&input, P384VERIFY_BASE_GAS_FEE).expect("ok");
        assert_eq!(outcome.bytes, Bytes::new());
    }

    #[test]
    fn wrong_pubkey_returns_empty() {
        let (mut input, _) = make_valid_input();
        // Zero out pub_y; not a valid point.
        for b in &mut input[176..224] {
            *b = 0;
        }
        let outcome = p384_verify(&input, P384VERIFY_BASE_GAS_FEE).expect("ok");
        assert_eq!(outcome.bytes, Bytes::new());
    }

    #[test]
    fn short_input_returns_empty() {
        let outcome = p384_verify(&[0u8; 100], P384VERIFY_BASE_GAS_FEE).expect("ok");
        assert_eq!(outcome.bytes, Bytes::new());
    }

    #[test]
    fn long_input_returns_empty() {
        let outcome =
            p384_verify(&[0u8; P384VERIFY_INPUT_LEN + 1], P384VERIFY_BASE_GAS_FEE).expect("ok");
        assert_eq!(outcome.bytes, Bytes::new());
    }

    #[test]
    fn empty_input_returns_empty() {
        let outcome = p384_verify(&[], P384VERIFY_BASE_GAS_FEE).expect("ok");
        assert_eq!(outcome.bytes, Bytes::new());
    }

    #[test]
    fn out_of_gas() {
        let (input, _) = make_valid_input();
        assert!(matches!(
            p384_verify(&input, P384VERIFY_BASE_GAS_FEE - 1),
            Err(PrecompileError::OutOfGas)
        ));
    }

    #[test]
    fn gas_charged_even_on_invalid_signature() {
        let mut input = vec![0u8; P384VERIFY_INPUT_LEN];
        input[0] = 1;
        let outcome = p384_verify(&input, P384VERIFY_BASE_GAS_FEE).expect("ok");
        assert_eq!(outcome.gas_used, P384VERIFY_BASE_GAS_FEE);
        assert_eq!(outcome.bytes, Bytes::new());
    }
}
