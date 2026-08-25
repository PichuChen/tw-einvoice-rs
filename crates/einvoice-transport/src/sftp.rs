use std::{
    error::Error,
    ffi::OsStr,
    fmt,
    io::{self, Write},
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    time::Duration,
};

use ssh2::{CheckResult, KnownHostFileKind, Session};

use crate::receiver::{RemoteInbox, RemoteObject};

/// Secret-bearing SFTP username/password pair.
#[derive(Clone, PartialEq, Eq)]
pub struct SftpCredentials {
    username: String,
    password: String,
}

impl SftpCredentials {
    #[must_use]
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    #[must_use]
    pub fn username(&self) -> &str {
        &self.username
    }
}

impl fmt::Debug for SftpCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SftpCredentials")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// Connection settings for the Ministry SFTP endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SftpConfig {
    pub host: String,
    pub port: u16,
    pub known_hosts_path: PathBuf,
    pub timeout: Duration,
    pub upload_directory: PathBuf,
    pub receive_directory: PathBuf,
}

impl SftpConfig {
    pub const DEFAULT_PORT: u16 = 2222;
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

    #[must_use]
    pub fn new(host: impl Into<String>, known_hosts_path: impl Into<PathBuf>) -> Self {
        Self {
            host: host.into(),
            port: Self::DEFAULT_PORT,
            known_hosts_path: known_hosts_path.into(),
            timeout: Self::DEFAULT_TIMEOUT,
            upload_directory: PathBuf::from("in"),
            receive_directory: PathBuf::from("out"),
        }
    }
}

/// Connection/authentication errors shared by `/in` and `/out` SFTP clients.
#[derive(Debug)]
pub enum SftpConnectionError {
    MissingHostKey,
    HostKeyNotFound { host: String, port: u16 },
    HostKeyMismatch { host: String, port: u16 },
    HostKeyCheckFailed { host: String, port: u16 },
    AuthenticationFailed,
    Io(io::Error),
    Ssh(ssh2::Error),
}

impl fmt::Display for SftpConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHostKey => f.write_str("SFTP server did not present a host key"),
            Self::HostKeyNotFound { host, port } => write!(
                f,
                "SFTP host key for {host}:{port} is not present in known_hosts"
            ),
            Self::HostKeyMismatch { host, port } => write!(
                f,
                "SFTP host key for {host}:{port} does not match known_hosts"
            ),
            Self::HostKeyCheckFailed { host, port } => {
                write!(f, "failed to validate SFTP host key for {host}:{port}")
            }
            Self::AuthenticationFailed => f.write_str("SFTP password authentication failed"),
            Self::Io(error) => write!(f, "SFTP TCP/I/O operation failed: {error}"),
            Self::Ssh(error) => write!(f, "SFTP/SSH operation failed: {error}"),
        }
    }
}

impl Error for SftpConnectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Ssh(error) => Some(error),
            Self::MissingHostKey
            | Self::HostKeyNotFound { .. }
            | Self::HostKeyMismatch { .. }
            | Self::HostKeyCheckFailed { .. }
            | Self::AuthenticationFailed => None,
        }
    }
}

impl From<io::Error> for SftpConnectionError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ssh2::Error> for SftpConnectionError {
    fn from(value: ssh2::Error) -> Self {
        Self::Ssh(value)
    }
}

fn connect_tcp(config: &SftpConfig) -> Result<TcpStream, SftpConnectionError> {
    let mut last_error = None;
    let addresses = (config.host.as_str(), config.port).to_socket_addrs()?;

    for address in addresses {
        match TcpStream::connect_timeout(&address, config.timeout) {
            Ok(stream) => {
                stream.set_read_timeout(Some(config.timeout))?;
                stream.set_write_timeout(Some(config.timeout))?;
                return Ok(stream);
            }
            Err(error) => last_error = Some(error),
        }
    }

    Err(SftpConnectionError::Io(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "SFTP host resolved to no addresses",
        )
    })))
}

fn connect_session(
    config: &SftpConfig,
    credentials: &SftpCredentials,
) -> Result<Session, SftpConnectionError> {
    let tcp = connect_tcp(config)?;
    let mut session = Session::new()?;
    let timeout_ms = u32::try_from(config.timeout.as_millis()).unwrap_or(u32::MAX);
    session.set_timeout(timeout_ms);
    session.set_tcp_stream(tcp);
    session.handshake()?;

    verify_host_key(config, &session)?;
    session.userauth_password(&credentials.username, &credentials.password)?;
    if !session.authenticated() {
        return Err(SftpConnectionError::AuthenticationFailed);
    }

    Ok(session)
}

