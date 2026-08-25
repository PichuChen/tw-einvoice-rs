#![forbid(unsafe_code)]

use std::{error::Error, fmt};

use tw_einvoice_envelope::InvoiceEnvelope;
use tw_einvoice_pack::{PackError, SignedArtifact, pack_and_sign};
use tw_einvoice_signing::CmsSigner;
use tw_einvoice_transport::{
    filename::TurnkeyPackFilename,
    gateway::{GatewayNotifier, GatewayParty, ServiceType, ZipMode},
    native::{
        GatewayVersionProfile, NativeMessageProfile, NativeSubmissionMetadata,
        NativeSubmissionReceipt, NativeSubmitter,
    },
    packed::{PackArtifactSubmission, PackArtifactSubmitError},
    sftp::ObjectUploader,
};

/// High-level description of one outgoing Turnkey-compatible submission.
///
/// The plan deliberately contains both the envelope and the recovered Turnkey
/// filename. [`prepare_submission`] cross-validates duplicated protocol fields
/// before signing, so an operator cannot accidentally sign one envelope while
/// notifying the platform under another message/routing identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionPlan {
    pub envelope: InvoiceEnvelope,
    pub filename: TurnkeyPackFilename,
    pub service_type: ServiceType,
    pub zip_mode: ZipMode,
    pub action: String,
    pub versions: GatewayVersionProfile,
}

impl SubmissionPlan {
    #[must_use]
    pub fn new(
        envelope: InvoiceEnvelope,
        filename: TurnkeyPackFilename,
        service_type: ServiceType,
        zip_mode: ZipMode,
        action: impl Into<String>,
        versions: GatewayVersionProfile,
    ) -> Self {
        Self {
            envelope,
            filename,
            service_type,
            zip_mode,
            action: action.into(),
            versions,
        }
    }
}

/// Durable-ready output of validation + Pack/signing, before any network I/O.
///
/// This separation is intentional: a production daemon can persist the signed
/// artifact and metadata before attempting SFTP/PFS001, then safely retry the
/// network phase without re-signing or regenerating identifiers.
#[derive(Debug)]
pub struct PreparedSubmission {
    filename: TurnkeyPackFilename,
    artifact: SignedArtifact,
    zip_mode: ZipMode,
    metadata: NativeSubmissionMetadata,
}

impl PreparedSubmission {
    #[must_use]
    pub fn filename(&self) -> &TurnkeyPackFilename {
        &self.filename
    }

    #[must_use]
    pub fn artifact(&self) -> &SignedArtifact {
        &self.artifact
    }

    #[must_use]
    pub const fn zip_mode(&self) -> ZipMode {
        self.zip_mode
    }

    #[must_use]
    pub fn metadata(&self) -> &NativeSubmissionMetadata {
        &self.metadata
    }

    /// Executes only the already-prepared network phase.
    ///
    /// # Errors
    ///
    /// Returns [`PackArtifactSubmitError`] when the typed SendFile/SFTP/PFS001
    /// pipeline rejects the prepared metadata, upload fails, notification fails,
    /// or the gateway returns a non-zero status.
    pub fn submit<U, N>(
        &self,
        submitter: &NativeSubmitter<U, N>,
    ) -> Result<NativeSubmissionReceipt, PackArtifactSubmitError<U::Error, N::Error>>
    where
        U: ObjectUploader,
        N: GatewayNotifier,
    {
        submitter.submit_pack_artifact(&PackArtifactSubmission {
            filename: &self.filename,
            artifact: &self.artifact,
            zip_mode: self.zip_mode,
            metadata: self.metadata.clone(),
        })
    }
}

/// Validates all duplicated cross-layer fields and creates the exact signed
/// artifact that will later be submitted.
///
/// No network operation occurs in this function.
///
/// # Errors
///
/// Returns [`PrepareSubmissionError::Consistency`] before signing when filename,
/// envelope, routing, message type, version, or count disagree. Returns
/// [`PrepareSubmissionError::Pack`] when strict envelope serialization or CMS
/// signing fails.
pub fn prepare_submission<S: CmsSigner>(
    plan: SubmissionPlan,
    signer: &S,
) -> Result<PreparedSubmission, PrepareSubmissionError<S::Error>> {
    validate_plan(&plan).map_err(PrepareSubmissionError::Consistency)?;

    let quantity = u32::try_from(plan.envelope.count())
        .expect("InvoiceEnvelope enforces at most 1000 payloads");
    let artifact = pack_and_sign(&plan.envelope, signer).map_err(PrepareSubmissionError::Pack)?;

    let from = gateway_party(&plan.envelope.routing.from, &plan.envelope.routing.from_vac);
    let to = gateway_party(&plan.envelope.routing.to, &plan.envelope.routing.to_vac);
    let metadata = NativeSubmissionMetadata::initial(
        from,
        to,
        plan.service_type,
        NativeMessageProfile::new(
            plan.envelope.metadata.message_type.clone(),
            plan.action,
            quantity,
            plan.envelope.metadata.version.as_str(),
        ),
        plan.versions,
    );

    Ok(PreparedSubmission {
        filename: plan.filename,
        artifact,
        zip_mode: plan.zip_mode,
        metadata,
    })
}

