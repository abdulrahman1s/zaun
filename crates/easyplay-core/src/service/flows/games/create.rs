//! CreateGame (SVC-004, LOGIC_SPECS §2): commit a confirmed draft, then optionally launch.
use crate::library::GameDraft;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn create_game(&mut self, draft: GameDraft, then: AfterCreate) -> Result<()> {
        let result = self.lib.create(&draft);
        self.setup_closed()?;
        let report = match result {
            Ok(r) => r,
            Err(e) => {
                // Raced: the path already belongs to a game → open its page.
                let owner = (e.code == ErrorCode::LibAlreadyInLibrary)
                    .then(|| e.context_value("game").and_then(|g| g.parse().ok()))
                    .flatten();
                self.fail(e);
                if let Some(g) = owner {
                    self.emit(Event::ShowGame(g));
                }
                return Ok(());
            }
        };
        let id = report.id;
        self.index_game_art(id);
        self.publish_library();
        self.check_availability();
        self.notice(Notice::GameAdded {
            game: id,
            name: draft.name.clone(),
        });
        if report.artwork_error.is_some() {
            self.notice(Notice::ArtworkKeptPlaceholder { game: id });
        }
        // ADR-0018: only with `artwork.online = "steam"`. A failed fetch is shown; the game
        // is still launched.
        if let Err(e) = self.fetch_online_artwork(id, false) {
            self.fail(e);
        }
        if then == AfterCreate::Launch {
            self.start_launch(id, None, LaunchOrigin::Library);
        }
        Ok(())
    }
}
