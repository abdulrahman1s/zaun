//! SQLite persistence (through `ormlite`/`sqlx` behind a synchronous facade, ADR-0017):
//! connection setup, migrations, repositories, backups, integrity checks, the cross-process
//! write lock.
//!
//! Layer 3. Owns `library.db` and its schema. See MODULES §4 and PERSISTENCE.
//!
//! # Shape
//!
//! - [`Db`] is a read-write handle, [`ReadDb`] a read-only one (`query_only = ON`), and
//!   [`Tx`] a `BEGIN IMMEDIATE` transaction borrowed from a [`Db`]. Each handle owns one
//!   SQLite connection and a private `tokio` current-thread runtime; every method blocks the
//!   calling thread until SQLite answers. No `ormlite`, `sqlx` or `tokio` type is public.
//! - All three implement the sealed [`Store`] trait. The repository traits ([`GameRepo`],
//!   [`ExecutableRepo`], [`TagRepo`], [`ArtworkRepo`], [`SessionRepo`], [`StatsRepo`],
//!   [`RuntimeRepo`], [`KvRepo`]) are implemented once for every `Store`; a write through a
//!   [`ReadDb`] fails with [`StorageError::ReadOnly`]. Multi-row writes are atomic on their
//!   own and nest (as savepoints) inside a [`Tx`].
//! - [`migrate`] creates and upgrades the schema, [`backup`] takes and rotates `VACUUM INTO`
//!   snapshots, [`integrity`] runs `quick_check` and salvages damaged files, [`lock`] is the
//!   advisory `library.lock`, [`path_codec`] stores paths losslessly.
//!
//! The module contains no business rules: callers decide *when* to back up, migrate, purge
//! or count a session; storage only does it atomically.

pub mod backup;
mod db;
pub mod integrity;
pub mod lock;
pub mod migrate;
pub mod path_codec;
mod repo;

use std::path::PathBuf;

pub use db::{Db, JournalMode, OpenConfig, ReadDb, Store, Tx};
pub use migrate::{MigrateOptions, MigrationOutcome};
pub use repo::{
    ArtworkRepo, AuditState, CloseOutcome, DailyPlaytime, ExecutableRepo, GamePatch, GameRecord,
    GameRepo, KvRepo, LibraryRows, MetaKey, NewSession, PlaytimePiece, RuntimeEvent, RuntimeRepo,
    SessionClose, SessionEvent, SessionRepo, SlrInstall, StatsRepo, TagRepo,
};

use crate::error::{Error, ErrorCode};
use crate::fsx::FsError;
use crate::ids::GameId;
use crate::model::{DocError, ValidationError};

/// Which kind of SQLite constraint failed (for violations without a dedicated variant).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstraintKind {
    /// `UNIQUE` or `PRIMARY KEY`.
    Unique,
    /// `FOREIGN KEY`.
    ForeignKey,
    /// `CHECK`.
    Check,
    /// `NOT NULL`.
    NotNull,
    /// Anything else.
    Other,
}

/// Which JSON document of a game could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocKind {
    /// `games.policy_json`.
    Policy,
    /// `games.launch_json`.
    Launch,
}

