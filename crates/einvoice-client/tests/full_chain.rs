//! Public, offline interoperability coverage. All invoice data and the ephemeral
//! RSA/PFX identity are synthetic; only the upload/notification adapters are mocked.

use std::{cell::RefCell, convert::Infallible, io::Read, rc::Rc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use openssl::{
    asn1::Asn1Time,
    bn::BigNum,
    cms::{CMSOptions, CmsContentInfo},
    error::ErrorStack,
    hash::MessageDigest,
    nid::Nid,
    pkcs12::Pkcs12,
    pkey::PKey,
    rsa::Rsa,
    x509::{X509, X509Name},
};
use serde_json::json;
use tw_einvoice_client::{SubmissionPlan, prepare_submission};
use tw_einvoice_core::{Ban, MigVersion};
use tw_einvoice_envelope::{
    EnvelopeRouting, InvoiceEnvelope, InvoicePackMetadata, MigPayload, PartyInfo, RoutingInfo,
};
use tw_einvoice_mig::{
    DonateMark, F0401Amount, F0401Invoice, F0401Main, InvoiceNumber, InvoiceType, Mig41MessageCode,
    MigDate, MigDecimal, MigTime, PrintMark, ProductItem, RoleDescription, TaxRate, TaxType,
};
use tw_einvoice_signing::{PfxSigner, SignatureAlgorithm};
use tw_einvoice_transport::{
    filename::{MessageCommonName, TurnkeyPackFilename},
    gateway::{
        GatewayCredentials, GatewayEncodeError, GatewayNotifier, GatewayProcessStatus, ServiceType,
        UploadNotification, ZipMode,
    },
    native::{GatewayVersionProfile, NativeSubmitter},
    sftp::ObjectUploader,
};
use zip::{CompressionMethod, ZipArchive};

const PASSWORD: &str = "synthetic-full-chain-password";
const UUID: &str = "550e8400-e29b-41d4-a716-446655440000";
const COMMON_NAME: &str = "4.1-F0401-20261025-143000123-550e8400-e29b-41d4-a716-446655440000";
const SOURCE_NAME: &str = "synthetic_F0401_QQ12345678.xml";

fn decimal(value: &str) -> MigDecimal {
    MigDecimal::parse(value, 20, 7, true).unwrap()
}

fn synthetic_invoice() -> F0401Invoice {
    let mut seller = RoleDescription::new(Ban::parse("12345678").unwrap(), "Synthetic 賣方 & Co");
    seller.address = Some("Test Road <1>".into());
    let main = F0401Main {
        invoice_number: InvoiceNumber::parse("QQ12345678").unwrap(),
        invoice_date: MigDate::parse("20261025").unwrap(),
        invoice_time: MigTime::parse("14:30:00").unwrap(),
        seller,
        buyer: RoleDescription::new(Ban::parse(Ban::B2C_BUYER).unwrap(), "ABCD"),
        buyer_remark: None,
        main_remark: Some("Synthetic interoperability fixture".into()),
        customs_clearance_mark: None,
        category: None,
        relate_number: Some("M10-F0401-0001".into()),
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
        "Synthetic 測試品 <A> & B",
        decimal("2"),
        decimal("50"),
        TaxType::Taxable,
        decimal("100"),
        "1",
    )];
    let amount = F0401Amount {
        sales_amount: decimal("100"),
        free_tax_sales_amount: decimal("0"),
        zero_tax_sales_amount: decimal("0"),
        tax_type: TaxType::Taxable,
        tax_rate: TaxRate::FivePercent,
        tax_amount: MigDecimal::parse("5", 20, 0, true).unwrap(),
        total_amount: decimal("105"),
        discount_amount: None,
        original_currency_amount: None,
        exchange_rate: None,
        currency: None,
    };
    F0401Invoice::new(main, details, amount).unwrap()
}

