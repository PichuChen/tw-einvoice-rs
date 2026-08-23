#![forbid(unsafe_code)]

use tw_einvoice_core::{MessageCode, MigVersion};

/// Metadata common to a MIG document before message-specific parsing is added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigDocumentMetadata {
    pub version: MigVersion,
    pub message_code: MessageCode,
}

/// Validation issue independent from a particular XML implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub path: Option<String>,
    pub code: String,
    pub message: String,
}

/// Result of schema and semantic validation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}
