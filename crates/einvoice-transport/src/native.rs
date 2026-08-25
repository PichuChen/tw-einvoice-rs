use std::{error::Error, fmt};

use crate::{
    gateway::{
        GatewayNotifier, GatewayParty, GatewayProcessStatus, ServiceType, UploadNotification,
        ZipMode,
    },
    package::{PrepareUploadError, prepare_upload_object},
    sftp::ObjectUploader,
};

/// PFS metadata that is independent from the physical SFTP object. Filename,
/// pre-ZIP size and ZIP flag are derived by the submission pipeline so callers
/// cannot accidentally notify the platform with metadata inconsistent with the
/// bytes that were uploaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSubmissionMetadata {
    pub from: GatewayParty,
    pub to: GatewayParty,
    pub service_type: ServiceType,
    pub message_type: String,
    pub action: String,
    pub quantity: u32,
    pub mig_version: String,
    pub retry: u32,
    pub api_version: String,
    pub turnkey_version: String,
}

impl NativeSubmissionMetadata {
    #[must_use]
    pub fn initial(
        from: GatewayParty,
        to: GatewayParty,
        service_type: ServiceType,
        message_type: impl Into<String>,
        action: impl Into<String>,
        quantity: u32,
        mig_version: impl Into<String>,
        api_version: impl Into<String>,
        turnkey_version: impl Into<String>,
    ) -> Self {
        Self {
            from,
            to,
            service_type,
            message_type: message_type.into(),
            action: action.into(),
            quantity,
            mig_version: mig_version.into(),
            retry: UploadNotification::INITIAL_RETRY,
            api_version: api_version.into(),
            turnkey_version: turnkey_version.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSubmissionRequest<'a> {
    pub signed_local_filename: &'a str,
    pub remote_filename: &'a str,
    pub signed_bytes: &'a [u8],
    pub zip_mode: ZipMode,
    pub metadata: NativeSubmissionMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSubmissionReceipt {
    pub remote_filename: String,
    pub gateway_status: GatewayProcessStatus,
}

/// Implements the recovered Turnkey 3.2.1 submission ordering:
///
/// 1. prepare the plain/ZIP SFTP object,
/// 2. upload it to SFTP `/in`,
/// 3. only after upload success, send PFS001,
/// 4. treat non-zero PFS001 `statusCode` as an enqueue rejection.
///
/// A notification failure deliberately does not attempt to delete the uploaded
/// object. The remote filename is retained in the error so a durable worker can
/// retry/reconcile without silently destroying the artifact needed by the
/// platform.
pub struct NativeSubmitter<U, N> {
    uploader: U,
    notifier: N,
}

impl<U, N> NativeSubmitter<U, N> {
    #[must_use]
    pub fn new(uploader: U, notifier: N) -> Self {
        Self { uploader, notifier }
    }

    #[must_use]
    pub fn uploader(&self) -> &U {
        &self.uploader
    }

    #[must_use]
    pub fn notifier(&self) -> &N {
        &self.notifier
    }
}

impl<U, N> NativeSubmitter<U, N>
where
    U: ObjectUploader,
    N: GatewayNotifier,
{
    /// Uploads and notifies one already-signed Turnkey-compatible package.
    ///
    /// # Errors
    ///
    /// Returns [`NativeSubmitError`] if object preparation fails, SFTP upload
    /// fails, PFS001 transport/decoding fails, or PFS001 returns a non-zero
    /// platform status code.
    pub fn submit(
        &self,
        request: &NativeSubmissionRequest<'_>,
    ) -> Result<NativeSubmissionReceipt, NativeSubmitError<U::Error, N::Error>> {
        let prepared = prepare_upload_object(
            request.signed_local_filename,
            request.remote_filename,
            request.signed_bytes,
            request.zip_mode,
        )
        .map_err(NativeSubmitError::Prepare)?;

        self.uploader
            .upload(&prepared.remote_filename, &prepared.uploaded_bytes)
            .map_err(|source| NativeSubmitError::Upload {
                remote_filename: prepared.remote_filename.clone(),
                source,
            })?;

        let notification = UploadNotification {
            from: request.metadata.from.clone(),
            to: request.metadata.to.clone(),
            service_type: request.metadata.service_type,
            zip_mode: prepared.zip_mode,
            message_type: request.metadata.message_type.clone(),
            action: request.metadata.action.clone(),
            quantity: request.metadata.quantity,
            mig_version: request.metadata.mig_version.clone(),
            filename: prepared.remote_filename.clone(),
            size: prepared.notification_size,
            retry: request.metadata.retry,
            api_version: request.metadata.api_version.clone(),
            turnkey_version: request.metadata.turnkey_version.clone(),
        };

        let gateway_status =
            self.notifier
                .notify(&notification)
                .map_err(|source| NativeSubmitError::Notify {
                    remote_filename: prepared.remote_filename.clone(),
                    source,
                })?;

        if !gateway_status.is_accepted() {
            return Err(NativeSubmitError::GatewayRejected {
                remote_filename: prepared.remote_filename,
                status: gateway_status,
            });
        }

        Ok(NativeSubmissionReceipt {
            remote_filename: prepared.remote_filename,
            gateway_status,
        })
    }
}

#[derive(Debug)]
pub enum NativeSubmitError<U, N> {
    Prepare(PrepareUploadError),
    Upload {
        remote_filename: String,
        source: U,
    },
    Notify {
        remote_filename: String,
        source: N,
    },
    GatewayRejected {
        remote_filename: String,
        status: GatewayProcessStatus,
    },
}

impl<U, N> fmt::Display for NativeSubmitError<U, N>
where
    U: fmt::Display,
    N: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Prepare(error) => write!(f, "failed to prepare upload object: {error}"),
            Self::Upload {
                remote_filename,
                source,
            } => write!(
                f,
                "failed to upload SFTP object {remote_filename}: {source}"
            ),
            Self::Notify {
                remote_filename,
                source,
            } => write!(
                f,
                "SFTP object {remote_filename} was uploaded but PFS001 notification failed: {source}"
            ),
            Self::GatewayRejected {
                remote_filename,
                status,
            } => write!(
                f,
                "SFTP object {remote_filename} was uploaded but PFS001 rejected it with status {}",
                status.status_code
            ),
        }
    }
}