fn synthetic_plan(zip_mode: ZipMode) -> SubmissionPlan {
    let invoice = synthetic_invoice();
    let validation = invoice.validate();
    assert!(validation.is_valid(), "{validation:?}");
    let xml = invoice.to_xml();
    assert!(xml.starts_with("<Invoice xmlns=\"urn:GEINV:eInvoiceMessage:F0401:4.1\">"));
    assert!(xml.contains("<Description>Synthetic 測試品 &lt;A&gt; &amp; B</Description>"));
    assert!(xml.contains("<RelateNumber>M10-F0401-0001</RelateNumber>"));

    let filename = TurnkeyPackFilename {
        source_name: format!(
            "synthetic_F0401_{}.xml",
            invoice.main.invoice_number.as_str()
        ),
        from_party: invoice.main.seller.identifier.as_str().into(),
        to_party: "PLATFORM".into(),
        common_name: MessageCommonName::from_parts(
            MigVersion::V4_1.as_str(),
            Mig41MessageCode::F0401.as_str(),
            invoice.main.invoice_date.as_str(),
            "143000123",
            UUID,
        )
        .unwrap(),
        pack_count: 1,
    };
    assert_eq!(filename.source_name, SOURCE_NAME);
    assert_eq!(filename.remote_filename(), COMMON_NAME);

    let envelope = InvoiceEnvelope::new(
        EnvelopeRouting {
            from: PartyInfo {
                identifier: invoice.main.seller.identifier.as_str().into(),
                description: Some(invoice.main.seller.name.clone()),
            },
            from_vac: RoutingInfo {
                identifier: "SYNTHETIC-FROM".into(),
                description: Some("Synthetic route <A>".into()),
            },
            to: PartyInfo {
                identifier: "PLATFORM".into(),
                description: None,
            },
            to_vac: RoutingInfo {
                identifier: "SYNTHETIC-TO".into(),
                description: None,
            },
        },
        InvoicePackMetadata {
            message_type: Mig41MessageCode::F0401.as_str().into(),
            version: MigVersion::V4_1,
        },
        vec![MigPayload::new(xml.as_bytes().to_vec()).unwrap()],
    )
    .unwrap();
    assert_eq!(envelope.payloads()[0].as_bytes(), xml.as_bytes());
    let envelope_xml = String::from_utf8(envelope.to_turnkey_xml().unwrap()).unwrap();
    assert!(envelope_xml.starts_with('\n'));
    assert!(envelope_xml.ends_with("</InvoiceEnvelope>\n"));
    assert!(
        envelope_xml.contains("<InvoicePack count=\"1\" messageType=\"F0401\" version=\"v41\">")
    );
    assert!(envelope_xml.contains(&format!("\n{xml}\n</InvoicePack>")));

    SubmissionPlan::new(
        envelope,
        filename,
        ServiceType::Store,
        zip_mode,
        "C0401",
        GatewayVersionProfile::new("3.1.3", "3.2.1"),
    )
}

fn synthetic_rsa_pfx_signer() -> PfxSigner {
    let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509Name::builder().unwrap();
    name.append_entry_by_nid(Nid::COMMONNAME, "synthetic-full-chain")
        .unwrap();
    let name = name.build();
    let serial = BigNum::from_u32(35).unwrap().to_asn1_integer().unwrap();
    let mut certificate = X509::builder().unwrap();
    certificate.set_version(2).unwrap();
    certificate.set_serial_number(&serial).unwrap();
    certificate.set_subject_name(&name).unwrap();
    certificate.set_issuer_name(&name).unwrap();
    certificate.set_pubkey(&key).unwrap();
    certificate
        .set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    certificate
        .set_not_after(&Asn1Time::days_from_now(1).unwrap())
        .unwrap();
    certificate.sign(&key, MessageDigest::sha256()).unwrap();
    let certificate = certificate.build();
    let pfx = Pkcs12::builder()
        .name("synthetic-full-chain")
        .pkey(&key)
        .cert(&certificate)
        .build2(PASSWORD)
        .unwrap()
        .to_der()
        .unwrap();
    PfxSigner::from_der(&pfx, PASSWORD).unwrap()
}

