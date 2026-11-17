//! A session ended: audit the runtimes in the background (SVC-008, RUNTIME_MANAGEMENT §6.3).
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn after_session_ended(&mut self) {
        self.audit_runtimes(false);
    }
}
