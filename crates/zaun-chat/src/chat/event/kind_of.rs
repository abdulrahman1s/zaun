//! Which `EventKind` a `ChatEvent` is.
use super::kind::EventKind;
use super::types::ChatEvent;

impl ChatEvent {
    /// `None` for an `F` chunk and for a name 7.1.4 does not know.
    pub fn kind(&self) -> Option<EventKind> {
        Some(match self {
            Self::MsgNew(_) => EventKind::MsgNew,
            Self::MsgFileDescr { .. } => EventKind::MsgFileDescr,
            Self::MsgUpdate { .. } => EventKind::MsgUpdate,
            Self::MsgDel { .. } => EventKind::MsgDel,
            Self::MsgDeleted => EventKind::MsgDeleted,
            Self::MsgReact { .. } => EventKind::MsgReact,
            Self::File(_) => EventKind::File,
            Self::FileAcpt { .. } => EventKind::FileAcpt,
            Self::FileAcptInv { .. } => EventKind::FileAcptInv,
            Self::FileCancel { .. } => EventKind::FileCancel,
            Self::Info { .. } => EventKind::Info,
            Self::Contact { .. } => EventKind::Contact,
            Self::Member { .. } => EventKind::Member,
            Self::DirectDel => EventKind::DirectDel,
            Self::GrpInv(_) => EventKind::GrpInv,
            Self::GrpAcpt { .. } => EventKind::GrpAcpt,
            Self::GrpLinkInv(_) => EventKind::GrpLinkInv,
            Self::GrpLinkReject(_) => EventKind::GrpLinkReject,
            Self::GrpLinkMem { .. } => EventKind::GrpLinkMem,
            Self::GrpLinkAcpt { .. } => EventKind::GrpLinkAcpt,
            Self::GrpMemNew { .. } => EventKind::GrpMemNew,
            Self::GrpMemIntro { .. } => EventKind::GrpMemIntro,
            Self::GrpMemInv { .. } => EventKind::GrpMemInv,
            Self::GrpMemFwd { .. } => EventKind::GrpMemFwd,
            Self::GrpMemInfo { .. } => EventKind::GrpMemInfo,
            Self::GrpMemRole { .. } => EventKind::GrpMemRole,
            Self::GrpMemRestrict { .. } => EventKind::GrpMemRestrict,
            Self::GrpMemCon { .. } => EventKind::GrpMemCon,
            Self::GrpMemConAll { .. } => EventKind::GrpMemConAll,
            Self::GrpMemDel { .. } => EventKind::GrpMemDel,
            Self::GrpLeave => EventKind::GrpLeave,
            Self::GrpDel => EventKind::GrpDel,
            Self::GrpInfo(_) => EventKind::GrpInfo,
            Self::GrpPrefs(_) => EventKind::GrpPrefs,
            Self::GrpDirectInv { .. } => EventKind::GrpDirectInv,
            Self::GrpMsgForward { .. } => EventKind::GrpMsgForward,
            Self::InfoProbe(_) => EventKind::InfoProbe,
            Self::InfoProbeCheck(_) => EventKind::InfoProbeCheck,
            Self::InfoProbeOk(_) => EventKind::InfoProbeOk,
            Self::CallInv { .. } => EventKind::CallInv,
            Self::CallOffer { .. } => EventKind::CallOffer,
            Self::CallAnswer { .. } => EventKind::CallAnswer,
            Self::CallExtra { .. } => EventKind::CallExtra,
            Self::CallEnd { .. } => EventKind::CallEnd,
            Self::Ok => EventKind::Ok,
            Self::FileChunk { .. } => return None,
            Self::Unknown { event, .. } => return EventKind::from_name(event),
        })
    }
}
