use std::{error::Error, fmt, io::Write};

use zip::{CompressionMethod, ZipWriter, result::ZipError, write::SimpleFileOptions};

use crate::gateway::ZipMode;

/// Maximum signed source-file length accepted by Turnkey 3.2.1 before the
/// optional ZIP stage. The official implementation performs this check before
/// compression and does not re-check the compressed object length.
pub const MAX_SIGNED_SOURCE_BYTES: usize = 20 * 1024 * 1024;

/// Object ready to be uploaded to the platform SFTP `/in` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedUploadObject {
    pub remote_filename: String,
    pub uploaded_bytes: Vec<u8>,
    /// Value reported as PFS001 `size`.
    ///
    /// Turnkey compatibility requires the pre-ZIP signed-source length even
    /// when [`ZipMode::Zip`] is enabled.
    pub notification_size: u64,
    pub zip_mode: ZipMode,
}

/// Produces the object bytes and metadata used by the native submission path.
///
/// In ZIP mode the archive contains exactly one entry whose name is the full
/// signed local filename. The SFTP object name itself remains `remote_filename`
/// and does not receive a `.zip` suffix, matching Turnkey 3.2.1 behavior.
///
/// # Errors
///
/// Returns [`PrepareUploadError`] when a filename is unsafe/empty, the signed
/// source exceeds the official 20 MiB pre-ZIP limit, the length cannot be
/// represented as `u64`, or ZIP creation fails.
pub fn prepare_upload_object(
    signed_local_filename: &str,
    remote_filename: &str,
    signed_bytes: &[u8],
    zip_mode: ZipMode,
) -> Result<PreparedUploadObject, PrepareUploadError> {
    validate_basename("signed local filename", signed_local_filename)?;
    validate_basename("remote filename", remote_filename)?;

    if signed_bytes.len() > MAX_SIGNED_SOURCE_BYTES {
        return Err(PrepareUploadError::SourceTooLarge {
            actual: signed_bytes.len(),
            maximum: MAX_SIGNED_SOURCE_BYTES,
        });
    }

    let notification_size = u64::try_from(signed_bytes.len())
        .map_err(|_| PrepareUploadError::LengthOutOfRange)?;

    let uploaded_bytes = match zip_mode {
        ZipMode::Plain => signed_bytes.to_vec(),
        ZipMode::Zip => zip_single_entry(signed_local_filename, signed_bytes)?,
    };

    Ok(PreparedUploadObject {
        remote_filename: remote_filename.to_owned(),
        uploaded_bytes,
        notification_size,
        zip_mode,
    })
}

fn zip_single_entry(filename: &str, bytes: &[u8]) -> Result<Vec<u8>, PrepareUploadError> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    writer.start_file(filename, options)?;
    writer.write_all(bytes)?;
    let cursor = writer.finish()?;
    Ok(cursor.into_inner())
}

fn validate_basename(field: &'static str, value: &str) -> Result<(), PrepareUploadError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
    {
        Err(PrepareUploadError::InvalidFilename { field })
    } else {
        Ok(())
    }
}

#[derive(Debug)]
pub enum PrepareUploadError {
    InvalidFilename {
        field: &'static str,
    },
    SourceTooLarge {
        actual: usize,
        maximum: usize,
    },
    LengthOutOfRange,
    Zip(ZipError),
    Io(std::io::Error),
}

impl fmt::Display for PrepareUploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFilename { field } => {
                write!(f, "{field} must be a non-empty basename without path separators")
            }
            Self::SourceTooLarge { actual, maximum } => write!(
                f,
                "signed source is {actual} bytes; Turnkey-compatible maximum is {maximum} bytes"
            ),
            Self::LengthOutOfRange => f.write_str("signed source length does not fit in u64"),
            Self::Zip(error) => write!(f, "failed to create Turnkey-compatible ZIP: {error}"),
            Self::Io(error) => write!(f, "failed to write Turnkey-compatible ZIP: {error}"),
        }
    }
}

impl Error for PrepareUploadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Zip(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::InvalidFilename { .. }
            | Self::SourceTooLarge { .. }
            | Self::LengthOutOfRange => None,
        }
    }
}

impl From<ZipError> for PrepareUploadError {
    fn from(value: ZipError) -> Self {
        Self::Zip(value)
    }
}

impl From<std::io::Error> for PrepareUploadError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use zip::ZipArchive;

    use super::*;

    #[test]
    fn plain_mode_preserves_bytes_and_source_size() {
        let source = b"signed-cms";
        let prepared = prepare_upload_object(
            "source_from_to_common_1",
            "4.1-F0401-common",
            source,
            ZipMode::Plain,
        )
        .unwrap();

        assert_eq!(prepared.uploaded_bytes, source);
        assert_eq!(
            prepared.notification_size,
            u64::try_from(source.len()).unwrap()
        );
        assert_eq!(prepared.remote_filename, "4.1-F0401-common");
    }

    #[test]
    fn zip_mode_uses_full_local_name_as_single_entry_but_keeps_remote_name() {
        let source = b"signed-cms-payload";
        let local = "erp.xml_12345678_PLATFORM_4.1-F0401-common_1";
        let remote = "4.1-F0401-common";
        let prepared = prepare_upload_object(local, remote, source, ZipMode::Zip).unwrap();

        assert_eq!(prepared.remote_filename, remote);
        assert_eq!(
            prepared.notification_size,
            u64::try_from(source.len()).unwrap()
        );
        assert_ne!(prepared.uploaded_bytes, source);

        let cursor = std::io::Cursor::new(&prepared.uploaded_bytes);
        let mut archive = ZipArchive::new(cursor).unwrap();
        assert_eq!(archive.len(), 1);
        let mut entry = archive.by_index(0).unwrap();
        assert_eq!(entry.name(), local);
        let mut decoded = Vec::new();
        entry.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, source);
    }

    #[test]
    fn enforces_twenty_mib_limit_before_zip() {
        let oversized = vec![0; MAX_SIGNED_SOURCE_BYTES + 1];
        let error = prepare_upload_object("local", "remote", &oversized, ZipMode::Zip).unwrap_err();

        assert!(matches!(
            error,
            PrepareUploadError::SourceTooLarge {
                actual,
                maximum: MAX_SIGNED_SOURCE_BYTES,
            } if actual == MAX_SIGNED_SOURCE_BYTES + 1
        ));
    }

    #[test]
    fn rejects_path_like_names() {
        assert!(matches!(
            prepare_upload_object("../signed", "remote", b"x", ZipMode::Plain),
            Err(PrepareUploadError::InvalidFilename { .. })
        ));
        assert!(matches!(
            prepare_upload_object("signed", "in/remote", b"x", ZipMode::Plain),
            Err(PrepareUploadError::InvalidFilename { .. })
        ));
    }
}
