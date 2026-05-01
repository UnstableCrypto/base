//! TDX attestation hydration for registrar proof generation.

use std::{
    collections::HashSet,
    error::Error,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use alloy_primitives::{Address, B256, Bytes, hex};
use base_proof_tee_tdx_attestation_prover::TdxAttestationProverInput;
use base_proof_tee_tdx_prover::TdxSignerAttestation;
use base_proof_tee_tdx_verifier::{
    AuthenticatedTdxCertificate, TdxCertificate, TdxCertificateRevocationList, TdxCollateral,
    TdxPckTcb, TdxPlatformIdentity, TdxQuote, TdxQuotePolicy, TdxRevocationEvidence,
    TdxSignedCollateral, TdxSignedCollateralBody, TdxVerifierError, TdxVerifierInput,
};
use reqwest::{
    StatusCode,
    header::{HeaderMap, HeaderName},
};
use x509_parser::{
    certificate::X509Certificate,
    extensions::{DistributionPointName, GeneralName, ParsedExtension},
    pem::parse_x509_pem,
    prelude::FromDer,
};

use crate::{RegistrarError, Result, TdxAttestationConfig, crl::build_crl_http_client};

/// Maximum allowed Intel PCS response size.
pub const MAX_TDX_COLLATERAL_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

const PCK_CERT_CHAIN_CERTIFICATION_DATA_TYPE: u16 = 5;
const TCB_INFO_ISSUER_CHAIN_HEADER: &str = "sgx-tcb-info-issuer-chain";
const TCB_INFO_SIGNATURE_HEADER: &str = "sgx-tcb-info-signature";
const QE_IDENTITY_ISSUER_CHAIN_HEADER: &str = "sgx-enclave-identity-issuer-chain";
const QE_IDENTITY_SIGNATURE_FIELD: &str = "signature";
const ALLOWED_INTEL_HOST_SUFFIX: &str = ".trustedservices.intel.com";

/// TDX collateral fetched from Intel PCS for one signer quote.
#[derive(Debug, Clone)]
pub struct TdxCollateralFetch {
    /// Root-to-leaf PCK certificate chain carried by the quote.
    pub pck_certificate_chain: Vec<TdxCertificate>,
    /// TCB info and QE identity collateral.
    pub collateral: TdxCollateral,
    /// CRLs covering non-root certificates in the verifier input.
    pub revocation: TdxRevocationEvidence,
    /// Trusted Intel root CA hash.
    pub trusted_root_ca_hash: B256,
}

/// Hydrates TDX signer RPC attestations into prover input bytes.
#[derive(Debug, Clone)]
pub struct TdxAttestationHydrator {
    /// Intel PCS and verifier policy configuration.
    pub config: TdxAttestationConfig,
    client: reqwest::Client,
}

impl TdxAttestationHydrator {
    /// Creates a hydrator with a hardened HTTP client.
    pub fn new(config: TdxAttestationConfig) -> Result<Self> {
        let client = build_crl_http_client(config.fetch_timeout)
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        Ok(Self { config, client })
    }

    /// Returns true if `attestation_bytes` are already encoded TDX prover input.
    pub fn is_encoded_prover_input(attestation_bytes: &[u8]) -> bool {
        TdxAttestationProverInput::decode(attestation_bytes).is_ok()
    }

    /// Converts a TDX signer attestation into encoded prover input.
    ///
    /// Legacy prover-input payloads are accepted only as containers for the
    /// quote, signer public key, and quote timestamp. Collateral and verifier
    /// policy are always rebuilt from registrar configuration.
    pub async fn hydrate_for_signer(
        &self,
        attestation_bytes: &[u8],
        expected_signer: Address,
    ) -> Result<Vec<u8>> {
        let attestation = Self::decode_attestation_payload(attestation_bytes)?;
        let collateral = self.fetch_collateral(&attestation.quote).await?;
        let verification_time = Self::now_seconds()?;
        let verifier_input = TdxVerifierInput {
            quote: attestation.quote,
            pck_certificate_chain: collateral.pck_certificate_chain,
            collateral: collateral.collateral,
            revocation: collateral.revocation,
            trusted_root_ca_hash: collateral.trusted_root_ca_hash,
            expected_public_key: attestation.signer_public_key,
            expected_signer,
            quote_timestamp_millis: attestation.quote_timestamp_millis,
            verification_time,
            policy: TdxQuotePolicy { max_quote_age_seconds: self.config.max_quote_age.as_secs() },
            allowed_tcb_statuses: self.config.allowed_tcb_statuses.clone(),
        };
        Ok(TdxAttestationProverInput::new(verifier_input).encode())
    }

    /// Decodes a current signer attestation or legacy prover input payload.
    ///
    /// Legacy prover input is reduced to the fields that originate from the
    /// signer endpoint; verifier collateral and policy must be rehydrated by
    /// the registrar.
    pub fn decode_attestation_payload(attestation_bytes: &[u8]) -> Result<TdxSignerAttestation> {
        match TdxSignerAttestation::decode(attestation_bytes) {
            Ok(attestation) => Ok(attestation),
            Err(signer_attestation_error) => {
                let prover_input = TdxAttestationProverInput::decode(attestation_bytes).map_err(
                    |prover_input_error| {
                        RegistrarError::TdxAttestation(Box::new(
                            TdxHydrationError::AttestationPayloadDecode {
                                signer_attestation_error: signer_attestation_error.to_string(),
                                prover_input_error: prover_input_error.to_string(),
                            },
                        ))
                    },
                )?;
                let verifier_input = prover_input.into_verifier_input();
                Ok(TdxSignerAttestation::new(
                    verifier_input.expected_public_key,
                    verifier_input.quote,
                    verifier_input.quote_timestamp_millis,
                ))
            }
        }
    }

    /// Fetches Intel PCS collateral and CRLs required to verify `quote`.
    pub async fn fetch_collateral(&self, quote: &[u8]) -> Result<TdxCollateralFetch> {
        let parsed_quote =
            TdxQuote::parse(quote).map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        let pck_certificate_chain = Self::pck_certificate_chain_from_quote(&parsed_quote)?;
        Self::verify_trusted_root_ca_hash(
            &pck_certificate_chain,
            self.config.trusted_root_ca_hash,
        )?;
        let pck_leaf = pck_certificate_chain.last().ok_or_else(|| {
            RegistrarError::TdxAttestation("PCK certificate chain is empty".into())
        })?;
        let platform = TdxPlatformIdentity::from_pck_certificate_der(&pck_leaf.raw)
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        let pck_tcb = TdxPckTcb::from_pck_certificate_der(&pck_leaf.raw)
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        let tcb_info = self.fetch_tcb_info(&platform).await?;
        let qe_identity = self.fetch_qe_identity().await?;
        let tcb_status = tcb_info
            .tcb_status_for_quote(&parsed_quote, &pck_tcb)
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        let collateral = TdxCollateral { tcb_info, qe_identity, tcb_status };
        let revocation = self
            .fetch_revocation_evidence(&[
                pck_certificate_chain.as_slice(),
                collateral.tcb_info.signing_chain.as_slice(),
                collateral.qe_identity.signing_chain.as_slice(),
            ])
            .await?;
        Ok(TdxCollateralFetch {
            pck_certificate_chain,
            collateral,
            revocation,
            trusted_root_ca_hash: self.config.trusted_root_ca_hash,
        })
    }

    fn now_seconds() -> Result<u64> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))
            .map(|duration| duration.as_secs())
    }

    fn pck_certificate_chain_from_quote(
        parsed_quote: &base_proof_tee_tdx_verifier::ParsedTdxQuote,
    ) -> Result<Vec<TdxCertificate>> {
        if parsed_quote.certification_data_type != PCK_CERT_CHAIN_CERTIFICATION_DATA_TYPE {
            return Err(RegistrarError::TdxAttestation(Box::new(
                TdxHydrationError::UnsupportedCertificationData {
                    actual: parsed_quote.certification_data_type,
                },
            )));
        }
        Self::certificate_chain_from_pem(&parsed_quote.certification_data)
    }

    async fn fetch_tcb_info(&self, platform: &TdxPlatformIdentity) -> Result<TdxSignedCollateral> {
        let mut url = self
            .config
            .pcs_tdx_base_url
            .join("tcb")
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        url.query_pairs_mut()
            .append_pair("fmspc", &hex::encode(&platform.fmspc))
            .append_pair("pceid", &hex::encode(&platform.pce_id));
        self.fetch_signed_collateral(
            url,
            HeaderName::from_static(TCB_INFO_ISSUER_CHAIN_HEADER),
            Some(HeaderName::from_static(TCB_INFO_SIGNATURE_HEADER)),
            TdxSignedCollateralBody::TcbInfo,
        )
        .await
    }

    async fn fetch_qe_identity(&self) -> Result<TdxSignedCollateral> {
        let url = self
            .config
            .pcs_tdx_base_url
            .join("qe/identity")
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        self.fetch_signed_collateral(
            url,
            HeaderName::from_static(QE_IDENTITY_ISSUER_CHAIN_HEADER),
            None,
            TdxSignedCollateralBody::QeIdentity,
        )
        .await
    }

    async fn fetch_signed_collateral(
        &self,
        url: url::Url,
        chain_header: HeaderName,
        signature_header: Option<HeaderName>,
        body_kind: TdxSignedCollateralBody,
    ) -> Result<TdxSignedCollateral> {
        let response = self.get(url).await?;
        let headers = response.headers().clone();
        let raw = Self::limited_body(response).await?;
        let signing_chain = Self::certificate_chain_from_header(&headers, &chain_header)?;
        Self::verify_trusted_root_ca_hash(&signing_chain, self.config.trusted_root_ca_hash)?;
        let signature = match signature_header {
            Some(header) => Self::signature_from_header(&headers, &header)?,
            None => Self::signature_from_json_field(&raw, QE_IDENTITY_SIGNATURE_FIELD)?,
        };
        let collateral =
            TdxSignedCollateral { raw, signing_chain, signature, issue_time: 0, next_update: 0 };
        let validity = match body_kind {
            TdxSignedCollateralBody::TcbInfo => {
                collateral.signed_validity(body_kind, TdxVerifierError::TcbInfoInvalid)
            }
            TdxSignedCollateralBody::QeIdentity => {
                collateral.signed_validity(body_kind, TdxVerifierError::QeIdentityInvalid)
            }
        }
        .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        Ok(TdxSignedCollateral {
            issue_time: validity.issue_time,
            next_update: validity.next_update,
            ..collateral
        })
    }

    async fn fetch_revocation_evidence(
        &self,
        chains: &[&[TdxCertificate]],
    ) -> Result<TdxRevocationEvidence> {
        let mut seen = HashSet::new();
        let mut certificate_crls = Vec::new();
        for chain in chains {
            for certificate in chain.iter().skip(1) {
                let crl_url = Self::crl_distribution_point(&certificate.raw)?;
                if !seen.insert(crl_url.clone()) {
                    continue;
                }
                let url = url::Url::parse(&crl_url)
                    .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
                if !Self::is_allowed_intel_url(&url) {
                    return Err(RegistrarError::TdxAttestation(Box::new(
                        TdxHydrationError::DisallowedCrlHost { url: crl_url },
                    )));
                }
                let response = self.get(url).await?;
                let raw = Self::limited_body(response).await?;
                certificate_crls.push(TdxCertificateRevocationList { raw });
            }
        }
        Ok(TdxRevocationEvidence { certificate_crls })
    }

    async fn get(&self, url: url::Url) -> Result<reqwest::Response> {
        let response = self
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        if !response.status().is_success() {
            return Err(RegistrarError::TdxAttestation(Box::new(TdxHydrationError::HttpStatus {
                url: url.to_string(),
                status: response.status(),
            })));
        }
        Ok(response)
    }

    async fn limited_body(response: reqwest::Response) -> Result<Bytes> {
        if response.content_length().is_some_and(|len| len > MAX_TDX_COLLATERAL_RESPONSE_BYTES) {
            return Err(RegistrarError::TdxAttestation(Box::new(
                TdxHydrationError::ResponseTooLarge,
            )));
        }
        let bytes =
            response.bytes().await.map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_TDX_COLLATERAL_RESPONSE_BYTES {
            return Err(RegistrarError::TdxAttestation(Box::new(
                TdxHydrationError::ResponseTooLarge,
            )));
        }
        Ok(Bytes::from(bytes.to_vec()))
    }

    fn certificate_chain_from_header(
        headers: &HeaderMap,
        header: &HeaderName,
    ) -> Result<Vec<TdxCertificate>> {
        let value = headers
            .get(header)
            .ok_or_else(|| {
                RegistrarError::TdxAttestation(Box::new(TdxHydrationError::MissingHeader {
                    header: header.as_str().to_string(),
                }))
            })?
            .to_str()
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        let decoded = Self::percent_decode(value)?;
        Self::certificate_chain_from_pem(&decoded)
    }

    fn signature_from_header(headers: &HeaderMap, header: &HeaderName) -> Result<Bytes> {
        let value = headers
            .get(header)
            .ok_or_else(|| {
                RegistrarError::TdxAttestation(Box::new(TdxHydrationError::MissingHeader {
                    header: header.as_str().to_string(),
                }))
            })?
            .to_str()
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        Self::signature_from_hex(value)
    }

    fn signature_from_json_field(raw: &[u8], field: &'static str) -> Result<Bytes> {
        let document: serde_json::Value =
            serde_json::from_slice(raw).map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        let value = document
            .get(field)
            .ok_or_else(|| {
                RegistrarError::TdxAttestation(Box::new(TdxHydrationError::MissingJsonField {
                    field,
                }))
            })?
            .as_str()
            .ok_or_else(|| {
                RegistrarError::TdxAttestation(Box::new(TdxHydrationError::InvalidJsonField {
                    field,
                }))
            })?;
        Self::signature_from_hex(value)
    }

    fn signature_from_hex(value: &str) -> Result<Bytes> {
        let trimmed = value.trim();
        let signature = trimmed.strip_prefix("0x").unwrap_or(trimmed);
        hex::decode(signature)
            .map(Bytes::from)
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))
    }

    fn certificate_chain_from_pem(pem_bytes: &[u8]) -> Result<Vec<TdxCertificate>> {
        let mut remaining = pem_bytes;
        let mut certs = Vec::new();
        while !remaining.iter().all(u8::is_ascii_whitespace) {
            let (rest, pem) = parse_x509_pem(remaining).map_err(|e| {
                RegistrarError::TdxAttestation(Box::new(TdxHydrationError::Pem(e.to_string())))
            })?;
            if pem.label == "CERTIFICATE" {
                certs.push(Bytes::from(pem.contents));
            }
            remaining = rest;
        }
        Self::chain_from_der_certs(certs)
    }

    fn chain_from_der_certs(certs: Vec<Bytes>) -> Result<Vec<TdxCertificate>> {
        if certs.is_empty() {
            return Err(RegistrarError::TdxAttestation("certificate chain is empty".into()));
        }
        let authenticated = certs
            .iter()
            .map(|cert| TdxCertificate::authenticated_from_der(cert))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        let ordered_indexes = Self::root_to_leaf_indexes(&authenticated)?;
        let mut ordered = Vec::with_capacity(ordered_indexes.len());
        for (position, index) in ordered_indexes.iter().copied().enumerate() {
            let issuer_public_key = if position == 0 {
                authenticated[index].subject_public_key.clone()
            } else {
                let issuer_index = ordered_indexes[position - 1];
                authenticated[issuer_index].subject_public_key.clone()
            };
            ordered.push(
                TdxCertificate::from_der(certs[index].clone(), issuer_public_key)
                    .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?,
            );
        }
        Ok(ordered)
    }

    fn root_to_leaf_indexes(certs: &[AuthenticatedTdxCertificate]) -> Result<Vec<usize>> {
        let mut root_index =
            certs.iter().position(|cert| cert.issuer_name == cert.subject_name).ok_or_else(
                || RegistrarError::TdxAttestation("certificate chain root is missing".into()),
            )?;
        let mut ordered = Vec::with_capacity(certs.len());
        let mut used = HashSet::new();
        ordered.push(root_index);
        used.insert(root_index);

        while ordered.len() < certs.len() {
            let parent = &certs[root_index];
            let Some(child_index) = certs.iter().enumerate().find_map(|(index, cert)| {
                (!used.contains(&index) && cert.issuer_name == parent.subject_name).then_some(index)
            }) else {
                return Err(RegistrarError::TdxAttestation(
                    "certificate chain is not contiguous".into(),
                ));
            };
            ordered.push(child_index);
            used.insert(child_index);
            root_index = child_index;
        }
        Ok(ordered)
    }

    fn verify_trusted_root_ca_hash(
        chain: &[TdxCertificate],
        trusted_root_ca_hash: B256,
    ) -> Result<()> {
        let actual_root_ca_hash = chain
            .first()
            .ok_or_else(|| RegistrarError::TdxAttestation("certificate chain is empty".into()))?
            .hash();
        if actual_root_ca_hash != trusted_root_ca_hash {
            return Err(RegistrarError::TdxAttestation(Box::new(
                TdxHydrationError::RootCaNotTrusted {
                    expected: trusted_root_ca_hash,
                    actual: actual_root_ca_hash,
                },
            )));
        }
        Ok(())
    }

    fn crl_distribution_point(certificate_der: &[u8]) -> Result<String> {
        let (_, certificate) = X509Certificate::from_der(certificate_der)
            .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
        for extension in certificate.extensions() {
            let ParsedExtension::CRLDistributionPoints(points) = extension.parsed_extension()
            else {
                continue;
            };
            for point in points.iter() {
                let Some(DistributionPointName::FullName(names)) = &point.distribution_point else {
                    continue;
                };
                for name in names {
                    let GeneralName::URI(uri) = name else { continue };
                    if uri.starts_with("https://") {
                        return Ok(uri.to_string());
                    }
                }
            }
        }
        Err(RegistrarError::TdxAttestation(
            "certificate is missing HTTPS CRL distribution point".into(),
        ))
    }

    fn is_allowed_intel_url(url: &url::Url) -> bool {
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| {
                let host = host.to_ascii_lowercase();
                host == "trustedservices.intel.com" || host.ends_with(ALLOWED_INTEL_HOST_SUFFIX)
            })
    }

    fn percent_decode(value: &str) -> Result<Vec<u8>> {
        let bytes = value.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != b'%' {
                decoded.push(bytes[index]);
                index += 1;
                continue;
            }
            let Some(hex_bytes) = bytes.get(index + 1..index + 3) else {
                return Err(RegistrarError::TdxAttestation(Box::new(
                    TdxHydrationError::InvalidPercentEncoding,
                )));
            };
            let text = std::str::from_utf8(hex_bytes)
                .map_err(|e| RegistrarError::TdxAttestation(Box::new(e)))?;
            let value = u8::from_str_radix(text, 16).map_err(|_| {
                RegistrarError::TdxAttestation(Box::new(TdxHydrationError::InvalidPercentEncoding))
            })?;
            decoded.push(value);
            index += 3;
        }
        Ok(decoded)
    }
}