#[derive(Debug)]
enum Call {
    Upload {
        filename: String,
        bytes: Vec<u8>,
    },
    Notify {
        metadata: Box<UploadNotification>,
        json: Vec<u8>,
    },
}

#[derive(Debug, Clone, Default)]
struct InMemoryTransport(Rc<RefCell<Vec<Call>>>);

impl ObjectUploader for InMemoryTransport {
    type Error = Infallible;

    fn upload(&self, remote_filename: &str, bytes: &[u8]) -> Result<(), Self::Error> {
        self.0.borrow_mut().push(Call::Upload {
            filename: remote_filename.into(),
            bytes: bytes.to_vec(),
        });
        Ok(())
    }
}

impl GatewayNotifier for InMemoryTransport {
    type Error = GatewayEncodeError;

    fn notify(
        &self,
        notification: &UploadNotification,
    ) -> Result<GatewayProcessStatus, Self::Error> {
        let json = notification.to_pfs001_json(&GatewayCredentials::new(
            "synthetic-login",
            "synthetic-password",
        ))?;
        self.0.borrow_mut().push(Call::Notify {
            metadata: Box::new(notification.clone()),
            json,
        });
        Ok(GatewayProcessStatus::from_json(br#"{"statusCode":0,"statusDetail":[]}"#).unwrap())
    }
}

fn uploaded_signed_bytes(bytes: &[u8], mode: ZipMode, local_filename: &str) -> Vec<u8> {
    match mode {
        ZipMode::Plain => bytes.to_vec(),
        ZipMode::Zip => {
            let mut archive = ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            assert_eq!(archive.len(), 1);
            let mut entry = archive.by_index(0).unwrap();
            assert_eq!(entry.name().unwrap().as_ref(), local_filename);
            assert_eq!(entry.compression(), CompressionMethod::Deflated);
            let mut extracted = Vec::new();
            entry.read_to_end(&mut extracted).unwrap();
            assert_eq!(entry.size(), u64::try_from(extracted.len()).unwrap());
            extracted
        }
    }
}

fn decode_turnkey_armor(bytes: &[u8]) -> Vec<u8> {
    let armor = str::from_utf8(bytes).unwrap();
    assert!(armor.ends_with('\n'));
    assert!(!armor.contains('\r'));
    assert!(!armor.contains("-----"));
    let lines: Vec<_> = armor.lines().collect();
    let (last, preceding) = lines.split_last().unwrap();
    assert!(!last.is_empty() && last.len() <= 64);
    assert!(preceding.iter().all(|line| line.len() == 64));
    STANDARD.decode(lines.concat()).unwrap()
}

fn verify_cms(cms: &[u8]) -> Result<Vec<u8>, ErrorStack> {
    let mut signed = CmsContentInfo::from_der(cms)?;
    let mut recovered = Vec::new();
    // Skip trust-chain validation for this disposable self-signed certificate.
    // Content/signature verification remains enabled, using the embedded leaf.
    signed.verify(
        None,
        None,
        None,
        Some(&mut recovered),
        CMSOptions::NOVERIFY | CMSOptions::BINARY,
    )?;
    Ok(recovered)
}

fn assert_pfs001(notification: &UploadNotification, wire: &[u8], mode: ZipMode, size: usize) {
    assert_eq!(notification.filename, COMMON_NAME);
    assert_eq!(notification.zip_mode, mode);
    assert_eq!(notification.size, u64::try_from(size).unwrap());
    let body: serde_json::Value = serde_json::from_slice(wire).unwrap();
    assert_eq!(
        body,
        json!({
            "loginId": "synthetic-login",
            "loginPassword": "synthetic-password",
            "transFrom": {
                "partyInfo": {"partyId": "12345678", "description": "Synthetic 賣方 & Co"},
                "routingInfo": {"routingId": "SYNTHETIC-FROM", "description": "Synthetic route <A>"}
            },
            "transTo": {
                "partyInfo": {"partyId": "PLATFORM"},
                "routingInfo": {"routingId": "SYNTHETIC-TO"}
            },
            "serviceType": "S",
            "messageType": "F0401",
            "action": "C0401",
            "quantity": 1,
            "migVersion": "4.1",
            "filename": COMMON_NAME,
            "size": size,
            "zip": match mode { ZipMode::Plain => "0", ZipMode::Zip => "1" },
            "retry": 0,
            "apiVersion": "3.1.3",
            "turnkeyVersion": "3.2.1"
        })
    );
}

fn assert_full_chain(mode: ZipMode) {
    let plan = synthetic_plan(mode);
    let expected_envelope = plan.envelope.to_turnkey_xml().unwrap();
    let expected_filename = format!("{SOURCE_NAME}_12345678_PLATFORM_{COMMON_NAME}_1");
    assert_eq!(plan.filename.render(), expected_filename);
    let prepared = prepare_submission(plan, &synthetic_rsa_pfx_signer()).unwrap();
    assert_eq!(prepared.filename().render(), expected_filename);
    assert_eq!(prepared.filename().remote_filename(), COMMON_NAME);
    assert_eq!(prepared.zip_mode(), mode);
    assert_eq!(prepared.artifact().envelope_size(), expected_envelope.len());
    assert_eq!(
        prepared.artifact().signature_algorithm(),
        SignatureAlgorithm::RsaPkcs1v15Sha256
    );

    let recording = InMemoryTransport::default();
    let submitter = NativeSubmitter::new(recording.clone(), recording.clone());
    let receipt = prepared.submit(&submitter).unwrap();
    assert_eq!(receipt.remote_filename, COMMON_NAME);
    assert!(receipt.gateway_status.is_accepted());

    let calls = recording.0.borrow();
    let [
        Call::Upload { filename, bytes },
        Call::Notify { metadata, json },
    ] = calls.as_slice()
    else {
        panic!("expected exactly one upload followed by one PFS001 notification: {calls:?}");
    };
    assert_eq!(filename, COMMON_NAME);
    let signed_bytes = uploaded_signed_bytes(bytes, mode, &expected_filename);
    assert_eq!(signed_bytes, prepared.artifact().as_upload_bytes());
    assert_eq!(
        signed_bytes.len(),
        prepared.artifact().turnkey_reported_size()
    );
    assert_pfs001(metadata, json, mode, signed_bytes.len());
    if mode == ZipMode::Zip {
        assert_ne!(
            bytes.len(),
            signed_bytes.len(),
            "fixture must distinguish ZIP and reported sizes"
        );
    }

    // Verify what crossed the upload boundary, including both armor and ZIP decoding.
    let cms = decode_turnkey_armor(&signed_bytes);
    assert_eq!(
        &cms[..2],
        &[0x30, 0x80],
        "strict BER indefinite-length profile"
    );
    assert_ne!(cms.len(), signed_bytes.len());
    assert_eq!(verify_cms(&cms).unwrap(), expected_envelope);

    // Change an invoice number without altering ASN.1 lengths or XML validity.
    // Parsing must still succeed, but the actual RSA signature check must fail.
    let marker = b"<InvoiceNumber>QQ12345678</InvoiceNumber>";
    let offset = cms
        .windows(marker.len())
        .position(|part| part == marker)
        .unwrap();
    let mut tampered = cms;
    tampered[offset + b"<InvoiceNumber>QQ".len()] = b'9';
    CmsContentInfo::from_der(&tampered).unwrap();
    assert!(
        verify_cms(&tampered).is_err(),
        "altered embedded invoice must fail verification"
    );
}

#[test]
fn f0401_rsa_pfx_plain_sendfile_pfs001_full_chain() {
    assert_full_chain(ZipMode::Plain);
}

#[test]
fn f0401_rsa_pfx_zip_sendfile_pfs001_full_chain() {
    assert_full_chain(ZipMode::Zip);
}
