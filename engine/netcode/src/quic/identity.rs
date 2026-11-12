//! Server identity and certificate pinning.
//!
//! The server makes a self-signed certificate when it starts and publishes its SHA-256 hash; the
//! lobby (or a dev console) hands that hash to clients. A client accepts only a server whose
//! certificate has exactly that hash, and rustls still checks that the server owns the key by
//! verifying the handshake signature with it.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

use crate::error::NetError;

/// SHA-256 of a server certificate (DER), written as 64 lowercase hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CertificateHash([u8; 32]);

impl CertificateHash {
    /// Hashes a DER certificate.
    #[must_use]
    pub fn of(certificate_der: &[u8]) -> Self {
        let digest = ring::digest::digest(&ring::digest::SHA256, certificate_der);
        let mut bytes = [0_u8; 32];
        bytes.copy_from_slice(digest.as_ref());
        Self(bytes)
    }
}

impl fmt::Display for CertificateHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for CertificateHash {
    type Err = NetError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || NetError::QuicSetup("certificate hash must be 64 hex digits".to_owned());
        if text.len() != 64 || !text.is_ascii() {
            return Err(invalid());
        }
        let mut bytes = [0_u8; 32];
        let (pairs, _) = text.as_bytes().as_chunks::<2>();
        for (byte, pair) in bytes.iter_mut().zip(pairs) {
            let pair = std::str::from_utf8(pair).map_err(|_| invalid())?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| invalid())?;
        }
        Ok(Self(bytes))
    }
}

/// A server's certificate and private key.
pub struct ServerIdentity {
    certificate: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
}

impl ServerIdentity {
    /// Generates a fresh self-signed certificate for `names` (DNS names or IP addresses the
    /// clients use; they are not checked by pinning clients, but are recorded in the certificate).
    ///
    /// # Errors
    /// Returns [`NetError::QuicSetup`] when key or certificate generation fails.
    pub fn generate(names: &[&str]) -> Result<Self, NetError> {
        let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        let certified = rcgen::generate_simple_self_signed(names)
            .map_err(|error| NetError::QuicSetup(format!("certificate: {error}")))?;
        Ok(Self {
            certificate: certified.cert.der().clone(),
            key: PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der()),
        })
    }

    /// The hash clients pin to trust this server.
    #[must_use]
    pub fn certificate_hash(&self) -> CertificateHash {
        CertificateHash::of(&self.certificate)
    }

    pub(super) fn into_parts(self) -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
        (self.certificate, PrivateKeyDer::Pkcs8(self.key))
    }
}

/// Accepts exactly the certificate with the pinned hash, and verifies handshake signatures with it.
#[derive(Debug)]
pub(super) struct PinnedServer {
    hash: CertificateHash,
    provider: Arc<CryptoProvider>,
}

impl PinnedServer {
    pub(super) fn new(hash: CertificateHash, provider: Arc<CryptoProvider>) -> Self {
        Self { hash, provider }
    }
}

impl ServerCertVerifier for PinnedServer {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if CertificateHash::of(end_entity) == self.hash {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            certificate,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}
