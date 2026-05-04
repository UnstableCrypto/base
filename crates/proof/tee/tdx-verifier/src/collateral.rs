//! Explicit TDX collateral, signing chain, and revocation evidence inputs.

use alloy_primitives::{B256, Bytes, keccak256};
use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
use serde::{Deserialize, Deserializer, de};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use x509_parser::{
    certificate::X509Certificate,
    extensions::ParsedExtension,
    prelude::{CertificateRevocationList, FromDer},
};

use crate::{
    ParsedTdxQuote, QE_REPORT_ATTRIBUTES_LEN, QE_REPORT_ATTRIBUTES_OFFSET,
    QE_REPORT_ISV_PROD_ID_OFFSET, QE_REPORT_ISV_SVN_OFFSET, QE_REPORT_MISCSELECT_LEN,
    QE_REPORT_MISCSELECT_OFFSET, QE_REPORT_MRSIGNER_LEN, QE_REPORT_MRSIGNER_OFFSET, Result,
    TDX_TEE_TYPE, TDXTcbStatus, TdxVerifierError,
};

/// Subject common name expected for Intel PCS TCB collateral signing certificates.
pub const INTEL_TCB_SIGNING_CERT_COMMON_NAME: &str = "Intel SGX TCB Signing";

/// Intel TCB info identifier expected for TDX collateral.
pub const TDX_TCB_INFO_ID: &str = "TDX";

/// Intel QE identity identifier expected for TDX quotes.
pub const TDX_QE_IDENTITY_ID: &str = "TD_QE";

/// Intel QE identity schema version expected for TDX quotes.
pub const TDX_QE_IDENTITY_VERSION: u16 = 2;

/// Intel TCB status values reported by TDX TCB collateral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntelTcbStatus {
    /// Platform TCB is up to date.
    UpToDate,
    /// Platform needs software hardening.
    SwHardeningNeeded,
    /// Platform needs configuration hardening.
    ConfigurationNeeded,
    /// Platform needs configuration and software hardening.
    ConfigurationAndSwHardeningNeeded,
    /// Platform TCB is out of date.
    OutOfDate,
    /// Platform TCB is out of date and needs configuration hardening.
    OutOfDateConfigurationNeeded,
    /// Platform TCB has been revoked.
    Revoked,
    /// Status is not understood by this verifier.
    Unsupported,
}

impl IntelTcbStatus {
    /// Parses an Intel TCB status string.
    pub fn from_intel_str(status: &str) -> Self {
        match status {
            "UpToDate" => Self::UpToDate,
            "SWHardeningNeeded" | "SwHardeningNeeded" => Self::SwHardeningNeeded,
            "ConfigurationNeeded" => Self::ConfigurationNeeded,
            "ConfigurationAndSWHardeningNeeded" | "ConfigurationAndSwHardeningNeeded" => {
                Self::ConfigurationAndSwHardeningNeeded
            }
            "OutOfDate" => Self::OutOfDate,
            "OutOfDateConfigurationNeeded" => Self::OutOfDateConfigurationNeeded,
            "Revoked" => Self::Revoked,
            _ => Self::Unsupported,
        }
    }

    /// Maps an Intel TCB status into the contract's reduced `TDXTcbStatus`.
    pub const fn to_contract_status(self) -> TDXTcbStatus {
        match self {
            Self::UpToDate => TDXTcbStatus::UpToDate,
            Self::SwHardeningNeeded => TDXTcbStatus::SwHardeningNeeded,
            Self::ConfigurationNeeded => TDXTcbStatus::ConfigurationNeeded,
            Self::ConfigurationAndSwHardeningNeeded => {
                TDXTcbStatus::ConfigurationAndSwHardeningNeeded
            }
            Self::OutOfDate => TDXTcbStatus::OutOfDate,
            Self::OutOfDateConfigurationNeeded => TDXTcbStatus::OutOfDateConfigurationNeeded,
            Self::Revoked => TDXTcbStatus::Revoked,
            Self::Unsupported => TDXTcbStatus::Unknown,
        }
    }

    /// Combines the platform TCB status with the TDX module identity TCB status.
    pub const fn converge_with_tdx_module_status(self, module_status: Self) -> Self {
        match module_status {
            Self::OutOfDate => match self {
                Self::UpToDate | Self::SwHardeningNeeded => Self::OutOfDate,
                Self::ConfigurationNeeded | Self::ConfigurationAndSwHardeningNeeded => {
                    Self::OutOfDateConfigurationNeeded
                }
                status => status,
            },
            Self::Revoked => Self::Revoked,
            Self::UpToDate => self,
            Self::SwHardeningNeeded
            | Self::ConfigurationNeeded
            | Self::ConfigurationAndSwHardeningNeeded
            | Self::OutOfDateConfigurationNeeded
            | Self::Unsupported => Self::Unsupported,
        }
    }

    /// Returns true when a QE identity TCB status is acceptable.
    pub const fn is_accepted_qe_identity_status(self) -> bool {
        matches!(self, Self::UpToDate)
    }
}

impl<'de> Deserialize<'de> for IntelTcbStatus {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from_intel_str(&value))
    }
}

/// Platform identity fields authenticated by the PCK certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdxPlatformIdentity {
    /// Intel FMSPC bytes for the platform.
    pub fmspc: Bytes,
    /// Intel PCE ID bytes for the platform.
    pub pce_id: Bytes,
}

impl TdxPlatformIdentity {
    /// Extracts Intel platform identity extensions from an authenticated PCK certificate.
    pub fn from_pck_certificate_der(raw: &[u8]) -> Result<Self> {
        let (_, cert) = X509Certificate::from_der(raw).map_err(|e| {
            TdxVerifierError::PckCertChainInvalid(format!("X.509 parse failed: {e}"))
        })?;
        let mut fmspc = None;
        let mut pce_id = None;

        for extension in cert.tbs_certificate.extensions() {
            if fmspc.is_some() && pce_id.is_some() {
                break;
            }
            match extension.oid.to_id_string().as_str() {
                "1.2.840.113741.1.13.1.3" => {
                    pce_id = Some(
                        Self::decode_extension_octets(extension.value)
                            .map_err(TdxVerifierError::PckCertChainInvalid)?,
                    );
                }
                "1.2.840.113741.1.13.1.4" => {
                    fmspc = Some(
                        Self::decode_extension_octets(extension.value)
                            .map_err(TdxVerifierError::PckCertChainInvalid)?,
                    );
                }
                _ => {
                    if fmspc.is_none() {
                        fmspc = Self::find_nested_oid_octets(
                            extension.value,
                            "1.2.840.113741.1.13.1.4",
                        );
                    }
                    if pce_id.is_none() {
                        pce_id = Self::find_nested_oid_octets(
                            extension.value,
                            "1.2.840.113741.1.13.1.3",
                        );
                    }
                }
            }
        }

        Ok(Self {
            fmspc: fmspc.ok_or_else(|| {
                TdxVerifierError::PckCertChainInvalid("PCK certificate is missing FMSPC".into())
            })?,
            pce_id: pce_id.ok_or_else(|| {
                TdxVerifierError::PckCertChainInvalid("PCK certificate is missing PCE ID".into())
            })?,
        })
    }

    /// Builds a platform identity from signed TCB info JSON hex fields.
    pub fn from_tcb_info(fmspc: &str, pce_id: &str) -> Result<Self> {
        Ok(Self {
            fmspc: CollateralVerifier::decode_hex(fmspc)
                .map_err(TdxVerifierError::TcbInfoInvalid)?,
            pce_id: CollateralVerifier::decode_hex(pce_id)
                .map_err(TdxVerifierError::TcbInfoInvalid)?,
        })
    }

    /// Reads an OCTET STRING extension payload if one wraps the platform bytes.
    pub fn decode_extension_octets(value: &[u8]) -> std::result::Result<Bytes, String> {
        if let Some((tag, content, end)) = CollateralVerifier::read_der_tlv(value, 0)
            && tag == 0x04
            && end == value.len()
        {
            return Ok(Bytes::copy_from_slice(content));
        }
        Ok(Bytes::copy_from_slice(value))
    }

