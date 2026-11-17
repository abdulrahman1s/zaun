//! UF-9: a launch that failed early, or a game that crashed at once, becomes a problem panel (SVC-005).
//!
//! The suggestions come from the evidence (`diagnostics::evidence`): the classified error,
//! the session's network mode, the game folder access, the program's bitness, sibling files
//! and the tail of `game.log`. Reads at most 64 KiB of the log and one directory listing.
use crate::diagnostics::evidence::{self, Evidence, LOG_TAIL_BYTES};
use crate::error::UserError;
use crate::model::{EndReason, PeMachine, Session};
use crate::service::prelude::*;

impl AppService {
    pub(super) fn early_failure(&mut self, s: &Session, game: GameId) -> Option<UserError> {
        let short = s.duration_ms.is_none_or(|d| d < 60_000);
        let (code, known) = match s.end_reason? {
            EndReason::FailedToStart { code } => (code, Some(code)),
            EndReason::Crashed { .. } if short => (ErrorCode::GameCrashed, None),
            EndReason::Exited { code } if short && code != 0 => (ErrorCode::GameClosedEarly, None),
            _ => return None,
        };
        let g = self.lib.game(game).ok();
        let fs = &*self.deps.fs;
        let exe = g.as_ref().and_then(|g| {
            s.executable_id
                .and_then(|id| g.executables.iter().find(|e| e.id == id))
                .or_else(|| g.primary())
        });
        let siblings = exe
            .and_then(|e| e.path.parent())
            .map(|d| crate::identity::hints::list_names(fs, &*self.deps.clock, d))
            .unwrap_or_default();
        let ev = Evidence {
            known_error: known,
            online_capable: crate::identity::hints::online_capable(&[], &siblings),
            game_dir: g.as_ref().map(|g| g.policy.game_dir),
            preset: s
                .context
                .preset
                .map(|p| p.base())
                .or(g.as_ref().map(|g| g.policy.preset.base())),
            is_32bit: exe.is_some_and(|e| e.pe.machine == PeMachine::X86),
            log: evidence::scan_log(&self.log_tail(s)),
            ..Evidence::default()
        }
        .with_network(s.context.network.or(g.as_ref().map(|g| g.policy.network)));
        let mut u = self.describe(&Error::new(code).with("session", s.id), Some(game));
        u.session = Some(s.id);
        let mut actions = evidence::suggestions(&ev);
        for a in &u.actions {
            if !actions.contains(a) {
                actions.push(*a);
            }
        }
        u.actions = actions;
        Some(u)
    }

    fn log_tail(&self, s: &Session) -> String {
        let fs = &*self.deps.fs;
        let path = self.deps.paths.session_dir(s.id).join("game.log");
        fs.metadata(&path)
            .ok()
            .and_then(|m| {
                let from = m.size.saturating_sub(LOG_TAIL_BYTES as u64);
                fs.read_range(&path, from, LOG_TAIL_BYTES).ok()
            })
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    }
}
