use crate::SignatureAlgorithm;

const EOC: [u8; 2] = [0x00, 0x00];
const ASN1_NULL: [u8; 2] = [0x05, 0x00];

// OID 1.2.840.113549.1.7.2 (signedData), including ASN.1 tag and length.
const OID_SIGNED_DATA: [u8; 11] = [
    0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02,
];

// OID 1.2.840.113549.1.7.1 (data), including ASN.1 tag and length.
const OID_DATA: [u8; 11] = [
    0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x01,
];

// OID 2.16.840.1.101.3.4.2.1 (sha256), including ASN.1 tag and length.
const OID_SHA256: [u8; 11] = [
    0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
];

// OID 1.2.840.113549.1.1.11 (sha256WithRSAEncryption).
const OID_SHA256_WITH_RSA: [u8; 11] = [
    0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b,
];

// OID 1.2.840.10045.4.3.2 (ecdsa-with-SHA256).
const OID_ECDSA_WITH_SHA256: [u8; 10] =
    [0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];

pub(crate) struct CmsParts<'a> {
    pub content: &'a [u8],
    pub certificate_der: &'a [u8],
    pub issuer_name_der: &'a [u8],
    pub serial_number: &'a [u8],
    pub signature_algorithm: SignatureAlgorithm,
    pub signature: &'a [u8],
}

/// Reproduces the single-signer CMS/PKCS#7 wire layout emitted by the official
/// Turnkey 3.2.1 KMS generator.
///
/// The layout intentionally mixes BER indefinite-length containers with
/// definite-length DER values. This is not a generic CMS encoder; it is the
/// narrow compatibility profile used by the e-Invoice transport.
pub(crate) fn encode_turnkey_signed_data(parts: &CmsParts<'_>) -> Vec<u8> {
    let digest_algorithm = algorithm_identifier(&OID_SHA256);
    let signature_algorithm = match parts.signature_algorithm {
        SignatureAlgorithm::RsaPkcs1v15Sha256 => algorithm_identifier(&OID_SHA256_WITH_RSA),
        SignatureAlgorithm::EcdsaSha256 => algorithm_identifier(&OID_ECDSA_WITH_SHA256),
    };

    let issuer_and_serial = sequence(&[
        parts.issuer_name_der.to_vec(),
        positive_integer(parts.serial_number),
    ]);

    let signer_info = sequence(&[
        integer_one(),
        issuer_and_serial,
        digest_algorithm.clone(),
        signature_algorithm,
        octet_string(parts.signature),
    ]);

    let mut output = Vec::with_capacity(
        parts.content.len()
            + parts.certificate_der.len()
            + parts.signature.len()
            + parts.issuer_name_der.len()
            + 256,
    );

    // ContentInfo ::= SEQUENCE (indefinite)
    open_indefinite(0x30, &mut output);
    output.extend_from_slice(&OID_SIGNED_DATA);

    // content [0] EXPLICIT SignedData (indefinite)
    open_indefinite(0xa0, &mut output);

    // SignedData ::= SEQUENCE (indefinite)
    open_indefinite(0x30, &mut output);
    output.extend_from_slice(&integer_one());
    output.extend_from_slice(&set(&[digest_algorithm]));

    // EncapsulatedContentInfo ::= SEQUENCE (indefinite)
    open_indefinite(0x30, &mut output);
    output.extend_from_slice(&OID_DATA);

    // eContent [0] EXPLICIT OCTET STRING (indefinite wrapper, definite primitive)
    open_indefinite(0xa0, &mut output);
    output.extend_from_slice(&octet_string(parts.content));
    close_indefinite(&mut output);
    close_indefinite(&mut output);

    // certificates [0] IMPLICIT CertificateSet (indefinite)
    open_indefinite(0xa0, &mut output);
    output.extend_from_slice(parts.certificate_der);
    close_indefinite(&mut output);

    // signerInfos SET OF SignerInfo (definite)
    output.extend_from_slice(&set(&[signer_info]));

    close_indefinite(&mut output); // SignedData
    close_indefinite(&mut output); // ContentInfo.content [0]
    close_indefinite(&mut output); // ContentInfo

    output
}

fn open_indefinite(tag: u8, output: &mut Vec<u8>) {
    output.extend_from_slice(&[tag, 0x80]);
}

fn close_indefinite(output: &mut Vec<u8>) {
    output.extend_from_slice(&EOC);
}

