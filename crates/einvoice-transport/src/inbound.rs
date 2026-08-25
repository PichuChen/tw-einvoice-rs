use std::{error::Error, fmt, str};

use tw_einvoice_results::{ProcessResult, ResultParseError, SummaryResult};

use crate::filename::MessageCommonName;

/// E05xx operational message classes delivered through the Turnkey `/out`
/// channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum E05MessageType {
    E0501,
    E0502,
    E0503,
    E0504,
}

/// File classes used by Turnkey 3.2.1 when distributing objects downloaded from
/// SFTP `/out`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundObjectKind {
    ProcessResult,
    SummaryResult,
    E05(E05MessageType),
    ExchangeAck,
    Invoice(MessageCommonName),
    Unknown,
}

/// Classifies one remote object using the filename rules recovered from
/// `ReceiveFile` in Turnkey 3.2.1.
///
/// The official implementation checks result/control suffixes first, then
/// treats an otherwise valid message-common-name as an invoice payload. This
/// function preserves that ordering rather than inferring the type from XML
/// content.
#[must_use]
pub fn classify_remote_name(name: &str) -> InboundObjectKind {
    if name.ends_with("ProcessResult") {
        return InboundObjectKind::ProcessResult;
    }
    if name.ends_with("SummaryResult") {
        return InboundObjectKind::SummaryResult;
    }
    if name.ends_with("E0501") {
        return InboundObjectKind::E05(E05MessageType::E0501);
    }
    if name.ends_with("E0502") {
        return InboundObjectKind::E05(E05MessageType::E0502);
    }
    if name.ends_with("E0503") {
        return InboundObjectKind::E05(E05MessageType::E0503);
    }
    if name.ends_with("E0504") {
        return InboundObjectKind::E05(E05MessageType::E0504);
    }
    if name.ends_with("Ack") {
        return InboundObjectKind::ExchangeAck;
    }

    match MessageCommonName::parse(name) {
        Ok(common_name) => InboundObjectKind::Invoice(common_name),
        Err(_) => InboundObjectKind::Unknown,
    }
}

/// Parsed reconciliation records that directly advance a submitted package's
/// durable state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedReconciliation {
    Process(ProcessResult),
    Summary(SummaryResult),
}

/// Parses a durable inbound object when its filename class is a reconciliation
/// result.
///
/// Non-result object classes return `Ok(None)` so E05xx, acknowledgements and
/// inbound invoice payloads can be routed to their own handlers without first
/// trying to deserialize them as result XML.
///
/// # Errors
///
/// Returns [`InboundParseError`] if a result object is not UTF-8 or does not
/// match the MIG 4.1 ProcessResult/SummaryResult structure.
pub fn parse_reconciliation(
    kind: &InboundObjectKind,
    bytes: &[u8],
) -> Result<Option<ParsedReconciliation>, InboundParseError> {
    let xml = match kind {
        InboundObjectKind::ProcessResult | InboundObjectKind::SummaryResult => {
            str::from_utf8(bytes).map_err(InboundParseError::Utf8)?
        }
        InboundObjectKind::E05(_)
        | InboundObjectKind::ExchangeAck
        | InboundObjectKind::Invoice(_)
        | InboundObjectKind::Unknown => return Ok(None),
    };

    match kind {
        InboundObjectKind::ProcessResult => ProcessResult::parse(xml)
            .map(ParsedReconciliation::Process)
            .map(Some)
            .map_err(InboundParseError::Result),
        InboundObjectKind::SummaryResult => SummaryResult::parse(xml)
            .map(ParsedReconciliation::Summary)
            .map(Some)
            .map_err(InboundParseError::Result),
        InboundObjectKind::E05(_)
        | InboundObjectKind::ExchangeAck
        | InboundObjectKind::Invoice(_)
        | InboundObjectKind::Unknown => Ok(None),
    }
}

#[derive(Debug)]
pub enum InboundParseError {
    Utf8(str::Utf8Error),
    Result(ResultParseError),
}

impl fmt::Display for InboundParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8(error) => write!(f, "inbound reconciliation XML is not UTF-8: {error}"),
            Self::Result(error) => write!(f, "invalid inbound reconciliation XML: {error}"),
        }
    }
}

impl Error for InboundParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Utf8(error) => Some(error),
            Self::Result(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMON: &str = "4.1-F0401-20260824-141623456-550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn classifier_matches_receivefile_suffix_precedence() {
        assert_eq!(
            classify_remote_name("batch-123_ProcessResult"),
            InboundObjectKind::ProcessResult
        );
        assert_eq!(
            classify_remote_name("batch-123_SummaryResult"),
            InboundObjectKind::SummaryResult
        );
        assert_eq!(
            classify_remote_name("batch_E0504"),
            InboundObjectKind::E05(E05MessageType::E0504)
        );
        assert_eq!(
            classify_remote_name("batch_Ack"),
            InboundObjectKind::ExchangeAck
        );
        assert!(matches!(
            classify_remote_name(COMMON),
            InboundObjectKind::Invoice(_)
        ));
        assert_eq!(
            classify_remote_name("garbage.bin"),
            InboundObjectKind::Unknown
        );
    }

    #[test]
    fn parses_process_result_only_after_filename_classification() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessResult xmlns="urn:GEINV:ProcessResult:4.1">
  <RoutingInfo>
    <From><PartyId>12345678</PartyId></From>
    <FromVAC><RoutingId>FROM</RoutingId></FromVAC>
    <To><PartyId>PLATFORM</PartyId></To>
    <ToVAC><RoutingId>TO</RoutingId></ToVAC>
  </RoutingInfo>
  <MessageInfo>
    <Id>4.1-F0401-20260824-141623456-550e8400-e29b-41d4-a716-446655440000</Id>
    <Size>123</Size>
    <MessageType>F0401</MessageType>
    <Service>S</Service>
    <Action>C0401</Action>
  </MessageInfo>
  <Result><Info><Code>0000</Code></Info></Result>
</ProcessResult>"#;

        let parsed = parse_reconciliation(&InboundObjectKind::ProcessResult, xml)
            .unwrap()
            .unwrap();
        assert!(matches!(parsed, ParsedReconciliation::Process(_)));

        assert!(
            parse_reconciliation(&InboundObjectKind::ExchangeAck, xml)
                .unwrap()
                .is_none()
        );
    }
}
