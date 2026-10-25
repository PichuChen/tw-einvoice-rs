#![forbid(unsafe_code)]

pub mod catalog;
pub mod f0401;
pub mod primitives;

use tw_einvoice_core::{MessageCode, MigVersion};

pub use catalog::{Mig41MessageCode, UnknownMig41MessageCode};
pub use f0401::{
    F0401_NAMESPACE, F0401Amount, F0401Invoice, F0401Main, F0401StructureError, ProductItem,
    RoleDescription,
};
pub use primitives::{
    BondedArea, BuyerRemark, CurrencyCode, CustomsClearanceMark, DonateMark, InvoiceNumber,
    InvoiceType, MigDate, MigDecimal, MigTime, PrimitiveError, PrintMark, TaxRate, TaxType,
    ZeroTaxRateReason,
};

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
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}
