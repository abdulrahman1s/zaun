//! `Store`: opens the file, migrates it and proves the key with a sealed check row.

use std::path::Path;

use super::StoreError;
use super::db::Db;
use super::{migrate, seal::Sealer};

const CHECK_AAD: &[u8] = b"meta";
const CHECK_KEY: &str = "check";
const CHECK_PLAIN: &[u8] = b"zaun-chat store ok";

pub struct Store {
    pub(super) db: Db,
    pub(super) sealer: Sealer,
}

impl Store {
    /// Opens or creates the store. A key that does not open the check row is `WrongKey`.
    /// Dropping the store closes it.
    pub fn open(path: &Path, key: &str) -> Result<Store, StoreError> {
        let sealer = Sealer::new(key);
        let mut db = Db::open(path)?;
        db.run(migrate::run)?;
        let s = &sealer;
        db.atomic(async |c| {
            let row: Option<(Vec<u8>,)> = ormlite::query_as("SELECT v FROM meta WHERE k = ?1")
                .bind(CHECK_KEY)
                .fetch_optional(&mut *c)
                .await?;
            match row {
                Some((sealed,)) => s
                    .open(&[CHECK_AAD, CHECK_KEY.as_bytes()], &sealed)
                    .map(drop)
                    .map_err(|_| StoreError::WrongKey),
                None => {
                    let sealed = s.seal(&[CHECK_AAD, CHECK_KEY.as_bytes()], CHECK_PLAIN)?;
                    ormlite::query("INSERT INTO meta (k, v) VALUES (?1, ?2)")
                        .bind(CHECK_KEY)
                        .bind(sealed)
                        .execute(c)
                        .await?;
                    Ok(())
                }
            }
        })?;
        Ok(Store { db, sealer })
    }
}
