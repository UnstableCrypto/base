//! Criterion benchmarks for the `p384verify` precompile.
//!
//! Implements input #1 of the pricing-methodology triangulation: native
//! execution wall-clock on representative hardware. To turn this into a
//! defensible gas number, run the bench, take the median, and:
//!
//!   gas_p384 ≈ gas_p256_osaka × (median_p384_ns / median_p256_ns)
//!
//! Then cross-reference against input #2 (RIP-7212 reverse-engineering) and
//! input #3 (proof-system circuit cost ceiling), per the project P/PS.
//! Take the maximum of the three, then sanity-check the result fits well
//! under the EIP-7825 per-tx gas cap.
//!
//! Run with: `cargo bench -p base-common-evm --bench precompiles`.

use std::hint::black_box;

use base_common_evm::{P384VERIFY_BASE_GAS_FEE, P384VERIFY_INPUT_LEN, p384_verify};
use criterion::{Criterion, criterion_group, criterion_main};
use p384::ecdsa::{Signature, SigningKey, signature::hazmat::PrehashSigner};
use p384::elliptic_curve::rand_core::OsRng;
use revm::precompile::secp256r1;

/// A known-good 160-byte `p256verify` calldata payload from the daimo-eth
/// p256-verifier test vectors (also used in revm's own secp256r1 tests).
/// Lives here as a constant so the bench is reproducible.
const P256_VALID_INPUT_HEX: &str = concat!(
    "4cee90eb86eaa050036147a12d49004b6b9c72bd725d39d4785011fe190f0b4d",
    "a73bd4903f0ce3b639bbbf6e8e80d16931ff4bcf5993d58468e8fb19086e8cac",
    "36dbcd03009df8c59286b162af3bd7fcc0450c9aa81be5d10d312af6c66b1d60",
    "4aebd3099c618202fcfe16ae7770b0c49ab5eadf74b754204a3bb6060e44eff3",
    "7618b065f9832de4ca6ca971a7a1adc826d0f7c00181a5fb2ddf79ae00b4e10e",
);

/// Build a valid 224-byte p384verify calldata by signing a fixed digest with
/// a freshly generated keypair. The keypair / sig is built once outside the
/// timed loop so the bench measures verification only.
fn make_valid_p384_input() -> Vec<u8> {
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let msg = [0x42u8; 32];
    let sig: Signature = signing_key.sign_prehash(&msg).expect("sign");

    let pub_point = verifying_key.to_encoded_point(false);
    let pub_x = pub_point.x().expect("x present");
    let pub_y = pub_point.y().expect("y present");

    let mut input = Vec::with_capacity(P384VERIFY_INPUT_LEN);
    input.extend_from_slice(&msg);
    input.extend_from_slice(&sig.to_bytes());
    input.extend_from_slice(pub_x);
    input.extend_from_slice(pub_y);
    input
}

/// Build an invalid 224-byte p384verify input by flipping one bit in `r`.
/// Hits the "verifier rejects" path, which is what an attacker minimises
/// per-call cost on, so it matters for DoS pricing.
fn make_invalid_p384_input() -> Vec<u8> {
    let mut input = make_valid_p384_input();
    input[32] ^= 0x01;
    input
}

fn decode_hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn bench_p384_verify(c: &mut Criterion) {
    let valid = make_valid_p384_input();
    let invalid = make_invalid_p384_input();

    c.bench_function("p384_verify/valid", |b| {
        b.iter(|| {
            let out = p384_verify(black_box(&valid), black_box(P384VERIFY_BASE_GAS_FEE));
            black_box(out)
        })
    });

    c.bench_function("p384_verify/invalid_sig", |b| {
        b.iter(|| {
            let out = p384_verify(black_box(&invalid), black_box(P384VERIFY_BASE_GAS_FEE));
            black_box(out)
        })
    });
}

fn bench_p256_verify_osaka_baseline(c: &mut Criterion) {
    let valid = decode_hex(P256_VALID_INPUT_HEX);

    c.bench_function("p256_verify_osaka/valid", |b| {
        b.iter(|| {
            let out =
                secp256r1::p256_verify_osaka(black_box(&valid), black_box(u64::MAX));
            black_box(out)
        })
    });
}

criterion_group!(benches, bench_p384_verify, bench_p256_verify_osaka_baseline);
criterion_main!(benches);
