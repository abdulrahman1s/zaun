//! Decoding of probe and call events.
use super::Obj;
use crate::chat::error::Result;
use crate::chat::event::{ChatEvent as E, EventKind as K};
use crate::chat::json::req;

pub(super) fn decode_probe(kind: K, o: &Obj) -> Result<E> {
    Ok(match kind {
        K::InfoProbe => E::InfoProbe(req(o, "probe")?),
        K::InfoProbeCheck => E::InfoProbeCheck(req(o, "probeHash")?),
        _ => E::InfoProbeOk(req(o, "probe")?),
    })
}

pub(super) fn decode_call(kind: K, o: &Obj) -> Result<E> {
    let call_id = req(o, "callId")?;
    Ok(match kind {
        K::CallInv => E::CallInv {
            call_id,
            invitation: req(o, "invitation")?,
        },
        K::CallOffer => E::CallOffer {
            call_id,
            offer: req(o, "offer")?,
        },
        K::CallAnswer => E::CallAnswer {
            call_id,
            answer: req(o, "answer")?,
        },
        K::CallExtra => E::CallExtra {
            call_id,
            extra: req(o, "extra")?,
        },
        _ => E::CallEnd { call_id },
    })
}
