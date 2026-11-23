//! TLS alerts (RFC 8446 section 6): decoding and names for error messages.
use std::fmt;

use super::error::TlsError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alert {
    pub level: u8,
    pub description: u8,
}

impl Alert {
    pub(super) const CLOSE_NOTIFY: Alert = Alert {
        level: 1,
        description: 0,
    };

    pub(super) fn parse(body: &[u8]) -> Result<Self, TlsError> {
        match body {
            [level, description] => Ok(Alert {
                level: *level,
                description: *description,
            }),
            _ => Err(TlsError::Decode("alert length")),
        }
    }

    pub(super) fn bytes(self) -> [u8; 2] {
        [self.level, self.description]
    }

    pub(super) fn is_close_notify(self) -> bool {
        self.description == 0
    }

    fn name(self) -> &'static str {
        match self.description {
            0 => "close_notify",
            10 => "unexpected_message",
            20 => "bad_record_mac",
            22 => "record_overflow",
            40 => "handshake_failure",
            42 => "bad_certificate",
            43 => "unsupported_certificate",
            44 => "certificate_revoked",
            45 => "certificate_expired",
            46 => "certificate_unknown",
            47 => "illegal_parameter",
            48 => "unknown_ca",
            49 => "access_denied",
            50 => "decode_error",
            51 => "decrypt_error",
            70 => "protocol_version",
            71 => "insufficient_security",
            80 => "internal_error",
            86 => "inappropriate_fallback",
            90 => "user_canceled",
            109 => "missing_extension",
            110 => "unsupported_extension",
            112 => "unrecognized_name",
            116 => "certificate_required",
            120 => "no_application_protocol",
            _ => "unknown",
        }
    }
}

impl fmt::Display for Alert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = if self.level == 2 { "fatal" } else { "warning" };
        write!(f, "{} ({level}, {})", self.name(), self.description)
    }
}
