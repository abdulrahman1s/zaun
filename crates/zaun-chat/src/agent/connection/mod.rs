//! The invitation handshake, both roles, and connection delete (Agent.hs `newConn`, `joinConn`, `allowConnection`, `deleteConnection`; HANDSHAKE.md, AGPL-3.0-only).
mod allow;
mod confirm;
mod delete;
mod invite;
mod join;
mod params;
mod queues;
mod reply;
mod resolve;
mod step_answer;
mod steps;
mod sub;

pub(crate) use confirm::Confirmation;
pub(crate) use queues::ack_request;
