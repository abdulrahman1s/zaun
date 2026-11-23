//! Chat items. `chat` is `HMAC(chat ref)`; each value is sealed against its chat and row id.

use ormlite::sqlite::SqliteConnection;

use super::StoreError;
use super::handle::Store;
use super::seal::Sealer;

fn chat_key(s: &Sealer, chat_ref: &[u8]) -> [u8; 32] {
    s.lookup("chat", chat_ref)
}

fn aad(chat: &[u8], id: i64) -> [Vec<u8>; 3] {
    [b"items".to_vec(), chat.to_vec(), id.to_le_bytes().to_vec()]
}

async fn seal_row(
    c: &mut SqliteConnection,
    s: &Sealer,
    chat: &[u8],
    id: i64,
    v: &[u8],
) -> Result<(), StoreError> {
    let a = aad(chat, id);
    let sealed = s.seal(&[&a[0], &a[1], &a[2]], v)?;
    let done = ormlite::query("UPDATE items SET v = ?1 WHERE id = ?2 AND chat = ?3")
        .bind(sealed)
        .bind(id)
        .bind(chat)
        .execute(c)
        .await?;
    if done.rows_affected() == 0 {
        return Err(StoreError::Invalid("no such item"));
    }
    Ok(())
}

pub(super) async fn insert(
    c: &mut SqliteConnection,
    s: &Sealer,
    chat_ref: &[u8],
    v: &[u8],
) -> Result<i64, StoreError> {
    let chat = chat_key(s, chat_ref);
    // The id is part of the AAD, so it must exist before the value is sealed.
    let id = ormlite::query("INSERT INTO items (chat, v) VALUES (?1, x'')")
        .bind(chat.as_slice())
        .execute(&mut *c)
        .await?
        .last_insert_rowid();
    seal_row(c, s, &chat, id, v).await?;
    Ok(id)
}

pub(super) async fn update(
    c: &mut SqliteConnection,
    s: &Sealer,
    chat_ref: &[u8],
    id: i64,
    v: &[u8],
) -> Result<(), StoreError> {
    seal_row(c, s, &chat_key(s, chat_ref), id, v).await
}

pub(super) async fn delete(
    c: &mut SqliteConnection,
    s: &Sealer,
    chat_ref: &[u8],
    id: i64,
) -> Result<(), StoreError> {
    ormlite::query("DELETE FROM items WHERE id = ?1 AND chat = ?2")
        .bind(id)
        .bind(chat_key(s, chat_ref).as_slice())
        .execute(c)
        .await?;
    Ok(())
}

impl Store {
    pub fn insert_item(&mut self, chat_ref: &[u8], v: &[u8]) -> Result<i64, StoreError> {
        let s = &self.sealer;
        self.db.atomic(async |c| insert(c, s, chat_ref, v).await)
    }

    pub fn update_item(&mut self, chat_ref: &[u8], id: i64, v: &[u8]) -> Result<(), StoreError> {
        let s = &self.sealer;
        self.db.run(async |c| update(c, s, chat_ref, id, v).await)
    }

    pub fn delete_item(&mut self, chat_ref: &[u8], id: i64) -> Result<(), StoreError> {
        let s = &self.sealer;
        self.db.run(async |c| delete(c, s, chat_ref, id).await)
    }

    /// The newest `count` items with id below `before` (all when `None`), oldest first.
    pub fn page(
        &mut self,
        chat_ref: &[u8],
        before: Option<i64>,
        count: u32,
    ) -> Result<Vec<(i64, Vec<u8>)>, StoreError> {
        let chat = chat_key(&self.sealer, chat_ref);
        let mut rows: Vec<(i64, Vec<u8>)> = self.db.run(async |c| {
            Ok(ormlite::query_as(
                "SELECT id, v FROM items WHERE chat = ?1 AND id < ?2 ORDER BY id DESC LIMIT ?3",
            )
            .bind(chat.as_slice())
            .bind(before.unwrap_or(i64::MAX))
            .bind(i64::from(count))
            .fetch_all(c)
            .await?)
        })?;
        rows.reverse();
        rows.into_iter()
            .map(|(id, sealed)| {
                let a = aad(&chat, id);
                Ok((id, self.sealer.open(&[&a[0], &a[1], &a[2]], &sealed)?))
            })
            .collect()
    }
}
