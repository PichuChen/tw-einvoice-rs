use std::{error::Error, fmt};

use openssl::{
    error::ErrorStack,
    hash::MessageDigest,
    pkcs12::Pkcs12,
    pkey::{Id, PKey, Private},
    rsa::Padding,
    sign::Signer,
    x509::X509,
};

use crate::{
    CmsSignedData, CmsSigner, SignatureAlgorithm,
    ber::{CmsParts, encode_turnkey_signed_data},
};

/// Software-certificate signer backed by a PKCS#12/PFX identity.
///
/// OpenSSL is deliberately used only for key/certificate parsing and the
/// private-key signature operation. CMS construction is performed by this crate
/// so the resulting ASN.1 profile matches the official Turnkey 3.2.1 output,
/// including its BER framing and `SignerInfo` algorithm identifiers.
pub struct PfxSigner {
    private_key: PKey<Private>,
    certificate: X509,
    signature_algorithm: SignatureAlgorithm,
}

impl PfxSigner {
    /// Loads a PFX identity from DER-encoded PKCS#12 bytes.
    ///
    /// The password is used only while OpenSSL parses the PFX and is not stored
    /// by this value.
    ///
    /// # Errors
    ///
    /// Returns [`PfxSignerError`] when the PKCS#12 object cannot be parsed, does
    /// not contain both a private key and certificate, contains a mismatched
    /// key/certificate pair, or uses a key type outside the RSA/EC profile
    /// observed in Turnkey 3.2.1.
    pub fn from_der(pfx_der: &[u8], password: &str) -> Result<Self, PfxSignerError> {
        let parsed = Pkcs12::from_der(pfx_der)?.parse2(password)?;
        let private_key = parsed.pkey.ok_or(PfxSignerError::MissingPrivateKey)?;
        let certificate = parsed.cert.ok_or(PfxSignerError::MissingCertificate)?;

        let certificate_key = certificate.public_key()?;
        if !certificate_key.public_eq(&private_key) {
            return Err(PfxSignerError::MismatchedKeyPair);
        }

        let signature_algorithm = match private_key.id() {
            Id::RSA => SignatureAlgorithm::RsaPkcs1v15Sha256,
            Id::EC => SignatureAlgorithm::EcdsaSha256,
            _ => return Err(PfxSignerError::UnsupportedPrivateKey),
        };

        Ok(Self {
            private_key,
            certificate,
            signature_algorithm,
        })
    }

    /// Returns the signing certificate without exposing the private key.
    #[must_use]
    pub fn certificate(&self) -> &X509 {
        &self.certificate
    }
}

impl fmt::Debug for PfxSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PfxSigner")
            .field("signature_algorithm", &self.signature_algorithm)
            .finish_non_exhaustive()
    }
}

impl CmsSigner for PfxSigner {
    type Error = PfxSignerError;

    fn signature_algorithm(&self) -> SignatureAlgorithm {
        self.signature_algorithm
    }

    fn sign_attached(&self, content: &[u8]) -> Result<CmsSignedData, Self::Error> {
        let mut signer = Signer::new(MessageDigest::sha256(), &self.private_key)?;
        if self.signature_algorithm == SignatureAlgorithm::RsaPkcs1v15Sha256 {
            signer.set_rsa_padding(Padding::PKCS1)?;
        }
        signer.update(content)?;
        let signature = signer.sign_to_vec()?;

        let certificate_der = self.certificate.to_der()?;
        let issuer_name_der = self.certificate.issuer_name().to_der()?;
        let serial_number = self.certificate.serial_number().to_bn()?.to_vec();

        let encoded = encode_turnkey_signed_data(&CmsParts {
            content,
            certificate_der: &certificate_der,
            issuer_name_der: &issuer_name_der,
            serial_number: &serial_number,
            signature_algorithm: self.signature_algorithm,
            signature: &signature,
        });

        Ok(CmsSignedData::from_encoded_unchecked(encoded))
    }
}

#[derive(Debug)]
pub enum PfxSignerError {
    OpenSsl(ErrorStack),
    MissingPrivateKey,
    MissingCertificate,
    MismatchedKeyPair,
    UnsupportedPrivateKey,
}

impl fmt::Display for PfxSignerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenSsl(error) => write!(f, "OpenSSL signing operation failed: {error}"),
            Self::MissingPrivateKey => f.write_str("PKCS#12 object does not contain a private key"),
            Self::MissingCertificate => {
                f.write_str("PKCS#12 object does not contain a signing certificate")
            }
            Self::MismatchedKeyPair => {
                f.write_str("PKCS#12 private key does not match the signing certificate")
            }
            Self::UnsupportedPrivateKey => {
                f.write_str("Turnkey compatibility signer supports only RSA and EC private keys")
            }
        }
    }
}

impl Error for PfxSignerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::OpenSsl(error) => Some(error),
            Self::MissingPrivateKey
            | Self::MissingCertificate
            | Self::MismatchedKeyPair
            | Self::UnsupportedPrivateKey => None,
        }
    }
}

impl From<ErrorStack> for PfxSignerError {
    fn from(value: ErrorStack) -> Self {
        Self::OpenSsl(value)
    }
}
