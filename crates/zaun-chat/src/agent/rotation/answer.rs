//! What the answer to a switch's own request leads to: SKEY, the confirmation, QTEST, QEND on the old queue (Agent.hs `ICQSndSecure`, the `AM_QTEST_` and `AM_QEND_` outcomes).
use crate::smp::{Response, SmpError};
use crate::store::Store;

use super::addr::switch_tag;
use super::state::{Moving, Step, Switch};
use crate::agent::client::{AgentClient, ConnId, is_temporary};
use crate::agent::error::Result;
use crate::agent::records::Batch;

impl AgentClient {
    pub(crate) fn on_switch_answer(
        &mut self,
        store: &mut Store,
        id: ConnId,
        tag: u64,
        answer: Result<Response, SmpError>,
    ) -> Result<()> {
        let retry_at = self.step_retry_after();
        let c = self.conn_mut(id)?;
        c.step.in_flight = false;
        // The answer for a switch a later QADD replaced is not this one's.
        let Some(sw) = c
            .rec
            .switch
            .clone()
            .filter(|s| switch_tag(s.queue()) == tag)
        else {
            self.kick(store, id);
            return Ok(());
        };
        match answer {
            Err(e) if is_temporary(&e) => {
                c.step.retry_at = retry_at;
                return Ok(());
            }
            Err(e) => self.switch_refused(store, id, sw, e.into())?,
            Ok(_) => self.switch_answered(store, id, sw)?,
        }
        self.kick(store, id);
        Ok(())
    }

    fn switch_answered(&mut self, store: &mut Store, id: ConnId, sw: Switch) -> Result<()> {
        let next = match sw {
            Switch::Moving(mut m) => match m.step {
                Step::Securing => {
                    m.new.secured = true;
                    Some(Switch::Moving(Moving {
                        step: Step::Confirming,
                        ..m
                    }))
                }
                // `AM_QTEST_`: the new queue takes our messages, so it becomes the one we send to.
                Step::Confirming | Step::Testing { .. } => Some(Switch::Moving(Moving {
                    step: Step::Ready,
                    ..m
                })),
                Step::Qkey | Step::WaitingQuse | Step::Qtest | Step::Ready => return Ok(()),
            },
            Switch::EndingOld { .. } => None,
        };
        let (mut rec, _) = self.editable(id)?;
        rec.switch = next;
        self.save(store, rec, None, Batch::default())
    }

    /// AUTH and other refusals: the new queue is unusable (`qError "QTEST: AUTH"`), or QEND on the old
    /// one is dropped silently.
    fn switch_refused(
        &mut self,
        store: &mut Store,
        id: ConnId,
        sw: Switch,
        err: crate::agent::AgentError,
    ) -> Result<()> {
        match sw {
            Switch::Moving(m) => match m.step {
                Step::Securing | Step::Confirming | Step::Testing { .. } => {
                    self.give_up(store, id, err)
                }
                Step::Qkey | Step::WaitingQuse | Step::Qtest | Step::Ready => Ok(()),
            },
            Switch::EndingOld { .. } => self.give_up(store, id, err),
        }
    }
}
