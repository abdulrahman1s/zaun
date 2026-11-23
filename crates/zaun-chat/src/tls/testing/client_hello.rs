//! The mock server's reading of a ClientHello: the x25519 key share, and a refusal of SNI.
use crate::tls::error::TlsError;
use crate::tls::wire::{Reader, extensions};

pub(super) fn client_key_share(message: &[u8]) -> Result<[u8; 32], TlsError> {
    let mut r = Reader::new(&message[4..]);
    r.take(2 + 32)?;
    r.vec8()?;
    r.vec16()?;
    r.vec8()?;
    let mut key = None;
    for (ty, data) in extensions(r.vec16()?)? {
        match ty {
            0 => return Err(TlsError::Illegal("client sent SNI")),
            51 => {
                let mut shares = Reader::new(Reader::new(data).vec16()?);
                shares.u16()?;
                key = shares.vec16()?.try_into().ok();
            }
            _ => {}
        }
    }
    key.ok_or(TlsError::Protocol("no x25519 key share"))
}
