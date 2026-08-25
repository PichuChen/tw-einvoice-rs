#![forbid(unsafe_code)]

use std::{error::Error, fmt};

use tw_einvoice_envelope::{EnvelopeSerializeError, InvoiceEnvelope};
use tw_einvoice_signing::{CmsSigner, SignatureAlgorithm};

/// Signed file produced at the Pack/SendFile boundary.
///
/// `upload_bytes` are the exact Base64-armored CMS bytes that the official
/// Turnkey writes to its target file before optional ZIP compression and SFTP
/// upload. Because attached CMS contains the complete invoice envelope, this
/// type intentionally redacts its payload from `Debug` output.
pub struct SignedArtifact {
    upload_bytes: Vec<u8>,
    envelope_size: usize,
    signature_algorithm: SignatureAlgorithm,
}

impl SignedArtifact {
    /// Bytes written by Pack and consumed by the SendFile stage.
    #[must_use]
    pub fn as_upload_bytes(&self) -> &[u8] {
        &self.upload_bytes
    }

    /// Size reported by current Turnkey PFS001 when ZIP is disabled, and also
    /// the pre-ZIP size reported by its compatibility quirk when ZIP is enabled.
    #[must_use]
    pub fn turnkey_reported_size(&self) -> usize {
        self.upload_bytes.len()
    }

    /// Size of the cleartext XML envelope that was covered by the CMS signature.
    #[must_use]
    pub fn envelope_size(&self) -> usize {
        self.envelope_size
    }

    #[must_use]
    pub fn signature_algorithm(&self) -> SignatureAlgorithm {
        self.signature_algorithm
    }
}

impl fmt::Debug for SignedArtifact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignedArtifact")
            .field("upload_bytes", &"[REDACTED]")
            .field("upload_size", &self.upload_bytes.len())
            .field("envelope_size", &self.envelope_size)
            .field("signature_algorithm", &self.signature_algorithm)
            .finish()
    }
}

/// Serializes an `InvoiceEnvelope`, signs the exact resulting bytes, and applies
/// the Linux Turnkey Base64 armor expected by SendFile.
///
/// # Errors
///
/// Returns [`PackError::Envelope`] when the strict envelope serializer rejects a
/// payload, or [`PackError::Signing`] when the configured CMS backend fails.
pub fn pack_and_sign<S: CmsSigner>(
    envelope: &InvoiceEnvelope,
    signer: &S,
) -> Result<SignedArtifact, PackError<S::Error>> {
    let envelope_bytes = envelope.to_turnkey_xml().map_err(PackError::Envelope)?;
    let envelope_size = envelope_bytes.len();
    let signature_algorithm = signer.signature_algorithm();
    let cms = signer
        .sign_attached(&envelope_bytes)
        .map_err(PackError::Signing)?;
    let upload_bytes = cms.to_turnkey_armored().into_bytes();

    Ok(SignedArtifact {
        upload_bytes,
        envelope_size,
        signature_algorithm,
    })
}

#[derive(Debug)]
pub enum PackError<E> {
    Envelope(EnvelopeSerializeError),
    Signing(E),
}

impl<E: fmt::Display> fmt::Display for PackError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Envelope(error) => write!(f, "failed to serialize invoice envelope: {error}"),
            Self::Signing(error) => write!(f, "failed to sign invoice envelope: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for PackError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Envelope(error) => Some(error),
            Self::Signing(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, convert::Infallible};

    use tw_einvoice_core::MigVersion;
    use tw_einvoice_envelope::{
        EnvelopeRouting, InvoicePackMetadata, MigPayload, PartyInfo, RoutingInfo,
    };
    use tw_einvoice_signing::{CmsSignedData, CmsSigner, SignatureAlgorithm};

    use super::*;

    #[derive(Debug, Default)]
    struct CapturingSigner {
        content: RefCell<Vec<u8>>,
    }

    impl CmsSigner for CapturingSigner {
        type Error = Infallible;

        fn signature_algorithm(&self) -> SignatureAlgorithm {
            SignatureAlgorithm::RsaPkcs1v15Sha256
        }

        fn sign_attached(&self, content: &[u8]) -> Result<CmsSignedData, Self::Error> {
            self.content.replace(content.to_vec());
            // Synthetic encoded bytes are sufficient here: strict CMS structure
            // and PFX cryptography are independently pinned in einvoice-signing.
            Ok(CmsSignedData::from_encoded(vec![0x30, 0x80, 0x00, 0x00]).unwrap())
        }
    }

    fn envelope() -> InvoiceEnvelope {
        InvoiceEnvelope::new(
            EnvelopeRouting {
                from: PartyInfo {
                    identifier: "12345678".into(),
                    description: None,
                },
                from_vac: RoutingInfo {
                    identifier: "FROM".into(),
                    description: None,
                },
                to: PartyInfo {
                    identifier: "0000000000".into(),
                    description: None,
                },
                to_vac: RoutingInfo {
                    identifier: "TO".into(),
                    description: None,
                },
            },
            InvoicePackMetadata {
                message_type: "F0401".into(),
                version: MigVersion::V4_1,
            },
            vec![
                MigPayload::new(
                    br#"<Invoice xmlns="urn:GEINV:eInvoiceMessage:F0401:4.1"><Main/></Invoice>"#
                        .to_vec(),
                )
                .unwrap(),
            ],
        )
        .unwrap()
    }

    #[test]
    fn signs_exact_serialized_envelope_bytes() {
        let envelope = envelope();
        let expected = envelope.to_turnkey_xml().unwrap();
        let signer = CapturingSigner::default();

        let artifact = pack_and_sign(&envelope, &signer).unwrap();

        assert_eq!(*signer.content.borrow(), expected);
        assert_eq!(artifact.envelope_size(), expected.len());
        assert_eq!(
            artifact.signature_algorithm(),
            SignatureAlgorithm::RsaPkcs1v15Sha256
        );
        assert_eq!(
            artifact.turnkey_reported_size(),
            artifact.as_upload_bytes().len()
        );
        assert!(artifact.as_upload_bytes().ends_with(b"\n"));
    }

    #[test]
    fn debug_redacts_attached_invoice_content() {
        let signer = CapturingSigner::default();
        let artifact = pack_and_sign(&envelope(), &signer).unwrap();
        let rendered = format!("{artifact:?}");

        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("InvoiceEnvelope"));
        assert!(!rendered.contains("12345678"));
    }
}
