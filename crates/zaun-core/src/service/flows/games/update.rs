//! `UpdateGame`: fold the edits into one patch (validated as a whole), then the program edits.
use crate::host::HostFacts;
use crate::identity::naming;
use crate::library::with_entry_kind;
use crate::model::{BasePreset, EntryKind};
use crate::sandbox::{apply_preset, classify};
use crate::service::prelude::*;
use crate::storage::GamePatch;

impl AppService {
    pub(crate) fn update_game(&mut self, id: GameId, edits: Vec<GameEdit>) -> Result<()> {
        let mut patch = GamePatch::default();
        let mut program_edits = Vec::new();
        for edit in edits {
            match edit {
                GameEdit::ExeRole(..)
                | GameEdit::ExeLabel(..)
                | GameEdit::ExeArgs(..)
                | GameEdit::RemoveExe(_) => program_edits.push(edit),
                edit => self.fold_edit(id, &mut patch, edit)?,
            }
        }
        if patch != GamePatch::default() {
            self.lib.update(id, patch)?;
        }
        for edit in program_edits {
            self.apply_program_edit(id, edit)?;
        }
        self.publish_library();
        Ok(())
    }

    /// Add one edit of the game's own fields to `patch`.
    fn fold_edit(&mut self, id: GameId, patch: &mut GamePatch, edit: GameEdit) -> Result<()> {
        match edit {
            GameEdit::Name(n) => {
                patch.name = Some(n);
                patch.name_is_user_set = Some(true);
            }
            GameEdit::Notes(n) => patch.notes = Some(n),
            GameEdit::Policy(p) => {
                let mut p = *p;
                p.preset = classify(&p);
                patch.policy = Some(p);
            }
            GameEdit::Launch(l) => patch.launch = Some(*l),
            GameEdit::Runtime { reference, follow } => {
                patch.runtime = Some(reference);
                patch.runtime_follow_updates = Some(follow);
            }
            GameEdit::UmuId(u) => patch.umu_id = Some(u),
            GameEdit::SteamAppId(a) => {
                patch.steam_app_id = Some(a);
                patch.steam_app_id_is_user_set = Some(a.is_some());
            }
            GameEdit::ResetName => self.reset_name(id, patch)?,
            GameEdit::Kind(k) => self.set_kind(id, patch, k)?,
            GameEdit::Preset(b) => self.switch_preset(id, patch, b)?,
            GameEdit::ExeRole(..)
            | GameEdit::ExeLabel(..)
            | GameEdit::ExeArgs(..)
            | GameEdit::RemoveExe(_) => {} // `update_game` sets these aside
        }
        Ok(())
    }

    /// Back to the name derived from the primary program (none: nothing to derive from).
    fn reset_name(&mut self, id: GameId, patch: &mut GamePatch) -> Result<()> {
        let game = self.lib.game(id)?;
        let Some(exe) = game.primary() else {
            return Ok(());
        };
        let name = naming::derive_name(&exe.pe, game.root.as_deref(), &exe.canonical_path);
        patch.sort_name = Some(naming::sort_name(&name));
        patch.name = Some(name);
        patch.name_is_user_set = Some(false);
        Ok(())
    }

    fn set_kind(&mut self, id: GameId, patch: &mut GamePatch, kind: EntryKind) -> Result<()> {
        let flags = match patch.flags {
            Some(f) => f,
            None => self.lib.game(id)?.flags,
        };
        patch.flags = Some(with_entry_kind(flags, kind));
        Ok(())
    }

    /// Keeps folder grants, game-folder access and save location (an earlier policy edit in
    /// the same batch is the base).
    fn switch_preset(
        &mut self,
        id: GameId,
        patch: &mut GamePatch,
        preset: BasePreset,
    ) -> Result<()> {
        let base = match patch.policy.take() {
            Some(p) => p,
            None => self.lib.game(id)?.policy,
        };
        let host = self
            .host
            .as_deref()
            .cloned()
            .unwrap_or_else(|| HostFacts::empty(self.deps.clock.now()));
        patch.policy = Some(apply_preset(&base, preset, &host));
        Ok(())
    }

    fn apply_program_edit(&mut self, id: GameId, edit: GameEdit) -> Result<()> {
        match edit {
            GameEdit::ExeRole(x, role) => self.lib.set_program_role(id, x, role),
            GameEdit::ExeLabel(x, label) => self.lib.set_program_label(id, x, label),
            GameEdit::ExeArgs(x, args) => self.lib.set_program_args(id, x, args),
            GameEdit::RemoveExe(x) => self.lib.remove_program(id, x),
            GameEdit::Name(_)
            | GameEdit::Notes(_)
            | GameEdit::Policy(_)
            | GameEdit::Launch(_)
            | GameEdit::Runtime { .. }
            | GameEdit::UmuId(_)
            | GameEdit::SteamAppId(_)
            | GameEdit::Preset(_)
            | GameEdit::ResetName
            | GameEdit::Kind(_) => Ok(()),
        }
    }
}
