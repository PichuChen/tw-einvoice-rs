use std::{
    error::Error,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Metadata for one object visible in the Ministry SFTP `/out` inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteObject {
    pub name: String,
    pub size: u64,
}

/// Minimal remote-inbox boundary required by the durable receiver.
///
/// A concrete SFTP implementation owns connection/authentication concerns; the
/// receive algorithm only needs deterministic download and delete operations.
pub trait RemoteInbox {
    type Error;

    /// Downloads `object` into `writer` without deleting the remote object.
    ///
    /// # Errors
    ///
    /// Returns the backend-specific error when the object cannot be read.
    fn download(
        &mut self,
        object: &RemoteObject,
        writer: &mut dyn Write,
    ) -> Result<(), Self::Error>;

    /// Deletes `object` after a durable local copy is known to exist.
    ///
    /// # Errors
    ///
    /// Returns the backend-specific error when remote deletion fails.
    fn delete(&mut self, object: &RemoteObject) -> Result<(), Self::Error>;
}

/// Result of durably receiving one remote object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiveOutcome {
    /// This call published a new durable local object.
    Persisted { path: PathBuf },
    /// An identical durable object already existed, typically after a crash
    /// between local persistence and remote deletion.
    AlreadyPersisted { path: PathBuf },
}

/// Remote operation that failed while receiving an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteOperation {
    Download,
    Delete,
}

/// Failure while moving a remote `/out` object across the local durability
/// boundary.
#[derive(Debug)]
pub enum ReceiveError<E> {
    InvalidRemoteName { name: String },
    EmptyObject { name: String },
    SizeMismatch {
        name: String,
        expected: u64,
        actual: u64,
    },
    ConflictingLocalObject { name: String, path: PathBuf },
    Io(io::Error),
    Remote {
        operation: RemoteOperation,
        source: E,
    },
}

impl<E: fmt::Display> fmt::Display for ReceiveError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRemoteName { name } => {
                write!(f, "unsafe remote result filename: {name:?}")
            }
            Self::EmptyObject { name } => write!(f, "remote result object {name:?} is empty"),
            Self::SizeMismatch {
                name,
                expected,
                actual,
            } => write!(
                f,
                "remote result object {name:?} expected {expected} bytes but downloaded {actual}"
            ),
            Self::ConflictingLocalObject { name, path } => write!(
                f,
                "remote result object {name:?} conflicts with existing durable file {}",
                path.display()
            ),
            Self::Io(error) => write!(f, "durable receive filesystem error: {error}"),
            Self::Remote { operation, source } => {
                write!(f, "remote {operation:?} operation failed: {source}")
            }
        }
    }
}

impl<E: Error + 'static> Error for ReceiveError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Remote { source, .. } => Some(source),
            Self::InvalidRemoteName { .. }
            | Self::EmptyObject { .. }
            | Self::SizeMismatch { .. }
            | Self::ConflictingLocalObject { .. } => None,
        }
    }
}

impl<E> From<io::Error> for ReceiveError<E> {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Crash-safe receive worker for SFTP `/out` objects.
///
/// The receiver intentionally separates remote transport from local durability.
/// It downloads into a same-directory temporary file, flushes and `fsync`s it,
/// verifies the advertised remote length, atomically publishes without
/// overwriting an existing result, and only then deletes the remote object.
/// Parsing/reconciliation happens after this boundary.
#[derive(Debug)]
pub struct DurableReceiver<R> {
    remote: R,
    inbox_dir: PathBuf,
}

impl<R> DurableReceiver<R> {
    /// Creates a receiver and ensures that the durable inbox directory exists.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the inbox directory cannot be created.
    pub fn new(remote: R, inbox_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let inbox_dir = inbox_dir.into();
        fs::create_dir_all(&inbox_dir)?;
        Ok(Self { remote, inbox_dir })
    }

    #[must_use]
    pub fn inbox_dir(&self) -> &Path {
        &self.inbox_dir
    }

    #[must_use]
    pub fn remote(&self) -> &R {
        &self.remote
    }

