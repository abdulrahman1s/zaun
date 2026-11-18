//! Editing a game's programs (Launch options tab, UI-020): role, label, own arguments,
//! removal, and detaching a program for "Set up as a new game" (LOGIC_SPECS §1, §4).

use std::path::PathBuf;

use super::service::{Library, storage};
use crate::error::{Error, ErrorCode, Result};
use crate::ids::{ExecutableId, GameId};
use crate::model::{ExeRole, Executable, Problem, ValidationError};
use crate::storage::ExecutableRepo;

impl Library {
    fn program(&mut self, game: GameId, exe: ExecutableId) -> Result<Executable> {
        match self.db_mut().executable(exe).map_err(storage)? {
            Some((g, e)) if g == game => Ok(e),
            _ => Err(Error::new(ErrorCode::LibExeNotFound)
                .with("game", game)
                .with("executable", exe)),
        }
    }

    fn store_program(&mut self, game: GameId, e: &Executable) -> Result<()> {
        e.validate()?;
        self.db_mut().update_executable(e).map_err(storage)?;
        self.refresh_game(game)
    }

    /// Change a program's role. `Primary` demotes the current primary to `Alternate`; the
    /// primary itself can only change by promoting another program.
    pub fn set_program_role(
        &mut self,
        game: GameId,
        exe: ExecutableId,
        role: ExeRole,
    ) -> Result<()> {
        let mut e = self.program(game, exe)?;
        if role == ExeRole::Primary {
            self.db_mut()
                .set_primary_executable(game, exe)
                .map_err(storage)?;
            return self.refresh_game(game);
        }
        if e.role == ExeRole::Primary {
            return Err(Error::from(ValidationError::new(
                "game.executables",
                Problem::ExactlyOnePrimary { found: 0 },
            )));
        }
        e.role = role;
        self.store_program(game, &e)
    }

    /// Rename a program (`None` = its file name).
    pub fn set_program_label(
        &mut self,
        game: GameId,
        exe: ExecutableId,
        label: Option<String>,
    ) -> Result<()> {
        let mut e = self.program(game, exe)?;
        e.label = label.map(|l| l.trim().to_owned()).filter(|l| !l.is_empty());
        self.store_program(game, &e)
    }

    /// A program's own arguments (`None` = the game's arguments).
    pub fn set_program_args(
        &mut self,
        game: GameId,
        exe: ExecutableId,
        args: Option<Vec<String>>,
    ) -> Result<()> {
        let mut e = self.program(game, exe)?;
        e.args_override = args;
        self.store_program(game, &e)
    }

    /// Remove a program that is not the primary.
    pub fn remove_program(&mut self, game: GameId, exe: ExecutableId) -> Result<()> {
        let e = self.program(game, exe)?;
        if e.role == ExeRole::Primary {
            return Err(Error::from(ValidationError::new(
                "game.executables",
                Problem::ExactlyOnePrimary { found: 0 },
            )));
        }
        self.db_mut().remove_executable(exe).map_err(storage)?;
        self.refresh_game(game)
    }

    /// Detach the program at `exe` from its game so the file can be set up as a new game
    /// ("Set up as a new game"). Another program of the game becomes primary when needed. A
    /// game's only program cannot be deleted: it is kept under a stand-in canonical path
    /// that no file has, so the old game shows its program as missing (*Locate…*) and
    /// keeps everything else.
    pub fn detach_program(&mut self, exe: ExecutableId) -> Result<GameId> {
        let Some((game, e)) = self.db_mut().executable(exe).map_err(storage)? else {
            return Err(Error::new(ErrorCode::LibExeNotFound).with("executable", exe));
        };
        let others: Vec<Executable> = self
            .db_mut()
            .executables_of(game)
            .map_err(storage)?
            .into_iter()
            .filter(|o| o.id != exe)
            .collect();
        if let Some(next) = others
            .iter()
            .find(|o| o.role == ExeRole::Alternate)
            .or(others.first())
        {
            let next = next.id;
            self.db_mut().transaction(|tx| -> Result<()> {
                if e.role == ExeRole::Primary {
                    tx.set_primary_executable(game, next).map_err(storage)?;
                }
                tx.remove_executable(exe).map_err(storage)?;
                Ok(())
            })?;
        } else {
            let mut kept = e.clone();
            let mut name = e.canonical_path.as_os_str().to_owned();
            name.push(format!(".detached-{}", exe.short()));
            kept.canonical_path = PathBuf::from(name);
            self.db_mut().update_executable(&kept).map_err(storage)?;
        }
        self.refresh_game(game)?;
        Ok(game)
    }
}
