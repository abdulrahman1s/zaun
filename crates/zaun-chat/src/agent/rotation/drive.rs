//! A switch's next step from its stored state, so that a received message, an answer, a tick, `Up` and a restart all resume it (Agent.hs `ICQSndSecure`, `qUseMsg`).
use crate::encoding::{Encode, Version};
use crate::smp::{Command, Request};
use crate::store::Store;

use super::addr::switch_tag;
use super::state::{Moving, Step, Switch};
use crate::agent::client::{AgentClient, AgentEvent, ConnId, OpKind, ms};
use crate::agent::delivery::seal;
use crate::agent::envelope::AgentMsgEnvelope;
use crate::agent::error::{AgentError, Result};
use crate::agent::records::SndQueue;

impl AgentClient {
    /// Called first by `kick`.
    pub(crate) fn drive_switch(&mut self, store: &mut Store, id: ConnId) {
        #[cfg(test)]
        if self.hold_switch {
            return;
        }
        if let Err(err) = self.switch_step(store, id) {
            self.emit(AgentEvent::Err { conn: id, err });
        }
    }

    fn switch_step(&mut self, store: &mut Store, id: ConnId) -> Result<()> {
        let now = self.now();
        let Some(c) = self.conns.get(&id) else {
            return Ok(());
        };
        let Some(sw) = c.rec.switch.clone() else {
            return Ok(());
        };
        let idle = !c.step.in_flight && c.step.retry_at <= now;
        let (sending, version) = (c.send.in_flight, c.rec.agent_version);
        if now - sw.started_at() > ms(self.cfg.delivery.message_timeout) {
            return self.give_up(store, id, AgentError::Expired);
        }
        let m = match sw {
            Switch::EndingOld { old, body, .. } => {
                if idle {
                    self.switch_request(id, &old, send(&old, body, false));
                }
                return Ok(());
            }
            Switch::Moving(m) => m,
        };
        self.skip_kept(id, &m);
        match &m.step {
            Step::Securing if idle => {
                let key = m.new.auth.public()?;
                let skey = Request::new(
                    Command::Skey(key),
                    m.new.snd_id.clone(),
                    Some(m.new.auth.clone()),
                );
                self.switch_request(id, &m.new, skey);
            }
            Step::Confirming if idle => {
                let conf = confirmation(version, &m.new)?;
                self.switch_request(id, &m.new, conf);
            }
            Step::Testing { body } if idle => {
                let qtest = send(&m.new, body.clone(), false);
                self.switch_request(id, &m.new, qtest);
            }
            Step::Qkey => self.queue_qkey(store, id, m)?,
            Step::Qtest => {
                self.build_qtest(store, id, m)?;
                return self.switch_step(store, id);
            }
            Step::Ready if !sending => self.move_to_new(store, id, m)?,
            Step::Securing
            | Step::Confirming
            | Step::WaitingQuse
            | Step::Testing { .. }
            | Step::Ready => {}
        }
        Ok(())
    }

    fn switch_request(&mut self, id: ConnId, sq: &SndQueue, request: Request) {
        let sent = self.request(id, OpKind::Switch(switch_tag(sq)), &sq.server, request);
        let retry_at = self.step_retry_after();
        if let Ok(c) = self.conn_mut(id) {
            c.step.in_flight = sent;
            c.step.retry_at = if sent { 0 } else { retry_at };
        }
    }

    /// After a restart the stored messages that went on the old queue head the list; they wait for the move.
    fn skip_kept(&mut self, id: ConnId, m: &Moving) {
        let (Some(kept), Ok(c)) = (m.kept, self.conn_mut(id)) else {
            return;
        };
        while c.sends.front().is_some_and(|&head| head <= kept) {
            c.sends.pop_front();
        }
    }
}

/// `AgentConfirmation` with no ratchet keys and no info: its header carries our new e2e key.
fn confirmation(agent_version: Version, sq: &SndQueue) -> Result<Request> {
    let env = AgentMsgEnvelope::Confirmation {
        agent_version,
        e2e_encryption: None,
        enc_conn_info: Vec::new(),
    };
    Ok(send(sq, seal(sq, &env.to_bytes()?, true)?, true))
}

pub(super) fn send(sq: &SndQueue, body: Vec<u8>, notification: bool) -> Request {
    let cmd = Command::Send { notification, body };
    Request::new(cmd, sq.snd_id.clone(), Some(sq.auth.clone()))
}
