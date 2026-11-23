//! Group invitation, introduction and member types (Types.hs `GroupInvitation`, `MemberInfo`, `IntroInvitation`).
use super::group_profile::GroupProfile;
use super::ids::{GroupLinkId, MemberId};
use super::open_enum::open_enum;
use super::profile::Profile;
use super::version::VersionRange;
use serde::{Deserialize, Serialize};
use serde_json::Value;

open_enum!(GroupMemberRole {
    Relay = "relay",
    Observer = "observer",
    Author = "author",
    Member = "member",
    Moderator = "moderator",
    Admin = "admin",
    Owner = "owner",
});

open_enum!(MemberRestrictionStatus { Blocked = "blocked", Unrestricted = "unrestricted" });

open_enum!(GroupRejectionReason { LongName = "long_name", BlockedName = "blocked_name" });

/// `GroupAcceptance`: how the host answers a join request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupAcceptance {
    #[serde(rename = "accepted")]
    Accepted,
    #[serde(rename = "pending")]
    PendingApproval,
    #[serde(rename = "pending_review")]
    PendingReview,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberIdRole {
    pub member_id: MemberId,
    pub member_role: GroupMemberRole,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberInfo {
    pub member_id: MemberId,
    pub member_role: GroupMemberRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v: Option<VersionRange>,
    pub profile: Profile,
    /// Ed25519 member key (X.509 DER, base64url), chat v20.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntroInvitation {
    pub group_conn_req: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberRestrictions {
    pub restriction: MemberRestrictionStatus,
}

/// `business` is the business-chat info of a business address; opaque here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInvitation {
    pub from_member: MemberIdRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_member_key: Option<String>,
    pub invited_member: MemberIdRole,
    pub conn_request: String,
    pub group_profile: GroupProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_link_id: Option<GroupLinkId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_size: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupLinkInvitation {
    pub from_member: MemberIdRole,
    pub from_member_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_member_key: Option<String>,
    pub invited_member: MemberIdRole,
    pub group_profile: GroupProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted: Option<GroupAcceptance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_size: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupLinkRejection {
    pub from_member: MemberIdRole,
    pub invited_member: MemberIdRole,
    pub group_profile: GroupProfile,
    pub rejection_reason: GroupRejectionReason,
}
