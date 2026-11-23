//! Parsing the server's handshake messages: ServerHello through CertificateVerify (RFC 8446 section 4).
use super::super::error::TlsError;
use super::super::wire::{Reader, extensions};
use super::client_hello::{
    EXT_ALPN, EXT_KEY_SHARE, EXT_SUPPORTED_VERSIONS, GROUP_X25519, SUITE_CHACHA20_POLY1305,
};

pub(super) const SERVER_HELLO: u8 = 2;
pub(super) const ENCRYPTED_EXTENSIONS: u8 = 8;
pub(super) const CERTIFICATE: u8 = 11;
pub(super) const CERTIFICATE_REQUEST: u8 = 13;
pub(super) const CERTIFICATE_VERIFY: u8 = 15;
pub(super) const FINISHED: u8 = 20;

const HELLO_RETRY_REQUEST_RANDOM: [u8; 32] = [
    0xcf, 0x21, 0xad, 0x74, 0xe5, 0x9a, 0x61, 0x11, 0xbe, 0x1d, 0x8c, 0x02, 0x1e, 0x65, 0xb8, 0x91,
    0xc2, 0xa2, 0x11, 0x16, 0x7a, 0xbb, 0x8c, 0x5e, 0x07, 0x9e, 0x09, 0xe2, 0xc8, 0xa8, 0x33, 0x9c,
];

/// The server's x25519 public key from a ServerHello body.
pub(super) fn parse_server_hello(body: &[u8]) -> Result<[u8; 32], TlsError> {
    let mut r = Reader::new(body);
    if r.u16()? != 0x0303 {
        return Err(TlsError::Illegal("legacy version"));
    }
    if r.take(32)? == HELLO_RETRY_REQUEST_RANDOM {
        return Err(TlsError::HelloRetryRequest);
    }
    if !r.vec8()?.is_empty() {
        return Err(TlsError::Illegal("session id echo"));
    }
    if r.u16()? != SUITE_CHACHA20_POLY1305 {
        return Err(TlsError::Illegal("cipher suite"));
    }
    if r.u8()? != 0 {
        return Err(TlsError::Illegal("compression"));
    }
    let mut key = None;
    for (ty, data) in extensions(r.vec16()?)? {
        match ty {
            EXT_SUPPORTED_VERSIONS if data == [3, 4] => {}
            EXT_SUPPORTED_VERSIONS => return Err(TlsError::Illegal("not TLS 1.3")),
            EXT_KEY_SHARE => key = Some(parse_key_share(data)?),
            _ => return Err(TlsError::Unsupported("ServerHello extension")),
        }
    }
    r.finish()?;
    key.ok_or(TlsError::Protocol("ServerHello without key_share"))
}

fn parse_key_share(data: &[u8]) -> Result<[u8; 32], TlsError> {
    let mut r = Reader::new(data);
    if r.u16()? != GROUP_X25519 {
        return Err(TlsError::Illegal("key share group"));
    }
    let key = r
        .vec16()?
        .try_into()
        .map_err(|_| TlsError::Illegal("key share length"))?;
    r.finish()?;
    Ok(key)
}

/// The ALPN protocol the server chose, which must be one we offered.
pub(super) fn parse_encrypted_extensions(
    body: &[u8],
    offered: &[&str],
) -> Result<Option<String>, TlsError> {
    let mut r = Reader::new(body);
    let exts = extensions(r.vec16()?)?;
    r.finish()?;
    let Some((_, data)) = exts.into_iter().find(|(ty, _)| *ty == EXT_ALPN) else {
        return Ok(None);
    };
    let mut list = Reader::new(data);
    let mut names = Reader::new(list.vec16()?);
    let name = std::str::from_utf8(names.vec8()?)
        .map_err(|_| TlsError::Decode("ALPN name"))?
        .to_owned();
    names.finish()?;
    list.finish()?;
    offered
        .contains(&name.as_str())
        .then_some(Some(name))
        .ok_or(TlsError::Alpn)
}

/// The request context to echo in our (empty) Certificate.
pub(super) fn parse_certificate_request(body: &[u8]) -> Result<Vec<u8>, TlsError> {
    Reader::new(body).vec8().map(<[u8]>::to_vec)
}

/// The DER certificates of the server's Certificate message, leaf first.
pub(super) fn parse_certificate(body: &[u8]) -> Result<Vec<Vec<u8>>, TlsError> {
    let mut r = Reader::new(body);
    if !r.vec8()?.is_empty() {
        return Err(TlsError::Illegal("certificate request context"));
    }
    let mut list = Reader::new(r.vec24()?);
    r.finish()?;
    let mut chain = Vec::new();
    while !list.is_empty() {
        chain.push(list.vec24()?.to_vec());
        list.vec16()?; // per-certificate extensions are ignored
    }
    if chain.is_empty() {
        return Err(TlsError::EmptyChain);
    }
    Ok(chain)
}

/// The signature scheme and signature of a CertificateVerify.
pub(super) fn parse_certificate_verify(body: &[u8]) -> Result<(u16, &[u8]), TlsError> {
    let mut r = Reader::new(body);
    let scheme = r.u16()?;
    let signature = r.vec16()?;
    r.finish()?;
    Ok((scheme, signature))
}
