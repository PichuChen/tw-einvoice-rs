#![forbid(unsafe_code)]

use std::{error::Error, fmt};

/// MIG generation currently targeted by this project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MigVersion {
    V4_1,
}

/// A Taiwan business identifier (統一編號) as represented in e-Invoice messages.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BusinessIdentifier(String);

impl BusinessIdentifier {
    pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        if value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit()) {
            Ok(Self(value))
        } else {
            Err(IdentifierError)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentifierError;

impl fmt::Display for IdentifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("business identifier must contain exactly 8 ASCII digits")
    }
}

impl Error for IdentifierError {}

/// MIG message code such as F0401.
///
/// This type deliberately validates only the lexical shape. Whether a code is
/// defined by a particular MIG release belongs to the MIG specification layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageCode(String);

impl MessageCode {
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
    fn accepts_eight_digit_business_identifier() {
        assert!(BusinessIdentifier::parse("12345678").is_ok());
    }

    #[test]
    fn rejects_non_eight_digit_business_identifier() {
        assert!(BusinessIdentifier::parse("1234A678").is_err());
        assert!(BusinessIdentifier::parse("1234567").is_err());
    }

    #[test]
    fn validates_message_code_lexically() {
        assert_eq!(MessageCode::parse("F0401").unwrap().as_str(), "F0401");
        assert!(MessageCode::parse("f0401").is_err());
        assert!(MessageCode::parse("F401").is_err());
    }
}
