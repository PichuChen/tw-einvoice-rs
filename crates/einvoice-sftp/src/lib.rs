#![forbid(unsafe_code)]

use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use russh::{
    client,
    keys::{self, PublicKeyOrCertificate},
};
use russh_sftp::client::SftpSession;

/// Default Ministry SFTP port used by the Turnkey service.
pub const DEFAULT_PORT: u16 = 2222;
/// Turnkey 3.2.x maximum transmitted object size (20 MiB).
pub const DEFAULT_MAX_OBJECT_SIZE: usize = 20 * 1024 * 1024;
/// Compatibility-oriented response/inactivity timeout.
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Password credentials for the Ministry transport account.
///
/// The password is intentionally private and redacted from `Debug` output.
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

/// Fail-closed SSH host-key policy.
///
/// The first production implementation deliberately supports only an explicit
/// OpenSSH `known_hosts` file. There is no "accept any host key" variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownHostsPolicy {
    path: PathBuf,
}

impl KnownHostsPolicy {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Connection settings for the native Ministry SFTP transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SftpConfig {
    pub host: String,
    pub port: u16,
    pub credentials: SftpCredentials,
    pub host_keys: KnownHostsPolicy,
    pub timeout_secs: u64,
    pub max_object_size: usize,
}

impl SftpConfig {
    #[must_use]
    pub fn ministry(
        host: impl Into<String>,
        credentials: SftpCredentials,
        known_hosts_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            host: host.into(),
            port: DEFAULT_PORT,
            credentials,
            host_keys: KnownHostsPolicy::new(known_hosts_path),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_object_size: DEFAULT_MAX_OBJECT_SIZE,
        }
    }
}

#[derive(Debug)]
struct KnownHostsHandler {
    host: String,
    port: u16,
    known_hosts_path: PathBuf,
}

impl client::Handler for KnownHostsHandler {
    type Error = SftpError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let public_key = server_public_key.public_key();
        keys::check_known_hosts_path(
            &self.host,
            self.port,
            &public_key,
            &self.known_hosts_path,
        )
        .map_err(SftpError::KnownHosts)
    }
}

/// Errors from the secure SSH/SFTP transport boundary.
#[derive(Debug)]
pub enum SftpError {
    InvalidRemoteName { name: String },
    AuthenticationRejected { username: String },
    ObjectTooLarge {
        size: usize,
        max: usize,
    },
    KnownHosts(keys::Error),
    Ssh(russh::Error),
    Sftp(russh_sftp::client::error::Error),
}

impl fmt::Display for SftpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRemoteName { name } => write!(f, "unsafe remote SFTP filename: {name:?}"),
            Self::AuthenticationRejected { username } => {
                write!(f, "SFTP password authentication rejected for {username:?}")
            }
            Self::ObjectTooLarge { size, max } => {
                write!(f, "SFTP object size {size} exceeds configured maximum {max}")
            }
            Self::KnownHosts(error) => write!(f, "SSH host-key verification failed: {error}"),
            Self::Ssh(error) => write!(f, "SSH transport failed: {error}"),
            Self::Sftp(error) => write!(f, "SFTP protocol failed: {error}"),
        }
    }
}

impl Error for SftpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::KnownHosts(error) => Some(error),
            Self::Ssh(error) => Some(error),
            Self::Sftp(error) => Some(error),
            Self::InvalidRemoteName { .. }
            | Self::AuthenticationRejected { .. }
            | Self::ObjectTooLarge { .. } => None,
        }
    }
}

impl From<russh::Error> for SftpError {
    fn from(value: russh::Error) -> Self {
        Self::Ssh(value)
    }
}

impl From<russh_sftp::client::error::Error> for SftpError {
    fn from(value: russh_sftp::client::error::Error) -> Self {
        Self::Sftp(value)
    }
}

/// Async, host-key-verified SFTP session.
///
/// The SSH handle is retained alongside the SFTP subsystem so both share one
/// explicitly authenticated session. No runtime is created internally; callers
/// own the Tokio/runtime boundary.
pub struct SecureSftpSession {
    ssh: client::Handle<KnownHostsHandler>,
    sftp: SftpSession,
    max_object_size: usize,
}

impl fmt::Debug for SecureSftpSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecureSftpSession")
            .field("max_object_size", &self.max_object_size)
            .finish_non_exhaustive()
    }
}

