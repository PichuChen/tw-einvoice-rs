#![forbid(unsafe_code)]

pub mod filename;
pub mod gateway;
pub mod inbound;
pub mod native;
pub mod package;
pub mod packed;
pub mod receiver;
pub mod reconciliation;
pub mod sftp;
mod sftp_reconciliation;

use tw_einvoice_core::SubmissionState;

/// An opaque package ready to be handed to a concrete transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionPackage {
    pub idempotency_key: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionReceipt {
    pub remote_id: String,
    pub state: SubmissionState,
}

/// Boundary implemented by adapters such as the official-Turnkey spool adapter
/// and, once protocol work is complete, a native MOF transport.
pub trait InvoiceTransport {
    type Error;

    /// Submits a prepared package to the concrete transport.
    ///
    /// # Errors
    ///
    /// Returns the adapter-specific error when upload, notification, or durable
    /// submission bookkeeping fails.
    fn submit(&self, package: &SubmissionPackage) -> Result<SubmissionReceipt, Self::Error>;

    /// Reconciles a previously submitted remote object/job with platform state.
    ///
    /// # Errors
    ///
    /// Returns the adapter-specific error when the remote result cannot be
    /// queried, downloaded, parsed, or correlated with the submission.
    fn reconcile(&self, remote_id: &str) -> Result<SubmissionReceipt, Self::Error>;
}