    pub fn remote_mut(&mut self) -> &mut R {
        &mut self.remote
    }
}

impl<R: RemoteInbox> DurableReceiver<R> {
    /// Durably receives one remote object and deletes the remote copy only after
    /// local persistence succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`ReceiveError`] for unsafe names, incomplete downloads, local
    /// conflicts/I/O failures, or remote download/delete failures. A remote
    /// delete failure deliberately leaves the durable local copy intact so a
    /// retry can recognize it and finish deletion without applying domain state
    /// twice.
    pub fn receive(&mut self, object: &RemoteObject) -> Result<ReceiveOutcome, ReceiveError<R::Error>> {
        validate_remote_name(&object.name)?;

        let final_path = self.inbox_dir.join(&object.name);
        let mut incoming = IncomingFile::create(&self.inbox_dir)?;

        self.remote
            .download(object, incoming.file_mut())
            .map_err(|source| ReceiveError::Remote {
                operation: RemoteOperation::Download,
                source,
            })?;

        incoming.flush_and_sync()?;
        let actual_size = incoming.len()?;

        if actual_size == 0 {
            return Err(ReceiveError::EmptyObject {
                name: object.name.clone(),
            });
        }
        if actual_size != object.size {
            return Err(ReceiveError::SizeMismatch {
                name: object.name.clone(),
                expected: object.size,
                actual: actual_size,
            });
        }

        let outcome = match publish_no_clobber(incoming.path(), &final_path) {
            Ok(()) => {
                sync_parent_directory(&self.inbox_dir)?;
                ReceiveOutcome::Persisted {
                    path: final_path.clone(),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if files_equal(incoming.path(), &final_path)? {
                    ReceiveOutcome::AlreadyPersisted {
                        path: final_path.clone(),
                    }
                } else {
                    return Err(ReceiveError::ConflictingLocalObject {
                        name: object.name.clone(),
                        path: final_path,
                    });
                }
            }
            Err(error) => return Err(ReceiveError::Io(error)),
        };

        self.remote
            .delete(object)
            .map_err(|source| ReceiveError::Remote {
                operation: RemoteOperation::Delete,
                source,
            })?;

        Ok(outcome)
    }
}

fn validate_remote_name<E>(name: &str) -> Result<(), ReceiveError<E>> {
    let path = Path::new(name);
    let safe = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && path.file_name().is_some_and(|value| value == name);

    if safe {
        Ok(())
    } else {
        Err(ReceiveError::InvalidRemoteName {
            name: name.to_owned(),
        })
    }
}

fn publish_no_clobber(source: &Path, destination: &Path) -> io::Result<()> {
    // Both files live in the same inbox directory. A hard link therefore gives
    // us an atomic create-if-absent publication step without rename-overwrite
    // semantics. The temporary link is removed by IncomingFile::drop.
    fs::hard_link(source, destination)
}

fn files_equal(left: &Path, right: &Path) -> io::Result<bool> {
    if fs::metadata(left)?.len() != fs::metadata(right)?.len() {
        return Ok(false);
    }

    let mut left = File::open(left)?;
    let mut right = File::open(right)?;
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];

