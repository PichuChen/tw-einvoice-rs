#![forbid(unsafe_code)]

use base64::{Engine as _, engine::general_purpose::STANDARD};

/// Digest algorithm observed in Turnkey 3.2.1 CMS signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DigestAlgorithm {
    Sha256,
}

impl DigestAlgorithm {
    /// ASN.1 object identifier used in CMS `digestAlgorithms`/`SignerInfo`.
    #[must_use]
    pub const fn oid(self) -> &'static str {
        match self {
            Self::Sha256 => "2.16.840.1.101.3.4.2.1",
        }
    }
}

/// Signature algorithms observed for software-certificate signing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureAlgorithm {
    RsaPkcs1v15Sha256,
    EcdsaSha256,
}

impl SignatureAlgorithm {
    /// ASN.1 signature algorithm object identifier.
    #[must_use]
    pub const fn oid(self) -> &'static str {
        match self {
            Self::RsaPkcs1v15Sha256 => "1.2.840.113549.1.1.11",
            Self::EcdsaSha256 => "1.2.840.10045.4.3.2",
        }
    }
}

/// CMS content mode required for Turnkey-compatible invoice signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CmsContentMode {
    /// The original invoice-envelope bytes are embedded in `SignedData`.
    Attached,
}

/// The normalized signing profile recovered from Turnkey 3.2.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TurnkeyCmsProfile {
    pub digest: DigestAlgorithm,
    pub content_mode: CmsContentMode,
    pub include_certificate: bool,
    pub signed_attributes: bool,
}

impl Default for TurnkeyCmsProfile {
    fn default() -> Self {
        Self {
            digest: DigestAlgorithm::Sha256,
            content_mode: CmsContentMode::Attached,
            include_certificate: true,
            signed_attributes: true,
        }
    }
}

/// DER-encoded CMS `SignedData` before the Turnkey transport armor is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmsSignedData(Vec<u8>);

impl CmsSignedData {
    /// Wraps DER bytes produced by a concrete CMS implementation.
    ///
    /// # Errors
    ///
    /// Returns [`SignedDataError::Empty`] when no DER bytes were supplied.
    pub fn from_der(der: impl Into<Vec<u8>>) -> Result<Self, SignedDataError> {
        let der = der.into();
        if der.is_empty() {
            Err(SignedDataError::Empty)
        } else {
            Ok(Self(der))
        }
    }

    #[must_use]
    pub fn as_der(&self) -> &[u8] {
        &self.0
    }

    /// Produces the text representation emitted by Turnkey 3.2.1.
    ///
    /// The CMS DER is standard Base64 encoded, wrapped at 64 characters, has
    /// no PEM `BEGIN/END PKCS7` markers, and ends with a line feed. This method
    /// uses LF deliberately so output is stable across build hosts and matches
    /// the Linux distribution under investigation.
    #[must_use]
    pub fn to_turnkey_armored(&self) -> String {
        let encoded = STANDARD.encode(&self.0);
        let line_count = encoded.len().div_ceil(64);
        let mut output = String::with_capacity(encoded.len() + line_count);

        for (index, character) in encoded.chars().enumerate() {
            output.push(character);
            if (index + 1).is_multiple_of(64) {
                output.push('\n');
            }
        }

        if !encoded.len().is_multiple_of(64) {
            output.push('\n');
        }

        output
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedDataError {
    Empty,
}

impl std::fmt::Display for SignedDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("CMS SignedData DER must not be empty"),
        }
    }
}

impl std::error::Error for SignedDataError {}

/// Cryptographic boundary for software certificates, smart cards, or HSMs.
///
/// Concrete implementations are responsible for producing attached CMS
/// `SignedData` using the profile above. Keeping key access behind this trait
/// lets the daemon support PFX today and PKCS#11/HSM backends without coupling
/// transport logic to private-key storage.
pub trait CmsSigner {
    type Error;

    fn signature_algorithm(&self) -> SignatureAlgorithm;

    /// Signs `content` as attached CMS `SignedData`.
    ///
    /// # Errors
    ///
    /// Returns the backend-specific signing error when key loading, certificate
    /// parsing, or CMS generation fails.
    fn sign_attached(&self, content: &[u8]) -> Result<CmsSignedData, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_matches_recovered_turnkey_algorithms() {
        let profile = TurnkeyCmsProfile::default();
        assert_eq!(profile.digest.oid(), "2.16.840.1.101.3.4.2.1");
        assert_eq!(
            SignatureAlgorithm::RsaPkcs1v15Sha256.oid(),
            "1.2.840.113549.1.1.11"
        );
        assert_eq!(SignatureAlgorithm::EcdsaSha256.oid(), "1.2.840.10045.4.3.2");
        assert_eq!(profile.content_mode, CmsContentMode::Attached);
        assert!(profile.include_certificate);
        assert!(profile.signed_attributes);
    }

    #[test]
    fn armor_wraps_at_sixty_four_characters_without_pem_markers() {
        // 48 zero bytes encode to exactly 64 Base64 characters.
        let signed = CmsSignedData::from_der(vec![0; 48]).unwrap();
        let armored = signed.to_turnkey_armored();

        assert_eq!(armored, format!("{}\n", "A".repeat(64)));
        assert!(!armored.contains("BEGIN PKCS7"));
        assert!(!armored.contains("END PKCS7"));
    }

    #[test]
    fn armor_wraps_subsequent_lines_and_keeps_final_newline() {
        let signed = CmsSignedData::from_der(vec![0; 49]).unwrap();
        let armored = signed.to_turnkey_armored();
        let lines: Vec<&str> = armored.lines().collect();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 64);
        assert_eq!(lines[1], "AA==");
        assert!(armored.ends_with('\n'));
    }
}
