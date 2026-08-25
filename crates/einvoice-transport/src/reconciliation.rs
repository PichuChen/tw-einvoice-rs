use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use crate::{
    inbound::{
        InboundObjectKind, InboundParseError, ParsedReconciliation, classify_remote_name,
        parse_reconciliation,
    },
    receiver::{DurableReceiver, ReceiveError, ReceiveOutcome, RemoteInbox, RemoteObject},
};

/// Remote inbox that can enumerate the objects subsequently consumed through
/// the [`RemoteInbox`] download/delete durability boundary.
pub trait RemoteInboxLister: RemoteInbox {
    /// Returns one snapshot of objects visible in the remote `/out` directory.
    ///
    /// # Errors
    ///
    /// Returns the same backend error type used by download/delete operations.
    fn list(&self) -> Result<Vec<RemoteObject>, Self::Error>;
}

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
    /// `ProcessResult` carries one `MessageInfo/Id`; `SummaryResult` may contain up
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

/// Successfully received and classified result from one remote object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciledObject {
    pub receive_outcome: ReceiveOutcome,
    pub record: DurableInboundRecord,
}

/// Per-object failure. One bad object does not prevent the poller from
/// attempting other files returned by the same `/out` listing snapshot.
#[derive(Debug)]
pub enum PollObjectError<E> {
    Receive(ReceiveError<E>),
    Parse(DurableInboundError),
}

impl<E: fmt::Display> fmt::Display for PollObjectError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Receive(error) => error.fmt(f),
            Self::Parse(error) => error.fmt(f),
        }
    }
}

impl<E: Error + 'static> Error for PollObjectError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Receive(error) => Some(error),
            Self::Parse(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub struct PollAttempt<E> {
    pub object: RemoteObject,
    pub result: Result<ReconciledObject, PollObjectError<E>>,
}

#[derive(Debug)]
pub enum PollError<E> {
    List(E),
}

impl<E: fmt::Display> fmt::Display for PollError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List(error) => write!(f, "failed to list SFTP /out inbox: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for PollError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::List(error) => Some(error),
        }
    }
}

/// One-pass `/out` poller that guarantees remote acknowledgement happens only
/// after the durable receive boundary.
#[derive(Debug)]
pub struct ReconciliationPoller<R> {
    receiver: DurableReceiver<R>,
}

impl<R> ReconciliationPoller<R> {
    /// Creates a poller using the supplied remote inbox and durable staging
    /// directory.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the staging directory cannot be created.
    pub fn new(remote: R, inbox_dir: impl Into<PathBuf>) -> io::Result<Self> {
        DurableReceiver::new(remote, inbox_dir).map(|receiver| Self { receiver })
    }

    #[must_use]
    pub fn receiver(&self) -> &DurableReceiver<R> {
        &self.receiver
    }

    #[must_use]
    pub fn receiver_mut(&mut self) -> &mut DurableReceiver<R> {
        &mut self.receiver
    }
}

impl<R: RemoteInboxLister> ReconciliationPoller<R> {
    /// Lists `/out`, durably receives each object, and parses result messages.
    ///
    /// A listing failure aborts the poll because no stable object snapshot is
    /// available. Individual download/ack/parse failures are returned beside
    /// their object and do not stop other entries from being processed.
    ///
    /// # Errors
    ///
    /// Returns [`PollError::List`] when remote enumeration fails.
    pub fn poll_once(&mut self) -> Result<Vec<PollAttempt<R::Error>>, PollError<R::Error>> {
        let objects = self.receiver.remote().list().map_err(PollError::List)?;
        let mut attempts = Vec::with_capacity(objects.len());

        for object in objects {
            let result = match self.receiver.receive(&object) {
                Ok(receive_outcome) => {
                    let durable_path = match &receive_outcome {
                        ReceiveOutcome::Persisted { path }
                        | ReceiveOutcome::AlreadyPersisted { path } => path.clone(),
                    };
                    DurableInboundRecord::load(durable_path)
                        .map(|record| ReconciledObject {
                            receive_outcome,
                            record,
                        })
                        .map_err(PollObjectError::Parse)
                }
                Err(error) => Err(PollObjectError::Receive(error)),
            };
            attempts.push(PollAttempt { object, result });
        }

        Ok(attempts)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicU64, Ordering},
    };

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

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FakeRemoteError;

    impl fmt::Display for FakeRemoteError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("synthetic remote error")
        }
    }

    impl Error for FakeRemoteError {}

    #[derive(Debug)]
    struct FakeListedRemote {
        objects: Vec<RemoteObject>,
        bytes: HashMap<String, Vec<u8>>,
        deleted: Vec<String>,
    }

    impl FakeListedRemote {
        fn with_object(name: &str, bytes: impl Into<Vec<u8>>) -> Self {
            let bytes = bytes.into();
            let size = u64::try_from(bytes.len()).unwrap();
            Self {
                objects: vec![RemoteObject {
                    name: name.to_owned(),
                    size,
                }],
                bytes: HashMap::from([(name.to_owned(), bytes)]),
                deleted: Vec::new(),
            }
        }
    }

    impl RemoteInbox for FakeListedRemote {
        type Error = FakeRemoteError;

        fn download(
            &mut self,
            object: &RemoteObject,
            writer: &mut dyn io::Write,
        ) -> Result<(), Self::Error> {
            let bytes = self.bytes.get(&object.name).ok_or(FakeRemoteError)?;
            writer.write_all(bytes).map_err(|_| FakeRemoteError)
        }

        fn delete(&mut self, object: &RemoteObject) -> Result<(), Self::Error> {
            self.deleted.push(object.name.clone());
            Ok(())
        }
    }

    impl RemoteInboxLister for FakeListedRemote {
        fn list(&self) -> Result<Vec<RemoteObject>, Self::Error> {
            Ok(self.objects.clone())
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

    #[test]
    fn poller_receives_deletes_and_parses_in_that_order() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessResult xmlns="urn:GEINV:ProcessResult:4.1">
  <RoutingInfo>
    <From><PartyId>12345678</PartyId></From>
    <FromVAC><RoutingId>FROM</RoutingId></FromVAC>
    <To><PartyId>PLATFORM</PartyId></To>
    <ToVAC><RoutingId>TO</RoutingId></ToVAC>
  </RoutingInfo>
  <MessageInfo>
    <Id>correlation-id</Id><Size>10</Size><MessageType>F0401</MessageType>
    <Service>S</Service><Action>C0401</Action>
  </MessageInfo>
  <Result><Info><Code>0000</Code></Info></Result>
</ProcessResult>"#;
        let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "tw-einvoice-poller-test-{}-{id}",
            std::process::id()
        ));
        let remote = FakeListedRemote::with_object("synthetic_ProcessResult", xml.to_vec());
        let mut poller = ReconciliationPoller::new(remote, &directory).unwrap();

        let attempts = poller.poll_once().unwrap();
        assert_eq!(attempts.len(), 1);
        let result = attempts.into_iter().next().unwrap().result.unwrap();
        assert_eq!(result.record.correlation_ids(), ["correlation-id"]);
        assert_eq!(
            poller.receiver().remote().deleted,
            ["synthetic_ProcessResult"]
        );
        assert!(directory.join("synthetic_ProcessResult").exists());
        let _ = fs::remove_dir_all(directory);
    }
}
