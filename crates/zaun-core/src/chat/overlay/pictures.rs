//! Profile pictures for the overlay: each is sent once and referenced by its key after
//! that. The service remembers the keys the last snapshot referenced (the overlay keeps
//! exactly those), so a picture that drops out of view and comes back is sent again.
use std::collections::BTreeSet;

use base64::Engine as _;

use super::snapshot::{OverlaySnapshot, Person, WirePicture};
use crate::model::{ChatSnapshot, Picture};

/// Every picture key the snapshot points at.
pub fn referenced(s: &OverlaySnapshot) -> BTreeSet<u64> {
    let mut people: Vec<&Person> = Vec::new();
    people.extend(s.me.as_ref());
    for c in &s.chats {
        people.push(&c.who);
        people.extend(c.last.as_ref().map(|m| &m.author));
    }
    if let Some(o) = &s.open {
        people.push(&o.who);
        people.extend(o.messages.iter().map(|m| &m.author));
    }
    if let Some(c) = &s.call {
        people.extend(c.people.iter().map(|p| &p.who));
    }
    people.extend(s.ringing.as_ref().map(|r| &r.who));
    people.extend(s.toasts.iter().map(|t| &t.who));
    people.iter().filter_map(|p| p.picture).collect()
}

/// Add the pictures `s` points at that are not in `held` (what the overlay has), and make
/// `held` what the overlay will hold after this snapshot.
pub fn attach(s: &mut OverlaySnapshot, chat: &ChatSnapshot, held: &mut BTreeSet<u64>) {
    let wanted = referenced(s);
    s.pictures = wanted
        .difference(held)
        .filter_map(|k| find(chat, *k))
        .map(|p| WirePicture {
            key: p.key(),
            data: base64::engine::general_purpose::STANDARD.encode(p.bytes()),
        })
        .collect();
    *held = wanted;
}

fn find(chat: &ChatSnapshot, key: u64) -> Option<&Picture> {
    let is = |p: &&Picture| p.key() == key;
    chat.profile
        .as_ref()
        .and_then(|p| p.picture.as_ref())
        .filter(is)
        .or_else(|| {
            chat.friends
                .iter()
                .filter_map(|f| f.picture.as_ref())
                .find(is)
        })
        .or_else(|| {
            chat.groups
                .iter()
                .filter_map(|g| g.picture.as_ref())
                .find(is)
        })
        .or_else(|| {
            chat.groups
                .iter()
                .flat_map(|g| &g.member_list)
                .filter_map(|m| m.picture.as_ref())
                .find(is)
        })
}
