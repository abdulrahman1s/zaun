//! A batch of writes applied in one transaction.

use super::StoreError;
use super::handle::Store;
use super::{files, items, kv};

/// One write. `apply` runs them in order and rolls all back if any fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Put {
        ns: &'static str,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Delete {
        ns: &'static str,
        key: Vec<u8>,
    },
    InsertItem {
        chat_ref: Vec<u8>,
        value: Vec<u8>,
    },
    UpdateItem {
        chat_ref: Vec<u8>,
        id: i64,
        value: Vec<u8>,
    },
    DeleteItem {
        chat_ref: Vec<u8>,
        id: i64,
    },
    PutFile {
        id: i64,
        value: Vec<u8>,
    },
    DeleteFile {
        id: i64,
    },
}

impl Store {
    /// Applies `ops` atomically. Returns the ids of the `InsertItem` ops, in order.
    pub fn apply(&mut self, ops: Vec<Op>) -> Result<Vec<i64>, StoreError> {
        let s = &self.sealer;
        self.db.atomic(async |c| {
            let mut ids = Vec::new();
            for op in &ops {
                match op {
                    Op::Put { ns, key, value } => kv::put(c, s, ns, key, value).await?,
                    Op::Delete { ns, key } => kv::delete(c, s, ns, key).await?,
                    Op::InsertItem { chat_ref, value } => {
                        ids.push(items::insert(c, s, chat_ref, value).await?)
                    }
                    Op::UpdateItem {
                        chat_ref,
                        id,
                        value,
                    } => items::update(c, s, chat_ref, *id, value).await?,
                    Op::DeleteItem { chat_ref, id } => items::delete(c, s, chat_ref, *id).await?,
                    Op::PutFile { id, value } => files::put(c, s, *id, value).await?,
                    Op::DeleteFile { id } => files::delete(c, *id).await?,
                }
            }
            Ok(ids)
        })
    }
}