    /// Finds a nested OID followed by an OCTET STRING payload inside Intel SGX extension data.
    pub fn find_nested_oid_octets(value: &[u8], target_oid: &str) -> Option<Bytes> {
        Self::find_nested_oid_value(value, target_oid).and_then(|(tag, content)| {
            if tag == 0x04 { Some(Bytes::copy_from_slice(content)) } else { None }
        })
    }

    /// Finds a nested OID followed by an unsigned INTEGER payload inside Intel SGX extension data.
    pub fn find_nested_oid_integer(
        value: &[u8],
        target_oid: &str,
    ) -> std::result::Result<Option<u64>, String> {
        Self::find_nested_oid_value(value, target_oid)
            .map(|(tag, content)| {
                if tag != 0x02 {
                    return Err(format!("{target_oid} is not encoded as DER INTEGER"));
                }
                Self::decode_der_unsigned_integer(content)
            })
            .transpose()
    }

    /// Finds a nested OID followed by any DER value inside Intel SGX extension data.
    pub fn find_nested_oid_value<'a>(value: &'a [u8], target_oid: &str) -> Option<(u8, &'a [u8])> {
        let mut offset = 0;
        while offset < value.len() {
            let (tag, content, end) = CollateralVerifier::read_der_tlv(value, offset)?;
            if tag == 0x06
                && CollateralVerifier::decode_der_oid(content).as_deref() == Some(target_oid)
                && let Some((next_tag, next_content, _)) =
                    CollateralVerifier::read_der_tlv(value, end)
            {
                return Some((next_tag, next_content));
            }
            if tag & 0x20 != 0
                && let Some(nested) = Self::find_nested_oid_value(content, target_oid)
            {
                return Some(nested);
            }
            offset = end;
        }
        None
    }

    /// Decodes a non-negative DER INTEGER body into an unsigned integer.
    pub fn decode_der_unsigned_integer(content: &[u8]) -> std::result::Result<u64, String> {
        if content.is_empty() {
            return Err("DER INTEGER is empty".into());
        }
        if content[0] & 0x80 != 0 {
            return Err("DER INTEGER is negative".into());
        }

        let significant_len = content.iter().skip_while(|byte| **byte == 0).count();
        if significant_len > std::mem::size_of::<u64>() {
            return Err("DER INTEGER exceeds u64".into());
        }

        Ok(content
            .iter()
            .skip_while(|byte| **byte == 0)
            .fold(0u64, |value, byte| (value << 8) | u64::from(*byte)))
    }
}

/// SGX/PCE TCB values authenticated by the PCK certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdxPckTcb {
    /// SGX CPU SVN component values authenticated by the PCK certificate.
    pub sgx_tcb_svn: [u8; 16],
    /// PCE SVN authenticated by the PCK certificate.
    pub pce_svn: u16,
}

impl TdxPckTcb {
    /// Extracts Intel SGX/PCE TCB extensions from an authenticated PCK certificate.
    pub fn from_pck_certificate_der(raw: &[u8]) -> Result<Self> {
        let (_, cert) = X509Certificate::from_der(raw).map_err(|e| {
            TdxVerifierError::PckCertChainInvalid(format!("X.509 parse failed: {e}"))
        })?;
        let mut sgx_tcb_svn = [0u8; 16];
        let mut sgx_tcb_seen = [false; 16];
        let mut pce_svn = None;

        for extension in cert.tbs_certificate.extensions() {
            if sgx_tcb_seen.iter().all(|seen| *seen) && pce_svn.is_some() {
                break;
            }
            for component_index in 0..sgx_tcb_svn.len() {
                if sgx_tcb_seen[component_index] {
                    continue;
                }

                let oid = format!("1.2.840.113741.1.13.1.2.{}", component_index + 1);
                if let Some(value) =
                    TdxPlatformIdentity::find_nested_oid_integer(extension.value, &oid)
                        .map_err(TdxVerifierError::PckCertChainInvalid)?
                {
                    sgx_tcb_svn[component_index] = u8::try_from(value).map_err(|_| {
                        TdxVerifierError::PckCertChainInvalid(format!(
                            "PCK certificate SGX TCB component {} exceeds u8",
                            component_index + 1
                        ))
                    })?;
                    sgx_tcb_seen[component_index] = true;
                }
            }

            if pce_svn.is_none() {
                pce_svn = TdxPlatformIdentity::find_nested_oid_integer(
                    extension.value,
                    "1.2.840.113741.1.13.1.2.17",
                )
                .map_err(TdxVerifierError::PckCertChainInvalid)?
                .map(|value| {
                    u16::try_from(value).map_err(|_| {
                        TdxVerifierError::PckCertChainInvalid(
                            "PCK certificate PCE SVN exceeds u16".into(),
                        )
                    })
                })
                .transpose()?;
            }
        }

        if sgx_tcb_seen.iter().any(|seen| !seen) {
            return Err(TdxVerifierError::PckCertChainInvalid(
                "PCK certificate is missing SGX TCB components".into(),
            ));
        }

        Ok(Self {
            sgx_tcb_svn,
            pce_svn: pce_svn.ok_or_else(|| {
                TdxVerifierError::PckCertChainInvalid("PCK certificate is missing PCE SVN".into())
            })?,
        })
    }
}

/// Certificate data supplied as explicit verifier input.
///
/// The verifier consumes the raw bytes for hashing and an explicit P-256
/// public key/signature envelope for deterministic ZK-guest-friendly chain
/// validation. Deployment code can construct this structure from DER X.509
/// collateral before entering the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdxCertificate {
    /// Raw certificate bytes, hashed into journals and trust anchors.
    pub raw: Bytes,
    /// Certificate serial number used by revocation evidence.
    pub serial: Bytes,
    /// Uncompressed P-256 subject public key: `0x04 || x || y`.
    pub subject_public_key: Bytes,
    /// Uncompressed P-256 issuer public key: `0x04 || x || y`.
    pub issuer_public_key: Bytes,
    /// Certificate validity start time in seconds since Unix epoch.
    pub not_before: u64,
    /// Certificate validity end time in seconds since Unix epoch.
    pub not_after: u64,
    /// Whether this certificate may issue child certificates.
    pub is_ca: bool,
    /// DER-encoded `TBSCertificate` bytes covered by the X.509 signature.
    pub tbs_certificate: Bytes,
    /// P-256 ECDSA signature over [`Self::to_be_signed_bytes`].
    pub signature: Bytes,
}

impl TdxCertificate {
    /// Builds a verifier certificate input from DER X.509 bytes.
    pub fn from_der(raw: Bytes, issuer_public_key: Bytes) -> Result<Self> {
        let authenticated = Self::authenticated_from_der(&raw)?;
        Ok(Self {
            raw,
            serial: authenticated.serial,
            subject_public_key: authenticated.subject_public_key,
            issuer_public_key,
            not_before: authenticated.not_before,
            not_after: authenticated.not_after,
            is_ca: authenticated.is_ca,
            tbs_certificate: authenticated.tbs_certificate,
            signature: authenticated.signature,
        })
    }

    /// Returns the contract-compatible hash of the raw certificate bytes.
    pub fn hash(&self) -> B256 {
        keccak256(&self.raw)
    }

    /// Returns the canonical certificate bytes covered by `signature`.
    pub fn to_be_signed_bytes(&self) -> &[u8] {
        &self.tbs_certificate
    }

    /// Verifies this certificate's signature with an issuer P-256 public key.
    pub fn verify_signature(&self, issuer_public_key: &[u8]) -> Result<()> {
        if self.tbs_certificate.is_empty() {
            return Err(TdxVerifierError::PckCertChainInvalid(
                "certificate TBS bytes are empty".into(),
            ));
        }
        CollateralVerifier::verify_p256_signature(
            issuer_public_key,
            self.to_be_signed_bytes(),
            &self.signature,
            TdxVerifierError::PckCertChainInvalid("certificate signature failed".into()),
        )
    }

