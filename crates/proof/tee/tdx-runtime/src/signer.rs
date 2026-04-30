use std::fmt;

use alloy_primitives::{Address, Bytes, keccak256};
use alloy_signer_local::PrivateKeySigner;
use k256::ecdsa::SigningKey;
use rand_08::CryptoRng;

use crate::{Result, TdxRuntimeError};

/// Public signer identity exposed by a TDX runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignerIdentity {
    /// Uncompressed 65-byte secp256k1 public key (`0x04 || x || y`).
    pub public_key: Bytes,
    /// Ethereum address derived from `keccak256(public_key[1..65])[12..]`.
    pub address: Address,
}

/// TDX guest secp256k1 signer.
pub struct TdxSigner {
    signer: PrivateKeySigner,
}

impl TdxSigner {
    /// Generates a fresh signer key using the supplied CSPRNG.
    pub fn generate<R: CryptoRng + rand_08::RngCore>(rng: &mut R) -> Self {
        let signing_key = SigningKey::random(rng);
        Self { signer: PrivateKeySigner::from_signing_key(signing_key) }
    }

    /// Loads a signer from a 32-byte secp256k1 private key.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let signing_key = SigningKey::from_slice(bytes)
            .map_err(|error| TdxRuntimeError::SignerKey(error.to_string()))?;
        Ok(Self { signer: PrivateKeySigner::from_signing_key(signing_key) })
    }

    /// Loads a signer from hex, optionally prefixed with `0x`.
    pub fn from_hex(hex_key: &str) -> Result<Self> {
        let hex_key = hex_key.strip_prefix("0x").unwrap_or(hex_key);
        let bytes = alloy_primitives::hex::decode(hex_key)
            .map_err(|error| TdxRuntimeError::Hex(error.to_string()))?;
        Self::from_bytes(&bytes)
    }

    /// Returns the signer's public identity.
    pub fn identity(&self) -> SignerIdentity {
        SignerIdentity { public_key: self.public_key(), address: self.address() }
    }

    /// Returns the uncompressed 65-byte public key (`0x04 || x || y`).
    pub fn public_key(&self) -> Bytes {
        let verifying_key = self.signer.credential().verifying_key();
        let encoded_point = verifying_key.to_encoded_point(false);
        Bytes::copy_from_slice(encoded_point.as_bytes())
    }

    /// Returns the Ethereum address derived the same way as Nitro.
    pub const fn address(&self) -> Address {
        self.signer.address()
    }

    /// Derives the Ethereum address from an uncompressed public key.
    pub fn address_from_public_key(public_key: &[u8]) -> Result<Address> {
        if public_key.len() != 65 || public_key.first() != Some(&0x04) {
            return Err(TdxRuntimeError::InvalidPublicKey);
        }

        let hash = keccak256(&public_key[1..65]);
        Ok(Address::from_slice(&hash.as_slice()[12..]))
    }
}

impl fmt::Debug for TdxSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TdxSigner").field("address", &self.address()).finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::address;
    use rand_08::rngs::OsRng;

    use super::*;

    const TEST_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    #[test]
    fn generated_signer_has_uncompressed_public_key() {
        let signer = TdxSigner::generate(&mut OsRng);
        let public_key = signer.public_key();

        assert_eq!(public_key.len(), 65);
        assert_eq!(public_key[0], 0x04);
    }

    #[test]
    fn loaded_signer_derives_expected_nitro_compatible_identity() {
        let signer = TdxSigner::from_hex(TEST_KEY).unwrap();
        let identity = signer.identity();

        assert_eq!(identity.public_key.len(), 65);
        assert_eq!(identity.address, address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"));
        assert_eq!(
            TdxSigner::address_from_public_key(&identity.public_key).unwrap(),
            identity.address
        );
    }

    #[test]
    fn signer_debug_does_not_expose_private_key_material() {
        let signer = TdxSigner::from_hex(TEST_KEY).unwrap();
        let debug = format!("{signer:?}");

        assert!(debug.contains("TdxSigner"));
        assert!(debug.contains("address"));
        assert!(
            !debug.contains("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
        );
    }
}
