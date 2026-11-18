//! Answers about the network: connection plans, requests that reach a relay, the relay check.
use super::respond::Answer;
use super::*;

impl Codec {
    pub(super) fn on_plan_answer(
        &mut self,
        request: Request,
        retried: bool,
        a: Answer<'_>,
        out: &mut Output,
    ) {
        if let Some(e) = a.error.filter(|_| a.ty != Some("connectionPlan")) {
            return self.on_net_error(RequestKind::Connect, request, retried, e, out);
        }
        self.on_plan(a.result, a.ty, request, retried, out);
    }

    pub(super) fn on_net_answer(
        &mut self,
        kind: RequestKind,
        request: Request,
        retried: bool,
        a: Answer<'_>,
        out: &mut Output,
    ) {
        if let Some(e) = a.error {
            return self.on_net_error(kind, request, retried, e, out);
        }
        self.links_busy.remove(&kind);
        self.on_request_done(kind, a.ty, a.result, out);
    }

    pub(super) fn on_servers(&mut self, a: Answer<'_>, out: &mut Output) {
        match a.of_type("userServers").and_then(|r| r.get("userServers")) {
            Some(list) => self.test_servers(list.clone(), out),
            None => {
                tracing::info!(error = %a.error_text(), "chat: the server list was not read");
                self.check_done(out);
            }
        }
    }

    pub(super) fn on_server_test(&mut self, server: String, a: Answer<'_>, out: &mut Output) {
        let failure = a.result.and_then(|r| r.get("testFailure"));
        let passed = a.ty == Some("serverTestResult") && failure.is_none_or(Value::is_null);
        if !passed {
            let why = failure.or(a.error).map(short).unwrap_or_default();
            tracing::info!(server = %host_of(&server), why, "chat: a relay did not answer its test");
        }
        if let Some(c) = &mut self.check {
            c.passed.insert(server, passed);
            c.waiting = c.waiting.saturating_sub(1);
            if c.waiting == 0 {
                self.apply_check(out);
            }
        }
    }

    pub(super) fn on_set_servers(&mut self, a: Answer<'_>, out: &mut Output) {
        if let Some(e) = a.error {
            tracing::info!(error = %short(e), "chat: the checked server list was refused");
        }
        self.check_done(out);
    }
}
