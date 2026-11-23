//! The mutable, signed part of link data: user data, owners, relays and address ratchet keys (Agent/Protocol.hs `ConnLinkData`, `UserContactData`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, List, VersionRange};

use super::owner::OwnerAuth;
use super::user_data::UserLinkData;
use crate::agent::e2e::E2eParamsUri;
use crate::agent::links::{AddressRatchetKeys, ConnShortLink, ConnectionMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserContactData {
    /// Joining through the connection request in the fixed data is allowed.
    pub direct: bool,
    /// Additional owners who may sign changes.
    pub owners: Vec<OwnerAuth>,
    /// Other addresses of relays that receive requests for this address.
    pub relays: Vec<ConnShortLink>,
    pub user_data: UserLinkData,
    pub ratchet_keys: Option<AddressRatchetKeys>,
}

/// What the owner of a link supplies, before it is given the agent versions.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "short-lived, built and matched once"
)]
pub enum UserConnLinkData {
    Invitation(UserLinkData),
    Contact(UserContactData),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "short-lived, built and matched once"
)]
pub enum ConnLinkData {
    Invitation {
        agent_v_range: VersionRange,
        user_data: UserLinkData,
    },
    Contact {
        agent_v_range: VersionRange,
        data: UserContactData,
    },
}

impl ConnLinkData {
    #[must_use]
    pub fn new(agent_v_range: VersionRange, user: UserConnLinkData) -> Self {
        match user {
            UserConnLinkData::Invitation(user_data) => Self::Invitation {
                agent_v_range,
                user_data,
            },
            UserConnLinkData::Contact(data) => Self::Contact {
                agent_v_range,
                data,
            },
        }
    }

    #[must_use]
    pub fn mode(&self) -> ConnectionMode {
        match self {
            Self::Invitation { .. } => ConnectionMode::Invitation,
            Self::Contact { .. } => ConnectionMode::Contact,
        }
    }

    #[must_use]
    pub fn user_data(&self) -> &UserLinkData {
        match self {
            Self::Invitation { user_data, .. }
            | Self::Contact {
                data: UserContactData { user_data, .. },
                ..
            } => user_data,
        }
    }
}

impl Encode for AddressRatchetKeys {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (&self.ratchet_key_id, &self.e2e).encode(out)
    }
}

impl Decode for AddressRatchetKeys {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (ratchet_key_id, e2e) = <(Vec<u8>, E2eParamsUri)>::decode(d)?;
        Ok(Self {
            ratchet_key_id,
            e2e,
        })
    }
}

impl Encode for UserContactData {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let (owners, relays) = (List(self.owners.clone()), List(self.relays.clone()));
        (
            self.direct,
            &owners,
            &relays,
            &self.user_data,
            &self.ratchet_keys,
        )
            .encode(out)
    }
}

impl Decode for UserContactData {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (direct, owners, relays, user_data) =
            <(bool, List<OwnerAuth>, List<ConnShortLink>, UserLinkData)>::decode(d)?;
        // The keys are optional, and the rest is left for future versions.
        let ratchet_keys = Option::<AddressRatchetKeys>::decode(&mut d.clone()).unwrap_or(None);
        d.take_all();
        Ok(Self {
            direct,
            owners: owners.0,
            relays: relays.0,
            user_data,
            ratchet_keys,
        })
    }
}

impl Encode for ConnLinkData {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::Invitation {
                agent_v_range,
                user_data,
            } => (ConnectionMode::Invitation, agent_v_range, user_data).encode(out),
            Self::Contact {
                agent_v_range,
                data,
            } => (ConnectionMode::Contact, agent_v_range, data).encode(out),
        }
    }
}

impl Decode for ConnLinkData {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match ConnectionMode::decode(d)? {
            ConnectionMode::Invitation => {
                let (agent_v_range, user_data) = <(VersionRange, UserLinkData)>::decode(d)?;
                d.take_all();
                Ok(Self::Invitation {
                    agent_v_range,
                    user_data,
                })
            }
            ConnectionMode::Contact => {
                let (agent_v_range, data) = <(VersionRange, UserContactData)>::decode(d)?;
                Ok(Self::Contact {
                    agent_v_range,
                    data,
                })
            }
        }
    }
}
