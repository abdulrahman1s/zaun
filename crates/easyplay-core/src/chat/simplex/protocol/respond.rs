//! The helper's answer to a command: routed by what the command was (`Pending`), one function
//! per kind (ADR-0033). Start-up answers are here; the rest are in the sibling files.
use super::respond_data::{on_code, on_group_link, on_members};
use super::*;

/// What a command's answer carried.
#[derive(Clone, Copy)]
pub(super) struct Answer<'a> {
    pub result: Option<&'a Value>,
    pub error: Option<&'a Value>,
    /// The result's `type`.
    pub ty: Option<&'a str>,
}

impl<'a> Answer<'a> {
    fn new(body: &'a Value) -> Self {
        let result = body.get("result");
        Self {
            result,
            error: body.get("error"),
            ty: result.and_then(|r| r.get("type")).and_then(Value::as_str),
        }
    }

    /// The error, compact, for the log or a failure's detail.
    pub fn error_text(&self) -> String {
        self.error.map(short).unwrap_or_default()
    }

    pub fn failed(&self, kind: RequestKind) -> ChatEvent {
        failed(kind, ErrorCode::ChtRequestFailed, self.error_text())
    }

    /// The result when it is of type `ty`.
    pub fn of_type(&self, ty: &str) -> Option<&'a Value> {
        self.result.filter(|_| self.ty == Some(ty))
    }
}

impl Codec {
    pub(super) fn on_response(&mut self, pending: Pending, body: &Value, out: &mut Output) {
        let a = Answer::new(body);
        match pending {
            Pending::ShowUser => self.on_show_user(a, out),
            Pending::CreateUser => self.on_create_user(a, out),
            Pending::FilePaths => self.on_file_paths(a, out),
            Pending::Start => self.on_started(a, out),
            Pending::Relays => self.on_relays(a, out),
            Pending::FileRelays => self.on_file_relays(a, out),
            Pending::ShowAddress => self.on_address(a, out),
            Pending::Chats => self.on_chats(a, out),
            Pending::History { tag, chat } => self.on_history(tag, chat, a, out),
            Pending::Plan { request, retried } => self.on_plan_answer(request, retried, a, out),
            Pending::Net {
                kind,
                request,
                retried,
            } => self.on_net_answer(kind, request, retried, a, out),
            Pending::Servers => self.on_servers(a, out),
            Pending::ServerTest(server) => self.on_server_test(server, a, out),
            Pending::SetServers => self.on_set_servers(a, out),
            Pending::Code(friend) => on_code(friend, a, out),
            Pending::Verify(friend) => self.on_verify(friend, a, out),
            Pending::Members(group) => on_members(group, a, out),
            Pending::MemberBlocked(group) => self.on_member_blocked(group, a, out),
            Pending::File { cancel } => self.on_file(cancel, a, out),
            Pending::GroupLink { group, read } => on_group_link(group, read, a, out),
            Pending::Preview { chat, last } => self.on_preview(chat, last, a, out),
            Pending::Quiet => {
                if let Some(e) = a.error {
                    tracing::info!(error = %short(e), "chat: a background command failed");
                }
            }
            Pending::Req(kind) => self.on_req(kind, a, out),
        }
    }

    fn on_show_user(&mut self, a: Answer<'_>, out: &mut Output) {
        if let (Some("activeUser"), Some(r)) = (a.ty, a.result) {
            return self.on_user(r, out);
        }
        let cmd = format!(
            "/_create user {}",
            serde_json::json!({
                "profile": { "displayName": self.name, "fullName": "" },
                "pastTimestamp": false,
            })
        );
        self.send(out, Pending::CreateUser, cmd);
    }

    fn on_create_user(&mut self, a: Answer<'_>, out: &mut Output) {
        match (a.ty, a.result) {
            (Some("activeUser"), Some(r)) => self.on_user(r, out),
            _ => out.events.push(a.failed(RequestKind::Start)),
        }
    }

    fn on_file_paths(&mut self, a: Answer<'_>, out: &mut Output) {
        if a.error.is_some() {
            out.events.push(a.failed(RequestKind::Start));
        } else {
            self.send(out, Pending::Start, "/_start".to_owned());
        }
    }

    fn on_started(&mut self, a: Answer<'_>, out: &mut Output) {
        match a.ty {
            Some("chatStarted" | "chatRunning") => self.set_relays(out),
            _ => out.events.push(a.failed(RequestKind::Start)),
        }
    }

    fn on_relays(&mut self, a: Answer<'_>, out: &mut Output) {
        if a.error.is_some() {
            // Fail closed: the user chose these relays, so never fall back to the presets.
            out.events.push(a.failed(RequestKind::Start));
        } else if self.xftp.is_empty() {
            self.become_ready(out);
        } else {
            let cmd = format!("/xftp {}", self.xftp.join(" "));
            self.send(out, Pending::FileRelays, cmd);
        }
    }

    fn on_file_relays(&mut self, a: Answer<'_>, out: &mut Output) {
        if a.error.is_some() {
            out.events.push(a.failed(RequestKind::Start));
        } else {
            self.become_ready(out);
        }
    }

    fn on_address(&mut self, a: Answer<'_>, out: &mut Output) {
        let link = a
            .of_type("userContactLink")
            .and_then(|r| r.get("contactLink"))
            .and_then(|l| l.get("connLinkContact"))
            .and_then(link_text);
        out.events.push(ChatEvent::Address(link));
    }

    fn on_req(&mut self, kind: RequestKind, a: Answer<'_>, out: &mut Output) {
        if let Some(e) = a.error {
            let code = error_code(kind, e);
            out.events.push(failed(kind, code, short(e)));
            if code == ErrorCode::ChtNoConnection {
                self.start_check(out);
            }
            return;
        }
        self.on_request_done(kind, a.ty, a.result, out);
    }
}
