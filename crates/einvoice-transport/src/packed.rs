use std::{error::Error, fmt};

use tw_einvoice_pack::SignedArtifact;

use crate::{
    filename::TurnkeyPackFilename,
    gateway::{GatewayNotifier, ZipMode},
    native::{
        NativeSubmissionMetadata, NativeSubmissionReceipt, NativeSubmissionRequest,
        NativeSubmitError, NativeSubmitter,
    },
    sftp::ObjectUploader,
};

/// Submission request at the boundary between the Pack and native `SendFile`
/// stages.
///
/// Unlike [`NativeSubmissionRequest`], this request does not let callers provide
/// raw signed bytes, a remote filename, or the `PFS001` size independently. Those
/// values are derived from the already-produced [`SignedArtifact`] and
/// [`TurnkeyPackFilename`] so transport metadata cannot silently diverge from
/// the object that is actually uploaded.
#[derive(Debug)]
pub struct PackArtifactSubmission<'a> {
    pub filename: &'a TurnkeyPackFilename,
    pub artifact: &'a SignedArtifact,
    pub zip_mode: ZipMode,
    pub metadata: NativeSubmissionMetadata,
}

impl<U, N> NativeSubmitter<U, N>
where
    U: ObjectUploader,
    N: GatewayNotifier,
{
    /// Submits a signed Pack artifact using Turnkey-compatible filename, ZIP,
    /// SFTP, and `PFS001` semantics.
    ///
    /// The pack count embedded in the local filename is validated against the
    /// `PFS001` `quantity` before any network operation. This prevents a malformed
    /// or stale filename from producing an upload whose notification describes
    /// a different number of enclosed MIG messages.
    ///
    /// # Errors
    ///
    /// Returns [`PackArtifactSubmitError`] when the filename pack count is not a
    /// positive integer, when it disagrees with the notification quantity, or
    /// when the underlying native submission fails.
    pub fn submit_pack_artifact(
        &self,
        request: &PackArtifactSubmission<'_>,
    ) -> Result<NativeSubmissionReceipt, PackArtifactSubmitError<U::Error, N::Error>> {
        let filename_count = u32::try_from(request.filename.pack_count)
            .ok()
            .filter(|count| *count > 0)
            .ok_or(PackArtifactSubmitError::InvalidPackCount {
                count: request.filename.pack_count,
            })?;

        if filename_count != request.metadata.message.quantity {
            return Err(PackArtifactSubmitError::QuantityMismatch {
                filename_count,
                notification_quantity: request.metadata.message.quantity,
            });
        }

        let signed_local_filename = request.filename.render();
        let remote_filename = request.filename.remote_filename();
        let native_request = NativeSubmissionRequest {
            signed_local_filename: &signed_local_filename,
            remote_filename: &remote_filename,
            signed_bytes: request.artifact.as_upload_bytes(),
            zip_mode: request.zip_mode,
            metadata: request.metadata.clone(),
        };

        self.submit(&native_request)
            .map_err(PackArtifactSubmitError::Native)
    }
}

#[derive(Debug)]
pub enum PackArtifactSubmitError<U, N> {
    InvalidPackCount {
        count: i32,
    },
    QuantityMismatch {
        filename_count: u32,
        notification_quantity: u32,
    },
    Native(NativeSubmitError<U, N>),
}

impl<U, N> fmt::Display for PackArtifactSubmitError<U, N>
where
    U: fmt::Display,
    N: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPackCount { count } => {
                write!(
                    f,
                    "Turnkey pack filename contains non-positive count {count}"
                )
            }
            Self::QuantityMismatch {
                filename_count,
                notification_quantity,
            } => write!(
                f,
                "Turnkey pack count {filename_count} does not match PFS001 quantity {notification_quantity}"
            ),
            Self::Native(error) => error.fmt(f),
        }
    }
}

