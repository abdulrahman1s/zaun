//! Send the overlay what it draws: the snapshot is built from Chat, the open conversation
//! and the toasts, and sent only when it differs from the last one, at most once per
//! [`PUSH_EVERY`] (changes in between go out together when the wait is over). Pictures go
//! once each (`chat::overlay::pictures`).
use super::state::{OverlayDone, PUSH_EVERY};
use crate::chat::overlay::ToOverlay;
use crate::chat::overlay::build::{self, Inputs};
use crate::chat::overlay::pictures;
use crate::chat::overlay::snapshot::{Corner, OverlayLook, OverlaySnapshot};
use crate::config::schema::{OverlayCorner, ReduceMotion, Theme};
use crate::model::SessionKind;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn overlay_push(&mut self) {
        let st = &self.flow.overlay;
        if st.overlay.is_none() || st.flush_pending {
            return;
        }
        let now = self.deps.clock.mono();
        if let Some(last) = st.last_push {
            let since = now.saturating_duration_since(last);
            if since < PUSH_EVERY {
                self.flow.overlay.flush_pending = true;
                let generation = self.flow.overlay.generation;
                self.overlay_after(PUSH_EVERY - since, OverlayDone::Flush { generation });
                return;
            }
        }
        let snap = self.overlay_snapshot();
        let st = &mut self.flow.overlay;
        if st.sent.as_ref() == Some(&snap) {
            return;
        }
        let mut wire = snap.clone();
        pictures::attach(&mut wire, &self.flow.chat.snapshot, &mut st.pictures);
        if let Some(o) = &st.overlay {
            o.send(&ToOverlay::State {
                state: Box::new(wire),
            });
        }
        st.sent = Some(snap);
        st.last_push = Some(now);
    }

    fn overlay_snapshot(&self) -> OverlaySnapshot {
        let st = &self.flow.overlay;
        let chat = &self.flow.chat.snapshot;
        build::build(&Inputs {
            chat,
            open: st.chat.map(|c| (c, &st.messages)),
            toasts: &st.toasts,
            look: self.overlay_look(),
            game: self
                .sessions
                .values()
                .find(|s| s.kind == SessionKind::Game && s.is_open())
                .map(|s| s.game_name.clone()),
            // Sharing the game in a call with the same friend is not carried yet (the share
            // flow refuses it, ADR-0036), so the overlay offers only Stop for a share started
            // in the window.
            can_share: false,
        })
    }

    fn overlay_look(&self) -> OverlayLook {
        let p = &self.prefs;
        let env = &self.deps.launch_env.host_env;
        OverlayLook {
            toasts: p.chat.overlay_toasts,
            voice_widget: p.chat.overlay_voice_widget,
            corner: match p.chat.overlay_corner {
                OverlayCorner::TopLeft => Corner::TopLeft,
                OverlayCorner::TopRight => Corner::TopRight,
                OverlayCorner::BottomLeft => Corner::BottomLeft,
                OverlayCorner::BottomRight => Corner::BottomRight,
            },
            key: p.chat.overlay_key.clone(),
            language: p.general.language.clone(),
            locale: ["LC_ALL", "LC_MESSAGES", "LANG"]
                .iter()
                .find_map(|k| env.get(*k).filter(|v| !v.is_empty()))
                .cloned()
                .unwrap_or_default(),
            accent: p.appearance.accent.as_str().to_owned(),
            high_contrast: p.appearance.theme == Theme::HighContrast,
            reduced_motion: p.appearance.reduce_motion == ReduceMotion::On,
        }
    }
}
