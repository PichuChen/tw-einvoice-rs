use std::{
    error::Error,
    fmt,
    fs,
    io,
    path::{Path, PathBuf},
};

use crate::inbound::{
    InboundObjectKind, InboundParseError, ParsedReconciliation, classify_remote_name,
    parse_reconciliation,
};

/// One object after it has crossed the durable local receive boundary and has
/// been classified/parsed for domain reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableInboundRecord {
    pub path: PathBuf,
    pub kind: InboundObjectKind,
    pub reconciliation: Option<ParsedReconciliation>,
}

impl DurableInboundRecord {
    /// Loads one durably persisted `/out` object.
    ///
    /// Classification is filename-driven to mirror Turnkey 3.2.1. Only
    /// ProcessResult/SummaryResult objects are parsed as reconciliation XML;
    /// control/invoice/error classes remain durable opaque records for their
    /// dedicated handlers.
    ///
    /// # Errors
    ///
    /// Returns [`DurableInboundError`] when the path lacks a UTF-8 basename,
    /// cannot be read, or a reconciliation result fails XML parsing.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, DurableInboundError> {
        let path = path.into();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| DurableInboundError::InvalidPath(path.clone()))?;
        let kind = classify_remote_name(name);
        let bytes = fs::read(&path).map_err(DurableInboundError::Io)?;
        let reconciliation =
            parse_reconciliation(&kind, &bytes).map_err(DurableInboundError::Parse)?;

        Ok(Self {
            path,
            kind,
            reconciliation,
        })
    }

    /// Platform message IDs that can be used to correlate this result with
    /// durable submission state.
    ///
    /// ProcessResult carries one `MessageInfo/Id`; SummaryResult may contain up
    /// to 5,000 `DetailList/Message/Info/Id` values.
    #[must_use]
    pub fn correlation_ids(&self) -> Vec<&str> {
        match &self.reconciliation {
            Some(ParsedReconciliation::Process(result)) => vec![result.message.id.as_str()],
            Some(ParsedReconciliation::Summary(result)) => result
                .messages
                .iter()
                .map(|message| message.info.id.as_str())
                .collect(),
            None => Vec::new(),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug)]
pub enum DurableInboundError {
    InvalidPath(PathBuf),
    Io(io::Error),
    Parse(InboundParseError),
}

impl fmt::Display for DurableInboundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(path) => write!(
                f,
                "durable inbound path has no UTF-8 basename: {}",
                path.display()
            ),
            Self::Io(error) => write!(f, "failed to read durable inbound object: {error}"),
            Self::Parse(error) => write!(f, "failed to parse durable inbound object: {error}"),
        }
    }
}

impl Error for DurableInboundError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPath(_) => None,
            Self::Io(error) => Some(error),
            Self::Parse(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEST_FILE: AtomicU64 = AtomicU64::new(0);

    struct TestFile(PathBuf);

    impl TestFile {
        fn create(name_suffix: &str, bytes: &[u8]) -> Self {
            let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "tw-einvoice-reconciliation-test-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&directory).unwrap();
            let path = directory.join(format!("synthetic_{name_suffix}"));
            fs::write(&path, bytes).unwrap();
            Self(path)
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            if let Some(parent) = self.0.parent() {
                let _ = fs::remove_dir_all(parent);
            }
        }
    }

    #[test]
    fn process_result_exposes_message_id_for_correlation() {
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
        let file = TestFile::create("ProcessResult", xml);

        let record = DurableInboundRecord::load(&file.0).unwrap();
        assert_eq!(record.kind, InboundObjectKind::ProcessResult);
        assert_eq!(
            record.correlation_ids(),
            ["4.1-F0401-20260824-141623456-550e8400-e29b-41d4-a716-446655440000"]
        );
    }

    #[test]
    fn non_result_control_file_remains_opaque() {
        let file = TestFile::create("Ack", b"opaque-control-payload");
        let record = DurableInboundRecord::load(&file.0).unwrap();

        assert_eq!(record.kind, InboundObjectKind::ExchangeAck);
        assert!(record.reconciliation.is_none());
        assert!(record.correlation_ids().is_empty());
    }
}
