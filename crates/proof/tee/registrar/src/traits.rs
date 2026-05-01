//! Abstraction traits for the registration driver.

use async_trait::async_trait;
use url::Url;

use crate::{ProverInstance, Result, SignerAttestationKind};

/// Discovers active prover instances from the infrastructure layer.
///
/// The primary implementation is [`AwsTargetGroupDiscovery`], which queries
/// an ALB target group via the AWS SDK. Other implementations (e.g., a static
/// list for local testing) can be substituted.
#[async_trait]
pub trait InstanceDiscovery: Send + Sync {
    /// Return the current set of prover instances with their health status.
    async fn discover_instances(&self) -> Result<Vec<ProverInstance>>;
}

#[async_trait]
impl InstanceDiscovery for Box<dyn InstanceDiscovery> {
    async fn discover_instances(&self) -> Result<Vec<ProverInstance>> {
        (**self).discover_instances().await
    }
}

/// Fetches signer identity data from a prover instance endpoint.
///
/// The primary implementation is [`ProverClient`](crate::ProverClient), which
/// makes JSON-RPC calls to the prover's `enclave_signerPublicKey` and
/// `enclave_signerAttestation` endpoints. Test code can substitute a mock
/// to avoid real HTTP calls.
///
/// The `endpoint` parameter is a [`Url`] (e.g. `http://10.0.1.5:8000/`).
#[async_trait]
pub trait SignerClient: Send + Sync {
    /// Fetches the TEE attestation family exposed by the prover endpoint.
    async fn attestation_kind(&self, endpoint: &Url) -> Result<SignerAttestationKind>;

    /// Fetches the SEC1-encoded public key for each enclave signer at the given endpoint.
    async fn signer_public_key(&self, endpoint: &Url) -> Result<Vec<Vec<u8>>>;

    /// Fetches the raw platform-specific attestation for each enclave signer at the endpoint.
    ///
    /// Optional `user_data` and `nonce` bind the attestation to a specific
    /// request (e.g. a random nonce for replay protection).
    async fn signer_attestation(
        &self,
        endpoint: &Url,
        user_data: Option<Vec<u8>>,
        nonce: Option<Vec<u8>>,
    ) -> Result<Vec<Vec<u8>>>;
}