fn integer_one() -> Vec<u8> {
    vec![0x02, 0x01, 0x01]
}

fn positive_integer(bytes: &[u8]) -> Vec<u8> {
    let mut body = if bytes.is_empty() {
        vec![0]
    } else {
        let first_nonzero = bytes
            .iter()
            .position(|byte| *byte != 0)
            .unwrap_or(bytes.len() - 1);
        bytes[first_nonzero..].to_vec()
    };

    if body[0] & 0x80 != 0 {
        body.insert(0, 0);
    }

    tlv(0x02, &body)
}

fn algorithm_identifier(oid: &[u8]) -> Vec<u8> {
    sequence(&[oid.to_vec(), ASN1_NULL.to_vec()])
}

fn octet_string(bytes: &[u8]) -> Vec<u8> {
    tlv(0x04, bytes)
}

fn sequence(elements: &[Vec<u8>]) -> Vec<u8> {
    constructed(0x30, elements)
}

fn set(elements: &[Vec<u8>]) -> Vec<u8> {
    constructed(0x31, elements)
}

fn constructed(tag: u8, elements: &[Vec<u8>]) -> Vec<u8> {
    let body_len = elements.iter().map(Vec::len).sum();
    let mut body = Vec::with_capacity(body_len);
    for element in elements {
        body.extend_from_slice(element);
    }
    tlv(tag, &body)
}

fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(1 + encoded_length_size(body.len()) + body.len());
    encoded.push(tag);
    write_length(body.len(), &mut encoded);
    encoded.extend_from_slice(body);
    encoded
}

fn encoded_length_size(length: usize) -> usize {
    if length < 128 {
        1
    } else {
        let bytes = length.to_be_bytes();
        let first_nonzero = bytes
            .iter()
            .position(|byte| *byte != 0)
            .expect("non-short ASN.1 length is nonzero");
        1 + bytes.len() - first_nonzero
    }
}

fn write_length(length: usize, output: &mut Vec<u8>) {
    if length < 128 {
        output.push(u8::try_from(length).expect("short-form ASN.1 length fits in u8"));
        return;
    }

    let bytes = length.to_be_bytes();
    let first_nonzero = bytes
        .iter()
        .position(|byte| *byte != 0)
        .expect("non-short ASN.1 length is nonzero");
    let significant = &bytes[first_nonzero..];
    output.push(0x80 | u8::try_from(significant.len()).expect("usize length-of-length fits in u8"));
    output.extend_from_slice(significant);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_hex(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect()
    }

    #[test]
    fn encodes_recovered_single_signer_layout() {
        // Deliberately tiny synthetic values make the entire expected BER
        // representation reviewable while exercising the real tag/length rules.
        let encoded = encode_turnkey_signed_data(&CmsParts {
            content: b"x",
            certificate_der: &[0x30, 0x00],
            issuer_name_der: &[0x30, 0x00],
            serial_number: &[0x80],
            signature_algorithm: SignatureAlgorithm::RsaPkcs1v15Sha256,
            signature: &[0xaa],
        });

        let expected = decode_hex(concat!(
            "308006092a864886f70d010702a0803080020101",
            "310f300d06096086480165030402010500",
            "308006092a864886f70d010701a08004017800000000",
            "a08030000000",
            "312e302c0201013006300002020080",
            "300d06096086480165030402010500",
            "300d06092a864886f70d01010b0500",
            "0401aa",
            "000000000000"
        ));

        assert_eq!(encoded, expected);
    }

    #[test]
    fn positive_integer_adds_sign_protection_byte() {
        assert_eq!(positive_integer(&[0x80]), vec![0x02, 0x02, 0x00, 0x80]);
        assert_eq!(positive_integer(&[0x00, 0x7f]), vec![0x02, 0x01, 0x7f]);
    }

    #[test]
    fn ecdsa_algorithm_identifier_keeps_observed_null_parameter() {
        let encoded = encode_turnkey_signed_data(&CmsParts {
            content: b"x",
            certificate_der: &[0x30, 0x00],
            issuer_name_der: &[0x30, 0x00],
            serial_number: &[1],
            signature_algorithm: SignatureAlgorithm::EcdsaSha256,
            signature: &[0x30, 0x00],
        });

        assert!(encoded.windows(14).any(|window| {
            window
                == [
                    0x30, 0x0c, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02, 0x05,
                    0x00,
                ]
        }));
    }
}