#[derive(Debug)]
enum TdxHydrationError {
    AttestationPayloadDecode { signer_attestation_error: String, prover_input_error: String },
    UnsupportedCertificationData { actual: u16 },
    MissingHeader { header: String },
    MissingJsonField { field: &'static str },
    InvalidJsonField { field: &'static str },
    HttpStatus { url: String, status: StatusCode },
    ResponseTooLarge,
    DisallowedCrlHost { url: String },
    RootCaNotTrusted { expected: B256, actual: B256 },
    InvalidPercentEncoding,
    Pem(String),
}

impl fmt::Display for TdxHydrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AttestationPayloadDecode { signer_attestation_error, prover_input_error } => {
                write!(
                    f,
                    "failed to decode TDX attestation payload as signer attestation ({signer_attestation_error}) or legacy prover input ({prover_input_error})"
                )
            }
            Self::UnsupportedCertificationData { actual } => {
                write!(f, "unsupported TDX quote certification data type {actual}")
            }
            Self::MissingHeader { header } => write!(f, "Intel PCS response missing {header}"),
            Self::MissingJsonField { field } => {
                write!(f, "Intel PCS response missing JSON field {field}")
            }
            Self::InvalidJsonField { field } => {
                write!(f, "Intel PCS response JSON field {field} is not a string")
            }
            Self::HttpStatus { url, status } => {
                write!(f, "Intel PCS request to {url} failed with status {status}")
            }
            Self::ResponseTooLarge => write!(f, "Intel PCS response exceeds size limit"),
            Self::DisallowedCrlHost { url } => {
                write!(f, "TDX certificate CRL URL is not an allowed Intel URL: {url}")
            }
            Self::RootCaNotTrusted { expected, actual } => {
                write!(
                    f,
                    "TDX certificate chain root is not trusted: expected {expected}, got {actual}"
                )
            }
            Self::InvalidPercentEncoding => write!(f, "invalid percent-encoded Intel PCS header"),
            Self::Pem(error) => write!(f, "PEM parse failed: {error}"),
        }
    }
}

