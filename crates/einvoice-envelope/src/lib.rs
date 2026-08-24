#![forbid(unsafe_code)]

use std::{error::Error, fmt};

use tw_einvoice_core::MigVersion;

/// Party information carried by the Turnkey invoice envelope.
///
/// The envelope XSD defines `PartyId` as an unconstrained XML string. Although
/// domestic configurations commonly use a BAN here, the envelope layer must not
/// impose the narrower MIG `BAN` lexical type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyInfo {
    pub identifier: String,
    pub description: Option<String>,
}

/// Routing/VAC information carried alongside a party.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingInfo {
    pub identifier: String,
    pub description: Option<String>,
}

/// The four routing principals required by the MIG 4.1 invoice envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeRouting {
    pub from: PartyInfo,
    pub from_vac: RoutingInfo,
    pub to: PartyInfo,
    pub to_vac: RoutingInfo,
}

/// Metadata attached to `InvoicePack`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoicePackMetadata {
    pub message_type: String,
    pub version: MigVersion,
}

/// Opaque serialized MIG payload. Message-specific parsing belongs to
/// `tw-einvoice-mig`; the envelope layer only owns batching and routing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigPayload(Vec<u8>);

impl MigPayload {
    /// Creates a non-empty serialized MIG payload.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError::EmptyPayload`] when `bytes` is empty.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, EnvelopeError> {
        let bytes = bytes.into();
        if bytes.is_empty() {
            Err(EnvelopeError::EmptyPayload)
        } else {
            Ok(Self(bytes))
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Specification-driven representation of `InvoiceEnvelope`.
///
/// MIG 4.1 permits one through one thousand embedded invoice messages in one
/// `InvoicePack`. This type enforces that cardinality independently of the XML
/// serializer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceEnvelope {
    pub routing: EnvelopeRouting,
    pub metadata: InvoicePackMetadata,
    payloads: Vec<MigPayload>,
}

impl InvoiceEnvelope {
    pub const MAX_PAYLOADS: usize = 1_000;

    /// Constructs an invoice envelope with the XSD-defined payload cardinality.
    ///
    /// # Errors
    ///
    /// Returns an error when the pack is empty or contains more than 1,000
    /// messages.
    pub fn new(
        routing: EnvelopeRouting,
        metadata: InvoicePackMetadata,
        payloads: Vec<MigPayload>,
    ) -> Result<Self, EnvelopeError> {
        match payloads.len() {
            0 => Err(EnvelopeError::NoPayloads),
            count if count > Self::MAX_PAYLOADS => Err(EnvelopeError::TooManyPayloads {
                count,
                max: Self::MAX_PAYLOADS,
            }),
            _ => Ok(Self {
                routing,
                metadata,
                payloads,
            }),
        }
    }

    #[must_use]
    pub fn payloads(&self) -> &[MigPayload] {
        &self.payloads
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.payloads.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    EmptyPayload,
    NoPayloads,
    TooManyPayloads { count: usize, max: usize },
}

impl fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPayload => f.write_str("MIG payload must not be empty"),
            Self::NoPayloads => f.write_str("invoice envelope requires at least one payload"),
            Self::TooManyPayloads { count, max } => {
                write!(
                    f,
                    "invoice envelope contains {count} payloads; maximum is {max}"
                )
            }
        }
    }
}

impl Error for EnvelopeError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn routing() -> EnvelopeRouting {
        EnvelopeRouting {
            from: PartyInfo {
                identifier: "12345678".into(),
                description: None,
            },
            from_vac: RoutingInfo {
                identifier: "FROM-VAC".into(),
                description: None,
            },
            to: PartyInfo {
                identifier: "87654321".into(),
                description: None,
            },
            to_vac: RoutingInfo {
                identifier: "TO-VAC".into(),
                description: None,
            },
        }
    }

    fn metadata() -> InvoicePackMetadata {
        InvoicePackMetadata {
            message_type: "F0401".into(),
            version: MigVersion::V4_1,
        }
    }

    #[test]
    fn rejects_empty_envelope() {
        assert_eq!(
            InvoiceEnvelope::new(routing(), metadata(), vec![]).unwrap_err(),
            EnvelopeError::NoPayloads
        );
    }

    #[test]
    fn accepts_one_thousand_payloads() {
        let payload = MigPayload::new(b"<Invoice/>".to_vec()).unwrap();
        let envelope = InvoiceEnvelope::new(
            routing(),
            metadata(),
            vec![payload; InvoiceEnvelope::MAX_PAYLOADS],
        )
        .unwrap();

        assert_eq!(envelope.count(), 1_000);
    }

    #[test]
    fn rejects_more_than_one_thousand_payloads() {
        let payload = MigPayload::new(b"<Invoice/>".to_vec()).unwrap();
        let error = InvoiceEnvelope::new(
            routing(),
            metadata(),
            vec![payload; InvoiceEnvelope::MAX_PAYLOADS + 1],
        )
        .unwrap_err();

        assert_eq!(
            error,
            EnvelopeError::TooManyPayloads {
                count: 1_001,
                max: 1_000,
            }
        );
    }
}
