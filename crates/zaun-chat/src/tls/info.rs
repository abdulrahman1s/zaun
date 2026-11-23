//! What a finished handshake tells the layers above: ALPN, the peer chain and the channel binding.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsInfo {
    pub(super) alpn: Option<String>,
    pub(super) peer_cert_chain: Vec<Vec<u8>>,
    pub(super) tls_unique: [u8; 32],
    pub(super) certificate_requested: bool,
}

impl TlsInfo {
    /// The ALPN protocol the server chose, if any.
    #[must_use]
    pub fn alpn(&self) -> Option<&str> {
        self.alpn.as_deref()
    }

    /// DER certificates the server sent, leaf first.
    #[must_use]
    pub fn peer_cert_chain(&self) -> &[Vec<u8>] {
        &self.peer_cert_chain
    }

    /// The client Finished verify_data: SMP's `sessionId` (simplexmq Transport.hs `getTLS` `tlsUnique`).
    #[must_use]
    pub fn tls_unique(&self) -> &[u8; 32] {
        &self.tls_unique
    }

    /// Whether the server sent a CertificateRequest (we answered with an empty Certificate).
    #[must_use]
    pub fn certificate_requested(&self) -> bool {
        self.certificate_requested
    }
}