fn verify_host_key(config: &SftpConfig, session: &Session) -> Result<(), SftpConnectionError> {
    let (host_key, _) = session
        .host_key()
        .ok_or(SftpConnectionError::MissingHostKey)?;
    let mut known_hosts = session.known_hosts()?;
    known_hosts.read_file(&config.known_hosts_path, KnownHostFileKind::OpenSSH)?;

    match known_hosts.check_port(&config.host, config.port, host_key) {
        CheckResult::Match => Ok(()),
        CheckResult::NotFound => Err(SftpConnectionError::HostKeyNotFound {
            host: config.host.clone(),
            port: config.port,
        }),
        CheckResult::Mismatch => Err(SftpConnectionError::HostKeyMismatch {
            host: config.host.clone(),
            port: config.port,
        }),
        CheckResult::Failure => Err(SftpConnectionError::HostKeyCheckFailed {
            host: config.host.clone(),
            port: config.port,
        }),
    }
}

fn safe_remote_basename(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains('\0')
}

/// Minimal boundary used by the submission orchestrator. Tests and alternate
/// SSH implementations can implement this trait without depending on libssh2.
pub trait ObjectUploader {
    type Error;

    /// Uploads bytes under the exact remote object filename.
    ///
    /// # Errors
    ///
    /// Returns the concrete uploader error when connection, authentication,
    /// host-key validation, remote-file creation, or write/close fails.
    fn upload(&self, remote_filename: &str, bytes: &[u8]) -> Result<(), Self::Error>;
}

/// Password-authenticated SFTP uploader with mandatory OpenSSH known-hosts
/// verification.
///
/// This implementation intentionally provides no "accept any host key" mode.
pub struct Ssh2SftpUploader {
    config: SftpConfig,
    credentials: SftpCredentials,
}

impl Ssh2SftpUploader {
    #[must_use]
    pub fn new(config: SftpConfig, credentials: SftpCredentials) -> Self {
        Self {
            config,
            credentials,
        }
    }
}

impl fmt::Debug for Ssh2SftpUploader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ssh2SftpUploader")
            .field("config", &self.config)
            .field("credentials", &self.credentials)
            .finish()
    }
}

impl ObjectUploader for Ssh2SftpUploader {
    type Error = SftpUploadError;

    fn upload(&self, remote_filename: &str, bytes: &[u8]) -> Result<(), Self::Error> {
        if !safe_remote_basename(remote_filename) {
            return Err(SftpUploadError::InvalidRemoteFilename);
        }
        let session = connect_session(&self.config, &self.credentials)?;
        let sftp = session.sftp()?;
        let remote_path = self.config.upload_directory.join(remote_filename);
        let mut remote_file = sftp.create(&remote_path)?;
        remote_file.write_all(bytes)?;
        remote_file.flush()?;
        remote_file.close()?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum SftpUploadError {
    InvalidRemoteFilename,
    Connection(SftpConnectionError),
}

impl fmt::Display for SftpUploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRemoteFilename => {
                f.write_str("SFTP remote filename must be a non-empty basename")
            }
            Self::Connection(error) => error.fmt(f),
        }
    }
}

impl Error for SftpUploadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRemoteFilename => None,
            Self::Connection(error) => Some(error),
        }
    }
}

impl From<SftpConnectionError> for SftpUploadError {
    fn from(value: SftpConnectionError) -> Self {
        Self::Connection(value)
    }
}

impl From<io::Error> for SftpUploadError {
    fn from(value: io::Error) -> Self {
        Self::Connection(SftpConnectionError::Io(value))
    }
}

impl From<ssh2::Error> for SftpUploadError {
    fn from(value: ssh2::Error) -> Self {
        Self::Connection(SftpConnectionError::Ssh(value))
    }
}

/// Verified SFTP client for the Ministry `/out` inbox.
///
/// Listing is kept separate from [`RemoteInbox`] because the durable receive
/// boundary only needs download/delete. A daemon can call [`Self::list`] and
/// feed each returned object into `DurableReceiver`.
pub struct Ssh2RemoteInbox {
    config: SftpConfig,
    credentials: SftpCredentials,
}

impl Ssh2RemoteInbox {
    #[must_use]
    pub fn new(config: SftpConfig, credentials: SftpCredentials) -> Self {
        Self {
            config,
            credentials,
        }
    }

