use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InvoiceNumber(String);

impl InvoiceNumber {
    /// Parses the official two-uppercase-letter plus eight-digit invoice number.
    ///
    /// # Errors
    ///
    /// Returns an error unless the value has exactly that lexical shape.
    pub fn parse(value: impl Into<String>) -> Result<Self, PrimitiveError> {
        let value = value.into();
        let bytes = value.as_bytes();
        let valid = bytes.len() == 10
            && bytes[..2].iter().all(u8::is_ascii_uppercase)
            && bytes[2..].iter().all(u8::is_ascii_digit);
        if valid {
            Ok(Self(value))
        } else {
            Err(PrimitiveError::InvalidInvoiceNumber)
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MigDate(String);

impl MigDate {
    /// Parses the canonical eight-digit outgoing date representation.
    ///
    /// The lexical check follows the documented month/day ranges. The stricter
    /// Gregorian calendar check is available through is_calendar_date.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid lexical date.
    pub fn parse(value: impl Into<String>) -> Result<Self, PrimitiveError> {
        let value = value.into();
        let bytes = value.as_bytes();
        if bytes.len() != 8 || !bytes.iter().all(u8::is_ascii_digit) {
            return Err(PrimitiveError::InvalidDate);
        }
        let month = parse_two(&bytes[4..6]);
        let day = parse_two(&bytes[6..8]);
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(PrimitiveError::InvalidDate);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_calendar_date(&self) -> bool {
        let bytes = self.0.as_bytes();
        let year = u32::from(bytes[0] - b'0') * 1000
            + u32::from(bytes[1] - b'0') * 100
            + u32::from(bytes[2] - b'0') * 10
            + u32::from(bytes[3] - b'0');
        let month = parse_two(&bytes[4..6]);
        let day = parse_two(&bytes[6..8]);
        let max_day = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if is_leap_year(year) => 29,
            2 => 28,
            _ => return false,
        };
        day <= max_day
    }
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100))
}

fn parse_two(bytes: &[u8]) -> u8 {
    (bytes[0] - b'0') * 10 + (bytes[1] - b'0')
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MigTime(String);

impl MigTime {
    /// Parses the canonical no-timezone outgoing time representation HH:MM:SS.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid clock time.
    pub fn parse(value: impl Into<String>) -> Result<Self, PrimitiveError> {
        let value = value.into();
        let bytes = value.as_bytes();
        if bytes.len() != 8
            || bytes[2] != b':'
            || bytes[5] != b':'
            || ![0, 1, 3, 4, 6, 7]
                .into_iter()
                .all(|index| bytes[index].is_ascii_digit())
        {
            return Err(PrimitiveError::InvalidTime);
        }
        let hour = parse_two(&bytes[0..2]);
        let minute = parse_two(&bytes[3..5]);
        let second = parse_two(&bytes[6..8]);
        if hour > 23 || minute > 59 || second > 59 {
            return Err(PrimitiveError::InvalidTime);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact decimal text retained without floating-point conversion.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MigDecimal(String);

impl MigDecimal {
    /// Parses an XML-style decimal with caller-supplied precision constraints.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed decimals, excessive precision, or a
    /// negative value when non_negative is true.
    pub fn parse(
        value: impl Into<String>,
        max_total_digits: usize,
        max_fraction_digits: usize,
        non_negative: bool,
    ) -> Result<Self, PrimitiveError> {
        let value = value.into();
        let unsigned = value.strip_prefix('-').unwrap_or(&value);
        if unsigned.is_empty() || value.starts_with('+') {
            return Err(PrimitiveError::InvalidDecimal);
        }
        if non_negative && value.starts_with('-') {
            return Err(PrimitiveError::NegativeDecimal);
        }

        let mut parts = unsigned.split('.');
        let integer = parts.next().unwrap_or_default();
        let fraction = parts.next();
        if parts.next().is_some()
            || integer.is_empty()
            || !integer.bytes().all(|byte| byte.is_ascii_digit())
            || fraction.is_some_and(|digits| {
                digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit())
            })
        {
            return Err(PrimitiveError::InvalidDecimal);
        }

        let fraction_len = fraction.map_or(0, str::len);
        let total_digits = integer.len() + fraction_len;
        if total_digits > max_total_digits || fraction_len > max_fraction_digits {
            return Err(PrimitiveError::DecimalPrecision {
                max_total_digits,
                max_fraction_digits,
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.0.starts_with('-')
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvoiceType {
    GeneralTax,
    SpecialTax,
}

impl InvoiceType {
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::GeneralTax => "07",
            Self::SpecialTax => "08",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DonateMark {
    NotDonated,
    Donated,
}

impl DonateMark {
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::NotDonated => "0",
            Self::Donated => "1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrintMark {
    Print,
    DoNotPrint,
}

impl PrintMark {
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Print => "Y",
            Self::DoNotPrint => "N",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaxType {
    Taxable,
    ZeroRated,
    TaxFree,
    SpecialTax,
    Mixed,
}

impl TaxType {
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Taxable => "1",
            Self::ZeroRated => "2",
            Self::TaxFree => "3",
            Self::SpecialTax => "4",
            Self::Mixed => "9",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaxRate {
    Zero,
    OnePercent,
    TwoPercent,
    FivePercent,
    FifteenPercent,
    TwentyFivePercent,
}

impl TaxRate {
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Zero => "0",
            Self::OnePercent => "0.01",
            Self::TwoPercent => "0.02",
            Self::FivePercent => "0.05",
            Self::FifteenPercent => "0.15",
            Self::TwentyFivePercent => "0.25",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuyerRemark {
    DeductibleExpense,
    DeductibleFixedAsset,
    NonDeductibleExpense,
    NonDeductibleFixedAsset,
}

impl BuyerRemark {
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::DeductibleExpense => "1",
            Self::DeductibleFixedAsset => "2",
            Self::NonDeductibleExpense => "3",
            Self::NonDeductibleFixedAsset => "4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CustomsClearanceMark {
    NotThroughCustoms,
    ThroughCustoms,
}

impl CustomsClearanceMark {
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::NotThroughCustoms => "1",
            Self::ThroughCustoms => "2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BondedArea {
    Article7Paragraph4,
    Article7Paragraph7,
    FreeTradeZone,
    Other,
}

impl BondedArea {
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Article7Paragraph4 => "1",
            Self::Article7Paragraph7 => "2",
            Self::FreeTradeZone => "3",
            Self::Other => "4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZeroTaxRateReason(u8);

impl ZeroTaxRateReason {
    /// Creates one of the official values 71 through 79.
    ///
    /// # Errors
    ///
    /// Returns an error outside the official range.
    pub fn new(value: u8) -> Result<Self, PrimitiveError> {
        if (71..=79).contains(&value) {
            Ok(Self(value))
        } else {
            Err(PrimitiveError::InvalidZeroTaxRateReason)
        }
    }

    #[must_use]
    pub fn wire_value(self) -> String {
        self.0.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CurrencyCode(String);

impl CurrencyCode {
    /// Parses the documented three-uppercase-letter currency-code shape.
    ///
    /// # Errors
    ///
    /// Returns an error unless the value has exactly three uppercase ASCII letters.
    pub fn parse(value: impl Into<String>) -> Result<Self, PrimitiveError> {
        let value = value.into();
        if value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase()) {
            Ok(Self(value))
        } else {
            Err(PrimitiveError::InvalidCurrencyCode)
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimitiveError {
    InvalidInvoiceNumber,
    InvalidDate,
    InvalidTime,
    InvalidDecimal,
    NegativeDecimal,
    DecimalPrecision {
        max_total_digits: usize,
        max_fraction_digits: usize,
    },
    InvalidZeroTaxRateReason,
    InvalidCurrencyCode,
}

impl fmt::Display for PrimitiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInvoiceNumber => {
                f.write_str("invoice number must be two uppercase letters followed by eight digits")
            }
            Self::InvalidDate => {
                f.write_str("MIG date must use YYYYMMDD with valid month/day ranges")
            }
            Self::InvalidTime => f.write_str("MIG time must use canonical HH:MM:SS"),
            Self::InvalidDecimal => f.write_str("value is not a canonical XML decimal"),
            Self::NegativeDecimal => f.write_str("decimal value must not be negative"),
            Self::DecimalPrecision {
                max_total_digits,
                max_fraction_digits,
            } => write!(
                f,
                "decimal exceeds {max_total_digits} total digits or {max_fraction_digits} fractional digits"
            ),
            Self::InvalidZeroTaxRateReason => {
                f.write_str("zero-tax-rate reason must be in 71 through 79")
            }
            Self::InvalidCurrencyCode => {
                f.write_str("currency code must contain exactly three uppercase ASCII letters")
            }
        }
    }
}

impl Error for PrimitiveError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_invoice_number() {
        assert_eq!(
            InvoiceNumber::parse("QQ12345678").unwrap().as_str(),
            "QQ12345678"
        );
        assert!(InvoiceNumber::parse("Q123456789").is_err());
        assert!(InvoiceNumber::parse("qq12345678").is_err());
    }

    #[test]
    fn distinguishes_date_lexical_and_calendar_validation() {
        assert!(MigDate::parse("20240229").unwrap().is_calendar_date());
        assert!(!MigDate::parse("20230229").unwrap().is_calendar_date());
        assert!(MigDate::parse("20241301").is_err());
    }

    #[test]
    fn validates_canonical_time() {
        assert!(MigTime::parse("23:59:59").is_ok());
        assert!(MigTime::parse("24:00:00").is_err());
        assert!(MigTime::parse("12:34").is_err());
    }

    #[test]
    fn validates_decimal_precision_and_sign() {
        assert_eq!(
            MigDecimal::parse("123.4567890", 20, 7, false)
                .unwrap()
                .as_str(),
            "123.4567890"
        );
        assert!(MigDecimal::parse("1.12345678", 20, 7, false).is_err());
        assert!(MigDecimal::parse("-1", 20, 7, true).is_err());
        assert!(MigDecimal::parse("-1", 20, 7, false).unwrap().is_negative());
    }

    #[test]
    fn wire_enums_match_mig_values() {
        assert_eq!(InvoiceType::GeneralTax.wire_value(), "07");
        assert_eq!(DonateMark::Donated.wire_value(), "1");
        assert_eq!(PrintMark::DoNotPrint.wire_value(), "N");
        assert_eq!(TaxType::Mixed.wire_value(), "9");
        assert_eq!(TaxRate::FivePercent.wire_value(), "0.05");
        assert_eq!(ZeroTaxRateReason::new(79).unwrap().wire_value(), "79");
    }
}
