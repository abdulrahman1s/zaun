//! The mock server after the handshake: KeyUpdate, canned data, close_notify, echo.
use std::io::Write;

use super::options::{MockOpts, MockResult};
use super::pipe::Pipe;
use crate::tls::error::TlsError;
use crate::tls::key_schedule::next_traffic_secret;
use crate::tls::record::{ALERT, APPLICATION_DATA, HANDSHAKE, Inbound, Protection};
use crate::tls::wire::handshake_message;

pub(super) fn app_phase(
    sock: &mut Pipe,
    mut inbound: Inbound,
    o: &MockOpts,
    res: &mut MockResult,
    server_ap: [u8; 32],
    client_ap: [u8; 32],
) -> Result<(), TlsError> {
    let (mut tx, mut rx) = (Protection::new(&server_ap), Protection::new(&client_ap));
    let mut client_secret = client_ap;
    if o.key_update {
        sock.write_all(&tx.seal(HANDSHAKE, &handshake_message(24, &[1]))?)?;
        tx = Protection::new(&next_traffic_secret(&server_ap));
    }
    if !o.send_data.is_empty() {
        let mut rec = tx.seal(APPLICATION_DATA, &o.send_data)?;
        if o.corrupt_app_record {
            rec[10] ^= 1;
        }
        sock.write_all(&rec)?;
    }
    if o.close_notify_after_data {
        sock.write_all(&tx.seal(ALERT, &[1, 0])?)?;
        return Ok(());
    }
    if !o.echo {
        return Ok(());
    }
    loop {
        let r = inbound.next_record(sock)?;
        match rx.open(&r.header, &r.body)? {
            (APPLICATION_DATA, data) => {
                res.received.extend_from_slice(&data);
                sock.write_all(&tx.seal(APPLICATION_DATA, &data)?)?;
            }
            (HANDSHAKE, m) if m.first() == Some(&24) => {
                client_secret = next_traffic_secret(&client_secret);
                rx = Protection::new(&client_secret);
            }
            (ALERT, _) => return Ok(()),
            _ => return Err(TlsError::Protocol("unexpected client record")),
        }
    }
}