    /// Lists regular files currently visible in the SFTP `/out` directory.
    ///
    /// # Errors
    ///
    /// Returns [`SftpInboxError`] when the connection fails, a directory entry
    /// has a non-UTF-8/unsafe name, or the server omits the object size required
    /// for Turnkey-compatible download verification.
    pub fn list(&self) -> Result<Vec<RemoteObject>, SftpInboxError> {
        let session = connect_session(&self.config, &self.credentials)?;
        let sftp = session.sftp()?;
        let entries = sftp.readdir(&self.config.receive_directory)?;
        let mut objects = Vec::with_capacity(entries.len());

        for (path, stat) in entries {
            if stat
                .perm
                .is_some_and(|mode| mode & 0o170_000 == 0o040_000)
            {
                continue;
            }

            let name = path
                .file_name()
                .and_then(OsStr::to_str)
                .ok_or(SftpInboxError::InvalidRemoteEntryName)?;
            if !safe_remote_basename(name) {
                return Err(SftpInboxError::InvalidRemoteEntryName);
            }
            let size = stat.size.ok_or_else(|| SftpInboxError::MissingRemoteSize {
                name: name.to_owned(),
            })?;
            objects.push(RemoteObject {
                name: name.to_owned(),
                size,
            });
        }

        objects.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(objects)
    }
}

impl fmt::Debug for Ssh2RemoteInbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ssh2RemoteInbox")
            .field("config", &self.config)
            .field("credentials", &self.credentials)
            .finish()
    }
}

impl RemoteInbox for Ssh2RemoteInbox {
    type Error = SftpInboxError;

    fn download(
        &mut self,
        object: &RemoteObject,
        writer: &mut dyn Write,
    ) -> Result<(), Self::Error> {
        if !safe_remote_basename(&object.name) {
            return Err(SftpInboxError::InvalidRemoteEntryName);
        }
        let session = connect_session(&self.config, &self.credentials)?;
        let sftp = session.sftp()?;
        let remote_path = self.config.receive_directory.join(&object.name);
        let mut remote_file = sftp.open(&remote_path)?;
        io::copy(&mut remote_file, writer)?;
        remote_file.close()?;
        Ok(())
    }

    fn delete(&mut self, object: &RemoteObject) -> Result<(), Self::Error> {
        if !safe_remote_basename(&object.name) {
            return Err(SftpInboxError::InvalidRemoteEntryName);
        }
        let session = connect_session(&self.config, &self.credentials)?;
        let sftp = session.sftp()?;
        let remote_path = self.config.receive_directory.join(&object.name);
        sftp.unlink(&remote_path)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum SftpInboxError {
    InvalidRemoteEntryName,
    MissingRemoteSize { name: String },
    Connection(SftpConnectionError),
}

impl fmt::Display for SftpInboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRemoteEntryName => {
                f.write_str("SFTP /out entry has a non-UTF-8 or unsafe filename")
            }
            Self::MissingRemoteSize { name } => {
                write!(f, "SFTP /out object {name:?} does not include a size")
            }
            Self::Connection(error) => error.fmt(f),
        }
    }
}

impl Error for SftpInboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRemoteEntryName | Self::MissingRemoteSize { .. } => None,
            Self::Connection(error) => Some(error),
        }
    }
}

impl From<SftpConnectionError> for SftpInboxError {
    fn from(value: SftpConnectionError) -> Self {
        Self::Connection(value)
    }
}

impl From<io::Error> for SftpInboxError {
    fn from(value: io::Error) -> Self {
        Self::Connection(SftpConnectionError::Io(value))
    }
}

impl From<ssh2::Error> for SftpInboxError {
    fn from(value: ssh2::Error) -> Self {
        Self::Connection(SftpConnectionError::Ssh(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_debug_redacts_password() {
        let credentials = SftpCredentials::new("transport-id", "super-secret");
        let rendered = format!("{credentials:?}");
        assert!(rendered.contains("transport-id"));
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("super-secret"));
    }

    #[test]
    fn default_config_matches_turnkey_directories() {
        let config = SftpConfig::new("tsftp.einvoice.nat.gov.tw", "/tmp/known_hosts");
        assert_eq!(config.port, 2222);
        assert_eq!(config.timeout, Duration::from_secs(120));
        assert_eq!(config.upload_directory, PathBuf::from("in"));
        assert_eq!(config.receive_directory, PathBuf::from("out"));
    }

    #[test]
    fn uploader_rejects_path_traversal_remote_names() {
        let error = SftpUploadError::InvalidRemoteFilename;
        assert_eq!(
            error.to_string(),
            "SFTP remote filename must be a non-empty basename"
        );
        assert!(!safe_remote_basename("../invoice"));
        assert!(!safe_remote_basename("in/invoice"));
        assert!(safe_remote_basename("4.1-F0401-common"));
    }

    #[test]
    fn inbox_debug_redacts_password() {
        let inbox = Ssh2RemoteInbox::new(
            SftpConfig::new("example.invalid", "/tmp/known_hosts"),
            SftpCredentials::new("transport-id", "super-secret"),
        );
        let rendered = format!("{inbox:?}");
        assert!(rendered.contains("transport-id"));
        assert!(!rendered.contains("super-secret"));
    }
}