    /// Validates this certificate's time window at `verification_time`.
    pub fn verify_validity(&self, verification_time: u64) -> Result<()> {
        if verification_time < self.not_before || verification_time >= self.not_after {
            return Err(TdxVerifierError::PckCertChainInvalid(
                "certificate is not valid at verification time".into(),
            ));
        }
        Ok(())
    }

    /// Parses and authenticates fields that must be sourced from DER X.509 bytes.
    pub fn authenticated_from_der(raw: &[u8]) -> Result<AuthenticatedTdxCertificate> {
        let (remaining, cert) = X509Certificate::from_der(raw).map_err(|e| {
            TdxVerifierError::PckCertChainInvalid(format!("X.509 parse failed: {e}"))
        })?;
        if !remaining.is_empty() {
            return Err(TdxVerifierError::PckCertChainInvalid(
                "certificate DER has trailing bytes".into(),
            ));
        }

        let not_before = u64::try_from(cert.validity().not_before.timestamp()).map_err(|_| {
            TdxVerifierError::PckCertChainInvalid("certificate notBefore is negative".into())
        })?;
        let not_after = u64::try_from(cert.validity().not_after.timestamp()).map_err(|_| {
            TdxVerifierError::PckCertChainInvalid("certificate notAfter is negative".into())
        })?;
        let basic_constraints = cert.basic_constraints().map_err(|e| {
            TdxVerifierError::PckCertChainInvalid(format!("basicConstraints parse failed: {e}"))
        })?;

        Ok(AuthenticatedTdxCertificate {
            serial: Bytes::copy_from_slice(cert.tbs_certificate.raw_serial()),
            issuer_name: Bytes::copy_from_slice(cert.tbs_certificate.issuer().as_raw()),
            subject_name: Bytes::copy_from_slice(cert.tbs_certificate.subject().as_raw()),
            subject_public_key: Bytes::copy_from_slice(
                cert.public_key().subject_public_key.data.as_ref(),
            ),
            not_before,
            not_after,
            is_ca: basic_constraints.map(|extension| extension.value.ca).unwrap_or(false),
            tbs_certificate: Bytes::copy_from_slice(cert.tbs_certificate.as_ref()),
            signature: Bytes::copy_from_slice(cert.signature_value.data.as_ref()),
        })
    }

    /// Verifies that explicit verifier fields match the authenticated DER certificate.
    pub fn verify_authenticated_fields(
        &self,
        authenticated: &AuthenticatedTdxCertificate,
    ) -> Result<()> {
        if self.serial != authenticated.serial
            || self.subject_public_key != authenticated.subject_public_key
            || self.not_before != authenticated.not_before
            || self.not_after != authenticated.not_after
            || self.is_ca != authenticated.is_ca
            || self.tbs_certificate != authenticated.tbs_certificate
            || self.signature != authenticated.signature
        {
            return Err(TdxVerifierError::PckCertChainInvalid(
                "explicit certificate fields do not match DER certificate".into(),
            ));
        }
        Ok(())
    }
}

/// Certificate fields authenticated by DER X.509 parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedTdxCertificate {
    /// DER certificate serial number.
    pub serial: Bytes,
    /// DER-encoded issuer name.
    pub issuer_name: Bytes,
    /// DER-encoded subject name.
    pub subject_name: Bytes,
    /// Uncompressed P-256 subject public key: `0x04 || x || y`.
    pub subject_public_key: Bytes,
    /// Certificate validity start time in seconds since Unix epoch.
    pub not_before: u64,
    /// Certificate validity end time in seconds since Unix epoch.
    pub not_after: u64,
    /// Whether this certificate may issue child certificates.
    pub is_ca: bool,
    /// DER-encoded `TBSCertificate` bytes covered by the X.509 signature.
    pub tbs_certificate: Bytes,
    /// DER-encoded P-256 ECDSA signature over `tbs_certificate`.
    pub signature: Bytes,
}

/// Signed collateral document with its signing chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdxSignedCollateral {
    /// Raw collateral bytes consumed by the verifier.
    pub raw: Bytes,
    /// Root-to-leaf signing certificate chain for this collateral.
    pub signing_chain: Vec<TdxCertificate>,
    /// P-256 ECDSA signature over the selected signed JSON body.
    pub signature: Bytes,
    /// Collateral issue time in seconds since Unix epoch.
    pub issue_time: u64,
    /// Collateral expiration time in seconds since Unix epoch.
    pub next_update: u64,
}

/// JSON body kind covered by an Intel PCS collateral signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TdxSignedCollateralBody {
    /// Signed TCB info body stored under `tcbInfo`.
    TcbInfo,
    /// Signed QE identity body stored under `enclaveIdentity`.
    QeIdentity,
}

impl TdxSignedCollateralBody {
    /// Returns the signed JSON field name for this collateral body.
    pub const fn json_key(self) -> &'static str {
        match self {
            Self::TcbInfo => "tcbInfo",
            Self::QeIdentity => "enclaveIdentity",
        }
    }
}

impl TdxSignedCollateral {
    /// Returns the contract-compatible hash of the raw collateral bytes.
    pub fn hash(&self) -> B256 {
        keccak256(&self.raw)
    }

    /// Derives the matching TCB status from the signed TCB info document.
    pub fn tcb_status_for_quote(
        &self,
        quote: &ParsedTdxQuote,
        pck_tcb: &TdxPckTcb,
    ) -> Result<IntelTcbStatus> {
        let document = self.tcb_info_document()?;
        document.tcb_info.tcb_status_for_quote(quote, pck_tcb)
    }

    /// Parses this signed collateral as an Intel TCB info JSON document.
    pub fn tcb_info_document(&self) -> Result<TdxTcbInfoDocument> {
        serde_json::from_slice(&self.raw).map_err(|e| {
            TdxVerifierError::TcbInfoInvalid(format!("TCB info JSON parse failed: {e}"))
        })
    }

    /// Parses this signed collateral as an Intel QE identity JSON document.
    pub fn qe_identity_document(&self) -> Result<TdxQeIdentityDocument> {
        serde_json::from_slice(&self.raw).map_err(|e| {
            TdxVerifierError::QeIdentityInvalid(format!("QE identity JSON parse failed: {e}"))
        })
    }

    /// Extracts issue and next-update times from the signed collateral JSON body.
    pub fn signed_validity(
        &self,
        body_kind: TdxSignedCollateralBody,
        error_mapper: fn(String) -> TdxVerifierError,
    ) -> Result<TdxSignedCollateralValidity> {
        let document: Value =
            serde_json::from_slice(&self.raw).map_err(|e| error_mapper(format!("{e}")))?;
        let body = Self::signed_body_value(&document, body_kind, error_mapper)?;
        let issue_time = Self::signed_time_field(body, "issueDate", error_mapper)?;
        let next_update = Self::signed_time_field(body, "nextUpdate", error_mapper)?;
        Ok(TdxSignedCollateralValidity { issue_time, next_update })
    }

    /// Serializes the JSON value covered by the PCS collateral signature.
    pub fn signed_body_bytes(
        &self,
        body_kind: TdxSignedCollateralBody,
        error_mapper: fn(String) -> TdxVerifierError,
    ) -> Result<Vec<u8>> {
        Self::signed_body_bytes_from_raw(&self.raw, body_kind, error_mapper)
    }

    /// Serializes the signed JSON body from raw Intel PCS collateral bytes.
    pub fn signed_body_bytes_from_raw(
        raw: &[u8],
        body_kind: TdxSignedCollateralBody,
        error_mapper: fn(String) -> TdxVerifierError,
    ) -> Result<Vec<u8>> {
        let document: Value =
            serde_json::from_slice(raw).map_err(|e| error_mapper(format!("{e}")))?;
        let body = Self::signed_body_value(&document, body_kind, error_mapper)?;
        serde_json::to_vec(body)
            .map_err(|e| error_mapper(format!("collateral signed body serialization failed: {e}")))
    }

