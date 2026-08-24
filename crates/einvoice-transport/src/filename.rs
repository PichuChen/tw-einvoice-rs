use std::{error::Error, fmt};

/// The common remote object name used by Turnkey 3.2.x.
///
/// Observed grammar:
/// `MIG-MESSAGE-YYYYMMDD-HHmmssSSS-UUID`, for example
/// `4.1-F0401-20260824-141623456-550e8400-e29b-41d4-a716-446655440000`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageCommonName {
    pub mig_version: String,
    pub message_type: String,
    pub date: String,
    pub time: String,
    pub uuid: String,
}

impl MessageCommonName {
    const SEGMENT_LENGTHS: [usize; 9] = [3, 5, 8, 9, 8, 4, 4, 4, 12];

    /// Parses the lexical grammar accepted by the observed Turnkey implementation.
    ///
    /// This deliberately checks the same structural boundary first: nine
    /// hyphen-separated components with fixed lengths. Calendar validity and UUID
    /// version semantics are higher-level validation concerns.
    ///
    /// # Errors
    ///
    /// Returns [`FilenameError`] when the common name has the wrong number of
    /// components or any component has the wrong length.
    pub fn parse(value: &str) -> Result<Self, FilenameError> {
        let parts: Vec<&str> = value.split('-').collect();
        if parts.len() != Self::SEGMENT_LENGTHS.len() {
            return Err(FilenameError::InvalidCommonNameParts {
                actual: parts.len(),
            });
        }

        for (index, (&part, &expected)) in parts.iter().zip(&Self::SEGMENT_LENGTHS).enumerate() {
            if part.len() != expected {
                return Err(FilenameError::InvalidCommonSegmentLength {
                    index,
                    expected,
                    actual: part.len(),
                });
            }
        }

        Ok(Self {
            mig_version: parts[0].to_owned(),
            message_type: parts[1].to_owned(),
            date: parts[2].to_owned(),
            time: parts[3].to_owned(),
            uuid: parts[4..].join("-"),
        })
    }

    /// Constructs a common name from its semantic parts while enforcing the
    /// observed wire grammar.
    ///
    /// # Errors
    ///
    /// Returns [`FilenameError`] when the rendered value does not satisfy the
    /// Turnkey-compatible component lengths.
    pub fn from_parts(
        mig_version: impl Into<String>,
        message_type: impl Into<String>,
        date: impl Into<String>,
        time: impl Into<String>,
        uuid: impl Into<String>,
    ) -> Result<Self, FilenameError> {
        let rendered = format!(
            "{}-{}-{}-{}-{}",
            mig_version.into(),
            message_type.into(),
            date.into(),
            time.into(),
            uuid.into()
        );
        Self::parse(&rendered)
    }

    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}-{}-{}-{}-{}",
            self.mig_version, self.message_type, self.date, self.time, self.uuid
        )
    }
}

/// Local filename produced before the Pack/SendFile boundary.
///
/// The parser is intentionally right-anchored because the original source
/// filename may itself contain underscores. The final four underscore-delimited
/// fields are `from`, `to`, common name, and pack count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnkeyPackFilename {
    pub source_name: String,
    pub from_party: String,
    pub to_party: String,
    pub common_name: MessageCommonName,
    pub pack_count: i32,
}

impl TurnkeyPackFilename {
    /// Parses the local Turnkey-compatible pack filename.
    ///
    /// # Errors
    ///
    /// Returns [`FilenameError`] when fewer than five underscore-delimited
    /// sections exist, the common name is invalid, or the count is not an
    /// `i32`, matching Java `Integer.parseInt` semantics.
    pub fn parse(value: &str) -> Result<Self, FilenameError> {
        let mut parts = value.rsplitn(5, '_');

        let count = parts.next().ok_or(FilenameError::InvalidPackFilename)?;
        let common = parts.next().ok_or(FilenameError::InvalidPackFilename)?;
        let to_party = parts.next().ok_or(FilenameError::InvalidPackFilename)?;
        let from_party = parts.next().ok_or(FilenameError::InvalidPackFilename)?;
        let source_name = parts.next().ok_or(FilenameError::InvalidPackFilename)?;

        if source_name.is_empty() {
            return Err(FilenameError::InvalidPackFilename);
        }

        let pack_count = count
            .parse::<i32>()
            .map_err(|_| FilenameError::InvalidPackCount)?;

        Ok(Self {
            source_name: source_name.to_owned(),
            from_party: from_party.to_owned(),
            to_party: to_party.to_owned(),
            common_name: MessageCommonName::parse(common)?,
            pack_count,
        })
    }

    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}_{}_{}_{}_{}",
            self.source_name,
            self.from_party,
            self.to_party,
            self.common_name.render(),
            self.pack_count
        )
    }

    /// Filename used for the SFTP `/in` object and PFS001 `filename` field.
    #[must_use]
    pub fn remote_filename(&self) -> String {
        self.common_name.render()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilenameError {
    InvalidPackFilename,
    InvalidPackCount,
    InvalidCommonNameParts {
        actual: usize,
    },
    InvalidCommonSegmentLength {
        index: usize,
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for FilenameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPackFilename => f.write_str(
                "Turnkey pack filename must contain the final from/to/common/count fields",
            ),
            Self::InvalidPackCount => {
                f.write_str("Turnkey pack count must be a signed 32-bit integer")
            }
            Self::InvalidCommonNameParts { actual } => write!(
                f,
                "Turnkey common name must contain 9 hyphen-delimited components; got {actual}"
            ),
            Self::InvalidCommonSegmentLength {
                index,
                expected,
                actual,
            } => write!(
                f,
                "Turnkey common-name component {index} must be {expected} bytes; got {actual}"
            ),
        }
    }
}

impl Error for FilenameError {}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMON: &str = "4.1-F0401-20260824-141623456-550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn parses_common_name_with_millisecond_time() {
        let value = MessageCommonName::parse(COMMON).unwrap();
        assert_eq!(value.mig_version, "4.1");
        assert_eq!(value.message_type, "F0401");
        assert_eq!(value.date, "20260824");
        assert_eq!(value.time, "141623456");
        assert_eq!(value.uuid, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(value.render(), COMMON);
    }

    #[test]
    fn preserves_underscores_in_source_filename() {
        let filename = format!("erp_batch_0001.xml_12345678_0000000000_{COMMON}_27");
        let parsed = TurnkeyPackFilename::parse(&filename).unwrap();

        assert_eq!(parsed.source_name, "erp_batch_0001.xml");
        assert_eq!(parsed.from_party, "12345678");
        assert_eq!(parsed.to_party, "0000000000");
        assert_eq!(parsed.pack_count, 27);
        assert_eq!(parsed.remote_filename(), COMMON);
        assert_eq!(parsed.render(), filename);
    }

    #[test]
    fn rejects_six_digit_time_without_milliseconds() {
        let value = "4.1-F0401-20260824-141623-550e8400-e29b-41d4-a716-446655440000";
        assert!(matches!(
            MessageCommonName::parse(value),
            Err(FilenameError::InvalidCommonSegmentLength {
                index: 3,
                expected: 9,
                actual: 6
            })
        ));
    }
}