impl SecureSftpSession {
    /// Connects, verifies the server key against the configured `known_hosts`
    /// file, authenticates with the transport account password, and opens the
    /// SFTP subsystem.
    ///
    /// # Errors
    ///
    /// Returns [`SftpError`] when host-key verification, SSH negotiation,
    /// authentication, channel creation, or SFTP initialization fails.
    pub async fn connect(config: SftpConfig) -> Result<Self, SftpError> {
        let ssh_config = client::Config {
            inactivity_timeout: Some(Duration::from_secs(config.timeout_secs)),
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            nodelay: true,
            ..Default::default()
        };

        let handler = KnownHostsHandler {
            host: config.host.clone(),
            port: config.port,
            known_hosts_path: config.host_keys.path,
        };

        let mut ssh = client::connect(
            Arc::new(ssh_config),
            (config.host.as_str(), config.port),
            handler,
        )
        .await?;

        let auth = ssh
            .authenticate_password(
                config.credentials.username.clone(),
                config.credentials.password.clone(),
            )
            .await?;
        if !auth.success() {
            return Err(SftpError::AuthenticationRejected {
                username: config.credentials.username,
            });
        }

        let channel = ssh.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let sftp = SftpSession::new(channel.into_stream()).await?;
        sftp.set_timeout(config.timeout_secs);

        Ok(Self {
            ssh,
            sftp,
            max_object_size: config.max_object_size,
        })
    }

    /// Uploads one prepared object to the Ministry `/in` directory.
    ///
    /// # Errors
    ///
    /// Returns [`SftpError`] for an unsafe filename, an oversized object, or an
    /// SFTP write failure.
    pub async fn upload_in(&self, remote_name: &str, bytes: &[u8]) -> Result<(), SftpError> {
        validate_remote_name(remote_name)?;
        self.validate_size(bytes.len())?;
        self.sftp.write(format!("in/{remote_name}"), bytes).await?;
        Ok(())
    }

    /// Downloads one object from the Ministry `/out` directory.
    ///
    /// # Errors
    ///
    /// Returns [`SftpError`] for an unsafe filename, an SFTP read failure, or a
    /// result exceeding the configured Turnkey-compatible size limit.
    pub async fn read_out(&self, remote_name: &str) -> Result<Vec<u8>, SftpError> {
        validate_remote_name(remote_name)?;
        let bytes = self.sftp.read(format!("out/{remote_name}")).await?;
        self.validate_size(bytes.len())?;
        Ok(bytes)
    }

    /// Deletes one already-durably-persisted object from `/out`.
    ///
    /// # Errors
    ///
    /// Returns [`SftpError`] for an unsafe filename or SFTP remove failure.
    pub async fn delete_out(&self, remote_name: &str) -> Result<(), SftpError> {
        validate_remote_name(remote_name)?;
        self.sftp.remove_file(format!("out/{remote_name}")).await?;
        Ok(())
    }

    /// Closes the SFTP subsystem and disconnects the underlying SSH session.
    ///
    /// # Errors
    ///
    /// Returns [`SftpError`] if either protocol shutdown step fails.
    pub async fn close(&mut self) -> Result<(), SftpError> {
        self.sftp.close().await?;
        self.ssh
            .disconnect(
                russh::DisconnectReason::ByApplication,
                "normal shutdown",
                "en",
            )
            .await?;
        Ok(())
    }

    fn validate_size(&self, size: usize) -> Result<(), SftpError> {
        if size <= self.max_object_size {
            Ok(())
        } else {
            Err(SftpError::ObjectTooLarge {
                size,
                max: self.max_object_size,
            })
        }
    }
}

fn validate_remote_name(name: &str) -> Result<(), SftpError> {
    let safe = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\');

    if safe {
        Ok(())
    } else {
        Err(SftpError::InvalidRemoteName {
            name: name.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_debug_output_never_contains_password() {
        let credentials = SftpCredentials::new("transport-user", "highly-secret-password");
        let rendered = format!("{credentials:?}");

        assert!(rendered.contains("transport-user"));
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("highly-secret-password"));
    }

    #[test]
    fn ministry_config_uses_turnkey_service_defaults() {
        let config = SftpConfig::ministry(
            "tsftp.einvoice.nat.gov.tw",
            SftpCredentials::new("user", "secret"),
            "/etc/tw-einvoice/known_hosts",
        );

        assert_eq!(config.port, 2222);
        assert_eq!(config.timeout_secs, 120);
        assert_eq!(config.max_object_size, 20 * 1024 * 1024);
    }

    #[test]
    fn rejects_remote_path_injection() {
        for value in ["", ".", "..", "../out", "nested/name", r"nested\name"] {
            assert!(matches!(
                validate_remote_name(value),
                Err(SftpError::InvalidRemoteName { .. })
            ));
        }

        assert!(validate_remote_name(
            "4.1-F0401-20260824-141623456-550e8400-e29b-41d4-a716-446655440000"
        )
        .is_ok());
    }
}