    /// Returns the JSON value covered by the PCS collateral signature.
    pub fn signed_body_value(
        document: &Value,
        body_kind: TdxSignedCollateralBody,
        error_mapper: fn(String) -> TdxVerifierError,
    ) -> Result<&Value> {
        let has_tcb_info = document.get(TdxSignedCollateralBody::TcbInfo.json_key()).is_some();
        let has_qe_identity =
            document.get(TdxSignedCollateralBody::QeIdentity.json_key()).is_some();
        if has_tcb_info && has_qe_identity {
            return Err(error_mapper("collateral JSON contains multiple signed bodies".into()));
        }

        document
            .get(body_kind.json_key())
            .ok_or_else(|| error_mapper(format!("{} body is missing", body_kind.json_key())))
    }

    /// Extracts a signed timestamp field from a collateral JSON body.
    pub fn signed_time_field(
        body: &Value,
        field: &str,
        error_mapper: fn(String) -> TdxVerifierError,
    ) -> Result<u64> {
        match body.get(field) {
            Some(Value::Number(number)) => number
                .as_u64()
                .ok_or_else(|| error_mapper(format!("{field} is not an unsigned timestamp"))),
            Some(Value::String(value)) => CollateralVerifier::parse_rfc3339_seconds(value)
                .map_err(|message| error_mapper(format!("{field} is invalid: {message}"))),
            Some(_) => Err(error_mapper(format!("{field} has unsupported type"))),
            None => Err(error_mapper(format!("{field} is missing"))),
        }
    }
}

/// Issue and expiration times authenticated by signed collateral JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TdxSignedCollateralValidity {
    /// Collateral issue time in seconds since Unix epoch.
    pub issue_time: u64,
    /// Collateral expiration time in seconds since Unix epoch.
    pub next_update: u64,
}

/// Signed Intel TCB info JSON document body.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxTcbInfoDocument {
    /// TCB info payload.
    #[serde(rename = "tcbInfo")]
    pub tcb_info: TdxTcbInfoBody,
}

/// Intel TCB info payload fields used by this verifier.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxTcbInfoBody {
    /// Intel collateral class identifier.
    pub id: String,
    /// Intel TEE type for TDX, when supplied by the PCS response.
    #[serde(default, rename = "teeType")]
    pub tee_type: Option<TdxTeeType>,
    /// Collateral issue date authenticated inside signed JSON.
    #[serde(rename = "issueDate")]
    pub issue_date: String,
    /// Collateral expiration authenticated inside signed JSON.
    #[serde(rename = "nextUpdate")]
    pub next_update: String,
    /// Platform FMSPC as Intel hex text.
    pub fmspc: String,
    /// Platform PCE ID as Intel hex text.
    #[serde(rename = "pceId", alias = "pceid")]
    pub pce_id: String,
    /// Default TDX module identity authenticated in this TCB info document.
    #[serde(rename = "tdxModule")]
    pub tdx_module: TdxModule,
    /// Versioned TDX module identities authenticated in this TCB info document.
    #[serde(rename = "tdxModuleIdentities")]
    pub tdx_module_identities: Vec<TdxModuleIdentity>,
    /// Ordered TCB levels from the signed TCB info document.
    #[serde(rename = "tcbLevels")]
    pub tcb_levels: Vec<TdxTcbLevel>,
}

impl TdxTcbInfoBody {
    /// Verifies that this signed TCB info document is TDX collateral.
    pub fn verify_tdx_collateral(&self) -> Result<()> {
        if self.id != TDX_TCB_INFO_ID
            || self.tee_type.is_some_and(|tee_type| tee_type.value != TDX_TEE_TYPE)
        {
            return Err(TdxVerifierError::TcbInfoInvalid("TCB info is not TDX collateral".into()));
        }
        Ok(())
    }

    /// Returns the signed platform identity for this TCB info document.
    pub fn platform_identity(&self) -> Result<TdxPlatformIdentity> {
        TdxPlatformIdentity::from_tcb_info(&self.fmspc, &self.pce_id)
    }

    /// Verifies that this signed TCB info applies to the PCK certificate platform.
    pub fn verify_platform(&self, pck_platform: &TdxPlatformIdentity) -> Result<()> {
        let tcb_platform = self.platform_identity()?;
        if tcb_platform != *pck_platform {
            return Err(TdxVerifierError::TcbInfoInvalid(
                "TCB info FMSPC/PCE ID does not match PCK certificate".into(),
            ));
        }
        Ok(())
    }

    /// Selects the first TCB level matching the PCK SGX/PCE TCB and quote TDX module identity.
    pub fn tcb_status_for_quote(
        &self,
        quote: &ParsedTdxQuote,
        pck_tcb: &TdxPckTcb,
    ) -> Result<IntelTcbStatus> {
        self.verify_tdx_collateral()?;
        let platform_status = self
            .tcb_levels
            .iter()
            .find(|level| level.tcb.matches_quote_and_pck(quote, pck_tcb))
            .map(|level| level.tcb_status)
            .ok_or_else(|| {
                TdxVerifierError::TcbInfoInvalid("no TCB info level matches quote TCB".into())
            })?;
        let module_status = self.tdx_module_status_for_quote(quote)?;
        Ok(platform_status.converge_with_tdx_module_status(module_status))
    }

    /// Returns the TDX module identity TCB status for the loaded module in the quote.
    pub fn tdx_module_status_for_quote(&self, quote: &ParsedTdxQuote) -> Result<IntelTcbStatus> {
        let module = self.tdx_module_for_quote(quote)?;
        module.verify_quote(quote)?;
        if quote.tee_tcb_svn[1] == 0 {
            return Ok(IntelTcbStatus::UpToDate);
        }
        let module_isvsvn = u32::from(quote.tee_tcb_svn[0]);
        module
            .tcb_levels()
            .iter()
            .find(|level| level.tcb.isvsvn <= module_isvsvn)
            .map(|level| level.tcb_status)
            .ok_or_else(|| {
                TdxVerifierError::TcbInfoInvalid(
                    "no TDX module identity TCB level matches quote".into(),
                )
            })
    }

    /// Returns the signed TDX module identity that applies to the quote.
    pub fn tdx_module_for_quote(&self, quote: &ParsedTdxQuote) -> Result<TdxModuleReference<'_>> {
        let module_version = quote.tee_tcb_svn[1];
        if module_version == 0 {
            return Ok(TdxModuleReference::Module(&self.tdx_module));
        }
        let expected_id = format!("TDX_{module_version:02X}");
        self.tdx_module_identities
            .iter()
            .find(|identity| identity.id.eq_ignore_ascii_case(&expected_id))
            .map(TdxModuleReference::Identity)
            .ok_or_else(|| {
                TdxVerifierError::TcbInfoInvalid(format!(
                    "no TDX module identity matches quote module version {module_version}"
                ))
            })
    }
}

/// Intel TEE type parsed from a signed TCB info document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TdxTeeType {
    /// Numeric TEE type value.
    pub value: u32,
}

impl TdxTeeType {
    /// Parses Intel TEE type text as hexadecimal, accepting an optional `0x` prefix.
    pub fn parse_hex(value: &str) -> std::result::Result<Self, String> {
        let value = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")).unwrap_or(value);
        u32::from_str_radix(value, 16)
            .map(|value| Self { value })
            .map_err(|e| format!("teeType parse failed: {e}"))
    }
}

impl<'de> Deserialize<'de> for TdxTeeType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Number(number) => {
                let value = number
                    .as_u64()
                    .ok_or_else(|| de::Error::custom("teeType is not an unsigned integer"))?;
                let value =
                    u32::try_from(value).map_err(|_| de::Error::custom("teeType exceeds u32"))?;
                Ok(Self { value })
            }
            Value::String(value) => Self::parse_hex(&value).map_err(de::Error::custom),
            _ => Err(de::Error::custom("teeType has unsupported type")),
        }
    }
}

