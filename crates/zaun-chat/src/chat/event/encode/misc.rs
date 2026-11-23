//! Params of probe and call events.
use crate::chat::event::ChatEvent as E;
use crate::chat::json::Params;
use serde_json::{Map, Value};

pub(super) fn params(ev: &E) -> Map<String, Value> {
    let p = Params::new();
    match ev {
        E::InfoProbe(x) | E::InfoProbeOk(x) => p.put("probe", x),
        E::InfoProbeCheck(h) => p.put("probeHash", h),
        E::CallInv {
            call_id,
            invitation,
        } => p.put("callId", call_id).put("invitation", invitation),
        E::CallOffer { call_id, offer } => p.put("callId", call_id).put("offer", offer),
        E::CallAnswer { call_id, answer } => p.put("callId", call_id).put("answer", answer),
        E::CallExtra { call_id, extra } => p.put("callId", call_id).put("extra", extra),
        E::CallEnd { call_id } => p.put("callId", call_id),
        _ => p,
    }
    .done()
}
