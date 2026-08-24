#![forbid(unsafe_code)]

use std::{error::Error, fmt};

use serde::Deserialize;

/// Party identity returned in the result-message routing tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultParty {
    pub party_id: String,
    pub description: Option<String>,
}

/// Routing/VAC identity returned in the result-message routing tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRoute {
    pub routing_id: String,
    pub description: Option<String>,
}

/// Routing tuple common to MIG 4.1 `ProcessResult` and `SummaryResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRouting {
    pub from: ResultParty,
    pub from_vac: ResultRoute,
    pub to: ResultParty,
    pub to_vac: ResultRoute,
}

/// Message metadata carried by `ProcessResult`.
///
/// `size` intentionally remains a string because the official MIG 4.1
/// `ProcessResult.xsd` declares `MessageInfo/Size` as `xsd:string`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessMessageInfo {
    pub id: String,
    pub size: String,
    pub message_type: String,
    pub service: String,
    pub action: String,
}

/// One diagnostic record from `ProcessResult/Result/Info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDiagnostic {
    pub code: String,
    pub description: Option<String>,
    pub parameters: [Option<String>; 5],
}

/// MIG 4.1 package/message processing result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessResult {
    pub routing: ResultRouting,
    pub message: ProcessMessageInfo,
    pub diagnostics: Vec<ProcessDiagnostic>,
}

impl ProcessResult {
    /// Parses a MIG 4.1 `ProcessResult` XML document.
    ///
    /// # Errors
    ///
    /// Returns [`ResultParseError`] when the XML is malformed or does not match
    /// the expected result-message structure.
    pub fn parse(xml: &str) -> Result<Self, ResultParseError> {
        let wire: WireProcessResult =
            quick_xml::de::from_str(xml).map_err(ResultParseError::Xml)?;
        Ok(wire.into())
    }
}

/// Message metadata carried by `SummaryResult`.
///
/// The schema declares `Size` as `xsd:positiveInteger`; the public model keeps
/// the lexical value so a parser cannot overflow on a schema-valid integer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryMessageInfo {
    pub id: String,
    pub size: String,
    pub message_type: String,
    pub service: String,
    pub action: String,
}

/// One invoice reference listed in a summary bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceReference {
    pub reference_number: String,
    pub invoice_date: String,
}

/// One of `Total`, `Good`, or `Failed` in a MIG 4.1 summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultDetail {
    pub count: i32,
    pub invoices: Vec<InvoiceReference>,
}

/// Count/list breakdown for one submitted message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultBreakdown {
    pub total: ResultDetail,
    pub good: ResultDetail,
    pub failed: ResultDetail,
}

/// One message entry in `SummaryResult/DetailList`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryMessage {
    pub info: SummaryMessageInfo,
    pub result: ResultBreakdown,
}

impl SummaryMessage {
    /// Checks arithmetic/count invariants useful for durable reconciliation.
    #[must_use]
    pub fn validation_issues(&self, expected_quantity: Option<u32>) -> Vec<SummaryValidationIssue> {
        let mut issues = Vec::new();

        for (bucket, detail) in [
            (ResultBucket::Total, &self.result.total),
            (ResultBucket::Good, &self.result.good),
            (ResultBucket::Failed, &self.result.failed),
        ] {
            if detail.count < 0 {
                issues.push(SummaryValidationIssue::NegativeCount {
                    bucket,
                    count: detail.count,
                });
            }

            if !detail.invoices.is_empty()
                && usize::try_from(detail.count).ok() != Some(detail.invoices.len())
            {
                issues.push(SummaryValidationIssue::InvoiceListCountMismatch {
                    bucket,
                    declared: detail.count,
                    listed: detail.invoices.len(),
                });
            }
        }

        if self.result.total.count != self.result.good.count + self.result.failed.count {
            issues.push(SummaryValidationIssue::TotalMismatch {
                total: self.result.total.count,
                good: self.result.good.count,
                failed: self.result.failed.count,
            });
        }

        if let Some(expected) = expected_quantity {
            if i64::from(self.result.total.count) != i64::from(expected) {
                issues.push(SummaryValidationIssue::ExpectedQuantityMismatch {
                    expected,
                    actual: self.result.total.count,
                });
            }
        }

        issues
    }

    /// Returns true when summary arithmetic and the optional expected batch
    /// quantity are consistent.
    #[must_use]
    pub fn is_consistent(&self, expected_quantity: Option<u32>) -> bool {
        self.validation_issues(expected_quantity).is_empty()
    }
}

/// MIG 4.1 reconciliation summary for one or more submitted message objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryResult {
    pub routing: ResultRouting,
    pub messages: Vec<SummaryMessage>,
}

