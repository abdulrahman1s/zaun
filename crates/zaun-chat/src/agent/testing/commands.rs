//! What the in-memory server does with each command, after checking its authorisation as a real server would.
use super::parse::{Cmd, NewLink, parse};
use super::queues::{Push, deliver, new_queue};
use super::server::Inner;
use crate::crypto::{Alg, CbNonce, PublicKey, Signature, cb_verify, random_vec};
use crate::smp::{ErrorType, Incoming, QueueIds, QueueMode, Response, authorized};

/// The command's authorisation by `key`: an Ed25519 signature, or the X25519 box under the server key.
struct Auth<'a> {
    t: &'a Incoming,
    msg: Vec<u8>,
    server: &'a crate::crypto::PrivateKey,
}

impl Auth<'_> {
    fn by(&self, key: &PublicKey) -> bool {
        match key.alg() {
            Alg::Ed25519 => Signature::from_raw(Alg::Ed25519, &self.t.auth)
                .is_ok_and(|s| key.verify(&s, &self.msg)),
            Alg::X25519 => CbNonce::from_slice(&self.t.corr_id)
                .is_ok_and(|n| cb_verify(key, self.server, &n, &self.t.auth, &self.msg)),
            Alg::Ed448 | Alg::X448 => false,
        }
    }
}

impl Inner {
    pub fn handle(&mut self, v: u16, sid: u64, t: &Incoming, pushes: &mut Vec<Push>) -> Response {
        let Ok(cmd) = parse(v, &t.command) else {
            return Response::Err(ErrorType::Block);
        };
        let session = self
            .sessions
            .get(&sid)
            .map(|s| s.session_id.clone())
            .unwrap_or_default();
        let server = self.server_key.private.clone();
        let auth = Auth {
            t,
            msg: authorized(&session, &t.corr_id, &t.entity_id, &t.command).unwrap(),
            server: &server,
        };
        let e = &t.entity_id;
        match cmd {
            Cmd::New {
                rcv_auth,
                rcv_dh,
                contact,
                link,
            } => {
                self.log.push("NEW");
                if !auth.by(&rcv_auth) {
                    return Response::Err(ErrorType::Auth);
                }
                self.create(sid, (rcv_auth, rcv_dh), contact, link)
            }
            Cmd::Skey(k) => {
                self.log.push("SKEY");
                if !auth.by(&k) {
                    return Response::Err(ErrorType::Auth);
                }
                self.queues.secure(e, k)
            }
            Cmd::Lkey(k) => {
                self.log.push("LKEY");
                if !auth.by(&k) {
                    return Response::Err(ErrorType::Auth);
                }
                self.lkey(e, k)
            }
            Cmd::Lget => {
                self.log.push("LGET");
                self.lget(e)
            }
            Cmd::Send(body) => {
                self.log.push("SEND");
                let q = self
                    .queues
                    .by_snd
                    .get(e)
                    .and_then(|r| self.queues.by_rcv.get(r));
                // An address takes unsigned messages; any other queue only those from its sender key.
                let allowed = match q {
                    Some(q) if q.contact => t.auth.is_empty(),
                    Some(q) => q.sender_key.as_ref().is_some_and(|k| auth.by(k)),
                    None => false,
                };
                if !allowed {
                    return Response::Err(ErrorType::Auth);
                }
                self.queues.send(e, body, pushes)
            }
            Cmd::Sub | Cmd::Ack(_) | Cmd::Del => {
                let key = self.queues.by_rcv.get(e).map(|q| q.rcv_auth.clone());
                if key.is_none_or(|k| !auth.by(&k)) {
                    return Response::Err(ErrorType::Auth);
                }
                self.recipient(cmd, sid, e, pushes)
            }
            Cmd::Ping => Response::Pong,
        }
    }

    fn create(
        &mut self,
        sid: u64,
        (rcv_auth, rcv_dh): (PublicKey, PublicKey),
        contact: bool,
        link: Option<NewLink>,
    ) -> Response {
        let snd_id = link
            .as_ref()
            .map_or_else(|| random_vec(24).unwrap(), |l| l.sender_id.clone());
        let mut q = new_queue(rcv_auth, rcv_dh, snd_id.clone());
        q.subscriber = Some(sid);
        q.contact = contact;
        let link_id = link
            .as_ref()
            .map(|l| l.link_id.clone().unwrap_or_else(|| random_vec(24).unwrap()));
        if let (Some(id), Some(l)) = (&link_id, link) {
            q.link = Some((id.clone(), l.data));
            self.queues.by_link.insert(id.clone(), q.rcv_id.clone());
        }
        let ids = QueueIds {
            rcv_id: q.rcv_id.clone(),
            snd_id: snd_id.clone(),
            rcv_dh_key: q.server_dh.public.clone(),
            queue_mode: Some(if contact {
                QueueMode::Contact
            } else {
                QueueMode::Messaging
            }),
            link_id,
            service_id: None,
            ntf_creds: None,
        };
        self.queues.by_snd.insert(snd_id, q.rcv_id.clone());
        self.queues.by_rcv.insert(q.rcv_id.clone(), q);
        Response::Ids(ids)
    }

    /// LGET returns an address's link data and secures nothing.
    fn lget(&self, link_id: &[u8]) -> Response {
        let q = self
            .queues
            .by_link
            .get(link_id)
            .and_then(|r| self.queues.by_rcv.get(r));
        match q {
            Some(q) if q.contact => {
                q.link
                    .clone()
                    .map_or(Response::Err(ErrorType::Auth), |(_, data)| Response::Lnk {
                        sender_id: q.snd_id.clone(),
                        data,
                    })
            }
            _ => Response::Err(ErrorType::Auth),
        }
    }

    /// LKEY secures the queue a one-time link names and returns its data.
    fn lkey(&mut self, link_id: &[u8], key: PublicKey) -> Response {
        let Some(q) = self
            .queues
            .by_link
            .get(link_id)
            .and_then(|r| self.queues.by_rcv.get(r))
        else {
            return Response::Err(ErrorType::Auth);
        };
        let Some((_, data)) = q.link.clone() else {
            return Response::Err(ErrorType::Auth);
        };
        let sender_id = q.snd_id.clone();
        match self.queues.secure(&sender_id, key) {
            Response::Ok => Response::Lnk { sender_id, data },
            other => other,
        }
    }

    fn recipient(&mut self, cmd: Cmd, sid: u64, rcv_id: &[u8], pushes: &mut Vec<Push>) -> Response {
        match cmd {
            Cmd::Sub => {
                self.log.push("SUB");
                let Some(q) = self.queues.by_rcv.get_mut(rcv_id) else {
                    return Response::Err(ErrorType::Auth);
                };
                q.subscriber = Some(sid);
                q.delivered = None;
                deliver(q, pushes);
                Response::Ok
            }
            Cmd::Ack(msg_id) => {
                self.log.push("ACK");
                self.queues.ack(rcv_id, &msg_id, pushes)
            }
            Cmd::Del => {
                self.log.push("DEL");
                if let Some(q) = self.queues.by_rcv.remove(rcv_id) {
                    self.queues.by_snd.remove(&q.snd_id);
                    if let Some((link_id, _)) = &q.link {
                        self.queues.by_link.remove(link_id);
                    }
                }
                Response::Ok
            }
            Cmd::New { .. }
            | Cmd::Skey(_)
            | Cmd::Lkey(_)
            | Cmd::Lget
            | Cmd::Send(_)
            | Cmd::Ping => Response::Err(ErrorType::Block),
        }
    }
}