/// One TCB level from the signed TCB info document.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxTcbLevel {
    /// Component SVN requirements for this level.
    pub tcb: TdxTcbComponents,
    /// Intel status for this level.
    #[serde(rename = "tcbStatus")]
    pub tcb_status: IntelTcbStatus,
}

/// Component SVN requirements from one TCB level.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxTcbComponents {
    /// Minimum PCE SVN for this level.
    pub pcesvn: u16,
    /// TDX TCB component SVNs for this level.
    #[serde(default, rename = "tdxtcbcomponents", alias = "tdxTcbComponents")]
    pub tdxtcbcomponents: Vec<TdxTcbComponent>,
    /// SGX TCB component SVNs used by some collateral encodings.
    #[serde(default, rename = "sgxtcbcomponents", alias = "sgxTcbComponents")]
    pub sgxtcbcomponents: Vec<TdxTcbComponent>,
}

impl TdxTcbComponents {
    /// Returns true when this TCB level applies to the PCK certificate and quote.
    pub fn matches_quote_and_pck(&self, quote: &ParsedTdxQuote, pck_tcb: &TdxPckTcb) -> bool {
        self.matches_pck_tcb(pck_tcb) && self.matches_quote_tdx_tcb(quote)
    }

    /// Returns true when this level's SGX/PCE requirements match the PCK certificate.
    pub fn matches_pck_tcb(&self, pck_tcb: &TdxPckTcb) -> bool {
        self.pcesvn <= pck_tcb.pce_svn
            && self.sgxtcbcomponents.len() == pck_tcb.sgx_tcb_svn.len()
            && self
                .sgxtcbcomponents
                .iter()
                .zip(pck_tcb.sgx_tcb_svn)
                .all(|(component, pck_svn)| component.svn <= u16::from(pck_svn))
    }

    /// Returns true when this level's TDX requirements match the quote report body.
    pub fn matches_quote_tdx_tcb(&self, quote: &ParsedTdxQuote) -> bool {
        let component_start = if quote.tee_tcb_svn[1] > 0 { 2 } else { 0 };
        self.tdxtcbcomponents.len() == quote.tee_tcb_svn.len()
            && self
                .tdxtcbcomponents
                .iter()
                .skip(component_start)
                .zip(quote.tee_tcb_svn.iter().skip(component_start))
                .all(|(component, quote_svn)| component.svn <= u16::from(*quote_svn))
    }
}

/// One TCB component SVN.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxTcbComponent {
    /// Security version number for this component.
    pub svn: u16,
}

/// Signed default TDX module identity fields from TCB info collateral.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxModule {
    /// Expected TDX module signer measurement as hex text.
    pub mrsigner: String,
    /// Expected TDX module SEAM attributes as hex text.
    pub attributes: String,
    /// Mask applied when comparing TDX module SEAM attributes.
    #[serde(rename = "attributesMask")]
    pub attributes_mask: String,
}

impl TdxModule {
    /// Verifies this module identity against the quote report body.
    pub fn verify_quote(&self, quote: &ParsedTdxQuote) -> Result<()> {
        TdxModuleIdentityFields::new(&self.mrsigner, &self.attributes, &self.attributes_mask)
            .verify_quote(quote)
    }

    /// Returns no module identity TCB levels for the default module entry.
    pub const fn tcb_levels(&self) -> &[TdxModuleTcbLevel] {
        &[]
    }
}

/// Signed versioned TDX module identity from TCB info collateral.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxModuleIdentity {
    /// Versioned module identity ID, such as `TDX_03`.
    pub id: String,
    /// Expected TDX module signer measurement as hex text.
    pub mrsigner: String,
    /// Expected TDX module SEAM attributes as hex text.
    pub attributes: String,
    /// Mask applied when comparing TDX module SEAM attributes.
    #[serde(rename = "attributesMask")]
    pub attributes_mask: String,
    /// Ordered TCB levels for this module identity.
    #[serde(rename = "tcbLevels")]
    pub tcb_levels: Vec<TdxModuleTcbLevel>,
}

impl TdxModuleIdentity {
    /// Verifies this module identity against the quote report body.
    pub fn verify_quote(&self, quote: &ParsedTdxQuote) -> Result<()> {
        TdxModuleIdentityFields::new(&self.mrsigner, &self.attributes, &self.attributes_mask)
            .verify_quote(quote)
    }

    /// Returns module identity TCB levels.
    pub fn tcb_levels(&self) -> &[TdxModuleTcbLevel] {
        &self.tcb_levels
    }
}

/// TDX module identity reference selected for a quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TdxModuleReference<'a> {
    /// Default TDX module identity.
    Module(&'a TdxModule),
    /// Versioned TDX module identity.
    Identity(&'a TdxModuleIdentity),
}

impl TdxModuleReference<'_> {
    /// Verifies this module identity against the quote report body.
    pub fn verify_quote(&self, quote: &ParsedTdxQuote) -> Result<()> {
        match self {
            Self::Module(module) => module.verify_quote(quote),
            Self::Identity(identity) => identity.verify_quote(quote),
        }
    }

    /// Returns TCB levels for this module identity.
    pub fn tcb_levels(&self) -> &[TdxModuleTcbLevel] {
        match self {
            Self::Module(module) => module.tcb_levels(),
            Self::Identity(identity) => identity.tcb_levels(),
        }
    }
}

/// One TDX module identity TCB level.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxModuleTcbLevel {
    /// Module identity TCB requirement.
    pub tcb: TdxModuleTcb,
    /// Intel status for this module identity level.
    #[serde(rename = "tcbStatus")]
    pub tcb_status: IntelTcbStatus,
}

/// TDX module identity TCB requirement.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxModuleTcb {
    /// Minimum module ISV SVN for this level.
    pub isvsvn: u32,
}

/// Shared TDX module identity fields used for quote matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TdxModuleIdentityFields<'a> {
    /// Expected TDX module signer measurement as hex text.
    pub mrsigner: &'a str,
    /// Expected TDX module SEAM attributes as hex text.
    pub attributes: &'a str,
    /// Mask applied when comparing TDX module SEAM attributes.
    pub attributes_mask: &'a str,
}

impl<'a> TdxModuleIdentityFields<'a> {
    /// Builds a borrowed view of common module identity fields.
    pub const fn new(mrsigner: &'a str, attributes: &'a str, attributes_mask: &'a str) -> Self {
        Self { mrsigner, attributes, attributes_mask }
    }

    /// Verifies common module identity fields against the quote report body.
    pub fn verify_quote(&self, quote: &ParsedTdxQuote) -> Result<()> {
        let expected_mrsigner =
            CollateralVerifier::decode_hex_exact(self.mrsigner, quote.mrsigner_seam.len())
                .map_err(TdxVerifierError::TcbInfoInvalid)?;
        if quote.mrsigner_seam.as_slice() != expected_mrsigner.as_ref() {
            return Err(TdxVerifierError::TcbInfoInvalid(
                "TDX module signer does not match quote MRSIGNERSEAM".into(),
            ));
        }
        let attributes_match = CollateralVerifier::masked_bytes_match(
            &quote.seam_attributes,
            self.attributes,
            self.attributes_mask,
        )
        .map_err(TdxVerifierError::TcbInfoInvalid)?;
        if !attributes_match {
            return Err(TdxVerifierError::TcbInfoInvalid(
                "TDX module attributes do not match quote SEAM attributes".into(),
            ));
        }
        Ok(())
    }
}

/// Signed Intel QE identity JSON document.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxQeIdentityDocument {
    /// QE identity payload.
    #[serde(rename = "enclaveIdentity")]
    pub enclave_identity: TdxQeIdentityBody,
}

