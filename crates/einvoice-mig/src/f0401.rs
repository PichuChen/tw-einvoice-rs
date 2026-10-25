use std::{error::Error, fmt};

use tw_einvoice_core::Ban;

use crate::{
    ValidationIssue, ValidationReport,
    primitives::{
        BondedArea, CurrencyCode, CustomsClearanceMark, DonateMark, InvoiceNumber, InvoiceType,
        MigDate, MigDecimal, MigTime, PrintMark, TaxRate, TaxType, ZeroTaxRateReason,
    },
};

pub const F0401_NAMESPACE: &str = "urn:GEINV:eInvoiceMessage:F0401:4.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleDescription {
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

impl RoleDescription {
    #[must_use]
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct F0401Main {
    pub invoice_number: InvoiceNumber,
    pub invoice_date: MigDate,
    pub invoice_time: MigTime,
    pub seller: RoleDescription,
    pub buyer: RoleDescription,
    pub buyer_remark: Option<String>,
    pub main_remark: Option<String>,
    pub customs_clearance_mark: Option<CustomsClearanceMark>,
    pub category: Option<String>,
    pub relate_number: Option<String>,
    pub invoice_type: InvoiceType,
    pub group_mark: Option<String>,
    pub donate_mark: DonateMark,
    pub carrier_type: Option<String>,
    pub carrier_id1: Option<String>,
    pub carrier_id2: Option<String>,
    pub print_mark: PrintMark,
    pub npo_ban: Option<Ban>,
    pub random_number: Option<String>,
    pub bonded_area_confirm: Option<BondedArea>,
    pub zero_tax_rate_reason: Option<ZeroTaxRateReason>,
    pub reserved1: Option<String>,
    pub reserved2: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductItem {
    pub description: String,
    pub quantity: MigDecimal,
    pub unit: Option<String>,
    pub unit_price: MigDecimal,
    pub tax_type: TaxType,
    pub amount: MigDecimal,
    pub sequence_number: String,
    pub remark: Option<String>,
    pub relate_number: Option<String>,
}

impl ProductItem {
    #[must_use]
    pub fn new(
        description: impl Into<String>,
        quantity: MigDecimal,
        unit_price: MigDecimal,
        tax_type: TaxType,
        amount: MigDecimal,
        sequence_number: impl Into<String>,
    ) -> Self {
        Self {
            description: description.into(),
            quantity,
            unit: None,
            unit_price,
            tax_type,
            amount,
            sequence_number: sequence_number.into(),
            remark: None,
            relate_number: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct F0401Amount {
    pub sales_amount: MigDecimal,
    pub free_tax_sales_amount: MigDecimal,
    pub zero_tax_sales_amount: MigDecimal,
    pub tax_type: TaxType,
    pub tax_rate: TaxRate,
    pub tax_amount: MigDecimal,
    pub total_amount: MigDecimal,
    pub discount_amount: Option<MigDecimal>,
    pub original_currency_amount: Option<MigDecimal>,
    pub exchange_rate: Option<MigDecimal>,
    pub currency: Option<CurrencyCode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct F0401Invoice {
    pub main: F0401Main,
    details: Vec<ProductItem>,
    pub amount: F0401Amount,
}

impl F0401Invoice {
    pub const MAX_PRODUCT_ITEMS: usize = 9_999;

    /// Creates a structurally valid F0401 document.
    ///
    /// # Errors
    ///
    /// Returns an error unless the document contains one through 9,999 product items.
    pub fn new(
        main: F0401Main,
        details: Vec<ProductItem>,
        amount: F0401Amount,
    ) -> Result<Self, F0401StructureError> {
        match details.len() {
            0 => Err(F0401StructureError::NoProductItems),
            count if count > Self::MAX_PRODUCT_ITEMS => {
                Err(F0401StructureError::TooManyProductItems {
                    count,
                    max: Self::MAX_PRODUCT_ITEMS,
                })
            }
            _ => Ok(Self {
                main,
                details,
                amount,
            }),
        }
    }

    #[must_use]
    pub fn details(&self) -> &[ProductItem] {
        &self.details
    }

    /// Runs semantic checks that are intentionally separate from XML lexical/schema constraints.
    #[must_use]
    pub fn validate(&self) -> ValidationReport {
        let mut report = ValidationReport::default();

        validate_date(&self.main.invoice_date, "Invoice/Main/InvoiceDate", &mut report);
        validate_role(&self.main.seller, true, "Invoice/Main/Seller", &mut report);
        validate_role(&self.main.buyer, false, "Invoice/Main/Buyer", &mut report);

        check_max(&self.main.buyer_remark, 1, "Invoice/Main/BuyerRemark", &mut report);
        check_max(&self.main.main_remark, 200, "Invoice/Main/MainRemark", &mut report);
        check_max(&self.main.category, 2, "Invoice/Main/Category", &mut report);
        check_max(&self.main.relate_number, 20, "Invoice/Main/RelateNumber", &mut report);
        check_exact(&self.main.group_mark, 1, "Invoice/Main/GroupMark", &mut report);
        check_max(&self.main.carrier_type, 6, "Invoice/Main/CarrierType", &mut report);
        check_max(&self.main.carrier_id1, 400, "Invoice/Main/CarrierId1", &mut report);
        check_max(&self.main.carrier_id2, 400, "Invoice/Main/CarrierId2", &mut report);
        check_max(&self.main.reserved1, 20, "Invoice/Main/Reserved1", &mut report);
        check_max(&self.main.reserved2, 100, "Invoice/Main/Reserved2", &mut report);

        let buyer_is_consumer = self.main.buyer.identifier.is_b2c_buyer();
        match (&self.main.random_number, buyer_is_consumer) {
            (Some(value), _) if !is_four_digits(value) => push_issue(
                &mut report,
                "Invoice/Main/RandomNumber",
                "F0401_RANDOM_NUMBER_FORMAT",
                "RandomNumber must contain exactly four ASCII digits",
            ),
            (None, true) => push_issue(
                &mut report,
                "Invoice/Main/RandomNumber",
                "F0401_B2C_RANDOM_NUMBER_REQUIRED",
                "RandomNumber is required when the buyer identifier is 0000000000",
            ),
            (Some(_), false) => push_issue(
                &mut report,
                "Invoice/Main/RandomNumber",
                "F0401_B2B_RANDOM_NUMBER_FORBIDDEN",
                "RandomNumber must be omitted for a business or organization buyer",
            ),
            _ => {}
        }

        if self.main.print_mark == PrintMark::Print {
            if self.main.carrier_type.is_some()
                || self.main.carrier_id1.is_some()
                || self.main.carrier_id2.is_some()
            {
                push_issue(
                    &mut report,
                    "Invoice/Main/PrintMark",
                    "F0401_PRINT_WITH_CARRIER",
                    "CarrierType, CarrierId1 and CarrierId2 must be omitted when PrintMark is Y",
                );
            }
            if self.main.donate_mark != DonateMark::NotDonated {
                push_issue(
                    &mut report,
                    "Invoice/Main/DonateMark",
                    "F0401_PRINT_DONATION_CONFLICT",
                    "DonateMark must be 0 when PrintMark is Y",
                );
            }
        }

        let zero_rated = self.amount.tax_type == TaxType::ZeroRated
            || self
                .details
                .iter()
                .any(|item| item.tax_type == TaxType::ZeroRated);
        if zero_rated && self.main.zero_tax_rate_reason.is_none() {
            push_issue(
                &mut report,
                "Invoice/Main/ZeroTaxRateReason",
                "F0401_ZERO_RATE_REASON_REQUIRED",
                "ZeroTaxRateReason is required when zero-rated tax is used",
            );
        }

        let mut expected_sequence = 1usize;
        for (index, item) in self.details.iter().enumerate() {
            let base = format!("Invoice/Details/ProductItem[{}]", index + 1);
            check_required_max(&item.description, 500, &format!("{base}/Description"), &mut report);
            check_max(&item.unit, 6, &format!("{base}/Unit"), &mut report);
            check_required_max(
                &item.sequence_number,
                4,
                &format!("{base}/SequenceNumber"),
                &mut report,
            );
            check_max(&item.remark, 120, &format!("{base}/Remark"), &mut report);
            check_max(&item.relate_number, 50, &format!("{base}/RelateNumber"), &mut report);

            if item.tax_type == TaxType::Mixed {
                push_issue(
                    &mut report,
                    &format!("{base}/TaxType"),
                    "F0401_PRODUCT_MIXED_TAX_FORBIDDEN",
                    "ProductItem TaxType must not use mixed-tax value 9",
                );
            }

            if item.sequence_number != expected_sequence.to_string() {
                push_issue(
                    &mut report,
                    &format!("{base}/SequenceNumber"),
                    "F0401_SEQUENCE_NOT_CONTIGUOUS",
                    "SequenceNumber should follow the invoice-detail order starting at 1",
                );
            }
            expected_sequence += 1;
        }

        report
    }

    /// Serializes the document in deterministic MIG 4.1 element order.
    #[must_use]
    pub fn to_xml(&self) -> String {
        let mut output = String::with_capacity(2048 + self.details.len() * 256);
        output.push_str("<Invoice xmlns=\"");
        output.push_str(F0401_NAMESPACE);
        output.push_str("\"><Main>");

        push_text_element(&mut output, "InvoiceNumber", self.main.invoice_number.as_str());
        push_text_element(&mut output, "InvoiceDate", self.main.invoice_date.as_str());
        push_text_element(&mut output, "InvoiceTime", self.main.invoice_time.as_str());
        push_role(&mut output, "Seller", &self.main.seller);
        push_role(&mut output, "Buyer", &self.main.buyer);
        push_optional(&mut output, "BuyerRemark", self.main.buyer_remark.as_deref());
        push_optional(&mut output, "MainRemark", self.main.main_remark.as_deref());
        if let Some(value) = self.main.customs_clearance_mark {
            push_text_element(&mut output, "CustomsClearanceMark", value.wire_value());
        }
        push_optional(&mut output, "Category", self.main.category.as_deref());
        push_optional(&mut output, "RelateNumber", self.main.relate_number.as_deref());
        push_text_element(&mut output, "InvoiceType", self.main.invoice_type.wire_value());
        push_optional(&mut output, "GroupMark", self.main.group_mark.as_deref());
        push_text_element(&mut output, "DonateMark", self.main.donate_mark.wire_value());
        push_optional(&mut output, "CarrierType", self.main.carrier_type.as_deref());
        push_optional(&mut output, "CarrierId1", self.main.carrier_id1.as_deref());
        push_optional(&mut output, "CarrierId2", self.main.carrier_id2.as_deref());
        push_text_element(&mut output, "PrintMark", self.main.print_mark.wire_value());
        if let Some(value) = &self.main.npo_ban {
            push_text_element(&mut output, "NPOBAN", value.as_str());
        }
        push_optional(&mut output, "RandomNumber", self.main.random_number.as_deref());
        if let Some(value) = self.main.bonded_area_confirm {
            push_text_element(&mut output, "BondedAreaConfirm", value.wire_value());
        }
        if let Some(value) = self.main.zero_tax_rate_reason {
            push_text_element(&mut output, "ZeroTaxRateReason", &value.wire_value());
        }
        push_optional(&mut output, "Reserved1", self.main.reserved1.as_deref());
        push_optional(&mut output, "Reserved2", self.main.reserved2.as_deref());

        output.push_str("</Main><Details>");
        for item in &self.details {
            output.push_str("<ProductItem>");
            push_text_element(&mut output, "Description", &item.description);
            push_text_element(&mut output, "Quantity", item.quantity.as_str());
            push_optional(&mut output, "Unit", item.unit.as_deref());
            push_text_element(&mut output, "UnitPrice", item.unit_price.as_str());
            push_text_element(&mut output, "TaxType", item.tax_type.wire_value());
            push_text_element(&mut output, "Amount", item.amount.as_str());
            push_text_element(&mut output, "SequenceNumber", &item.sequence_number);
            push_optional(&mut output, "Remark", item.remark.as_deref());
            push_optional(&mut output, "RelateNumber", item.relate_number.as_deref());
            output.push_str("</ProductItem>");
        }
        output.push_str("</Details><Amount>");

        push_text_element(&mut output, "SalesAmount", self.amount.sales_amount.as_str());
        push_text_element(
            &mut output,
            "FreeTaxSalesAmount",
            self.amount.free_tax_sales_amount.as_str(),
        );
        push_text_element(
            &mut output,
            "ZeroTaxSalesAmount",
            self.amount.zero_tax_sales_amount.as_str(),
        );
        push_text_element(&mut output, "TaxType", self.amount.tax_type.wire_value());
        push_text_element(&mut output, "TaxRate", self.amount.tax_rate.wire_value());
        push_text_element(&mut output, "TaxAmount", self.amount.tax_amount.as_str());
        push_text_element(&mut output, "TotalAmount", self.amount.total_amount.as_str());
        if let Some(value) = &self.amount.discount_amount {
            push_text_element(&mut output, "DiscountAmount", value.as_str());
        }
        if let Some(value) = &self.amount.original_currency_amount {
            push_text_element(&mut output, "OriginalCurrencyAmount", value.as_str());
        }
        if let Some(value) = &self.amount.exchange_rate {
            push_text_element(&mut output, "ExchangeRate", value.as_str());
        }
        if let Some(value) = &self.amount.currency {
            push_text_element(&mut output, "Currency", value.as_str());
        }

        output.push_str("</Amount></Invoice>");
        output
    }
}

fn validate_date(date: &MigDate, path: &str, report: &mut ValidationReport) {
    if !date.is_calendar_date() {
        push_issue(
            report,
            path,
            "MIG_DATE_NOT_GREGORIAN",
            "date passes the MIG lexical shape but is not a real Gregorian calendar date",
        );
    }
}

fn validate_role(
    role: &RoleDescription,
    seller: bool,
    path: &str,
    report: &mut ValidationReport,
) {
    check_required_max(&role.name, 60, &format!("{path}/Name"), report);
    check_max(&role.address, 100, &format!("{path}/Address"), report);
    check_max(
        &role.person_in_charge,
        30,
        &format!("{path}/PersonInCharge"),
        report,
    );
    check_max(
        &role.telephone_number,
        26,
        &format!("{path}/TelephoneNumber"),
        report,
    );
    check_max(
        &role.facsimile_number,
        26,
        &format!("{path}/FacsimileNumber"),
        report,
    );
    check_max(
        &role.email_address,
        400,
        &format!("{path}/EmailAddress"),
        report,
    );
    check_max(
        &role.customer_number,
        20,
        &format!("{path}/CustomerNumber"),
        report,
    );
    check_exact(
        &role.role_remark,
        40,
        &format!("{path}/RoleRemark"),
        report,
    );
    if seller && role.address.as_deref().is_none_or(str::is_empty) {
        push_issue(
            report,
            &format!("{path}/Address"),
            "F0401_SELLER_ADDRESS_REQUIRED",
            "seller Address is required for F0401",
        );
    }
}

fn check_required_max(value: &str, max: usize, path: &str, report: &mut ValidationReport) {
    let length = value.chars().count();
    if value.is_empty() || length > max {
        push_issue(
            report,
            path,
            "MIG_STRING_LENGTH",
            &format!("value must contain 1 through {max} characters"),
        );
    }
}

fn check_max(value: &Option<String>, max: usize, path: &str, report: &mut ValidationReport) {
    if value.as_ref().is_some_and(|value| value.chars().count() > max) {
        push_issue(
            report,
            path,
            "MIG_STRING_LENGTH",
            &format!("value must contain at most {max} characters"),
        );
    }
}

fn check_exact(value: &Option<String>, length: usize, path: &str, report: &mut ValidationReport) {
    if value
        .as_ref()
        .is_some_and(|value| value.chars().count() != length)
    {
        push_issue(
            report,
            path,
            "MIG_STRING_LENGTH",
            &format!("value must contain exactly {length} characters"),
        );
    }
}

fn is_four_digits(value: &str) -> bool {
    value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn push_issue(report: &mut ValidationReport, path: &str, code: &str, message: &str) {
    report.issues.push(ValidationIssue {
        path: Some(path.to_owned()),
        code: code.to_owned(),
        message: message.to_owned(),
    });
}

fn push_role(output: &mut String, tag: &str, role: &RoleDescription) {
    output.push('<');
    output.push_str(tag);
    output.push('>');
    push_text_element(output, "Identifier", role.identifier.as_str());
    push_text_element(output, "Name", &role.name);
    push_optional(output, "Address", role.address.as_deref());
    push_optional(output, "PersonInCharge", role.person_in_charge.as_deref());
    push_optional(output, "TelephoneNumber", role.telephone_number.as_deref());
    push_optional(output, "FacsimileNumber", role.facsimile_number.as_deref());
    push_optional(output, "EmailAddress", role.email_address.as_deref());
    push_optional(output, "CustomerNumber", role.customer_number.as_deref());
    push_optional(output, "RoleRemark", role.role_remark.as_deref());
    output.push_str("</");
    output.push_str(tag);
    output.push('>');
}

fn push_optional(output: &mut String, tag: &str, value: Option<&str>) {
    if let Some(value) = value {
        push_text_element(output, tag, value);
    }
}

fn push_text_element(output: &mut String, tag: &str, value: &str) {
    output.push('<');
    output.push_str(tag);
    output.push('>');
    push_escaped(output, value);
    output.push_str("</");
    output.push_str(tag);
    output.push('>');
}

fn push_escaped(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            _ => output.push(character),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum F0401StructureError {
    NoProductItems,
    TooManyProductItems { count: usize, max: usize },
}

impl fmt::Display for F0401StructureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoProductItems => f.write_str("F0401 requires at least one ProductItem"),
            Self::TooManyProductItems { count, max } => {
                write!(f, "F0401 contains {count} ProductItems; maximum is {max}")
            }
        }
    }
}

impl Error for F0401StructureError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{MigDecimal, TaxRate};

    fn decimal(value: &str, non_negative: bool) -> MigDecimal {
        MigDecimal::parse(value, 20, 7, non_negative).unwrap()
    }

    fn role(id: &str, name: &str, address: Option<&str>) -> RoleDescription {
        let mut role = RoleDescription::new(Ban::parse(id).unwrap(), name);
        role.address = address.map(str::to_owned);
        role
    }

    fn invoice() -> F0401Invoice {
        let main = F0401Main {
            invoice_number: InvoiceNumber::parse("QQ12345678").unwrap(),
            invoice_date: MigDate::parse("20261025").unwrap(),
            invoice_time: MigTime::parse("14:30:00").unwrap(),
            seller: role("12345678", "Synthetic Seller & Co", Some("Taipei <Test>")),
            buyer: role(Ban::B2C_BUYER, "ABCD", None),
            buyer_remark: None,
            main_remark: None,
            customs_clearance_mark: None,
            category: None,
            relate_number: None,
            invoice_type: InvoiceType::GeneralTax,
            group_mark: None,
            donate_mark: DonateMark::NotDonated,
            carrier_type: None,
            carrier_id1: None,
            carrier_id2: None,
            print_mark: PrintMark::Print,
            npo_ban: None,
            random_number: Some("1234".into()),
            bonded_area_confirm: None,
            zero_tax_rate_reason: None,
            reserved1: None,
            reserved2: None,
        };
        let details = vec![ProductItem::new(
            "Synthetic product",
            decimal("1", false),
            decimal("100", false),
            TaxType::Taxable,
            decimal("100", false),
            "1",
        )];
        let amount = F0401Amount {
            sales_amount: decimal("100", true),
            free_tax_sales_amount: decimal("0", true),
            zero_tax_sales_amount: decimal("0", true),
            tax_type: TaxType::Taxable,
            tax_rate: TaxRate::FivePercent,
            tax_amount: MigDecimal::parse("5", 20, 0, true).unwrap(),
            total_amount: decimal("105", true),
            discount_amount: None,
            original_currency_amount: None,
            exchange_rate: None,
            currency: None,
        };
        F0401Invoice::new(main, details, amount).unwrap()
    }

    #[test]
    fn validates_nominal_synthetic_b2c_invoice() {
        assert!(invoice().validate().is_valid());
    }

    #[test]
    fn detects_b2c_and_print_semantic_conflicts() {
        let mut invoice = invoice();
        invoice.main.random_number = None;
        invoice.main.carrier_type = Some("3J0002".into());
        invoice.main.donate_mark = DonateMark::Donated;

        let report = invoice.validate();
        let codes: Vec<&str> = report.issues.iter().map(|issue| issue.code.as_str()).collect();
        assert!(codes.contains(&"F0401_B2C_RANDOM_NUMBER_REQUIRED"));
        assert!(codes.contains(&"F0401_PRINT_WITH_CARRIER"));
        assert!(codes.contains(&"F0401_PRINT_DONATION_CONFLICT"));
    }

    #[test]
    fn detects_mixed_product_tax_and_missing_zero_rate_reason() {
        let mut invoice = invoice();
        invoice.details[0].tax_type = TaxType::Mixed;
        invoice.amount.tax_type = TaxType::ZeroRated;

        let report = invoice.validate();
        let codes: Vec<&str> = report.issues.iter().map(|issue| issue.code.as_str()).collect();
        assert!(codes.contains(&"F0401_PRODUCT_MIXED_TAX_FORBIDDEN"));
        assert!(codes.contains(&"F0401_ZERO_RATE_REASON_REQUIRED"));
    }

    #[test]
    fn rejects_invalid_product_cardinality() {
        let invoice = invoice();
        assert_eq!(
            F0401Invoice::new(invoice.main.clone(), Vec::new(), invoice.amount.clone()).unwrap_err(),
            F0401StructureError::NoProductItems
        );
    }

    #[test]
    fn emits_deterministic_mig41_xml() {
        let xml = invoice().to_xml();
        assert_eq!(
            xml,
            concat!(
                "<Invoice xmlns=\"urn:GEINV:eInvoiceMessage:F0401:4.1\">",
                "<Main>",
                "<InvoiceNumber>QQ12345678</InvoiceNumber>",
                "<InvoiceDate>20261025</InvoiceDate>",
                "<InvoiceTime>14:30:00</InvoiceTime>",
                "<Seller><Identifier>12345678</Identifier><Name>Synthetic Seller &amp; Co</Name><Address>Taipei &lt;Test&gt;</Address></Seller>",
                "<Buyer><Identifier>0000000000</Identifier><Name>ABCD</Name></Buyer>",
                "<InvoiceType>07</InvoiceType>",
                "<DonateMark>0</DonateMark>",
                "<PrintMark>Y</PrintMark>",
                "<RandomNumber>1234</RandomNumber>",
                "</Main>",
                "<Details><ProductItem>",
                "<Description>Synthetic product</Description>",
                "<Quantity>1</Quantity>",
                "<UnitPrice>100</UnitPrice>",
                "<TaxType>1</TaxType>",
                "<Amount>100</Amount>",
                "<SequenceNumber>1</SequenceNumber>",
                "</ProductItem></Details>",
                "<Amount>",
                "<SalesAmount>100</SalesAmount>",
                "<FreeTaxSalesAmount>0</FreeTaxSalesAmount>",
                "<ZeroTaxSalesAmount>0</ZeroTaxSalesAmount>",
                "<TaxType>1</TaxType>",
                "<TaxRate>0.05</TaxRate>",
                "<TaxAmount>5</TaxAmount>",
                "<TotalAmount>105</TotalAmount>",
                "</Amount>",
                "</Invoice>"
            )
        );
    }

    #[test]
    fn xml_escapes_optional_text() {
        let mut invoice = invoice();
        invoice.main.main_remark = Some("A&B <C>".into());
        let xml = invoice.to_xml();
        assert!(xml.contains("<MainRemark>A&amp;B &lt;C&gt;</MainRemark>"));
    }
}
