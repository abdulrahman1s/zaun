//! The stored kind of an agent message (Agent/Protocol.hs `AgentMessageType`, AGPL-3.0-only).
use super::agent_message::AgentMessage;
use super::amessage::AMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentMessageType {
    ConnInfo,
    ConnInfoReply,
    RatchetInfo,
    Hello,
    AMsg,
    ARcvd,
    Qcont,
    Qadd,
    Qkey,
    Quse,
    Qtest,
    Qend,
    Eready,
    SrvReq,
    SrvResp,
    Rjct,
}

impl AgentMessageType {
    /// The tag that is stored in the database.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::ConnInfo => "C",
            Self::ConnInfoReply => "D",
            Self::RatchetInfo => "S",
            Self::Hello => "H",
            Self::AMsg => "M",
            Self::ARcvd => "V",
            Self::Qcont => "QC",
            Self::Qadd => "QA",
            Self::Qkey => "QK",
            Self::Quse => "QU",
            Self::Qtest => "QT",
            Self::Qend => "QE",
            Self::Eready => "E",
            Self::SrvReq => "A",
            Self::SrvResp => "P",
            Self::Rjct => "J",
        }
    }

    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        const ALL: [AgentMessageType; 16] = [
            AgentMessageType::ConnInfo,
            AgentMessageType::ConnInfoReply,
            AgentMessageType::RatchetInfo,
            AgentMessageType::Hello,
            AgentMessageType::AMsg,
            AgentMessageType::ARcvd,
            AgentMessageType::Qcont,
            AgentMessageType::Qadd,
            AgentMessageType::Qkey,
            AgentMessageType::Quse,
            AgentMessageType::Qtest,
            AgentMessageType::Qend,
            AgentMessageType::Eready,
            AgentMessageType::SrvReq,
            AgentMessageType::SrvResp,
            AgentMessageType::Rjct,
        ];
        ALL.into_iter().find(|t| t.tag() == tag)
    }
}

impl AMessage {
    /// `aMessageType`.
    #[must_use]
    pub fn message_type(&self) -> AgentMessageType {
        match self {
            Self::Hello => AgentMessageType::Hello,
            Self::AMsg(_) => AgentMessageType::AMsg,
            Self::ARcvd(_) => AgentMessageType::ARcvd,
            Self::AQcont(_) => AgentMessageType::Qcont,
            Self::Qadd(_) => AgentMessageType::Qadd,
            Self::Qkey(_) => AgentMessageType::Qkey,
            Self::Quse(_) => AgentMessageType::Quse,
            Self::Qtest(_) => AgentMessageType::Qtest,
            Self::Qend(_) => AgentMessageType::Qend,
            Self::Eready(_) => AgentMessageType::Eready,
        }
    }
}

impl AgentMessage {
    /// `agentMessageType`.
    #[must_use]
    pub fn message_type(&self) -> AgentMessageType {
        match self {
            Self::ConnInfo(_) => AgentMessageType::ConnInfo,
            Self::ConnInfoReply { .. } => AgentMessageType::ConnInfoReply,
            Self::RatchetInfo(_) => AgentMessageType::RatchetInfo,
            Self::Message { message, .. } => message.message_type(),
            Self::ServiceRequest { .. } => AgentMessageType::SrvReq,
            Self::ServiceResponse(_) => AgentMessageType::SrvResp,
            Self::Rejection(_) => AgentMessageType::Rjct,
        }
    }
}