impl SummaryResult {
    /// Parses a MIG 4.1 `SummaryResult` XML document.
    ///
    /// # Errors
    ///
    /// Returns [`ResultParseError`] when the XML is malformed or does not match
    /// the expected result-message structure.
    pub fn parse(xml: &str) -> Result<Self, ResultParseError> {
        let wire: WireSummaryResult =
            quick_xml::de::from_str(xml).map_err(ResultParseError::Xml)?;
        Ok(wire.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResultBucket {
    Total,
    Good,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryValidationIssue {
    NegativeCount {
        bucket: ResultBucket,
        count: i32,
    },
    TotalMismatch {
        total: i32,
        good: i32,
        failed: i32,
    },
    ExpectedQuantityMismatch {
        expected: u32,
        actual: i32,
    },
    InvoiceListCountMismatch {
        bucket: ResultBucket,
        declared: i32,
        listed: usize,
    },
}

#[derive(Debug)]
pub enum ResultParseError {
    Xml(quick_xml::DeError),
}

impl fmt::Display for ResultParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(error) => write!(f, "failed to parse MIG reconciliation XML: {error}"),
        }
    }
}

impl Error for ResultParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Xml(error) => Some(error),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireProcessResult {
    routing_info: WireRouting,
    message_info: WireProcessMessageInfo,
    result: WireProcessDiagnosticList,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireRouting {
    from: WireParty,
    from_vac: WireRoute,
    to: WireParty,
    to_vac: WireRoute,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireParty {
    party_id: String,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireRoute {
    routing_id: String,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireProcessMessageInfo {
    id: String,
    size: String,
    message_type: String,
    service: String,
    action: String,
}

#[derive(Debug, Deserialize)]
struct WireProcessDiagnosticList {
    #[serde(rename = "Info")]
    info: Vec<WireProcessDiagnostic>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireProcessDiagnostic {
    code: String,
    description: Option<String>,
    parameter0: Option<String>,
    parameter1: Option<String>,
    parameter2: Option<String>,
    parameter3: Option<String>,
    parameter4: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireSummaryResult {
    routing_info: WireRouting,
    detail_list: WireSummaryDetailList,
}

#[derive(Debug, Deserialize)]
struct WireSummaryDetailList {
    #[serde(rename = "Message")]
    messages: Vec<WireSummaryMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireSummaryMessage {
    info: WireSummaryMessageInfo,
    result_type: WireResultBreakdown,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireSummaryMessageInfo {
    id: String,
    size: String,
    message_type: String,
    service: String,
    action: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireResultBreakdown {
    total: WireResultBucket,
    good: WireResultBucket,
    failed: WireResultBucket,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireResultBucket {
    result_detail_type: WireResultDetail,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireResultDetail {
    count: i32,
    invoices: Option<WireInvoices>,
}

#[derive(Debug, Deserialize)]
struct WireInvoices {
    #[serde(rename = "Invoice")]
    invoices: Vec<WireInvoiceReference>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireInvoiceReference {
    reference_number: String,
    invoice_date: String,
}

impl From<WireRouting> for ResultRouting {
    fn from(value: WireRouting) -> Self {
        Self {
            from: value.from.into(),
            from_vac: value.from_vac.into(),
            to: value.to.into(),
            to_vac: value.to_vac.into(),
        }
    }
}

impl From<WireParty> for ResultParty {
    fn from(value: WireParty) -> Self {
        Self {
            party_id: value.party_id,
            description: value.description,
        }
    }
}

impl From<WireRoute> for ResultRoute {
    fn from(value: WireRoute) -> Self {
        Self {
            routing_id: value.routing_id,
            description: value.description,
        }
    }
}

impl From<WireProcessResult> for ProcessResult {
    fn from(value: WireProcessResult) -> Self {
        Self {
            routing: value.routing_info.into(),
            message: ProcessMessageInfo {
                id: value.message_info.id,
                size: value.message_info.size,
                message_type: value.message_info.message_type,
                service: value.message_info.service,
                action: value.message_info.action,
            },
            diagnostics: value.result.info.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<WireProcessDiagnostic> for ProcessDiagnostic {
    fn from(value: WireProcessDiagnostic) -> Self {
        Self {
            code: value.code,
            description: value.description,
            parameters: [
                value.parameter0,
                value.parameter1,
                value.parameter2,
                value.parameter3,
                value.parameter4,
            ],
        }
    }
}

impl From<WireSummaryResult> for SummaryResult {
    fn from(value: WireSummaryResult) -> Self {
        Self {
            routing: value.routing_info.into(),
            messages: value
                .detail_list
                .messages
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

impl From<WireSummaryMessage> for SummaryMessage {
    fn from(value: WireSummaryMessage) -> Self {
        Self {
            info: SummaryMessageInfo {
                id: value.info.id,
                size: value.info.size,
                message_type: value.info.message_type,
                service: value.info.service,
                action: value.info.action,
            },
            result: value.result_type.into(),
        }
    }
}

impl From<WireResultBreakdown> for ResultBreakdown {
    fn from(value: WireResultBreakdown) -> Self {
        Self {
            total: value.total.result_detail_type.into(),
            good: value.good.result_detail_type.into(),
            failed: value.failed.result_detail_type.into(),
        }
    }
}

impl From<WireResultDetail> for ResultDetail {
    fn from(value: WireResultDetail) -> Self {
        Self {
            count: value.count,
            invoices: value.invoices.map_or_else(Vec::new, |invoices| {
                invoices.invoices.into_iter().map(Into::into).collect()
            }),
        }
    }
}

impl From<WireInvoiceReference> for InvoiceReference {
    fn from(value: WireInvoiceReference) -> Self {
        Self {
            reference_number: value.reference_number,
            invoice_date: value.invoice_date,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTING: &str = r#"
        <RoutingInfo>
          <From><PartyId>12345678</PartyId><Description>sender</Description></From>
          <FromVAC><RoutingId>FROM-VAC</RoutingId></FromVAC>
          <To><PartyId>0000000000</PartyId></To>
          <ToVAC><RoutingId>TO-VAC</RoutingId><Description>platform</Description></ToVAC>
        </RoutingInfo>
    "#;

    #[test]
    fn parses_process_result_and_preserves_string_size() {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <ProcessResult xmlns="urn:GEINV:ProcessResult:4.1">
              {ROUTING}
              <MessageInfo>
                <Id>4.1-F0401-20260824-141623456-550e8400-e29b-41d4-a716-446655440000</Id>
                <Size>0012345</Size>
                <MessageType>F0401</MessageType>
                <Service>S</Service>
                <Action>C0401</Action>
              </MessageInfo>
              <Result>
                <Info>
                  <Code>E001</Code>
                  <Description>synthetic validation error</Description>
                  <Parameter0>InvoiceNumber</Parameter0>
                  <Parameter1>AB12345678</Parameter1>
                </Info>
              </Result>
            </ProcessResult>"#
        );

        let result = ProcessResult::parse(&xml).unwrap();
        assert_eq!(result.routing.from.party_id, "12345678");
        assert_eq!(result.message.size, "0012345");
        assert_eq!(result.message.message_type, "F0401");
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, "E001");
        assert_eq!(
            result.diagnostics[0].parameters[0].as_deref(),
            Some("InvoiceNumber")
        );
        assert_eq!(
            result.diagnostics[0].parameters[1].as_deref(),
            Some("AB12345678")
        );
    }

    #[test]
    fn parses_consistent_summary_result() {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <SummaryResult xmlns="urn:GEINV:SummaryResult:4.1">
              {ROUTING}
              <DetailList>
                <Message>
                  <Info>
                    <Id>4.1-F0401-20260824-141623456-550e8400-e29b-41d4-a716-446655440000</Id>
                    <Size>12345</Size>
                    <MessageType>F0401</MessageType>
                    <Service>S</Service>
                    <Action>C0401</Action>
                  </Info>
                  <ResultType>
                    <Total><ResultDetailType><Count>2</Count></ResultDetailType></Total>
                    <Good>
                      <ResultDetailType>
                        <Count>1</Count>
                        <Invoices><Invoice><ReferenceNumber>AB12345678</ReferenceNumber><InvoiceDate>20260824</InvoiceDate></Invoice></Invoices>
                      </ResultDetailType>
                    </Good>
                    <Failed>
                      <ResultDetailType>
                        <Count>1</Count>
                        <Invoices><Invoice><ReferenceNumber>AB12345679</ReferenceNumber><InvoiceDate>20260824</InvoiceDate></Invoice></Invoices>
                      </ResultDetailType>
                    </Failed>
                  </ResultType>
                </Message>
              </DetailList>
            </SummaryResult>"#
        );

        let result = SummaryResult::parse(&xml).unwrap();
        assert_eq!(result.messages.len(), 1);
        let message = &result.messages[0];
        assert_eq!(message.info.size, "12345");
        assert!(message.is_consistent(Some(2)));
        assert_eq!(
            message.result.good.invoices[0].reference_number,
            "AB12345678"
        );
        assert_eq!(
            message.result.failed.invoices[0].reference_number,
            "AB12345679"
        );
    }

    #[test]
    fn reports_summary_count_inconsistencies() {
        let message = SummaryMessage {
            info: SummaryMessageInfo {
                id: "synthetic".into(),
                size: "100".into(),
                message_type: "F0401".into(),
                service: "S".into(),
                action: "C0401".into(),
            },
            result: ResultBreakdown {
                total: ResultDetail {
                    count: 3,
                    invoices: vec![],
                },
                good: ResultDetail {
                    count: 1,
                    invoices: vec![],
                },
                failed: ResultDetail {
                    count: 1,
                    invoices: vec![],
                },
            },
        };

        assert_eq!(
            message.validation_issues(Some(4)),
            vec![
                SummaryValidationIssue::TotalMismatch {
                    total: 3,
                    good: 1,
                    failed: 1,
                },
                SummaryValidationIssue::ExpectedQuantityMismatch {
                    expected: 4,
                    actual: 3,
                },
            ]
        );
    }
}