    loop {
        let left_read = left.read(&mut left_buffer)?;
        let right_read = right.read(&mut right_buffer)?;
        if left_read != right_read {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
        if left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
    }
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[derive(Debug)]
struct IncomingFile {
    path: PathBuf,
    file: File,
}

impl IncomingFile {
    fn create(directory: &Path) -> io::Result<Self> {
        loop {
            let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!(".incoming-{}-{id}", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok(Self { path, file }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn flush_and_sync(&mut self) -> io::Result<()> {
        self.file.flush()?;
        self.file.sync_all()
    }

    fn len(&self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }
}

impl Drop for IncomingFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FakeRemoteError(&'static str);

    impl fmt::Display for FakeRemoteError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl Error for FakeRemoteError {}

    #[derive(Debug)]
    struct FakeRemote {
        bytes: Vec<u8>,
        delete_attempts: usize,
        deleted: bool,
        fail_first_delete: bool,
    }

    impl FakeRemote {
        fn new(bytes: impl Into<Vec<u8>>) -> Self {
            Self {
                bytes: bytes.into(),
                delete_attempts: 0,
                deleted: false,
                fail_first_delete: false,
            }
        }
    }

    impl RemoteInbox for FakeRemote {
        type Error = FakeRemoteError;

        fn download(
            &mut self,
            _object: &RemoteObject,
            writer: &mut dyn Write,
        ) -> Result<(), Self::Error> {
            writer
                .write_all(&self.bytes)
                .map_err(|_| FakeRemoteError("write failed"))
        }

        fn delete(&mut self, _object: &RemoteObject) -> Result<(), Self::Error> {
            self.delete_attempts += 1;
            if self.fail_first_delete && self.delete_attempts == 1 {
                return Err(FakeRemoteError("synthetic delete failure"));
            }
            self.deleted = true;
            Ok(())
        }
    }

    #[derive(Debug)]
    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn create() -> Self {
            let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "tw-einvoice-receiver-test-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn object(name: &str, bytes: &[u8]) -> RemoteObject {
        RemoteObject {
            name: name.to_owned(),
            size: bytes.len() as u64,
        }
    }

    #[test]
    fn persists_complete_object_before_remote_delete() {
        let directory = TestDirectory::create();
        let bytes = b"<ProcessResult/>";
        let remote = FakeRemote::new(bytes.to_vec());
        let mut receiver = DurableReceiver::new(remote, &directory.0).unwrap();
        let remote_object = object("ProcessResult.xml", bytes);

        let outcome = receiver.receive(&remote_object).unwrap();

        assert_eq!(
            outcome,
            ReceiveOutcome::Persisted {
                path: directory.0.join("ProcessResult.xml")
            }
        );
        assert_eq!(fs::read(directory.0.join("ProcessResult.xml")).unwrap(), bytes);
        assert!(receiver.remote().deleted);
    }

    #[test]
    fn incomplete_download_is_not_published_or_deleted() {
        let directory = TestDirectory::create();
        let remote = FakeRemote::new(b"short".to_vec());
        let mut receiver = DurableReceiver::new(remote, &directory.0).unwrap();
        let remote_object = RemoteObject {
            name: "SummaryResult.xml".into(),
            size: 100,
        };

        let error = receiver.receive(&remote_object).unwrap_err();

        assert!(matches!(error, ReceiveError::SizeMismatch { .. }));
        assert!(!directory.0.join("SummaryResult.xml").exists());
        assert!(!receiver.remote().deleted);
    }

    #[test]
    fn retry_after_delete_failure_recognizes_identical_durable_copy() {
        let directory = TestDirectory::create();
        let bytes = b"<SummaryResult/>";
        let mut remote = FakeRemote::new(bytes.to_vec());
        remote.fail_first_delete = true;
        let mut receiver = DurableReceiver::new(remote, &directory.0).unwrap();
        let remote_object = object("SummaryResult.xml", bytes);

        let first = receiver.receive(&remote_object).unwrap_err();
        assert!(matches!(
            first,
            ReceiveError::Remote {
                operation: RemoteOperation::Delete,
                ..
            }
        ));
        assert_eq!(fs::read(directory.0.join("SummaryResult.xml")).unwrap(), bytes);
        assert!(!receiver.remote().deleted);

        let second = receiver.receive(&remote_object).unwrap();
        assert_eq!(
            second,
            ReceiveOutcome::AlreadyPersisted {
                path: directory.0.join("SummaryResult.xml")
            }
        );
        assert!(receiver.remote().deleted);
        assert_eq!(receiver.remote().delete_attempts, 2);
    }

    #[test]
    fn same_name_same_size_different_content_is_a_conflict() {
        let directory = TestDirectory::create();
        fs::write(directory.0.join("ProcessResult.xml"), b"AAAA").unwrap();
        let remote = FakeRemote::new(b"BBBB".to_vec());
        let mut receiver = DurableReceiver::new(remote, &directory.0).unwrap();
        let remote_object = object("ProcessResult.xml", b"BBBB");

        let error = receiver.receive(&remote_object).unwrap_err();

        assert!(matches!(
            error,
            ReceiveError::ConflictingLocalObject { .. }
        ));
        assert_eq!(fs::read(directory.0.join("ProcessResult.xml")).unwrap(), b"AAAA");
        assert!(!receiver.remote().deleted);
    }

    #[test]
    fn rejects_path_traversal_before_remote_access() {
        let directory = TestDirectory::create();
        let remote = FakeRemote::new(b"x".to_vec());
        let mut receiver = DurableReceiver::new(remote, &directory.0).unwrap();
        let remote_object = object("../escape.xml", b"x");

        let error = receiver.receive(&remote_object).unwrap_err();

        assert!(matches!(error, ReceiveError::InvalidRemoteName { .. }));
        assert_eq!(receiver.remote().delete_attempts, 0);
    }
}