/// Errors of the storage module.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Another connection holds the write lock beyond the busy timeout.
    #[error("the library is busy")]
    Busy,
    /// SQLite reported corruption (`SQLITE_CORRUPT`) or `quick_check` failed.
    #[error("the library file is damaged: {0}")]
    Corrupt(String),
    /// The file is not an SQLite database at all (`SQLITE_NOTADB`).
    #[error("the file is not an SQLite database")]
    NotADatabase,
    /// A valid SQLite database that belongs to another application.
    #[error("not an EasyPlay library (application_id {found:#010x})")]
    Foreign {
        /// The `application_id` found.
        found: u32,
    },
    /// The schema version is newer than this build supports.
    #[error("library schema version {found} is newer than the supported {supported}")]
    NewerSchema {
        /// `user_version` of the file.
        found: u32,
        /// `DB_SCHEMA_VERSION`.
        supported: u32,
    },
    /// A write through a read-only handle.
    #[error("the connection is read-only")]
    ReadOnly,
    /// The disk is full (`SQLITE_FULL`).
    #[error("the disk is full")]
    DiskFull,
    /// `executables.canonical_path` is already used.
    #[error("an executable with this path is already in the library")]
    DuplicateExecutablePath,
    /// A second executable with role `primary` for one game.
    #[error("a game can have only one primary executable")]
    MultiplePrimaries,
    /// A tag with this name (compared case-insensitively) exists.
    #[error("a tag with this name already exists")]
    DuplicateTag,
    /// A runtime build with this path is already catalogued.
    #[error("a runtime with this path is already catalogued")]
    DuplicateRuntimePath,
    /// Another constraint violation.
    #[error("{kind:?} constraint failed: {detail}")]
    Constraint {
        /// Constraint kind.
        kind: ConstraintKind,
        /// SQLite's message, e.g. `UNIQUE constraint failed: games.id`.
        detail: String,
    },
    /// A row that must exist does not.
    #[error("{what} not found: {key}")]
    NotFound {
        /// Table or entity.
        what: &'static str,
        /// The key looked up.
        key: String,
    },
    /// A value violates a field limit.
    #[error(transparent)]
    Invalid(#[from] ValidationError),
    /// A count limit of GAME_LIBRARY §13 would be exceeded.
    #[error("limit reached: {what} (at most {max})")]
    Limit {
        /// What is limited.
        what: &'static str,
        /// The limit.
        max: usize,
    },
    /// A game's JSON document is too new or unreadable (LOGIC_SPECS §15).
    #[error("{doc:?} document of game {game} is unreadable: {error}")]
    Document {
        /// The game.
        game: GameId,
        /// Which document.
        doc: DocKind,
        /// Why.
        error: DocError,
    },
    /// A stored value that cannot be decoded into a domain value.
    #[error("stored value cannot be decoded: {0}")]
    Decode(String),
    /// A filesystem operation failed (backups, restore, salvage).
    #[error(transparent)]
    Fs(#[from] FsError),
    /// A migration step failed and was rolled back.
    #[error("migration to version {step} failed: {message}")]
    Migration {
        /// The version the failing step would have produced.
        step: u32,
        /// The cause.
        message: String,
    },
    /// The backup could not be made or is not usable.
    #[error("backup failed: {0}")]
    Backup(String),
    /// `library.lock` is held by another process.
    #[error("the library is locked by another EasyPlay process")]
    LockHeld,
    /// The handle was closed.
    #[error("the database handle is closed")]
    Closed,
    /// Any other SQLite or driver error.
    #[error("database error: {0}")]
    Sql(String),
}

impl StorageError {
    /// Whether the error indicates a damaged file (PERSISTENCE §8 detection point).
    pub fn is_corruption(&self) -> bool {
        matches!(self, StorageError::Corrupt(_) | StorageError::NotADatabase)
    }

    /// The stable error code.
    pub fn code(&self) -> ErrorCode {
        match self {
            StorageError::Busy | StorageError::LockHeld => ErrorCode::DbBusy,
            StorageError::Corrupt(_)
            | StorageError::NotADatabase
            | StorageError::Foreign { .. } => ErrorCode::DbCorrupt,
            StorageError::NewerSchema { .. } => ErrorCode::DbNewerVersion,
            StorageError::DiskFull => ErrorCode::FsDiskFull,
            StorageError::DuplicateExecutablePath => ErrorCode::LibAlreadyInLibrary,
            StorageError::MultiplePrimaries
            | StorageError::DuplicateTag
            | StorageError::DuplicateRuntimePath
            | StorageError::Constraint { .. }
            | StorageError::Invalid(_)
            | StorageError::Limit { .. } => ErrorCode::LibInvalidField,
            StorageError::Document { .. } => ErrorCode::SbxPolicyUnreadable,
            StorageError::Fs(e) => Error::from(clone_fs_error(e)).code,
            StorageError::Migration { .. } => ErrorCode::DbMigrationFailed,
            StorageError::Backup(_) => ErrorCode::DbBackupFailed,
            StorageError::ReadOnly
            | StorageError::NotFound { .. }
            | StorageError::Decode(_)
            | StorageError::Closed
            | StorageError::Sql(_) => ErrorCode::IntTaskFailed,
        }
    }
}

fn clone_fs_error(e: &FsError) -> FsError {
    FsError::new(e.op, e.path.clone(), e.kind)
}

impl From<StorageError> for Error {
    fn from(e: StorageError) -> Self {
        let e = match e {
            StorageError::Fs(fs) => return Error::from(fs),
            other => other,
        };
        let code = e.code();
        let err = Error::new(code);
        let err = match &e {
            StorageError::Document { game, .. } => err.with("game", game),
            StorageError::Migration { step, .. } => err.with("step", step),
            StorageError::NewerSchema { found, .. } => err.with("version", found),
            _ => err,
        };
        err.with_source(e)
    }
}

/// Result alias of the storage module.
pub type StorageResult<T> = Result<T, StorageError>;

/// The backups directory layout used by [`backup`] and [`integrity`] (PERSISTENCE §2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryFiles {
    /// `library.db`.
    pub db_file: PathBuf,
    /// `backups/`.
    pub backups_dir: PathBuf,
}

impl LibraryFiles {
    /// From the resolved application paths.
    pub fn from_paths(paths: &crate::paths::AppPaths) -> Self {
        Self {
            db_file: paths.db_file.clone(),
            backups_dir: paths.backups_dir.clone(),
        }
    }
}
