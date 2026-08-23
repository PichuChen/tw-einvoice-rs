//! MIG 4.1 F0401 — platform-certification invoice message.
//!
//! This module models the public F0401 document shape independently from the
//! Turnkey transport. It intentionally keeps decimal values as lexical strings
//! so invoice and tax amounts never pass through binary floating point.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::too_many_lines
)]

use tw_einvoice_core::Ban;

use crate::{ValidationIssue, ValidationReport};

pub const MESSAGE_CODE: &str = "F0401";
pub const NAMESPACE: &str = "urn:GEINV:eInvoiceMessage:F0401:4.1";
pub const MAX_PRODUCT_ITEMS: usize = 9_999;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InvoiceNumber(String);

impl InvoiceNumber {
    /// Parse the MIG invoice-number wire format: two uppercase letters followed
    /// by eight decimal digits.
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        let bytes = value.as_bytes();
        let valid = bytes.len() == 10
            && bytes[..2].iter().all(u8::is_ascii_uppercase)
            && bytes[2..].iter().all(u8::is_ascii_digit);
        valid
            .then_some(Self(value))
            .ok_or("invoice number must match [A-Z]{2}[0-9]{8}")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// XML Schema decimal value whose input spelling is preserved for MIG output.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecimalValue(String);

impl DecimalValue {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        is_decimal_lexical(&value)
            .then_some(Self(value))
            .ok_or("value is not an XML Schema decimal lexical form")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_negative(&self) -> bool {
        self.0.starts_with('-') && !is_decimal_zero(&self.0)
    }
}

macro_rules! wire_enum {
    ($(#[$meta:meta])* pub enum $name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire),+ }
            }
        }
    };
}

wire_enum!(
    /// F0401 InvoiceType wire codes.
    pub enum InvoiceType {
        Code07 => "07",
        Code08 => "08",
    }
);

wire_enum!(
    pub enum BuyerRemark {
        Code1 => "1",
        Code2 => "2",
        Code3 => "3",
        Code4 => "4",
    }
);

wire_enum!(
    pub enum CustomsClearanceMark {
        Code1 => "1",
        Code2 => "2",
    }
);

wire_enum!(
    pub enum DonateMark {
        No => "0",
        Yes => "1",
    }
);

wire_enum!(
    pub enum PrintMark {
        No => "N",
        Yes => "Y",
    }
);

wire_enum!(
    pub enum TaxType {
        Taxable => "1",
        ZeroRated => "2",
        TaxExempt => "3",
        SpecialTaxRate => "4",
        Mixed => "9",
    }
);

wire_enum!(
    pub enum TaxRate {
        Zero => "0",
        OnePercent => "0.01",
        TwoPercent => "0.02",
        FivePercent => "0.05",
        FifteenPercent => "0.15",
        TwentyFivePercent => "0.25",
    }
);

wire_enum!(
    pub enum BondedAreaConfirm {
        Code1 => "1",
        Code2 => "2",
        Code3 => "3",
        Code4 => "4",
    }
);