/// Intel QE identity fields used to authenticate the quote's QE report.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxQeIdentityBody {
    /// Intel collateral class identifier.
    pub id: String,
    /// Intel collateral schema version.
    pub version: u16,
    /// Collateral issue date authenticated inside signed JSON.
    #[serde(rename = "issueDate")]
    pub issue_date: String,
    /// Collateral expiration authenticated inside signed JSON.
    #[serde(rename = "nextUpdate")]
    pub next_update: String,
    /// Expected QE `MISCSELECT` as hex text.
    pub miscselect: String,
    /// QE `MISCSELECT` mask as hex text.
    #[serde(rename = "miscselectMask")]
    pub miscselect_mask: String,
    /// Expected QE attributes as hex text.
    pub attributes: String,
    /// QE attributes mask as hex text.
    #[serde(rename = "attributesMask")]
    pub attributes_mask: String,
    /// Expected QE signer measurement as hex text.
    pub mrsigner: String,
    /// Expected QE product ID.
    pub isvprodid: u16,
    /// Ordered QE identity TCB levels.
    #[serde(rename = "tcbLevels")]
    pub tcb_levels: Vec<TdxQeIdentityLevel>,
}

impl TdxQeIdentityBody {
    /// Verifies this signed QE identity against the PCK-signed QE report.
    pub fn verify_qe_report(&self, quote: &ParsedTdxQuote) -> Result<()> {
        self.verify_tdx_identity()?;

        let miscselect = quote
            .qe_report
            .get(
                QE_REPORT_MISCSELECT_OFFSET..QE_REPORT_MISCSELECT_OFFSET + QE_REPORT_MISCSELECT_LEN,
            )
            .ok_or_else(|| {
                TdxVerifierError::InvalidQuote("QE report miscselect read out of bounds".into())
            })?;
        let attributes = quote
            .qe_report
            .get(
                QE_REPORT_ATTRIBUTES_OFFSET..QE_REPORT_ATTRIBUTES_OFFSET + QE_REPORT_ATTRIBUTES_LEN,
            )
            .ok_or_else(|| {
                TdxVerifierError::InvalidQuote("QE report attributes read out of bounds".into())
            })?;
        let mrsigner = quote
            .qe_report
            .get(QE_REPORT_MRSIGNER_OFFSET..QE_REPORT_MRSIGNER_OFFSET + QE_REPORT_MRSIGNER_LEN)
            .ok_or_else(|| {
                TdxVerifierError::InvalidQuote("QE report mrsigner read out of bounds".into())
            })?;
        let isvprodid =
            CollateralVerifier::read_u16_le_bytes(&quote.qe_report, QE_REPORT_ISV_PROD_ID_OFFSET)
                .map_err(TdxVerifierError::InvalidQuote)?;
        let isvsvn =
            CollateralVerifier::read_u16_le_bytes(&quote.qe_report, QE_REPORT_ISV_SVN_OFFSET)
                .map_err(TdxVerifierError::InvalidQuote)?;

        Self::verify_masked_field(
            miscselect,
            &self.miscselect,
            &self.miscselect_mask,
            "miscselect",
        )?;
        Self::verify_masked_field(
            attributes,
            &self.attributes,
            &self.attributes_mask,
            "attributes",
        )?;
        if mrsigner
            != CollateralVerifier::decode_hex_exact(&self.mrsigner, QE_REPORT_MRSIGNER_LEN)
                .map_err(TdxVerifierError::QeIdentityInvalid)?
                .as_ref()
        {
            return Err(TdxVerifierError::QeIdentityInvalid(
                "QE report signer does not match QE identity".into(),
            ));
        }
        if isvprodid != self.isvprodid {
            return Err(TdxVerifierError::QeIdentityInvalid(
                "QE report ISV product ID does not match QE identity".into(),
            ));
        }

        let status = self
            .tcb_levels
            .iter()
            .find(|level| level.tcb.isvsvn <= isvsvn)
            .map(|level| level.tcb_status)
            .ok_or_else(|| {
                TdxVerifierError::QeIdentityInvalid(
                    "no QE identity TCB level matches QE report".into(),
                )
            })?;
        if !status.is_accepted_qe_identity_status() {
            return Err(TdxVerifierError::QeIdentityInvalid(
                "QE identity TCB status is not accepted".into(),
            ));
        }

        Ok(())
    }

    /// Verifies a QE report field matches the signed QE identity under a hex mask.
    pub fn verify_masked_field(
        actual: &[u8],
        expected_hex: &str,
        mask_hex: &str,
        field_name: &str,
    ) -> Result<()> {
        let matches = CollateralVerifier::masked_bytes_match(actual, expected_hex, mask_hex)
            .map_err(TdxVerifierError::QeIdentityInvalid)?;
        if !matches {
            return Err(TdxVerifierError::QeIdentityInvalid(format!(
                "QE report {field_name} does not match QE identity"
            )));
        }
        Ok(())
    }

    /// Verifies the signed QE identity is the TDX identity type and schema version.
    pub fn verify_tdx_identity(&self) -> Result<()> {
        if self.id != TDX_QE_IDENTITY_ID || self.version != TDX_QE_IDENTITY_VERSION {
            return Err(TdxVerifierError::QeIdentityInvalid(
                "QE identity is not TDX TD_QE v2 collateral".into(),
            ));
        }
        Ok(())
    }
}

/// One QE identity TCB level.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxQeIdentityLevel {
    /// QE identity TCB threshold.
    pub tcb: TdxQeIdentityTcb,
    /// Intel status for this QE identity level.
    #[serde(rename = "tcbStatus")]
    pub tcb_status: IntelTcbStatus,
}

/// QE identity TCB SVN threshold.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TdxQeIdentityTcb {
    /// Minimum QE ISV SVN for this level.
    pub isvsvn: u16,
}

/// TCB info and QE identity collateral bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdxCollateral {
    /// TCB info collateral and signing chain.
    pub tcb_info: TdxSignedCollateral,
    /// QE identity collateral and signing chain.
    pub qe_identity: TdxSignedCollateral,
    /// Intel TCB status selected from the TCB info levels.
    pub tcb_status: IntelTcbStatus,
}

/// DER X.509 CRL supplied as revocation evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdxCertificateRevocationList {
    /// Raw DER-encoded X.509 certificate revocation list.
    pub raw: Bytes,
}

impl TdxCertificateRevocationList {
    /// Parses authenticated CRL fields from DER bytes.
    pub fn authenticated_from_der(raw: &[u8]) -> Result<AuthenticatedTdxCrl> {
        let (remaining, crl) = CertificateRevocationList::from_der(raw)
            .map_err(|e| TdxVerifierError::PckCertChainInvalid(format!("CRL parse failed: {e}")))?;
        if !remaining.is_empty() {
            return Err(TdxVerifierError::PckCertChainInvalid("CRL DER has trailing bytes".into()));
        }

        let this_update = u64::try_from(crl.last_update().timestamp()).map_err(|_| {
            TdxVerifierError::PckCertChainInvalid("CRL thisUpdate is negative".into())
        })?;
        let next_update = crl
            .next_update()
            .ok_or_else(|| {
                TdxVerifierError::PckCertChainInvalid("CRL nextUpdate is missing".into())
            })
            .and_then(|next_update| {
                u64::try_from(next_update.timestamp()).map_err(|_| {
                    TdxVerifierError::PckCertChainInvalid("CRL nextUpdate is negative".into())
                })
            })?;

        Ok(AuthenticatedTdxCrl {
            issuer_name: Bytes::copy_from_slice(crl.issuer().as_raw()),
            this_update,
            next_update,
            revoked_serials: crl
                .iter_revoked_certificates()
                .map(|revoked| Bytes::copy_from_slice(revoked.raw_serial()))
                .collect(),
            tbs_cert_list: Bytes::copy_from_slice(crl.tbs_cert_list.as_ref()),
            signature: Bytes::copy_from_slice(crl.signature_value.data.as_ref()),
        })
    }
}

