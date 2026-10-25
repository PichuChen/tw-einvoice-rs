#![forbid(unsafe_code)]

use std::{error::Error, fmt};

use tw_einvoice_results::{
    InvoiceReference, ProcessResult, SummaryMessage, SummaryResult, SummaryValidationIssue,
};

/// Monotonic protocol/transport progress for one submitted message.
///
/// This deliberately excludes business acceptance/rejection. A message may be
/// fully transported and acknowledged while still containing failed invoices;
/// terminal business disposition is represented separately by TerminalSummary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TransportState {
    Created,
    Prepared,
    Uploaded,
    Enqueued,
    ProcessObserved,
    SummaryObserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportAdvance {
    Advanced {
        from: TransportState,
        to: TransportState,
    },
    Unchanged,
    IgnoredStale {
        current: TransportState,
        attempted: TransportState,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminalDisposition {
    AllGood,
    AllFailed,
    PartialSuccess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSummary {
    pub disposition: TerminalDisposition,
    pub total: u32,
    pub good: u32,
    pub failed: u32,
    pub good_invoices: Vec<InvoiceReference>,
    pub failed_invoices: Vec<InvoiceReference>,
    pub message: SummaryMessage,
}

impl TerminalSummary {
    fn from_validated_message(message: SummaryMessage) -> Self {
        let total = u32::try_from(message.result.total.count)
            .expect("validated SummaryMessage total count is non-negative");
        let good = u32::try_from(message.result.good.count)
            .expect("validated SummaryMessage good count is non-negative");
        let failed = u32::try_from(message.result.failed.count)
            .expect("validated SummaryMessage failed count is non-negative");

        let disposition = if failed == 0 {
            TerminalDisposition::AllGood
        } else if good == 0 {
            TerminalDisposition::AllFailed
        } else {
            TerminalDisposition::PartialSuccess
        };

        Self {
            disposition,
            total,
            good,
            failed,
            good_invoices: message.result.good.invoices.clone(),
            failed_invoices: message.result.failed.invoices.clone(),
            message,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    Changed,
    UnchangedReplay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionRecord {
    message_id: String,
    expected_quantity: u32,
    transport: TransportState,
    process_results: Vec<ProcessResult>,
    terminal: Option<TerminalSummary>,
}

impl SubmissionRecord {
    /// Creates a new state record before Pack/signing.
    ///
    /// # Errors
    ///
    /// Returns an error when message_id is empty or expected_quantity is zero.
    pub fn new(
        message_id: impl Into<String>,
        expected_quantity: u32,
    ) -> Result<Self, SubmissionInitError> {
        let message_id = message_id.into();
        if message_id.is_empty() {
            return Err(SubmissionInitError::EmptyMessageId);
        }
        if expected_quantity == 0 {
            return Err(SubmissionInitError::ZeroExpectedQuantity);
        }

        Ok(Self {
            message_id,
            expected_quantity,
            transport: TransportState::Created,
            process_results: Vec::new(),
            terminal: None,
        })
    }

    #[must_use]
    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    #[must_use]
    pub const fn expected_quantity(&self) -> u32 {
        self.expected_quantity
    }

    #[must_use]
    pub const fn transport_state(&self) -> TransportState {
        self.transport
    }

    #[must_use]
    pub fn process_results(&self) -> &[ProcessResult] {
        &self.process_results
    }

    #[must_use]
    pub const fn terminal_summary(&self) -> Option<&TerminalSummary> {
        self.terminal.as_ref()
    }

    #[must_use]
    pub fn advance_transport(&mut self, target: TransportState) -> TransportAdvance {
        if target > self.transport {
            let from = self.transport;
            self.transport = target;
            TransportAdvance::Advanced { from, to: target }
        } else if target == self.transport {
            TransportAdvance::Unchanged
        } else {
            TransportAdvance::IgnoredStale {
                current: self.transport,
                attempted: target,
            }
        }
    }

    /// Applies one ProcessResult as protocol acknowledgement/diagnostics.
    ///
    /// A ProcessResult never creates a terminal business disposition.
    ///
    /// # Errors
    ///
    /// Returns an error if the result belongs to a different message ID.
    pub fn apply_process_result(
        &mut self,
        result: ProcessResult,
    ) -> Result<ApplyOutcome, ApplyError> {
        self.ensure_message_id(&result.message.id)?;

        if self.process_results.contains(&result) {
            return Ok(ApplyOutcome::UnchangedReplay);
        }

        self.process_results.push(result);
        let _ = self.advance_transport(TransportState::ProcessObserved);
        Ok(ApplyOutcome::Changed)
    }

    /// Selects and applies this submission's entry from a multi-message SummaryResult.
    ///
    /// # Errors
    ///
    /// Returns an error when the message is absent, duplicated, inconsistent, or conflicting.
    pub fn apply_summary_result(
        &mut self,
        result: &SummaryResult,
    ) -> Result<ApplyOutcome, ApplyError> {
        let mut matching = result
            .messages
            .iter()
            .filter(|message| message.info.id == self.message_id);

        let Some(message) = matching.next() else {
            return Err(ApplyError::SummaryMessageMissing {
                message_id: self.message_id.clone(),
            });
        };

        if matching.next().is_some() {
            let count = result
                .messages
                .iter()
                .filter(|candidate| candidate.info.id == self.message_id)
                .count();
            return Err(ApplyError::DuplicateSummaryMessage {
                message_id: self.message_id.clone(),
                count,
            });
        }

        self.apply_summary_message(message.clone())
    }

    /// Applies one terminal SummaryMessage after count validation.
    ///
    /// # Errors
    ///
    /// Returns an error for ID mismatch, inconsistent counts, or conflicting terminal results.
    pub fn apply_summary_message(
        &mut self,
        message: SummaryMessage,
    ) -> Result<ApplyOutcome, ApplyError> {
        self.ensure_message_id(&message.info.id)?;

        let issues = message.validation_issues(Some(self.expected_quantity));
        if !issues.is_empty() {
            return Err(ApplyError::InconsistentSummary {
                message_id: self.message_id.clone(),
                issues,
            });
        }

        let incoming = TerminalSummary::from_validated_message(message);

        if let Some(existing) = &self.terminal {
            if existing == &incoming {
                return Ok(ApplyOutcome::UnchangedReplay);
            }
            return Err(ApplyError::ConflictingTerminalSummary {
                message_id: self.message_id.clone(),
                existing: existing.clone(),
                incoming,
            });
        }

        self.terminal = Some(incoming);
        let _ = self.advance_transport(TransportState::SummaryObserved);
        Ok(ApplyOutcome::Changed)
    }

    fn ensure_message_id(&self, actual: &str) -> Result<(), ApplyError> {
        if actual == self.message_id {
            Ok(())
        } else {
            Err(ApplyError::MessageIdMismatch {
                expected: self.message_id.clone(),
                actual: actual.to_owned(),
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionInitError {
    EmptyMessageId,
    ZeroExpectedQuantity,
}

impl fmt::Display for SubmissionInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMessageId => f.write_str("submission message ID must not be empty"),
            Self::ZeroExpectedQuantity => {
                f.write_str("submission expected quantity must be greater than zero")
            }
        }
    }
}

impl Error for SubmissionInitError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    MessageIdMismatch {
        expected: String,
        actual: String,
    },
    SummaryMessageMissing {
        message_id: String,
    },
    DuplicateSummaryMessage {
        message_id: String,
        count: usize,
    },
    InconsistentSummary {
        message_id: String,
        issues: Vec<SummaryValidationIssue>,
    },
    ConflictingTerminalSummary {
        message_id: String,
        existing: TerminalSummary,
        incoming: TerminalSummary,
    },
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageIdMismatch { expected, actual } => write!(
                f,
                "reconciliation message ID {actual:?} does not match submission {expected:?}"
            ),
            Self::SummaryMessageMissing { message_id } => write!(
                f,
                "SummaryResult does not contain submission message ID {message_id:?}"
            ),
            Self::DuplicateSummaryMessage { message_id, count } => write!(
                f,
                "SummaryResult contains submission message ID {message_id:?} {count} times"
            ),
            Self::InconsistentSummary { message_id, issues } => write!(
                f,
                "SummaryResult for {message_id:?} failed {} consistency check(s)",
                issues.len()
            ),
            Self::ConflictingTerminalSummary { message_id, .. } => write!(
                f,
                "submission {message_id:?} already has a different terminal SummaryResult"
            ),
        }
    }
}

impl Error for ApplyError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionedSubmission {
    pub revision: u64,
    pub record: SubmissionRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreWriteOutcome {
    Stored { revision: u64 },
    Conflict { current_revision: Option<u64> },
}

/// Persistence boundary for a future PostgreSQL implementation.
///
/// expected_revision = None means create-only. Implementations should atomically
/// increment the revision on successful updates.
pub trait SubmissionStore {
    type Error;

    /// Loads one submission by platform/common message ID.
    ///
    /// # Errors
    ///
    /// Returns the backend-specific storage error.
    fn load(&self, message_id: &str) -> Result<Option<VersionedSubmission>, Self::Error>;

    /// Atomically creates or updates one submission using optimistic concurrency.
    ///
    /// # Errors
    ///
    /// Returns the backend-specific storage error.
    fn compare_and_swap(
        &mut self,
        expected_revision: Option<u64>,
        record: &SubmissionRecord,
    ) -> Result<StoreWriteOutcome, Self::Error>;
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, convert::Infallible};

    use tw_einvoice_results::{
        ProcessDiagnostic, ProcessMessageInfo, ResultBreakdown, ResultDetail, ResultParty,
        ResultRoute, ResultRouting, SummaryMessageInfo,
    };

    use super::*;

    const MESSAGE_ID: &str = "4.1-F0401-20260824-141623456-550e8400-e29b-41d4-a716-446655440000";

    fn routing() -> ResultRouting {
        ResultRouting {
            from: ResultParty {
                party_id: "12345678".into(),
                description: None,
            },
            from_vac: ResultRoute {
                routing_id: "FROM".into(),
                description: None,
            },
            to: ResultParty {
                party_id: "PLATFORM".into(),
                description: None,
            },
            to_vac: ResultRoute {
                routing_id: "TO".into(),
                description: None,
            },
        }
    }

    fn process(code: &str) -> ProcessResult {
        ProcessResult {
            routing: routing(),
            message: ProcessMessageInfo {
                id: MESSAGE_ID.into(),
                size: "123".into(),
                message_type: "F0401".into(),
                service: "S".into(),
                action: "C0401".into(),
            },
            diagnostics: vec![ProcessDiagnostic {
                code: code.into(),
                description: Some("synthetic diagnostic".into()),
                parameters: [None, None, None, None, None],
            }],
        }
    }

    fn summary(good: i32, failed: i32) -> SummaryMessage {
        let total = good + failed;
        SummaryMessage {
            info: SummaryMessageInfo {
                id: MESSAGE_ID.into(),
                size: "123".into(),
                message_type: "F0401".into(),
                service: "S".into(),
                action: "C0401".into(),
            },
            result: ResultBreakdown {
                total: ResultDetail {
                    count: total,
                    invoices: Vec::new(),
                },
                good: ResultDetail {
                    count: good,
                    invoices: Vec::new(),
                },
                failed: ResultDetail {
                    count: failed,
                    invoices: Vec::new(),
                },
            },
        }
    }

    #[test]
    fn transport_state_is_monotonic_and_idempotent() {
        let mut record = SubmissionRecord::new(MESSAGE_ID, 2).unwrap();
        assert_eq!(
            record.advance_transport(TransportState::Prepared),
            TransportAdvance::Advanced {
                from: TransportState::Created,
                to: TransportState::Prepared
            }
        );
        assert_eq!(
            record.advance_transport(TransportState::Prepared),
            TransportAdvance::Unchanged
        );
        assert_eq!(
            record.advance_transport(TransportState::Created),
            TransportAdvance::IgnoredStale {
                current: TransportState::Prepared,
                attempted: TransportState::Created,
            }
        );
        assert_eq!(record.transport_state(), TransportState::Prepared);
    }

    #[test]
    fn process_result_records_diagnostics_without_terminal_disposition() {
        let mut record = SubmissionRecord::new(MESSAGE_ID, 2).unwrap();
        assert_eq!(
            record.apply_process_result(process("E001")).unwrap(),
            ApplyOutcome::Changed
        );
        assert_eq!(record.transport_state(), TransportState::ProcessObserved);
        assert_eq!(record.process_results()[0].diagnostics[0].code, "E001");
        assert!(record.terminal_summary().is_none());
    }

    #[test]
    fn exact_process_result_replay_is_idempotent() {
        let mut record = SubmissionRecord::new(MESSAGE_ID, 2).unwrap();
        let result = process("0000");
        assert_eq!(
            record.apply_process_result(result.clone()).unwrap(),
            ApplyOutcome::Changed
        );
        assert_eq!(
            record.apply_process_result(result).unwrap(),
            ApplyOutcome::UnchangedReplay
        );
        assert_eq!(record.process_results().len(), 1);
    }

    #[test]
    fn applies_all_good_all_failed_and_partial_terminal_states() {
        for (good, failed, expected) in [
            (2, 0, TerminalDisposition::AllGood),
            (0, 2, TerminalDisposition::AllFailed),
            (1, 1, TerminalDisposition::PartialSuccess),
        ] {
            let mut record = SubmissionRecord::new(MESSAGE_ID, 2).unwrap();
            record.apply_summary_message(summary(good, failed)).unwrap();
            assert_eq!(record.terminal_summary().unwrap().disposition, expected);
            assert_eq!(record.transport_state(), TransportState::SummaryObserved);
        }
    }

    #[test]
    fn inconsistent_summary_does_not_change_terminal_state() {
        let mut record = SubmissionRecord::new(MESSAGE_ID, 2).unwrap();
        let error = record.apply_summary_message(summary(1, 0)).unwrap_err();
        assert!(matches!(
            error,
            ApplyError::InconsistentSummary { ref issues, .. }
                if issues.iter().any(|issue| matches!(
                    issue,
                    SummaryValidationIssue::ExpectedQuantityMismatch { expected: 2, actual: 1 }
                ))
        ));
        assert!(record.terminal_summary().is_none());
        assert_eq!(record.transport_state(), TransportState::Created);
    }

    #[test]
    fn exact_terminal_replay_is_idempotent_but_conflict_is_rejected() {
        let mut record = SubmissionRecord::new(MESSAGE_ID, 2).unwrap();
        let first = summary(2, 0);
        assert_eq!(
            record.apply_summary_message(first.clone()).unwrap(),
            ApplyOutcome::Changed
        );
        assert_eq!(
            record.apply_summary_message(first).unwrap(),
            ApplyOutcome::UnchangedReplay
        );
        let error = record.apply_summary_message(summary(1, 1)).unwrap_err();
        assert!(matches!(
            error,
            ApplyError::ConflictingTerminalSummary { .. }
        ));
        assert_eq!(
            record.terminal_summary().unwrap().disposition,
            TerminalDisposition::AllGood
        );
    }

    #[test]
    fn late_process_result_adds_diagnostics_without_transport_regression() {
        let mut record = SubmissionRecord::new(MESSAGE_ID, 2).unwrap();
        record.apply_summary_message(summary(2, 0)).unwrap();
        assert_eq!(
            record.apply_process_result(process("LATE")).unwrap(),
            ApplyOutcome::Changed
        );
        assert_eq!(record.transport_state(), TransportState::SummaryObserved);
        assert_eq!(record.process_results().len(), 1);
    }

    #[test]
    fn selects_matching_entry_from_multi_message_summary() {
        let mut record = SubmissionRecord::new(MESSAGE_ID, 2).unwrap();
        let mut other = summary(2, 0);
        other.info.id = "other-message-id".into();
        let result = SummaryResult {
            routing: routing(),
            messages: vec![other, summary(1, 1)],
        };
        record.apply_summary_result(&result).unwrap();
        assert_eq!(
            record.terminal_summary().unwrap().disposition,
            TerminalDisposition::PartialSuccess
        );
    }

    #[derive(Debug, Default)]
    struct MemoryStore {
        rows: HashMap<String, VersionedSubmission>,
    }

    impl SubmissionStore for MemoryStore {
        type Error = Infallible;

        fn load(&self, message_id: &str) -> Result<Option<VersionedSubmission>, Self::Error> {
            Ok(self.rows.get(message_id).cloned())
        }

        fn compare_and_swap(
            &mut self,
            expected_revision: Option<u64>,
            record: &SubmissionRecord,
        ) -> Result<StoreWriteOutcome, Self::Error> {
            let current = self.rows.get(record.message_id()).map(|row| row.revision);
            if current != expected_revision {
                return Ok(StoreWriteOutcome::Conflict {
                    current_revision: current,
                });
            }
            let revision = current.map_or(0, |value| value + 1);
            self.rows.insert(
                record.message_id().to_owned(),
                VersionedSubmission {
                    revision,
                    record: record.clone(),
                },
            );
            Ok(StoreWriteOutcome::Stored { revision })
        }
    }

    #[test]
    fn persistence_boundary_supports_create_and_optimistic_update() {
        let mut store = MemoryStore::default();
        let mut record = SubmissionRecord::new(MESSAGE_ID, 2).unwrap();
        assert_eq!(
            store.compare_and_swap(None, &record).unwrap(),
            StoreWriteOutcome::Stored { revision: 0 }
        );
        let _ = record.advance_transport(TransportState::Prepared);
        assert_eq!(
            store.compare_and_swap(Some(0), &record).unwrap(),
            StoreWriteOutcome::Stored { revision: 1 }
        );
        assert_eq!(
            store.compare_and_swap(Some(0), &record).unwrap(),
            StoreWriteOutcome::Conflict {
                current_revision: Some(1)
            }
        );
        assert_eq!(store.load(MESSAGE_ID).unwrap().unwrap().revision, 1);
    }
}
