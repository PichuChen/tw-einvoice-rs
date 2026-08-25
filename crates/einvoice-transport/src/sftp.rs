use std::{
    error::Error,
    fmt,
    io::{self, Write},
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    time::Duration,
};

use ssh2::{CheckResult, KnownHostFileKind, Session};

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
        }
    }
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
/// Production e-Invoice traffic contains credentials and regulated business
/// data; a host missing from the configured known-hosts file is treated as an
/// error rather than silently trusted on first use.
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

    fn connect_tcp(&self) -> Result<TcpStream, SftpUploadError> {
        let mut last_error = None;
        let addresses = (self.config.host.as_str(), self.config.port).to_socket_addrs()?;

        for address in addresses {
            match TcpStream::connect_timeout(&address, self.config.timeout) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(self.config.timeout))?;
                    stream.set_write_timeout(Some(self.config.timeout))?;
                    return Ok(stream);
                }
                Err(error) => last_error = Some(error),
            }
        }

        Err(SftpUploadError::Io(last_error.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "SFTP host resolved to no addresses",
            )
        })))
    }

    fn connect_session(&self) -> Result<Session, SftpUploadError> {
        let tcp = self.connect_tcp()?;
        let mut session = Session::new()?;
        let timeout_ms = u32::try_from(self.config.timeout.as_millis()).unwrap_or(u32::MAX);
        session.set_timeout(timeout_ms);
        session.set_tcp_stream(tcp);
        session.handshake()?;

        self.verify_host_key(&session)?;
        session.userauth_password(&self.credentials.username, &self.credentials.password)?;
        if !session.authenticated() {
            return Err(SftpUploadError::AuthenticationFailed);
        }

        Ok(session)
    }

    fn verify_host_key(&self, session: &Session) -> Result<(), SftpUploadError> {
        let (host_key, _) = session.host_key().ok_or(SftpUploadError::MissingHostKey)?;
        let mut known_hosts = session.known_hosts()?;
        known_hosts.read_file(&self.config.known_hosts_path, KnownHostFileKind::OpenSSH)?;

        match known_hosts.check_port(&self.config.host, self.config.port, host_key) {
            CheckResult::Match => Ok(()),
            CheckResult::NotFound => Err(SftpUploadError::HostKeyNotFound {
                host: self.config.host.clone(),
                port: self.config.port,
            }),
            CheckResult::Mismatch => Err(SftpUploadError::HostKeyMismatch {
                host: self.config.host.clone(),
                port: self.config.port,
            }),
            CheckResult::Failure => Err(SftpUploadError::HostKeyCheckFailed {
                host: self.config.host.clone(),
                port: self.config.port,
            }),
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
        validate_remote_filename(remote_filename)?;
        let session = self.connect_session()?;
        let sftp = session.sftp()?;
        let remote_path = self.config.upload_directory.join(remote_filename);
        let mut remote_file = sftp.create(&remote_path)?;
        remote_file.write_all(bytes)?;
        remote_file.flush()?;
        remote_file.close()?;
        Ok(())
    }
}

fn validate_remote_filename(value: &str) -> Result<(), SftpUploadError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
    {
        Err(SftpUploadError::InvalidRemoteFilename)
    } else {
        Ok(())
    }
}

#[derive(Debug)]
pub enum SftpUploadError {
    InvalidRemoteFilename,
    MissingHostKey,
    HostKeyNotFound { host: String, port: u16 },
    HostKeyMismatch { host: String, port: u16 },
    HostKeyCheckFailed { host: String, port: u16 },
    AuthenticationFailed,
    Io(io::Error),
    Ssh(ssh2::Error),
}

impl fmt::Display for SftpUploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRemoteFilename => {
                f.write_str("SFTP remote filename must be a non-empty basename")
            }
            Self::MissingHostKey => f.write_str("SFTP server did not present a host key"),
            Self::HostKeyNotFound { host, port } => {
                write!(
                    f,
                    "SFTP host key for {host}:{port} is not present in known_hosts"
                )
            }
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

impl Error for SftpUploadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Ssh(error) => Some(error),
            Self::InvalidRemoteFilename
            | Self::MissingHostKey
            | Self::HostKeyNotFound { .. }
            | Self::HostKeyMismatch { .. }
            | Self::HostKeyCheckFailed { .. }
            | Self::AuthenticationFailed => None,
        }
    }
}

impl From<io::Error> for SftpUploadError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ssh2::Error> for SftpUploadError {
    fn from(value: ssh2::Error) -> Self {
        Self::Ssh(value)
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
    fn default_config_matches_turnkey_endpoint_shape() {
        let config = SftpConfig::new("tsftp.einvoice.nat.gov.tw", "/tmp/known_hosts");
        assert_eq!(config.port, 2222);
        assert_eq!(config.timeout, Duration::from_secs(120));
        assert_eq!(config.upload_directory, PathBuf::from("in"));
    }

    #[test]
    fn rejects_path_traversal_remote_names() {
        assert!(matches!(
            validate_remote_filename("../invoice"),
            Err(SftpUploadError::InvalidRemoteFilename)
        ));
        assert!(matches!(
            validate_remote_filename("in/invoice"),
            Err(SftpUploadError::InvalidRemoteFilename)
        ));
    }
}