/// Authenticated CRL fields parsed from DER.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedTdxCrl {
    /// DER-encoded issuer name.
    pub issuer_name: Bytes,
    /// CRL issue time in seconds since Unix epoch.
    pub this_update: u64,
    /// CRL expiration time in seconds since Unix epoch.
    pub next_update: u64,
    /// Certificate serials revoked by this CRL.
    pub revoked_serials: Vec<Bytes>,
    /// DER-encoded `TBSCertList` bytes covered by the CRL signature.
    pub tbs_cert_list: Bytes,
    /// P-256 ECDSA signature over `tbs_cert_list`.
    pub signature: Bytes,
}

impl AuthenticatedTdxCrl {
    /// Verifies the CRL signature with the issuer's P-256 public key.
    pub fn verify_signature(&self, issuer_public_key: &[u8]) -> Result<()> {
        CollateralVerifier::verify_p256_signature(
            issuer_public_key,
            &self.tbs_cert_list,
            &self.signature,
            TdxVerifierError::PckCertChainInvalid("CRL signature failed".into()),
        )
    }

    /// Validates this CRL's time window at `verification_time`.
    pub fn verify_validity(&self, verification_time: u64) -> Result<()> {
        if verification_time < self.this_update || verification_time >= self.next_update {
            return Err(TdxVerifierError::PckCertChainInvalid(
                "CRL is not valid at verification time".into(),
            ));
        }
        Ok(())
    }

    /// Returns true when this CRL revokes `certificate`.
    pub fn revokes_certificate(&self, certificate: &AuthenticatedTdxCertificate) -> bool {
        self.revoked_serials.iter().any(|serial| serial == &certificate.serial)
    }
}

/// Explicit signed revocation evidence supplied to the verifier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TdxRevocationEvidence {
    /// DER X.509 CRLs for all non-root certificate issuers used by verification.
    pub certificate_crls: Vec<TdxCertificateRevocationList>,
}

impl TdxRevocationEvidence {
    /// Pre-parses all supplied CRLs into authenticated form for repeated lookups.
    pub fn authenticate_crls(&self) -> Result<Vec<AuthenticatedTdxCrl>> {
        self.certificate_crls
            .iter()
            .map(|crl| TdxCertificateRevocationList::authenticated_from_der(&crl.raw))
            .collect()
    }

    /// Verifies that a certificate is covered by a fresh issuer CRL and is not revoked.
    pub fn verify_certificate_not_revoked(
        &self,
        certificate: &AuthenticatedTdxCertificate,
        issuer: &AuthenticatedTdxCertificate,
        verification_time: u64,
    ) -> Result<u64> {
        let authenticated_crls = self.authenticate_crls()?;
        Self::verify_certificate_not_revoked_with_crls(
            &authenticated_crls,
            certificate,
            issuer,
            verification_time,
        )
    }

    /// Verifies a certificate against pre-authenticated CRLs.
    pub fn verify_certificate_not_revoked_with_crls(
        authenticated_crls: &[AuthenticatedTdxCrl],
        certificate: &AuthenticatedTdxCertificate,
        issuer: &AuthenticatedTdxCertificate,
        verification_time: u64,
    ) -> Result<u64> {
        let mut found_issuer_crl = false;
        let mut earliest_next_update = u64::MAX;
        for authenticated in authenticated_crls {
            if authenticated.issuer_name != issuer.subject_name {
                continue;
            }
            found_issuer_crl = true;
            authenticated.verify_signature(&issuer.subject_public_key)?;
            authenticated.verify_validity(verification_time)?;
            earliest_next_update = earliest_next_update.min(authenticated.next_update);
            if authenticated.revokes_certificate(certificate) {
                return Err(TdxVerifierError::PckCertChainInvalid("certificate is revoked".into()));
            }
        }

        if !found_issuer_crl {
            return Err(TdxVerifierError::PckCertChainInvalid(
                "missing issuer CRL for certificate".into(),
            ));
        }
        Ok(earliest_next_update)
    }

    /// Returns the earliest nextUpdate among CRLs used to validate a certificate chain.
    pub fn certificate_chain_next_update(
        &self,
        chain: &[TdxCertificate],
        verification_time: u64,
    ) -> Result<u64> {
        let authenticated_chain = chain
            .iter()
            .map(|cert| TdxCertificate::authenticated_from_der(&cert.raw))
            .collect::<Result<Vec<_>>>()?;
        let authenticated_crls = self.authenticate_crls()?;
        let mut earliest_next_update = u64::MAX;
        for index in 1..authenticated_chain.len() {
            earliest_next_update =
                earliest_next_update.min(Self::verify_certificate_not_revoked_with_crls(
                    &authenticated_crls,
                    &authenticated_chain[index],
                    &authenticated_chain[index - 1],
                    verification_time,
                )?);
        }
        Ok(earliest_next_update)
    }
}

/// Stateless helper for collateral validation.
#[derive(Debug)]
pub struct CollateralVerifier;

impl CollateralVerifier {
    /// Validates a root-to-leaf certificate chain and returns the leaf key.
    pub fn verify_certificate_chain(
        chain: &[TdxCertificate],
        trusted_root_ca_hash: B256,
        verification_time: u64,
        revocation: &TdxRevocationEvidence,
    ) -> Result<Bytes> {
        let root = chain.first().ok_or_else(|| {
            TdxVerifierError::PckCertChainInvalid("certificate chain is empty".into())
        })?;
        if root.hash() != trusted_root_ca_hash {
            return Err(TdxVerifierError::RootCaNotTrusted);
        }

        let authenticated_chain = chain
            .iter()
            .map(|cert| TdxCertificate::authenticated_from_der(&cert.raw))
            .collect::<Result<Vec<_>>>()?;
        let authenticated_crls = revocation.authenticate_crls()?;

        for (index, cert) in chain.iter().enumerate() {
            let authenticated = &authenticated_chain[index];
            cert.verify_authenticated_fields(authenticated)?;
            if verification_time < authenticated.not_before
                || verification_time >= authenticated.not_after
            {
                return Err(TdxVerifierError::PckCertChainInvalid(
                    "certificate is not valid at verification time".into(),
                ));
            }
            if index == 0 {
                cert.verify_signature(&authenticated.subject_public_key)?;
                continue;
            }

            let issuer = &authenticated_chain[index - 1];
            if !issuer.is_ca {
                return Err(TdxVerifierError::PckCertChainInvalid(
                    "issuer certificate is not a CA".into(),
                ));
            }
            if cert.issuer_public_key != issuer.subject_public_key {
                return Err(TdxVerifierError::PckCertChainInvalid(
                    "certificate issuer key does not match parent".into(),
                ));
            }
            cert.verify_signature(&issuer.subject_public_key)?;
            if authenticated.issuer_name != issuer.subject_name {
                return Err(TdxVerifierError::PckCertChainInvalid(
                    "certificate issuer name does not match parent".into(),
                ));
            }
            TdxRevocationEvidence::verify_certificate_not_revoked_with_crls(
                &authenticated_crls,
                authenticated,
                issuer,
                verification_time,
            )?;
        }

        Ok(authenticated_chain.last().expect("chain is non-empty").subject_public_key.clone())
    }

