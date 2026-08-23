# CMS signing interoperability target

The signing layer operates on immutable bytes. It does not own or parse MIG XML.

## Target profile

Current interoperability research establishes this target profile for software-certificate signing:

- CMS `ContentInfo` / `SignedData`;
- SignedData version 1;
- attached content with content type `pkcs7-data`;
- SHA-256 digest;
- signer identified by issuer + certificate serial number;
- no signed CMS attributes;
- no unsigned CMS attributes;
- leaf signer certificate included in the CMS certificate set;
- RSA/SHA-256 and EC/SHA-256 signer support;
- markerless Base64 text for the transport package.

The interoperability target is a CMS semantic profile, not a requirement that every standards-equivalent implementation use identical ASN.1 length encoding. Platform tests must determine whether canonical DER and the observed BER form are both accepted.

## API shape

The public signing abstraction should resemble:

```rust,ignore
pub trait PackageSigner {
    type Error;

    fn sign(&self, content: &[u8]) -> Result<SignedPackage, Self::Error>;
}

pub struct SignedPackage {
    cms: Vec<u8>,
}
```

Text armor is a separate representation concern from CMS construction.

Recommended implementations:

```text
PfxSigner       PKCS#12 software certificate
Pkcs11Signer    HSM/smart-card integration
```

A production API should prefer secret references/handles over long-lived password strings.

## Text package representation

For legacy-compatible output, support:

- Base64 at 64 columns;
- LF line ending;
- terminal LF;
- no PEM `BEGIN/END PKCS7` marker lines.

The decoder should be able to accept this representation without assuming it is canonical PEM.

## Tests

Use a synthetic CA and signer generated only for tests. Verify:

- exact recovery of attached content;
- CMS signature verification;
- issuer+serial signer identifier;
- certificate set behavior;
- RSA deterministic repeatability for identical synthetic input/key under the target profile;
- EC verification without requiring whole-file byte equality;
- markerless text encode/decode round trips;
- rejection of corrupted CMS/signature/content.

No real fiscal private key belongs in source control or CI.