fn gateway_party(
    party: &tw_einvoice_envelope::PartyInfo,
    routing: &tw_einvoice_envelope::RoutingInfo,
) -> GatewayParty {
    GatewayParty {
        party_id: party.identifier.clone(),
        party_description: party.description.clone(),
        routing_id: routing.identifier.clone(),
        routing_description: routing.description.clone(),
    }
}

fn validate_plan(plan: &SubmissionPlan) -> Result<(), SubmissionConsistencyError> {
    let envelope_count = plan.envelope.count();
    let filename_count = usize::try_from(plan.filename.pack_count)
        .ok()
        .filter(|count| *count > 0)
        .ok_or(SubmissionConsistencyError::InvalidFilenameCount {
            count: plan.filename.pack_count,
        })?;

    if filename_count != envelope_count {
        return Err(SubmissionConsistencyError::CountMismatch {
            envelope_count,
            filename_count,
        });
    }

    if plan.filename.common_name.message_type != plan.envelope.metadata.message_type {
        return Err(SubmissionConsistencyError::MessageTypeMismatch {
            envelope: plan.envelope.metadata.message_type.clone(),
            filename: plan.filename.common_name.message_type.clone(),
        });
    }

    let envelope_version = plan.envelope.metadata.version.as_str();
    if plan.filename.common_name.mig_version != envelope_version {
        return Err(SubmissionConsistencyError::MigVersionMismatch {
            envelope: envelope_version.to_owned(),
            filename: plan.filename.common_name.mig_version.clone(),
        });
    }

    if plan.filename.from_party != plan.envelope.routing.from.identifier {
        return Err(SubmissionConsistencyError::FromPartyMismatch {
            envelope: plan.envelope.routing.from.identifier.clone(),
            filename: plan.filename.from_party.clone(),
        });
    }

    if plan.filename.to_party != plan.envelope.routing.to.identifier {
        return Err(SubmissionConsistencyError::ToPartyMismatch {
            envelope: plan.envelope.routing.to.identifier.clone(),
            filename: plan.filename.to_party.clone(),
        });
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmissionConsistencyError {
    InvalidFilenameCount {
        count: i32,
    },
    CountMismatch {
        envelope_count: usize,
        filename_count: usize,
    },
    MessageTypeMismatch {
        envelope: String,
        filename: String,
    },
    MigVersionMismatch {
        envelope: String,
        filename: String,
    },
    FromPartyMismatch {
        envelope: String,
        filename: String,
    },
    ToPartyMismatch {
        envelope: String,
        filename: String,
    },
}

impl fmt::Display for SubmissionConsistencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFilenameCount { count } => {
                write!(f, "Turnkey filename contains invalid pack count {count}")
            }
            Self::CountMismatch {
                envelope_count,
                filename_count,
            } => write!(
                f,
                "envelope contains {envelope_count} messages but filename count is {filename_count}"
            ),
            Self::MessageTypeMismatch { envelope, filename } => write!(
                f,
                "envelope message type {envelope:?} does not match filename {filename:?}"
            ),
            Self::MigVersionMismatch { envelope, filename } => write!(
                f,
                "envelope MIG version {envelope:?} does not match filename {filename:?}"
            ),
            Self::FromPartyMismatch { envelope, filename } => write!(
                f,
                "envelope From PartyId {envelope:?} does not match filename {filename:?}"
            ),
            Self::ToPartyMismatch { envelope, filename } => write!(
                f,
                "envelope To PartyId {envelope:?} does not match filename {filename:?}"
            ),
        }
    }
}

impl Error for SubmissionConsistencyError {}

#[derive(Debug)]
pub enum PrepareSubmissionError<E> {
    Consistency(SubmissionConsistencyError),
    Pack(PackError<E>),
}

impl<E: fmt::Display> fmt::Display for PrepareSubmissionError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Consistency(error) => error.fmt(f),
            Self::Pack(error) => error.fmt(f),
        }
    }
}