    /// Validates signed collateral and returns its leaf signing key.
    pub fn verify_signed_collateral(
        collateral: &TdxSignedCollateral,
        body_kind: TdxSignedCollateralBody,
        trusted_root_ca_hash: B256,
        verification_time: u64,
        revocation: &TdxRevocationEvidence,
        error_mapper: fn(String) -> TdxVerifierError,
    ) -> Result<Bytes> {
        let signed_validity = collateral.signed_validity(body_kind, error_mapper)?;
        if collateral.issue_time != signed_validity.issue_time
            || collateral.next_update != signed_validity.next_update
        {
            return Err(error_mapper(
                "explicit collateral validity does not match signed JSON".into(),
            ));
        }
        if verification_time < signed_validity.issue_time
            || verification_time >= signed_validity.next_update
        {
            return Err(TdxVerifierError::CollateralExpired);
        }

        let leaf_key = Self::verify_certificate_chain(
            &collateral.signing_chain,
            trusted_root_ca_hash,
            verification_time,
            revocation,
        )
        .map_err(|e| match e {
            TdxVerifierError::RootCaNotTrusted => TdxVerifierError::RootCaNotTrusted,
            other => error_mapper(other.to_string()),
        })?;
        let leaf = collateral
            .signing_chain
            .last()
            .ok_or_else(|| error_mapper("collateral signing chain is empty".into()))?;
        Self::verify_collateral_signing_certificate(leaf, error_mapper)?;

        let signed_body = collateral.signed_body_bytes(body_kind, error_mapper)?;
        Self::verify_p256_signature(
            &leaf_key,
            &signed_body,
            &collateral.signature,
            error_mapper("collateral signature failed".into()),
        )?;

        Ok(leaf_key)
    }

    /// Verifies that a collateral leaf is the expected Intel PCS TCB signing certificate.
    pub fn verify_collateral_signing_certificate(
        certificate: &TdxCertificate,
        error_mapper: fn(String) -> TdxVerifierError,
    ) -> Result<()> {
        let (remaining, cert) = X509Certificate::from_der(&certificate.raw).map_err(|e| {
            error_mapper(format!("collateral signing certificate parse failed: {e}"))
        })?;
        if !remaining.is_empty() {
            return Err(error_mapper(
                "collateral signing certificate DER has trailing bytes".into(),
            ));
        }

        let mut common_names = cert.subject().iter_common_name();
        let common_name =
            common_names.next().and_then(|name| name.as_str().ok()).ok_or_else(|| {
                error_mapper("collateral signing certificate is missing subject common name".into())
            })?;
        if common_names.next().is_some() || common_name != INTEL_TCB_SIGNING_CERT_COMMON_NAME {
            return Err(error_mapper(
                "collateral signing certificate subject is not Intel TCB Signing".into(),
            ));
        }

        let basic_constraints = cert.basic_constraints().map_err(|e| {
            error_mapper(format!("collateral signing basicConstraints parse failed: {e}"))
        })?;
        if basic_constraints.map(|extension| extension.value.ca).unwrap_or(false) {
            return Err(error_mapper("collateral signing certificate must not be a CA".into()));
        }

        let has_digital_signature_usage =
            cert.tbs_certificate.extensions().iter().any(|extension| {
                matches!(
                    extension.parsed_extension(),
                    ParsedExtension::KeyUsage(key_usage) if key_usage.digital_signature()
                )
            });
        if !has_digital_signature_usage {
            return Err(error_mapper(
                "collateral signing certificate is missing digitalSignature key usage".into(),
            ));
        }

        Ok(())
    }

    /// Verifies a raw P-256 ECDSA signature over `message`.
    pub fn verify_p256_signature(
        public_key: &[u8],
        message: &[u8],
        signature: &[u8],
        error: TdxVerifierError,
    ) -> Result<()> {
        let verifying_key = VerifyingKey::from_sec1_bytes(public_key).map_err(|e| {
            TdxVerifierError::PckCertChainInvalid(format!("invalid P-256 public key: {e}"))
        })?;
        let signature =
            match Signature::from_slice(signature).or_else(|_| Signature::from_der(signature)) {
                Ok(signature) => signature,
                Err(e) => return Err(error.with_message(format!("{e}"))),
            };
        verifying_key.verify(message, &signature).map_err(|_| error)?;
        Ok(())
    }

    /// Parses an RFC3339 timestamp into Unix seconds.
    pub fn parse_rfc3339_seconds(value: &str) -> std::result::Result<u64, String> {
        let timestamp = OffsetDateTime::parse(value, &Rfc3339)
            .map_err(|e| format!("RFC3339 parse failed: {e}"))?
            .unix_timestamp();
        u64::try_from(timestamp).map_err(|_| "timestamp is negative".into())
    }

    /// Decodes hex text, accepting an optional `0x` prefix.
    pub fn decode_hex(value: &str) -> std::result::Result<Bytes, String> {
        let value = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")).unwrap_or(value);
        if !value.len().is_multiple_of(2) {
            return Err("hex string has odd length".into());
        }
        let mut out = Vec::with_capacity(value.len() / 2);
        let bytes = value.as_bytes();
        for index in (0..bytes.len()).step_by(2) {
            let high = Self::decode_hex_nibble(bytes[index])?;
            let low = Self::decode_hex_nibble(bytes[index + 1])?;
            out.push((high << 4) | low);
        }
        Ok(Bytes::from(out))
    }

    /// Decodes hex text and enforces a specific byte length.
    pub fn decode_hex_exact(
        value: &str,
        expected_len: usize,
    ) -> std::result::Result<Bytes, String> {
        let decoded = Self::decode_hex(value)?;
        if decoded.len() != expected_len {
            return Err(format!(
                "hex string length {} does not match expected {expected_len}",
                decoded.len()
            ));
        }
        Ok(decoded)
    }

    /// Returns true when masked bytes match expected hex text.
    pub fn masked_bytes_match(
        actual: &[u8],
        expected_hex: &str,
        mask_hex: &str,
    ) -> std::result::Result<bool, String> {
        let expected = Self::decode_hex_exact(expected_hex, actual.len())?;
        let mask = Self::decode_hex_exact(mask_hex, actual.len())?;
        Ok(actual
            .iter()
            .zip(expected.iter())
            .zip(mask.iter())
            .all(|((actual, expected), mask)| actual & mask == expected & mask))
    }

    /// Reads a little-endian `u16` from a byte slice.
    pub fn read_u16_le_bytes(bytes: &[u8], offset: usize) -> std::result::Result<u16, String> {
        let slice =
            bytes.get(offset..offset + 2).ok_or_else(|| "u16 read out of bounds".to_string())?;
        Ok(u16::from_le_bytes([slice[0], slice[1]]))
    }

    /// Reads one DER TLV element from `bytes` at `offset`.
    pub fn read_der_tlv(bytes: &[u8], offset: usize) -> Option<(u8, &[u8], usize)> {
        let tag = *bytes.get(offset)?;
        let first_len = *bytes.get(offset + 1)?;
        let mut cursor = offset + 2;
        let len = if first_len & 0x80 == 0 {
            usize::from(first_len)
        } else {
            let len_len = usize::from(first_len & 0x7f);
            if len_len == 0 || len_len > std::mem::size_of::<usize>() {
                return None;
            }
            let mut len = 0usize;
            for byte in bytes.get(cursor..cursor + len_len)? {
                len = (len << 8) | usize::from(*byte);
            }
            cursor += len_len;
            len
        };
        let end = cursor.checked_add(len)?;
        let content = bytes.get(cursor..end)?;
        Some((tag, content, end))
    }

    /// Decodes a DER OBJECT IDENTIFIER body into dotted text.
    pub fn decode_der_oid(content: &[u8]) -> Option<String> {
        let first = *content.first()?;
        let mut parts = vec![u32::from(first / 40), u32::from(first % 40)];
        let mut value = 0u32;
        for byte in &content[1..] {
            value = value.checked_mul(128)?.checked_add(u32::from(byte & 0x7f))?;
            if byte & 0x80 == 0 {
                parts.push(value);
                value = 0;
            }
        }
        if content.len() > 1 && content.last().is_some_and(|byte| byte & 0x80 != 0) {
            return None;
        }
        Some(parts.iter().map(u32::to_string).collect::<Vec<_>>().join("."))
    }

    /// Decodes one ASCII hex nibble.
    pub fn decode_hex_nibble(value: u8) -> std::result::Result<u8, String> {
        match value {
            b'0'..=b'9' => Ok(value - b'0'),
            b'a'..=b'f' => Ok(value - b'a' + 10),
            b'A'..=b'F' => Ok(value - b'A' + 10),
            _ => Err("hex string contains non-hex character".into()),
        }
    }
}