wire_enum!(
    pub enum ZeroTaxRateReason {
        Code71 => "71",
        Code72 => "72",
        Code73 => "73",
        Code74 => "74",
        Code75 => "75",
        Code76 => "76",
        Code77 => "77",
        Code78 => "78",
        Code79 => "79",
    }
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Party {
    pub identifier: Ban,
    pub name: String,
    pub address: Option<String>,
    pub person_in_charge: Option<String>,
    pub telephone_number: Option<String>,
    pub facsimile_number: Option<String>,
    pub email_address: Option<String>,
    pub customer_number: Option<String>,
    pub role_remark: Option<String>,
}

impl Party {
    pub fn new(identifier: Ban, name: impl Into<String>) -> Self {
        Self {
            identifier,
            name: name.into(),
            address: None,
            person_in_charge: None,
            telephone_number: None,
            facsimile_number: None,
            email_address: None,
            customer_number: None,
            role_remark: None,
        }
    }

    fn validate(&self, prefix: &str, report: &mut ValidationReport) {
        text_range(report, &format!("{prefix}.Name"), &self.name, 1, 60);
        optional_max(
            report,
            &format!("{prefix}.Address"),
            self.address.as_deref(),
            100,
        );
        optional_max(
            report,
            &format!("{prefix}.PersonInCharge"),
            self.person_in_charge.as_deref(),
            30,
        );
        optional_max(
            report,
            &format!("{prefix}.TelephoneNumber"),
            self.telephone_number.as_deref(),
            26,
        );
        optional_max(
            report,
            &format!("{prefix}.FacsimileNumber"),
            self.facsimile_number.as_deref(),
            26,
        );
        optional_max(
            report,
            &format!("{prefix}.EmailAddress"),
            self.email_address.as_deref(),
            400,
        );
        optional_max(
            report,
            &format!("{prefix}.CustomerNumber"),
            self.customer_number.as_deref(),
            20,
        );
        optional_max(
            report,
            &format!("{prefix}.RoleRemark"),
            self.role_remark.as_deref(),
            40,
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Main {
    pub invoice_number: InvoiceNumber,
    pub invoice_date: String,
    pub invoice_time: String,
    pub seller: Party,
    pub buyer: Party,
    pub buyer_remark: Option<BuyerRemark>,
    pub main_remark: Option<String>,
    pub customs_clearance_mark: Option<CustomsClearanceMark>,
    pub category: Option<String>,
    pub relate_number: Option<String>,
    pub invoice_type: InvoiceType,
    pub group_mark: bool,
    pub donate_mark: DonateMark,
    pub carrier_type: Option<String>,
    pub carrier_id1: Option<String>,
    pub carrier_id2: Option<String>,
    pub print_mark: PrintMark,
    pub npo_ban: Option<String>,
    pub random_number: Option<String>,
    pub bonded_area_confirm: Option<BondedAreaConfirm>,
    pub zero_tax_rate_reason: Option<ZeroTaxRateReason>,
    pub reserved1: Option<String>,
    pub reserved2: Option<String>,
}

impl Main {
    fn validate(&self, report: &mut ValidationReport) {
        if !is_mig_date(&self.invoice_date) {
            issue(
                report,
                "Main.InvoiceDate",
                "f0401.date",
                "InvoiceDate must match the MIG YYYYMMDD lexical constraint",
            );
        }
        if !is_xsd_time(&self.invoice_time) {
            issue(
                report,
                "Main.InvoiceTime",
                "f0401.time",
                "InvoiceTime must use a valid XML Schema time lexical form",
            );
        }
        self.seller.validate("Main.Seller", report);
        self.buyer.validate("Main.Buyer", report);
        optional_max(report, "Main.MainRemark", self.main_remark.as_deref(), 200);
        optional_max(report, "Main.Category", self.category.as_deref(), 2);
        optional_max(
            report,
            "Main.RelateNumber",
            self.relate_number.as_deref(),
            20,
        );
        optional_max(report, "Main.CarrierType", self.carrier_type.as_deref(), 6);
        optional_max(report, "Main.CarrierId1", self.carrier_id1.as_deref(), 400);
        optional_max(report, "Main.CarrierId2", self.carrier_id2.as_deref(), 400);
        optional_max(report, "Main.NPOBAN", self.npo_ban.as_deref(), 10);
        optional_max(report, "Main.Reserved1", self.reserved1.as_deref(), 20);
        optional_max(report, "Main.Reserved2", self.reserved2.as_deref(), 100);

        if let Some(random) = &self.random_number {
            let valid = random.is_empty()
                || (random.len() == 4 && random.bytes().all(|byte| byte.is_ascii_digit()));
            if !valid {
                issue(
                    report,
                    "Main.RandomNumber",
                    "f0401.random-number",
                    "RandomNumber must be empty or exactly four ASCII digits",
                );
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductItem {
    pub description: String,
    pub quantity: DecimalValue,
    pub unit: Option<String>,
    pub unit_price: DecimalValue,
    pub tax_type: TaxType,
    pub amount: DecimalValue,
    pub sequence_number: String,
    pub remark: Option<String>,
    pub relate_number: Option<String>,
}

impl ProductItem {
    fn validate(&self, index: usize, report: &mut ValidationReport) {
        let prefix = format!("Details.ProductItem[{index}]");
        text_max(
            report,
            &format!("{prefix}.Description"),
            &self.description,
            500,
        );
        decimal_constraint(
            report,
            &format!("{prefix}.Quantity"),
            &self.quantity,
            20,
            7,
            false,
        );
        optional_max(report, &format!("{prefix}.Unit"), self.unit.as_deref(), 6);
        decimal_constraint(
            report,
            &format!("{prefix}.UnitPrice"),
            &self.unit_price,
            20,
            7,
            false,
        );
        decimal_constraint(
            report,
            &format!("{prefix}.Amount"),
            &self.amount,
            20,
            7,
            false,
        );
        text_max(
            report,
            &format!("{prefix}.SequenceNumber"),
            &self.sequence_number,
            4,
        );
        optional_max(
            report,
            &format!("{prefix}.Remark"),
            self.remark.as_deref(),
            120,
        );
        optional_max(
            report,
            &format!("{prefix}.RelateNumber"),
            self.relate_number.as_deref(),
            50,
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Amount {
    pub sales_amount: DecimalValue,
    pub free_tax_sales_amount: DecimalValue,
    pub zero_tax_sales_amount: DecimalValue,
    pub tax_type: TaxType,
    pub tax_rate: TaxRate,
    pub tax_amount: DecimalValue,
    pub total_amount: DecimalValue,
    pub discount_amount: Option<DecimalValue>,
    pub original_currency_amount: Option<DecimalValue>,
    pub exchange_rate: Option<DecimalValue>,
    pub currency: Option<String>,
}

impl Amount {
    fn validate(&self, report: &mut ValidationReport) {
        for (path, value) in [
            ("Amount.SalesAmount", &self.sales_amount),
            ("Amount.FreeTaxSalesAmount", &self.free_tax_sales_amount),
            ("Amount.ZeroTaxSalesAmount", &self.zero_tax_sales_amount),
            ("Amount.TotalAmount", &self.total_amount),
        ] {
            decimal_constraint(report, path, value, 20, 7, true);
        }
        decimal_constraint(report, "Amount.TaxAmount", &self.tax_amount, 20, 0, true);
        if let Some(value) = &self.discount_amount {
            decimal_constraint(report, "Amount.DiscountAmount", value, 20, 7, true);
        }
        if let Some(value) = &self.original_currency_amount {
            decimal_constraint(report, "Amount.OriginalCurrencyAmount", value, 20, 7, true);
        }
        if let Some(value) = &self.exchange_rate {
            decimal_constraint(report, "Amount.ExchangeRate", value, 13, 5, true);
        }
        if let Some(currency) = &self.currency {
            let valid =
                currency.len() == 3 && currency.bytes().all(|byte| byte.is_ascii_uppercase());
            if !valid {
                issue(
                    report,
                    "Amount.Currency",
                    "f0401.currency-shape",
                    "Currency must use the three-letter uppercase MIG code shape",
                );
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invoice {
    pub main: Main,
    pub details: Vec<ProductItem>,
    pub amount: Amount,
}

impl Invoice {
    /// Perform fast message-specific preflight checks.
    ///
    /// Full XSD validation remains a separate layer; this method intentionally
    /// checks the public F0401 constraints most useful before serialization.
    pub fn validate(&self) -> ValidationReport {
        let mut report = ValidationReport::default();
        self.main.validate(&mut report);
        if self.details.is_empty() || self.details.len() > MAX_PRODUCT_ITEMS {
            issue(
                &mut report,
                "Details.ProductItem",
                "f0401.product-count",
                "F0401 requires between 1 and 9999 ProductItem elements",
            );
        }
        for (index, item) in self.details.iter().enumerate() {
            item.validate(index, &mut report);
        }
        self.amount.validate(&mut report);
        report
    }

    /// Serialize deterministically as UTF-8 MIG 4.1 XML.
    ///
    /// The exact returned byte sequence is suitable as the future CMS signing
    /// boundary: callers must not parse and reserialize it after signing.
    pub fn to_xml_string(&self) -> String {
        let mut out = String::with_capacity(2048 + self.details.len() * 256);
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        out.push_str("<Invoice xmlns=\"");
        out.push_str(NAMESPACE);
        out.push_str("\"><Main>");

        element(&mut out, "InvoiceNumber", self.main.invoice_number.as_str());
        element(&mut out, "InvoiceDate", &self.main.invoice_date);
        element(&mut out, "InvoiceTime", &self.main.invoice_time);
        party_xml(&mut out, "Seller", &self.main.seller);
        party_xml(&mut out, "Buyer", &self.main.buyer);
        optional_element(
            &mut out,
            "BuyerRemark",
            self.main.buyer_remark.map(BuyerRemark::as_str),
        );
        optional_element(&mut out, "MainRemark", self.main.main_remark.as_deref());
        optional_element(
            &mut out,
            "CustomsClearanceMark",
            self.main
                .customs_clearance_mark
                .map(CustomsClearanceMark::as_str),
        );
        optional_element(&mut out, "Category", self.main.category.as_deref());
        optional_element(&mut out, "RelateNumber", self.main.relate_number.as_deref());
        element(&mut out, "InvoiceType", self.main.invoice_type.as_str());
        if self.main.group_mark {
            element(&mut out, "GroupMark", "*");
        }
        element(&mut out, "DonateMark", self.main.donate_mark.as_str());
        optional_element(&mut out, "CarrierType", self.main.carrier_type.as_deref());
        optional_element(&mut out, "CarrierId1", self.main.carrier_id1.as_deref());
        optional_element(&mut out, "CarrierId2", self.main.carrier_id2.as_deref());
        element(&mut out, "PrintMark", self.main.print_mark.as_str());
        optional_element(&mut out, "NPOBAN", self.main.npo_ban.as_deref());
        optional_element(&mut out, "RandomNumber", self.main.random_number.as_deref());
        optional_element(
            &mut out,
            "BondedAreaConfirm",
            self.main.bonded_area_confirm.map(BondedAreaConfirm::as_str),
        );
        optional_element(
            &mut out,
            "ZeroTaxRateReason",
            self.main
                .zero_tax_rate_reason
                .map(ZeroTaxRateReason::as_str),
        );
        optional_element(&mut out, "Reserved1", self.main.reserved1.as_deref());
        optional_element(&mut out, "Reserved2", self.main.reserved2.as_deref());
        out.push_str("</Main><Details>");

        for item in &self.details {
            out.push_str("<ProductItem>");
            element(&mut out, "Description", &item.description);
            element(&mut out, "Quantity", item.quantity.as_str());
            optional_element(&mut out, "Unit", item.unit.as_deref());
            element(&mut out, "UnitPrice", item.unit_price.as_str());
            element(&mut out, "TaxType", item.tax_type.as_str());
            element(&mut out, "Amount", item.amount.as_str());
            element(&mut out, "SequenceNumber", &item.sequence_number);
            optional_element(&mut out, "Remark", item.remark.as_deref());
            optional_element(&mut out, "RelateNumber", item.relate_number.as_deref());
            out.push_str("</ProductItem>");
        }

        out.push_str("</Details><Amount>");
        element(&mut out, "SalesAmount", self.amount.sales_amount.as_str());
        element(
            &mut out,
            "FreeTaxSalesAmount",
            self.amount.free_tax_sales_amount.as_str(),
        );
        element(
            &mut out,
            "ZeroTaxSalesAmount",
            self.amount.zero_tax_sales_amount.as_str(),
        );
        element(&mut out, "TaxType", self.amount.tax_type.as_str());
        element(&mut out, "TaxRate", self.amount.tax_rate.as_str());
        element(&mut out, "TaxAmount", self.amount.tax_amount.as_str());
        element(&mut out, "TotalAmount", self.amount.total_amount.as_str());
        optional_decimal(
            &mut out,
            "DiscountAmount",
            self.amount.discount_amount.as_ref(),
        );
        optional_decimal(
            &mut out,
            "OriginalCurrencyAmount",
            self.amount.original_currency_amount.as_ref(),
        );
        optional_decimal(&mut out, "ExchangeRate", self.amount.exchange_rate.as_ref());
        optional_element(&mut out, "Currency", self.amount.currency.as_deref());
        out.push_str("</Amount></Invoice>");
        out
    }

    pub fn to_xml_bytes(&self) -> Vec<u8> {
        self.to_xml_string().into_bytes()
    }
}

fn party_xml(out: &mut String, tag: &str, party: &Party) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    element(out, "Identifier", party.identifier.as_str());
    element(out, "Name", &party.name);
    optional_element(out, "Address", party.address.as_deref());
    optional_element(out, "PersonInCharge", party.person_in_charge.as_deref());
    optional_element(out, "TelephoneNumber", party.telephone_number.as_deref());
    optional_element(out, "FacsimileNumber", party.facsimile_number.as_deref());
    optional_element(out, "EmailAddress", party.email_address.as_deref());
    optional_element(out, "CustomerNumber", party.customer_number.as_deref());
    optional_element(out, "RoleRemark", party.role_remark.as_deref());
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn element(out: &mut String, tag: &str, value: &str) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    escape_xml(out, value);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn optional_element(out: &mut String, tag: &str, value: Option<&str>) {
    if let Some(value) = value {
        element(out, tag, value);
    }
}

fn optional_decimal(out: &mut String, tag: &str, value: Option<&DecimalValue>) {
    if let Some(value) = value {
        element(out, tag, value.as_str());
    }
}

fn escape_xml(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
}

fn issue(report: &mut ValidationReport, path: &str, code: &str, message: &str) {
    report.issues.push(ValidationIssue {
        path: Some(path.to_owned()),
        code: code.to_owned(),
        message: message.to_owned(),
    });
}

fn text_range(report: &mut ValidationReport, path: &str, value: &str, min: usize, max: usize) {
    let count = value.chars().count();
    if !(min..=max).contains(&count) {
        issue(
            report,
            path,
            "f0401.length",
            &format!("value length must be between {min} and {max} characters"),
        );
    }
}

fn text_max(report: &mut ValidationReport, path: &str, value: &str, max: usize) {
    if value.chars().count() > max {
        issue(
            report,
            path,
            "f0401.max-length",
            &format!("value length must not exceed {max} characters"),
        );
    }
}

fn optional_max(report: &mut ValidationReport, path: &str, value: Option<&str>, max: usize) {
    if let Some(value) = value {
        text_max(report, path, value, max);
    }
}

fn decimal_constraint(
    report: &mut ValidationReport,
    path: &str,
    value: &DecimalValue,
    total_digits: usize,
    fraction_digits: usize,
    non_negative: bool,
) {
    // XSD facets apply to the decimal value, not the original spelling.
    let raw = strip_decimal_sign(value.as_str());
    let (integer, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    let integer = integer.trim_start_matches('0');
    let fraction = fraction.trim_end_matches('0');
    let value_total_digits = if integer.is_empty() && fraction.is_empty() {
        1
    } else {
        integer.len() + fraction.len()
    };
    let value_fraction_digits = fraction.len();

    if value_total_digits > total_digits || value_fraction_digits > fraction_digits {
        issue(
            report,
            path,
            "f0401.decimal-digits",
            &format!(
                "decimal exceeds totalDigits={total_digits} or fractionDigits={fraction_digits}"
            ),
        );
    }
    if non_negative && value.is_negative() {
        issue(
            report,
            path,
            "f0401.non-negative",
            "value must not be negative",
        );
    }
}

fn strip_decimal_sign(value: &str) -> &str {
    value
        .strip_prefix('+')
        .or_else(|| value.strip_prefix('-'))
        .unwrap_or(value)
}

fn is_decimal_lexical(value: &str) -> bool {
    let unsigned = strip_decimal_sign(value);
    if unsigned.is_empty() {
        return false;
    }
    match unsigned.split_once('.') {
        Some((whole, fraction)) => {
            (!whole.is_empty() || !fraction.is_empty())
                && whole.bytes().all(|byte| byte.is_ascii_digit())
                && fraction.bytes().all(|byte| byte.is_ascii_digit())
        }
        None => unsigned.bytes().all(|byte| byte.is_ascii_digit()),
    }
}

fn is_decimal_zero(value: &str) -> bool {
    value
        .bytes()
        .filter(u8::is_ascii_digit)
        .all(|byte| byte == b'0')
}

fn is_mig_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 8 || !bytes.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let month = (bytes[4] - b'0') * 10 + (bytes[5] - b'0');
    let day = (bytes[6] - b'0') * 10 + (bytes[7] - b'0');
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn is_xsd_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 8 || bytes[2] != b':' || bytes[5] != b':' {
        return false;
    }
    if ![0, 1, 3, 4, 6, 7]
        .into_iter()
        .all(|index| bytes.get(index).is_some_and(u8::is_ascii_digit))
    {
        return false;
    }

    let hour = (bytes[0] - b'0') * 10 + (bytes[1] - b'0');
    let minute = (bytes[3] - b'0') * 10 + (bytes[4] - b'0');
    let second = (bytes[6] - b'0') * 10 + (bytes[7] - b'0');
    if hour > 24 || minute > 59 || second > 59 {
        return false;
    }

    let mut rest = &value[8..];
    let mut fractional_nonzero = false;
    if let Some(fractional) = rest.strip_prefix('.') {
        let fraction_len = fractional.bytes().take_while(u8::is_ascii_digit).count();
        if fraction_len == 0 {
            return false;
        }
        let (digits, suffix) = fractional.split_at(fraction_len);
        fractional_nonzero = digits.bytes().any(|byte| byte != b'0');
        rest = suffix;
    }

    if hour == 24 && (minute != 0 || second != 0 || fractional_nonzero) {
        return false;
    }
    if rest.is_empty() || rest == "Z" {
        return true;
    }

    let timezone = rest.as_bytes();
    if timezone.len() != 6
        || !matches!(timezone[0], b'+' | b'-')
        || timezone[3] != b':'
        || ![1, 2, 4, 5]
            .into_iter()
            .all(|index| timezone.get(index).is_some_and(u8::is_ascii_digit))
    {
        return false;
    }
    let zone_hour = (timezone[1] - b'0') * 10 + (timezone[2] - b'0');
    let zone_minute = (timezone[4] - b'0') * 10 + (timezone[5] - b'0');
    zone_hour < 14 || (zone_hour == 14 && zone_minute == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decimal(value: &str) -> DecimalValue {
        DecimalValue::parse(value).unwrap()
    }

    fn sample() -> Invoice {
        Invoice {
            main: Main {
                invoice_number: InvoiceNumber::parse("AB12345678").unwrap(),
                invoice_date: "20260823".to_owned(),
                invoice_time: "19:30:00".to_owned(),
                seller: Party::new(Ban::parse("12345678").unwrap(), "Seller & Co."),
                buyer: Party::new(Ban::parse(Ban::B2C_BUYER).unwrap(), "0000"),
                buyer_remark: None,
                main_remark: None,
                customs_clearance_mark: None,
                category: None,
                relate_number: None,
                invoice_type: InvoiceType::Code07,
                group_mark: false,
                donate_mark: DonateMark::No,
                carrier_type: None,
                carrier_id1: None,
                carrier_id2: None,
                print_mark: PrintMark::Yes,
                npo_ban: None,
                random_number: Some("1234".to_owned()),
                bonded_area_confirm: None,
                zero_tax_rate_reason: None,
                reserved1: None,
                reserved2: None,
            },
            details: vec![ProductItem {
                description: "Tea <large>".to_owned(),
                quantity: decimal("1"),
                unit: Some("杯".to_owned()),
                unit_price: decimal("100"),
                tax_type: TaxType::Taxable,
                amount: decimal("100"),
                sequence_number: "1".to_owned(),
                remark: None,
                relate_number: None,
            }],
            amount: Amount {
                sales_amount: decimal("95"),
                free_tax_sales_amount: decimal("0"),
                zero_tax_sales_amount: decimal("0"),
                tax_type: TaxType::Taxable,
                tax_rate: TaxRate::FivePercent,
                tax_amount: decimal("5"),
                total_amount: decimal("100"),
                discount_amount: None,
                original_currency_amount: None,
                exchange_rate: None,
                currency: Some("TWD".to_owned()),
            },
        }
    }

    #[test]
    fn validates_minimal_f0401() {
        assert!(sample().validate().is_valid());
    }

    #[test]
    fn serializes_namespace_and_escapes_text() {
        let xml = sample().to_xml_string();
        assert!(xml.contains("<Invoice xmlns=\"urn:GEINV:eInvoiceMessage:F0401:4.1\">"));
        assert!(xml.contains("<Name>Seller &amp; Co.</Name>"));
        assert!(xml.contains("<Description>Tea &lt;large&gt;</Description>"));
        assert!(xml.contains("<Identifier>0000000000</Identifier>"));
    }

    #[test]
    fn rejects_fractional_tax_amount() {
        let mut invoice = sample();
        invoice.amount.tax_amount = decimal("1.5");
        let report = invoice.validate();
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.path.as_deref() == Some("Amount.TaxAmount"))
        );
    }

    #[test]
    fn decimal_preflight_uses_xsd_value_space_digit_counts() {
        let mut invoice = sample();
        invoice.amount.sales_amount = decimal("000000000000000000095.0000000");
        assert!(invoice.validate().is_valid());

        invoice.amount.exchange_rate = Some(decimal("0.000001"));
        let report = invoice.validate();
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.path.as_deref() == Some("Amount.ExchangeRate"))
        );
    }

    #[test]
    fn decimal_parser_accepts_xsd_edge_spellings() {
        assert!(DecimalValue::parse(".5").is_ok());
        assert!(DecimalValue::parse("5.").is_ok());
        assert!(DecimalValue::parse("+5.0").is_ok());
        assert!(DecimalValue::parse(".").is_err());
    }

    #[test]
    fn time_preflight_accepts_xsd_time_variants() {
        for value in [
            "19:30:00",
            "19:30:00.125",
            "19:30:00Z",
            "19:30:00+08:00",
            "24:00:00",
            "24:00:00.0",
        ] {
            assert!(is_xsd_time(value), "expected valid xsd:time: {value}");
        }
        for value in ["24:00:01", "19:30:60", "19:30:00+14:01", "19:30:00junk"] {
            assert!(!is_xsd_time(value), "expected invalid xsd:time: {value}");
        }
    }
}