impl<E: Error + 'static> Error for PrepareSubmissionError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Consistency(error) => Some(error),
            Self::Pack(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, cell::RefCell, convert::Infallible};

    use tw_einvoice_envelope::{
        EnvelopeRouting, InvoicePackMetadata, MigPayload, PartyInfo, RoutingInfo,
    };
    use tw_einvoice_signing::{CmsSignedData, SignatureAlgorithm};
    use tw_einvoice_transport::gateway::{GatewayProcessStatus, UploadNotification};

    use super::*;

    #[derive(Debug, Default)]
    struct CountingSigner {
        calls: Cell<usize>,
    }

    impl CmsSigner for CountingSigner {
        type Error = Infallible;

        fn signature_algorithm(&self) -> SignatureAlgorithm {
            SignatureAlgorithm::RsaPkcs1v15Sha256
        }

        fn sign_attached(&self, content: &[u8]) -> Result<CmsSignedData, Self::Error> {
            self.calls.set(self.calls.get() + 1);
            let mut bytes = vec![0x30, 0x80];
            bytes.extend_from_slice(content);
            bytes.extend_from_slice(&[0, 0]);
            Ok(CmsSignedData::from_encoded(bytes).unwrap())
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
                    description: Some("sender".into()),
                },
                from_vac: RoutingInfo {
                    identifier: "FROM-ROUTE".into(),
                    description: None,
                },
                to: PartyInfo {
                    identifier: "PLATFORM".into(),
                    description: None,
                },
                to_vac: RoutingInfo {
                    identifier: "TO-ROUTE".into(),
                    description: Some("platform route".into()),
                },
            },
            InvoicePackMetadata {
                message_type: "F0401".into(),
                version: tw_einvoice_core::MigVersion::V4_1,
            },
            vec![MigPayload::new(b"<Invoice><Main/></Invoice>".to_vec()).unwrap()],
        )
        .unwrap()
    }

    fn filename() -> TurnkeyPackFilename {
        TurnkeyPackFilename::parse(
            "source.xml_12345678_PLATFORM_4.1-F0401-20260825-170000000-550e8400-e29b-41d4-a716-446655440000_1",
        )
        .unwrap()
    }

    fn plan() -> SubmissionPlan {
        SubmissionPlan::new(
            envelope(),
            filename(),
            ServiceType::Store,
            ZipMode::Plain,
            "C0401",
            GatewayVersionProfile::new("3.1.3", "0.1.0"),
        )
    }

    #[test]
    fn rejects_cross_layer_mismatch_before_signing() {
        let signer = CountingSigner::default();
        let mut plan = plan();
        plan.filename.common_name.message_type = "F0501".into();

        let error = prepare_submission(plan, &signer).unwrap_err();
        assert!(matches!(
            error,
            PrepareSubmissionError::Consistency(
                SubmissionConsistencyError::MessageTypeMismatch { .. }
            )
        ));
        assert_eq!(signer.calls.get(), 0);
    }

    #[test]
    fn prepare_derives_gateway_metadata_from_envelope() {
        let signer = CountingSigner::default();
        let prepared = prepare_submission(plan(), &signer).unwrap();

        assert_eq!(signer.calls.get(), 1);
        assert_eq!(prepared.metadata().from.party_id, "12345678");
        assert_eq!(prepared.metadata().from.routing_id, "FROM-ROUTE");
        assert_eq!(prepared.metadata().to.party_id, "PLATFORM");
        assert_eq!(prepared.metadata().to.routing_id, "TO-ROUTE");
        assert_eq!(prepared.metadata().message.message_type, "F0401");
        assert_eq!(prepared.metadata().message.mig_version, "4.1");
        assert_eq!(prepared.metadata().message.quantity, 1);
        assert_eq!(prepared.metadata().message.action, "C0401");
    }

    #[test]
    fn prepared_submission_reaches_native_transport_without_reentering_metadata() {
        let signer = CountingSigner::default();
        let prepared = prepare_submission(plan(), &signer).unwrap();
        let submitter =
            NativeSubmitter::new(RecordingUploader::default(), RecordingNotifier::default());

        let receipt = prepared.submit(&submitter).unwrap();

        assert_eq!(receipt.remote_filename, prepared.filename().remote_filename());
        assert_eq!(submitter.uploader().calls.borrow().len(), 1);
        let notification = submitter.notifier().notification.borrow().clone().unwrap();
        assert_eq!(notification.filename, prepared.filename().remote_filename());
        assert_eq!(notification.quantity, 1);
        assert_eq!(notification.message_type, "F0401");
        assert_eq!(notification.mig_version, "4.1");
        assert_eq!(notification.from.party_id, "12345678");
        assert_eq!(notification.to.party_id, "PLATFORM");
    }
}
