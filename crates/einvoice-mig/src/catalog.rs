use std::{error::Error, fmt, str::FromStr};

/// Complete MIG 4.1 message-code catalog listed by the current MOF guideline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mig41MessageCode {
    A0101,
    A0102,
    A0201,
    A0202,
    A0301,
    A0302,
    B0101,
    B0102,
    B0201,
    B0202,
    E0401,
    E0402,
    E0501,
    E0502,
    E0503,
    E0504,
    F0401,
    F0501,
    F0701,
    G0401,
    G0501,
}

impl Mig41MessageCode {
    pub const ALL: [Self; 21] = [
        Self::A0101,
        Self::A0102,
        Self::A0201,
        Self::A0202,
        Self::A0301,
        Self::A0302,
        Self::B0101,
        Self::B0102,
        Self::B0201,
        Self::B0202,
        Self::E0401,
        Self::E0402,
        Self::E0501,
        Self::E0502,
        Self::E0503,
        Self::E0504,
        Self::F0401,
        Self::F0501,
        Self::F0701,
        Self::G0401,
        Self::G0501,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A0101 => "A0101",
            Self::A0102 => "A0102",
            Self::A0201 => "A0201",
            Self::A0202 => "A0202",
            Self::A0301 => "A0301",
            Self::A0302 => "A0302",
            Self::B0101 => "B0101",
            Self::B0102 => "B0102",
            Self::B0201 => "B0201",
            Self::B0202 => "B0202",
            Self::E0401 => "E0401",
            Self::E0402 => "E0402",
            Self::E0501 => "E0501",
            Self::E0502 => "E0502",
            Self::E0503 => "E0503",
            Self::E0504 => "E0504",
            Self::F0401 => "F0401",
            Self::F0501 => "F0501",
            Self::F0701 => "F0701",
            Self::G0401 => "G0401",
            Self::G0501 => "G0501",
        }
    }
}

impl fmt::Display for Mig41MessageCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Mig41MessageCode {
    type Err = UnknownMig41MessageCode;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|code| code.as_str() == value)
            .ok_or_else(|| UnknownMig41MessageCode(value.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownMig41MessageCode(String);

impl fmt::Display for UnknownMig41MessageCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown MIG 4.1 message code {:?}", self.0)
    }
}

impl Error for UnknownMig41MessageCode {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_current_twenty_one_message_codes() {
        assert_eq!(Mig41MessageCode::ALL.len(), 21);
        assert_eq!(Mig41MessageCode::F0401.as_str(), "F0401");
        assert_eq!("E0504".parse::<Mig41MessageCode>().unwrap(), Mig41MessageCode::E0504);
        assert!("F0601".parse::<Mig41MessageCode>().is_err());
    }
}
