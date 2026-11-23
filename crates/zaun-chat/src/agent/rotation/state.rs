//! A switch in progress, as stored with its connection (Agent/Protocol.hs `SndSwitchStatus`; Agent/Store.hs `SndQueue.sndSwchStatus`, `dbReplaceQueueId`).
use crate::agent::QueueMode;
use crate::agent::client::AgentMsgId;
use crate::agent::records::SndQueue;

/// What a switch does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// v8: SKEY on the new queue (`SSSecuringQueue`, `ICQSndSecure`).
    Securing,
    /// v8: the empty confirmation that gives the peer our new e2e key.
    Confirming,
    /// v7: QKEY is to be queued on the current queue.
    Qkey,
    /// v7: QKEY is queued; the peer answers QUSE once KEY has secured the new queue (`SSSendingQKEY`).
    WaitingQuse,
    /// v7: QUSE came; QTEST is to be built.
    Qtest,
    /// v7: QTEST, sealed for the new queue, goes there alone; its success proves the queue (`SSSendingQTEST`).
    Testing { body: Vec<u8> },
    /// The new queue takes messages: what waits moves to it once no send is in flight.
    Ready,
}

/// The peer moves the queue we send to; `rec.snd` is still the old one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moving {
    pub new: SndQueue,
    /// The new queue's mode as QADD gave it, for the address in QKEY.
    pub mode: Option<QueueMode>,
    pub step: Step,
    /// Messages after this one that go on the old queue are kept for the new one too, as 7.1.4 delivers
    /// to both: at v8 all from QADD (`copyPendingSndDeliveries`), at v7 those after QTEST (`isActiveSndQ`).
    pub keep_after: Option<AgentMsgId>,
    /// The last message kept. Those up to it that are still stored went on the old queue already, and
    /// go again, quiet, on the new one after the move.
    pub kept: Option<AgentMsgId>,
    /// Wall-clock ms of QADD: a switch that has not moved by `messageTimeout` is given up.
    pub started_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Switch {
    Moving(Moving),
    /// v8, after the move: QEND on the old queue as well, sent only when the old queue carried every
    /// message before it, so that it opens no gap in the peer's chain (`SSSendingQEND`). It ends when
    /// either copy of QEND is sent (`last` is the one on the new queue), or at `messageTimeout`.
    EndingOld {
        old: SndQueue,
        body: Vec<u8>,
        last: AgentMsgId,
        started_at: i64,
    },
}

impl Switch {
    /// The queue the switch's own requests go to.
    #[must_use]
    pub fn queue(&self) -> &SndQueue {
        match self {
            Self::Moving(m) => &m.new,
            Self::EndingOld { old, .. } => old,
        }
    }

    #[must_use]
    pub fn started_at(&self) -> i64 {
        match self {
            Self::Moving(m) => m.started_at,
            Self::EndingOld { started_at, .. } => *started_at,
        }
    }
}