impl<U, N> Error for NativeSubmitError<U, N>
where
    U: Error + 'static,
    N: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Prepare(error) => Some(error),
            Self::Upload { source, .. } => Some(source),
            Self::Notify { source, .. } => Some(source),
            Self::GatewayRejected { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, convert::Infallible, io::Read};

    use zip::ZipArchive;

    use super::*;

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

    #[derive(Debug)]
    struct RecordingNotifier {
        notification: RefCell<Option<UploadNotification>>,
        status_code: i32,
    }

    impl GatewayNotifier for RecordingNotifier {
        type Error = Infallible;

        fn notify(
            &self,
            notification: &UploadNotification,
        ) -> Result<GatewayProcessStatus, Self::Error> {
            self.notification.replace(Some(notification.clone()));
            Ok(GatewayProcessStatus {
                status_code: self.status_code,
                details: Vec::new(),
            })
        }
    }

    fn party(id: &str, route: &str) -> GatewayParty {
        GatewayParty {
            party_id: id.into(),
            party_description: None,
            routing_id: route.into(),
            routing_description: None,
        }
    }

    fn metadata() -> NativeSubmissionMetadata {
        NativeSubmissionMetadata::initial(
            party("12345678", "FROM"),
            party("PLATFORM", "TO"),
            ServiceType::Store,
            "F0401",
            "C0401",
            1,
            "4.1",
            "3.1.3",
            "3.2.1",
        )
    }

    #[test]
    fn derives_zip_notification_metadata_from_actual_prepared_object() {
        let uploader = RecordingUploader::default();
        let notifier = RecordingNotifier {
            notification: RefCell::new(None),
            status_code: 0,
        };
        let submitter = NativeSubmitter::new(uploader, notifier);
        let signed = b"armored-cms\n";
        let request = NativeSubmissionRequest {
            signed_local_filename: "source_12345678_PLATFORM_common_1",
            remote_filename: "common",
            signed_bytes: signed,
            zip_mode: ZipMode::Zip,
            metadata: metadata(),
        };

        let receipt = submitter.submit(&request).unwrap();
        assert_eq!(receipt.remote_filename, "common");

        {
            let calls = submitter.uploader().calls.borrow();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, "common");
            let mut archive = ZipArchive::new(std::io::Cursor::new(&calls[0].1)).unwrap();
            let mut entry = archive.by_index(0).unwrap();
            assert_eq!(entry.name(), request.signed_local_filename);
            let mut unzipped = Vec::new();
            entry.read_to_end(&mut unzipped).unwrap();
            assert_eq!(unzipped, signed);
        }

        let notification = submitter.notifier().notification.borrow().clone().unwrap();
        assert_eq!(notification.filename, "common");
        assert_eq!(notification.size, u64::try_from(signed.len()).unwrap());
        assert_eq!(notification.zip_mode, ZipMode::Zip);
    }

    #[test]
    fn preserves_remote_filename_when_pfs001_rejects_after_upload() {
        let submitter = NativeSubmitter::new(
            RecordingUploader::default(),
            RecordingNotifier {
                notification: RefCell::new(None),
                status_code: 42,
            },
        );
        let request = NativeSubmissionRequest {
            signed_local_filename: "signed_local",
            remote_filename: "remote-common-name",
            signed_bytes: b"cms",
            zip_mode: ZipMode::Plain,
            metadata: metadata(),
        };

        let error = submitter.submit(&request).unwrap_err();
        assert!(matches!(
            error,
            NativeSubmitError::GatewayRejected {
                remote_filename,
                status: GatewayProcessStatus { status_code: 42, .. },
            } if remote_filename == "remote-common-name"
        ));
        assert_eq!(submitter.uploader().calls.borrow().len(), 1);
    }
}
