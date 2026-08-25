use openssl::{
    asn1::Asn1Time,
    bn::BigNum,
    cms::{CMSOptions, CmsContentInfo},
    ec::{EcGroup, EcKey},
    hash::MessageDigest,
    nid::Nid,
    pkcs12::Pkcs12,
    pkey::{PKey, Private},
    rsa::Rsa,
    x509::{X509, X509Name},
};
use tw_einvoice_signing::{CmsSigner, PfxSigner, SignatureAlgorithm};

const PASSWORD: &str = "synthetic-test-password";
const CONTENT: &[u8] = b"synthetic-turnkey-interoperability-payload";

fn self_signed_certificate(private_key: &PKey<Private>, common_name: &str) -> X509 {
    let mut name = X509Name::builder().unwrap();
    name.append_entry_by_nid(Nid::COMMONNAME, common_name)
        .unwrap();
    let name = name.build();

    let serial = BigNum::from_u32(42).unwrap().to_asn1_integer().unwrap();
    let not_before = Asn1Time::days_from_now(0).unwrap();
    let not_after = Asn1Time::days_from_now(1).unwrap();

    let mut certificate = X509::builder().unwrap();
    certificate.set_version(2).unwrap();
    certificate.set_serial_number(&serial).unwrap();
    certificate.set_subject_name(&name).unwrap();
    certificate.set_issuer_name(&name).unwrap();
    certificate.set_not_before(&not_before).unwrap();
    certificate.set_not_after(&not_after).unwrap();
    certificate.set_pubkey(private_key).unwrap();
    certificate
        .sign(private_key, MessageDigest::sha256())
        .unwrap();
    certificate.build()
}

fn pfx_der(private_key: &PKey<Private>, certificate: &X509) -> Vec<u8> {
    Pkcs12::builder()
        .name("synthetic-turnkey-test")
        .pkey(private_key)
        .cert(certificate)
        .build2(PASSWORD)
        .unwrap()
        .to_der()
        .unwrap()
}

fn verify_attached_cms(encoded: &[u8]) {
    let mut cms = CmsContentInfo::from_der(encoded).unwrap();
    let mut recovered = Vec::new();
    cms.verify(
        None,
        None,
        None,
        Some(&mut recovered),
        CMSOptions::NOVERIFY | CMSOptions::BINARY,
    )
    .unwrap();

    assert_eq!(recovered, CONTENT);
}

#[test]
fn rsa_pfx_produces_verifiable_turnkey_profile() {
    let private_key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let certificate = self_signed_certificate(&private_key, "synthetic-rsa");
    let pfx = pfx_der(&private_key, &certificate);

    let signer = PfxSigner::from_der(&pfx, PASSWORD).unwrap();
    assert_eq!(
        signer.signature_algorithm(),
        SignatureAlgorithm::RsaPkcs1v15Sha256
    );

    let cms = signer.sign_attached(CONTENT).unwrap();
    assert_eq!(&cms.as_encoded()[..2], &[0x30, 0x80]);
    verify_attached_cms(cms.as_encoded());
}

#[test]
fn ec_pfx_produces_verifiable_turnkey_profile() {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
    let private_key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
    let certificate = self_signed_certificate(&private_key, "synthetic-ec");
    let pfx = pfx_der(&private_key, &certificate);

    let signer = PfxSigner::from_der(&pfx, PASSWORD).unwrap();
    assert_eq!(
        signer.signature_algorithm(),
        SignatureAlgorithm::EcdsaSha256
    );

    let cms = signer.sign_attached(CONTENT).unwrap();
    assert_eq!(&cms.as_encoded()[..2], &[0x30, 0x80]);
    verify_attached_cms(cms.as_encoded());
}
