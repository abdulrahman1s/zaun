//! The official CLI driven over its WebSocket chat server (`-p <port>`), as nook's client.live.test.ts does.
use std::time::Duration;

use serde_json::{Value, json};

use super::ws::Ws;

pub struct Cli {
    ws: Ws,
    seq: u64,
    /// Events not yet looked at. Eviction: `take` removes what it finds, and the test ends with the CLI.
    pub events: Vec<Value>,
}

/// The response or event inside one message; 7.1 may wrap it in `Right`.
fn resp(v: &Value) -> Value {
    let r = v.get("resp").cloned().unwrap_or(Value::Null);
    r.get("Right").cloned().unwrap_or(r)
}

impl Cli {
    pub fn open(port: u16) -> Self {
        Self {
            ws: Ws::connect(port),
            seq: 0,
            events: Vec::new(),
        }
    }

    /// Sends `cmd` and waits for its answer, keeping the events that come meanwhile.
    pub fn cmd(&mut self, cmd: &str) -> Value {
        self.seq += 1;
        let corr = self.seq.to_string();
        self.ws
            .send(&json!({ "corrId": corr, "cmd": cmd }).to_string());
        loop {
            let text = self.ws.recv(Duration::from_secs(30)).expect("CLI answer");
            let v: Value = serde_json::from_str(&text).unwrap();
            if v.get("corrId").and_then(Value::as_str) == Some(corr.as_str()) {
                return resp(&v);
            }
            self.events.push(resp(&v));
        }
    }

    /// Reads what has arrived, waiting at most `wait`.
    pub fn poll(&mut self, wait: Duration) {
        if let Some(text) = self.ws.recv(wait) {
            let v: Value = serde_json::from_str(&text).unwrap();
            self.events.push(resp(&v));
        }
    }

    pub fn take<T>(&mut self, f: impl Fn(&Value) -> Option<T>) -> Option<T> {
        let i = self.events.iter().position(|e| f(e).is_some())?;
        f(&self.events.remove(i))
    }
}

pub fn kind(e: &Value) -> &str {
    e.get("type").and_then(Value::as_str).unwrap_or_default()
}

/// `contactConnected` for a contact named `name`: its id.
pub fn connected(e: &Value, name: &str) -> Option<i64> {
    let c = e.get("contact")?;
    let n = c.pointer("/profile/displayName")?.as_str()?;
    (kind(e) == "contactConnected" && n == name).then(|| c.get("contactId")?.as_i64())?
}

/// A received text in `newChatItems` from contact `cid`.
pub fn received_text(e: &Value, cid: i64) -> Option<String> {
    if kind(e) != "newChatItems" {
        return None;
    }
    e.get("chatItems")?.as_array()?.iter().find_map(|ci| {
        let from = ci.pointer("/chatInfo/contact/contactId")?.as_i64()?;
        let dir = ci.pointer("/chatItem/chatDir/type")?.as_str()?;
        let text = ci.pointer("/chatItem/content/msgContent/text")?.as_str()?;
        (from == cid && dir == "directRcv").then(|| text.to_owned())
    })
}

pub fn send_text(cli: &mut Cli, cid: i64, text: &str) {
    let items = json!([{ "msgContent": { "type": "text", "text": text } }]);
    let r = cli.cmd(&format!("/_send @{cid} json {items}"));
    assert_eq!(kind(&r), "newChatItems", "{r}");
}
