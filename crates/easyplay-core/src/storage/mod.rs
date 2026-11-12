//! SQLite persistence (through `ormlite`/`sqlx` behind a synchronous facade, ADR-0017):
//! connection setup, migrations, repositories, backups, integrity checks, the cross-process
//! write lock.
//!
//! Layer 3. Owns `library.db` and its schema. See MODULES §4 and PERSISTENCE.
