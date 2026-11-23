//! What a kind implies when sending (Protocol.hs `hasNotification`, `hasDeliveryReceipt`, `requiresSignature`, `isForwardedGroupMsg`).
use super::kind::EventKind;
use super::types::ChatEvent;

impl EventKind {
    pub const fn has_notification(self) -> bool {
        matches!(
            self,
            Self::MsgNew
                | Self::File
                | Self::Contact
                | Self::GrpInv
                | Self::GrpMemFwd
                | Self::GrpDel
                | Self::CallInv
        )
    }

    pub const fn has_delivery_receipt(self) -> bool {
        matches!(self, Self::MsgNew | Self::GrpInv | Self::CallInv)
    }

    /// Events 7.1.4 signs for group peers at chat version 18 and above.
    pub const fn requires_signature(self) -> bool {
        matches!(
            self,
            Self::GrpDel
                | Self::GrpInfo
                | Self::GrpPrefs
                | Self::GrpMemDel
                | Self::GrpMemRole
                | Self::GrpMemRestrict
                | Self::GrpLeave
                | Self::GrpRelayNew
                | Self::GrpRoster
                | Self::Info
                | Self::GrpLinkMem
                | Self::GrpRelayInv
        )
    }

    /// Content events a member may sign (opt-in for `x.msg.new`).
    pub const fn signable_content(self) -> bool {
        matches!(self, Self::MsgNew | Self::MsgUpdate | Self::MsgDel)
    }

    /// Whether an admin forwards such an event to members who are not yet connected to its author.
    pub const fn is_forwarded_group_msg(self) -> bool {
        matches!(
            self,
            Self::MsgNew
                | Self::MsgFileDescr
                | Self::MsgUpdate
                | Self::MsgDel
                | Self::MsgReact
                | Self::FileCancel
                | Self::Info
                | Self::GrpRelayNew
                | Self::GrpMemNew
                | Self::GrpMemRole
                | Self::GrpMemRestrict
                | Self::GrpMemDel
                | Self::GrpLeave
                | Self::GrpDel
                | Self::GrpInfo
                | Self::GrpPrefs
                | Self::GrpRoster
        )
    }
}

impl ChatEvent {
    /// `isForwardedGroupMsg`: as the kind says, except an `x.msg.new` with an inline file.
    pub fn is_forwarded_group_msg(&self) -> bool {
        match self {
            Self::MsgNew(c) if c.file.as_ref().is_some_and(|f| f.file_inline.is_some()) => false,
            other => other.kind().is_some_and(EventKind::is_forwarded_group_msg),
        }
    }
}
