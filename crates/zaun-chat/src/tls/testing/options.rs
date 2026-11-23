//! What the mock TLS server should do wrong, and what it saw.
use crate::crypto::Alg;

pub(in crate::tls) struct MockOpts {
    pub key: Alg,
    pub alpn: Option<&'static str>,
    pub bad_finished: bool,
    pub bad_sig: bool,
    pub empty_chain: bool,
    pub hrr: bool,
    pub cert_request: bool,
    pub compat_ccs: bool,
    pub coalesce: bool,
    pub fragment: usize,
    pub corrupt_handshake_record: bool,
    pub alert_in_handshake: bool,
    pub key_update: bool,
    pub send_data: Vec<u8>,
    pub corrupt_app_record: bool,
    pub close_notify_after_data: bool,
    pub echo: bool,
}

impl Default for MockOpts {
    fn default() -> Self {
        Self {
            key: Alg::Ed25519,
            alpn: Some("smp/1"),
            bad_finished: false,
            bad_sig: false,
            empty_chain: false,
            hrr: false,
            cert_request: false,
            compat_ccs: false,
            coalesce: false,
            fragment: 0,
            corrupt_handshake_record: false,
            alert_in_handshake: false,
            key_update: false,
            send_data: Vec::new(),
            corrupt_app_record: false,
            close_notify_after_data: false,
            echo: false,
        }
    }
}

#[derive(Default)]
pub(in crate::tls) struct MockResult {
    pub client_finished: Vec<u8>,
    pub client_cert_empty: bool,
    pub received: Vec<u8>,
    pub error: Option<String>,
}
