use base_proof_tee_attestation::{BoxError, TeeAttestationKind};
use base_proof_tee_tdx_collateral::TdxCollateralError;
use base_tx_manager::TxManagerError;
use thiserror::Error;

use crate::SignerAttestationKind;

/// Errors that can occur in the prover registrar.
#[derive(Debug, Error)]
pub enum RegistrarError {
    /// Instance discovery failed.
    #[error("instance discovery failed")]
    Discovery(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Failed to contact a prover instance.
    #[error("prover client error for instance {instance}")]
    ProverClient {
        /// The instance ID or IP that was being contacted.
        instance: String,
        /// The underlying error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Public key returned by a prover instance is malformed.
    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),

    /// ZK proof generation failed.
    #[error("proof generation failed")]
    ProofGeneration(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// TDX attestation hydration failed before proof generation.
    #[error("TDX attestation hydration failed")]
    TdxAttestation(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// The generated proof kind did not match the prover's advertised endpoint kind.
    #[error(
        "attestation kind mismatch for instance {instance}: expected {expected:?}, got {actual:?}"
    )]
    AttestationKindMismatch {
        /// The instance ID or IP whose proof kind did not match its RPC kind.
        instance: String,
        /// The attestation kind advertised by the prover endpoint.
        expected: SignerAttestationKind,
        /// The attestation kind produced by the proof provider.
        actual: TeeAttestationKind,
    },

    /// The endpoint's advertised attestation kind did not match its configured fleet.
    #[error(
        "endpoint attestation kind mismatch for instance {instance}: expected {expected:?}, got {actual:?}"
    )]
    EndpointAttestationKindMismatch {
        /// The instance ID or IP whose RPC kind did not match its configured fleet.
        instance: String,
        /// The attestation kind configured for the fleet.
        expected: SignerAttestationKind,
        /// The attestation kind advertised by the prover endpoint.
        actual: SignerAttestationKind,
    },

    /// On-chain registry operation failed.
    #[error("registry error")]
    Registry(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// An on-chain registry contract call failed.
    #[error("registry call failed: {context}")]
    RegistryCall {
        /// Description of the call that failed (e.g. `"isValidSigner(0x1234…)"`).
        context: String,
        /// The underlying contract call error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Transaction signing or submission failed.
    #[error("signing error")]
    Signing(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Transaction submission or confirmation failed (RPC, nonce, fee, timeout).
    #[error("transaction error")]
    Transaction(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Configuration is invalid.
    #[error("config error: {0}")]
    Config(String),

    /// CRL (Certificate Revocation List) check failed.
    #[error("CRL error: {0}")]
    Crl(#[from] crate::crl::CrlError),
}

impl From<BoxError> for RegistrarError {
    fn from(e: BoxError) -> Self {
        Self::ProofGeneration(e)
    }
}

impl From<TxManagerError> for RegistrarError {
    fn from(e: TxManagerError) -> Self {
        Self::Transaction(Box::new(e))
    }
}

impl From<TdxCollateralError> for RegistrarError {
    fn from(e: TdxCollateralError) -> Self {
        Self::TdxAttestation(Box::new(e))
    }
}

/// Convenience result alias for registrar operations.
pub type Result<T, E = RegistrarError> = std::result::Result<T, E>;
