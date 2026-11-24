//! The engine's own `Request` and `Event` in the pipe's line envelopes. Their JSON is the
//! vocabulary `zaun-chat-wire` copies; `tests/it/contract.rs` holds the two to the same shape.

use serde::{Deserialize, Serialize};
use simplex_client::types::{Config, Event, ProxyFallback, ProxyMode, Request};
use zaun_chat_wire::Hello;

#[derive(Debug, Deserialize)]
pub struct ReqLine {
    pub req: Request,
}

#[derive(Debug, Serialize)]
pub struct EvLine<'a> {
    pub ev: &'a Event,
}

/// What the engine opens with: the Hello's folder, key, name and relays, with the chat's own
/// private routing settings.
#[must_use]
pub fn config(hello: &Hello) -> Config {
    Config {
        dir: hello.dir.clone(),
        key: hello.key.clone(),
        name: hello.name.clone(),
        relays: hello.relays.clone(),
        proxy_mode: ProxyMode::default(),
        proxy_fallback: ProxyFallback::default(),
    }
}
