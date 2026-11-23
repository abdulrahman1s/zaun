//! The four key algorithms, their sizes and OID bytes (Crypto.hs `SignatureAlgorithm`, `DhAlgorithm`; RFC 8410).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alg {
    Ed25519,
    Ed448,
    X25519,
    X448,
}

impl Alg {
    pub const ALL: [Alg; 4] = [Alg::Ed25519, Alg::Ed448, Alg::X25519, Alg::X448];

    /// Raw public key length.
    #[must_use]
    pub const fn public_len(self) -> usize {
        match self {
            Alg::Ed25519 | Alg::X25519 => 32,
            Alg::Ed448 => 57,
            Alg::X448 => 56,
        }
    }

    /// Raw private key length (seed or scalar).
    #[must_use]
    pub const fn private_len(self) -> usize {
        self.public_len()
    }

    #[must_use]
    pub const fn is_signing(self) -> bool {
        matches!(self, Alg::Ed25519 | Alg::Ed448)
    }

    /// The last byte of OID 1.3.101.x.
    pub(super) const fn oid(self) -> u8 {
        match self {
            Alg::X25519 => 0x6e,
            Alg::X448 => 0x6f,
            Alg::Ed25519 => 0x70,
            Alg::Ed448 => 0x71,
        }
    }

    pub(super) fn from_oid(b: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.oid() == b)
    }
}
