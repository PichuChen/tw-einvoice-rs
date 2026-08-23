#![forbid(unsafe_code)]

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

    /// Submit a package to the concrete transport.
    ///
    /// # Errors
    ///
    /// Returns the adapter-specific error when the package cannot be submitted
    /// or when submission outcome cannot be determined safely.
    fn submit(&self, package: &SubmissionPackage) -> Result<SubmissionReceipt, Self::Error>;

    /// Reconcile a previously submitted remote object or operation.
    ///
    /// # Errors
    ///
    /// Returns the adapter-specific error when remote state cannot be queried
    /// or interpreted reliably.
    fn reconcile(&self, remote_id: &str) -> Result<SubmissionReceipt, Self::Error>;
}
