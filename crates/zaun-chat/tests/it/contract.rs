//! The pipe contract: the engine's `Request`, `Event` and models have the JSON of the copies in
//! `zaun-chat-wire` that core reads (ADR-0033), variant for variant.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use simplex_client::types::{Event, Request, model as sx};

use crate::samples;

/// `wire` read as the engine's type writes back the same JSON.
fn same_json<Sx: Serialize + DeserializeOwned>(wire: &impl Serialize) {
    let value = serde_json::to_value(wire).unwrap();
    let engine: Sx = serde_json::from_value(value.clone())
        .unwrap_or_else(|e| panic!("the engine cannot read {value}: {e}"));
    assert_eq!(serde_json::to_value(&engine).unwrap(), value);
}

#[test]
fn every_wire_request_is_an_engine_request_of_the_same_json() {
    for req in samples::requests() {
        same_json::<Request>(&req);
    }
}

#[test]
fn every_wire_event_is_an_engine_event_of_the_same_json() {
    for ev in samples::events() {
        let mut value = serde_json::to_value(&ev).unwrap();
        // The engine says why a request failed with its own codes; core maps them (from_wire).
        if value["t"] == "failed" {
            value["c"]["code"] = json!("SX-CHT-005");
        }
        same_json::<Event>(&value);
    }
}

#[test]
fn every_wire_body_notice_delivery_and_file_state_is_the_engines() {
    for body in samples::bodies() {
        same_json::<sx::ChatMessage>(&samples::message(body));
    }
    for notice in samples::notices() {
        same_json::<sx::ChatNotice>(&notice);
    }
    for delivery in samples::deliveries() {
        same_json::<sx::Delivery>(&delivery);
    }
    for state in samples::states() {
        same_json::<sx::FileState>(&state);
    }
}

#[test]
fn every_failure_code_the_engine_gives_is_a_string_core_reads() {
    use simplex_client::types::ErrorCode as C;
    for code in [
        C::NoConnection,
        C::BadLink,
        C::RequestFailed,
        C::CallFailed,
        C::StreamFailed,
        C::ReactionNotAllowed,
        C::FileTooLarge,
        C::TextTooLong,
        C::TooManyMentions,
        C::InvalidName,
        C::NotAllowed,
        C::PictureTooLarge,
    ] {
        assert_eq!(
            serde_json::to_value(code).unwrap(),
            Value::from(code.as_str())
        );
    }
}

/// Compile-time guards: a variant simplex-client adds stops the build here until the wire copy,
/// its samples and core learn it. Without them core would drop every line that carries it.
#[allow(dead_code, reason = "only compiled, never called")]
fn engine_events_are_all_known(ev: &Event) {
    match ev {
        Event::Features { .. }
        | Event::Ready(..)
        | Event::Profile(..)
        | Event::Chats { .. }
        | Event::Invite { .. }
        | Event::Address { .. }
        | Event::Messages { .. }
        | Event::Updated(..)
        | Event::History { .. }
        | Event::Code { .. }
        | Event::Verified { .. }
        | Event::Members { .. }
        | Event::Offline { .. }
        | Event::Preview { .. }
        | Event::OpenFile { .. }
        | Event::Located { .. }
        | Event::GroupLink { .. }
        | Event::Call(..)
        | Event::Stream(..)
        | Event::Failed { .. }
        | Event::Exited { .. } => {}
    }
}

#[allow(dead_code, reason = "only compiled, never called")]
fn engine_notices_and_deliveries_are_all_known(n: &sx::ChatNotice, d: sx::Delivery) {
    use sx::ChatNotice as N;
    match n {
        N::Connected
        | N::GroupCreated
        | N::YouJoined
        | N::MemberJoined(_)
        | N::MemberAdded { .. }
        | N::MemberLeft(_)
        | N::MemberBlocked { .. }
        | N::MemberRemoved { .. }
        | N::YouLeft
        | N::YouRemoved(_)
        | N::GroupDeleted
        | N::GroupNameChanged(_)
        | N::NameChanged { .. }
        | N::FriendDeleted
        | N::GroupInvitation { .. }
        | N::CodeChanged
        | N::AddressSwitch { .. }
        | N::RatchetSync { .. }
        | N::BadSignature
        | N::Integrity(_)
        | N::DecryptionError { .. }
        | N::DirectInvitation => {}
    }
    match d {
        sx::Delivery::Sending
        | sx::Delivery::Sent
        | sx::Delivery::Delivered
        | sx::Delivery::DeliveredBadHash
        | sx::Delivery::Failed
        | sx::Delivery::Received => {}
    }
}