impl Error for TdxHydrationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn certificate_with_raw(raw: &'static [u8]) -> TdxCertificate {
        TdxCertificate {
            raw: Bytes::from_static(raw),
            serial: Bytes::new(),
            subject_public_key: Bytes::new(),
            issuer_public_key: Bytes::new(),
            not_before: 0,
            not_after: 0,
            is_ca: false,
            tbs_certificate: Bytes::new(),
            signature: Bytes::new(),
        }
    }

    #[test]
    fn percent_decode_preserves_plus_and_decodes_escapes() {
        let decoded = TdxAttestationHydrator::percent_decode("a+b%0Ac").unwrap();

        assert_eq!(decoded, b"a+b\nc");
    }

    #[test]
    fn qe_identity_signature_from_json_body_decodes_top_level_signature() {
        let raw = br#"{"enclaveIdentity":{},"signature":"0x0102ff"}"#;

        let signature =
            TdxAttestationHydrator::signature_from_json_field(raw, QE_IDENTITY_SIGNATURE_FIELD)
                .unwrap();

        assert_eq!(signature, Bytes::from_static(&[0x01, 0x02, 0xff]));
    }

    #[test]
    fn qe_identity_signature_from_json_body_requires_signature_field() {
        let raw = br#"{"enclaveIdentity":{}}"#;

        let error =
            TdxAttestationHydrator::signature_from_json_field(raw, QE_IDENTITY_SIGNATURE_FIELD)
                .unwrap_err();

        assert!(
            error
                .source()
                .expect("missing JSON field error should be retained as the source")
                .to_string()
                .contains("Intel PCS response missing JSON field signature")
        );
    }

    #[test]
    fn trusted_root_ca_hash_accepts_configured_root() {
        let root = certificate_with_raw(b"trusted-root");
        let leaf = certificate_with_raw(b"leaf");
        let trusted_root_ca_hash = root.hash();

        TdxAttestationHydrator::verify_trusted_root_ca_hash(&[root, leaf], trusted_root_ca_hash)
            .unwrap();
    }

    #[test]
    fn trusted_root_ca_hash_rejects_quote_supplied_root() {
        let untrusted_root = certificate_with_raw(b"untrusted-root");
        let leaf = certificate_with_raw(b"leaf");
        let trusted_root_ca_hash = B256::repeat_byte(0x42);

        let error = TdxAttestationHydrator::verify_trusted_root_ca_hash(
            &[untrusted_root, leaf],
            trusted_root_ca_hash,
        )
        .unwrap_err();

        assert!(
            error
                .source()
                .expect("root CA error should be retained as the source")
                .to_string()
                .contains("TDX certificate chain root is not trusted")
        );
    }
}
