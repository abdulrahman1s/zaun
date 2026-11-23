//! Event names (`CMEventTag` string forms), written once.

macro_rules! event_kinds {
    ($($var:ident = $name:literal),+ $(,)?) => {
        /// Every `x.*` event name 7.1.4 knows, including channel and roster ones that are kept as `Unknown`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum EventKind {
            $($var,)+
        }

        impl EventKind {
            pub const ALL: &'static [Self] = &[$(Self::$var,)+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$var => $name,)+
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$var),)+
                    _ => None,
                }
            }
        }
    };
}

event_kinds! {
    MsgNew = "x.msg.new",
    MsgFileDescr = "x.msg.file.descr",
    MsgUpdate = "x.msg.update",
    MsgDel = "x.msg.del",
    MsgDeleted = "x.msg.deleted",
    MsgReact = "x.msg.react",
    File = "x.file",
    FileAcpt = "x.file.acpt",
    FileAcptInv = "x.file.acpt.inv",
    FileCancel = "x.file.cancel",
    Info = "x.info",
    Contact = "x.contact",
    Member = "x.member",
    DirectDel = "x.direct.del",
    GrpInv = "x.grp.inv",
    GrpAcpt = "x.grp.acpt",
    GrpLinkInv = "x.grp.link.inv",
    GrpLinkReject = "x.grp.link.reject",
    GrpLinkMem = "x.grp.link.mem",
    GrpLinkAcpt = "x.grp.link.acpt",
    GrpRelayInv = "x.grp.relay.inv",
    GrpRelayAcpt = "x.grp.relay.acpt",
    GrpRelayTest = "x.grp.relay.test",
    GrpRelayNew = "x.grp.relay.new",
    GrpRelayReject = "x.grp.relay.reject",
    GrpRelayCap = "x.grp.relay.cap",
    GrpMemNew = "x.grp.mem.new",
    GrpMemIntro = "x.grp.mem.intro",
    GrpMemInv = "x.grp.mem.inv",
    GrpMemFwd = "x.grp.mem.fwd",
    GrpMemInfo = "x.grp.mem.info",
    GrpMemRole = "x.grp.mem.role",
    GrpMemRestrict = "x.grp.mem.restrict",
    GrpMemCon = "x.grp.mem.con",
    GrpMemConAll = "x.grp.mem.con.all",
    GrpMemDel = "x.grp.mem.del",
    GrpLeave = "x.grp.leave",
    GrpDel = "x.grp.del",
    GrpInfo = "x.grp.info",
    GrpPrefs = "x.grp.prefs",
    GrpDirectInv = "x.grp.direct.inv",
    GrpRoster = "x.grp.roster",
    GrpRosterAck = "x.grp.roster.ack",
    GrpRosterRequest = "x.grp.roster.request",
    GrpMsgForward = "x.grp.msg.forward",
    InfoProbe = "x.info.probe",
    InfoProbeCheck = "x.info.probe.check",
    InfoProbeOk = "x.info.probe.ok",
    CallInv = "x.call.inv",
    CallOffer = "x.call.offer",
    CallAnswer = "x.call.answer",
    CallExtra = "x.call.extra",
    CallEnd = "x.call.end",
    Ok = "x.ok",
}