impl<U, N> Error for PackArtifactSubmitError<U, N>
where
    U: Error + 'static,
    N: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Native(error) => Some(error),
            Self::InvalidPackCount { .. } | Self::QuantityMismatch { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, convert::Infallible, io::Read};

    use tw_einvoice_core::MigVersion;
    use tw_einvoice_envelope::{
        EnvelopeRouting, InvoiceEnvelope, InvoicePackMetadata, MigPayload, PartyInfo, RoutingInfo,
    };
    use tw_einvoice_pack::pack_and_sign;
    use tw_einvoice_signing::{CmsSignedData, CmsSigner, SignatureAlgorithm};
    use zip::ZipArchive;

    use crate::{
        gateway::{
            GatewayNotifier, GatewayParty, GatewayProcessStatus, ServiceType, UploadNotification,
            ZipMode,
        },
        native::{GatewayVersionProfile, NativeMessageProfile, NativeSubmissionMetadata},
        sftp::ObjectUploader,
    };

    use super::*;

    #[derive(Debug, Default)]
    struct SyntheticSigner;

    impl CmsSigner for SyntheticSigner {
        type Error = Infallible;

        fn signature_algorithm(&self) -> SignatureAlgorithm {
            SignatureAlgorithm::RsaPkcs1v15Sha256
        }

        fn sign_attached(&self, content: &[u8]) -> Result<CmsSignedData, Self::Error> {
            let mut encoded = vec![0x30, 0x80];
            encoded.extend_from_slice(content);
            encoded.extend_from_slice(&[0x00, 0x00]);
            Ok(CmsSignedData::from_encoded(encoded).unwrap())
        }
    }

    #[derive(Debug, Default)]
    struct RecordingUploader {
        calls: RefCell<Vec<(String, Vec<u8>)>>,
    }

    impl ObjectUploader for RecordingUploader {
        type Error = Infallible;

        fn upload(&self, remote_filename: &str, bytes: &[u8]) -> Result<(), Self::Error> {
            self.calls
                .borrow_mut()
                .push((remote_filename.to_owned(), bytes.to_vec()));
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct RecordingNotifier {
        notification: RefCell<Option<UploadNotification>>,
    }

    impl GatewayNotifier for RecordingNotifier {
        type Error = Infallible;

        fn notify(
            &self,
            notification: &UploadNotification,
        ) -> Result<GatewayProcessStatus, Self::Error> {
            self.notification.replace(Some(notification.clone()));
            Ok(GatewayProcessStatus {
                status_code: 0,
                details: Vec::new(),
            })
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
                    identifier: "PLATFORM".into(),
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

    fn filename() -> TurnkeyPackFilename {
        TurnkeyPackFilename::parse(
            "source.xml_12345678_PLATFORM_4.1-F0401-20260825-163300000-550e8400-e29b-41d4-a716-446655440000_1",
        )
        .unwrap()
    }

    fn party(id: &str, route: &str) -> GatewayParty {
        GatewayParty {
            party_id: id.into(),
            party_description: None,
            routing_id: route.into(),
            routing_description: None,
        }
    }

    fn metadata(quantity: u32) -> NativeSubmissionMetadata {
        NativeSubmissionMetadata::initial(
            party("12345678", "FROM"),
            party("PLATFORM", "TO"),
            ServiceType::Store,
            NativeMessageProfile::new("F0401", "C0401", quantity, "4.1"),
            GatewayVersionProfile::new("3.1.3", "3.2.1"),
        )
    }

    #[test]
    fn derives_zip_upload_and_pfs001_from_pack_artifact() {
        let artifact = pack_and_sign(&envelope(), &SyntheticSigner).unwrap();
        let filename = filename();
        let submitter =
            NativeSubmitter::new(RecordingUploader::default(), RecordingNotifier::default());
        let request = PackArtifactSubmission {
            filename: &filename,
            artifact: &artifact,
            zip_mode: ZipMode::Zip,
            metadata: metadata(1),
        };

        let receipt = submitter.submit_pack_artifact(&request).unwrap();
        assert_eq!(receipt.remote_filename, filename.remote_filename());

        let calls = submitter.uploader().calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, filename.remote_filename());
        assert_ne!(calls[0].1, artifact.as_upload_bytes());

        let mut archive = ZipArchive::new(std::io::Cursor::new(&calls[0].1)).unwrap();
        let mut entry = archive.by_index(0).unwrap();
        assert_eq!(entry.name(), filename.render());
        let mut unzipped = Vec::new();
        entry.read_to_end(&mut unzipped).unwrap();
        assert_eq!(unzipped, artifact.as_upload_bytes());
        drop(entry);
        drop(archive);
        drop(calls);

        let notification = submitter.notifier().notification.borrow().clone().unwrap();
        assert_eq!(notification.filename, filename.remote_filename());
        assert_eq!(
            notification.size,
            u64::try_from(artifact.turnkey_reported_size()).unwrap()
        );
        assert_eq!(notification.zip_mode, ZipMode::Zip);
        assert_eq!(notification.quantity, 1);
    }

    #[test]
    fn rejects_quantity_mismatch_before_upload() {
        let artifact = pack_and_sign(&envelope(), &SyntheticSigner).unwrap();
        let filename = filename();
        let submitter =
            NativeSubmitter::new(RecordingUploader::default(), RecordingNotifier::default());
        let request = PackArtifactSubmission {
            filename: &filename,
            artifact: &artifact,
            zip_mode: ZipMode::Plain,
            metadata: metadata(2),
        };

        let error = submitter.submit_pack_artifact(&request).unwrap_err();
        assert!(matches!(
            error,
            PackArtifactSubmitError::QuantityMismatch {
                filename_count: 1,
                notification_quantity: 2
            }
        ));
        assert!(submitter.uploader().calls.borrow().is_empty());
        assert!(submitter.notifier().notification.borrow().is_none());
    }
}
