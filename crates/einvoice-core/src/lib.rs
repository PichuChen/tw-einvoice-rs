#![forbid(unsafe_code)]

use std::{error::Error, fmt};

/// MIG generation currently targeted by this project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MigVersion {
    V4_1,
}

/// MIG `BAN` data element.
///
/// MIG 4.1 defines this as 8 to 10 ASCII digits. For B2C messages the buyer
/// identifier is represented by ten zeroes. This type intentionally models the
/// wire-level MIG constraint rather than assuming that every value is an
/// 8-digit domestic business registration number.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ban(String);

impl Ban {
    pub const B2C_BUYER: &'static str = "0000000000";

    /// Parse a MIG `BAN` wire value.
    ///
    /// # Errors
    ///
    /// Returns [`BanError`] when the value is not 8 to 10 ASCII digits.
    pub fn parse(value: impl Into<String>) -> Result<Self, BanError> {
        let value = value.into();
        if (8..=10).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit()) {
            Ok(Self(value))
        } else {
            Err(BanError)
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_b2c_buyer(&self) -> bool {
        self.0 == Self::B2C_BUYER
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BanError;

impl fmt::Display for BanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MIG BAN must contain 8 to 10 ASCII digits")
    }
}

impl Error for BanError {}

/// MIG message code such as F0401.
///
/// This type deliberately validates only the lexical shape. Whether a code is
/// defined by a particular MIG release belongs to the MIG specification layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageCode(String);

impl MessageCode {
    /// Parse the lexical MIG message-code shape.
    ///
    /// # Errors
    ///
    /// Returns [`MessageCodeError`] unless the input matches
    /// `[A-Z][0-9]{4}`.
    pub fn parse(value: impl Into<String>) -> Result<Self, MessageCodeError> {
        let value = value.into();
        let bytes = value.as_bytes();
        let valid = bytes.len() == 5
            && bytes[0].is_ascii_uppercase()
            && bytes[1..].iter().all(u8::is_ascii_digit);

        if valid {
            Ok(Self(value))
        } else {
            Err(MessageCodeError)
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageCodeError;

impl fmt::Display for MessageCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MIG message code must match [A-Z][0-9]{4}")
    }
}

impl Error for MessageCodeError {}

/// Durable submission lifecycle used by the future gateway/daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubmissionState {
    Created,
    Validated,
    Queued,
    Packaged,
    Signed,
    Uploaded,
    Enqueued,
    PlatformProcessing,
    Accepted,
    Rejected,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_mig_ban_lengths() {
        assert!(Ban::parse("12345678").is_ok());
        assert!(Ban::parse("123456789").is_ok());
        assert!(Ban::parse("1234567890").is_ok());
    }

    #[test]
    fn recognizes_b2c_buyer_placeholder() {
        assert!(Ban::parse(Ban::B2C_BUYER).unwrap().is_b2c_buyer());
        assert!(!Ban::parse("12345678").unwrap().is_b2c_buyer());
    }

    #[test]
    fn rejects_invalid_mig_ban() {
        assert!(Ban::parse("1234A678").is_err());
        assert!(Ban::parse("1234567").is_err());
        assert!(Ban::parse("12345678901").is_err());
    }

    #[test]
    fn validates_message_code_lexically() {
        assert_eq!(MessageCode::parse("F0401").unwrap().as_str(), "F0401");
        assert!(MessageCode::parse("f0401").is_err());
        assert!(MessageCode::parse("F401").is_err());
    }
}
