//! ShowCode and Verify: a friend's security code, read and compared (Commands.hs
//! `APIGetContactCode`, `APIVerifyContact`; Internal.hs `getConnectionCode`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::ChatNotice;

use crate::crypto::sha256;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::FriendRecord;

/// Types.hs `verificationCode`: the hash as a big-endian number in decimal, in groups of five.
fn verification_code(hash: &[u8; 32]) -> String {
    let mut n = hash.to_vec();
    let mut digits = Vec::new();
    while n.iter().any(|&b| b != 0) {
        let mut rem = 0u32;
        for b in &mut n {
            let cur = rem * 256 + u32::from(*b);
            *b = u8::try_from(cur / 10).unwrap_or(0); // cur < 2560, so the quotient fits
            rem = cur % 10;
        }
        digits.push(b'0' + u8::try_from(rem).unwrap_or(0));
    }
    if digits.is_empty() {
        digits.push(b'0');
    }
    digits.reverse();
    digits
        .chunks(5)
        .map(|g| g.iter().map(|&d| char::from(d)).collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Types.hs `sameVerificationCode`: spaces do not count.
fn same_code(a: &str, b: &str) -> bool {
    let bare = |s: &str| s.chars().filter(|&c| c != ' ').collect::<String>();
    bare(a) == bare(b)
}

impl<A: AgentPort> Engine<A> {
    /// `getConnectionCode`: the SHA-256 of the ratchet's associated data.
    fn connection_code(&mut self, f: &FriendRecord) -> Result<String> {
        let ad = self.agent.ratchet_ad(&mut self.store, &f.conn)?;
        Ok(verification_code(&sha256(&ad)))
    }

    /// A code that no longer matches the verified one ends the verification (Commands.hs
    /// `APIGetContactCode`); a ratchet re-sync does the same (Subscriber.hs `securityCodeChanged`).
    pub(crate) fn security_code_changed(&mut self, friend: i64) -> Result<()> {
        let mut f = self.friend(friend)?;
        if f.verified.take().is_none() {
            return Ok(());
        }
        self.save_friend(f)?;
        self.notice(friend, ChatNotice::CodeChanged)?;
        self.state.chats_dirty = true;
        Ok(())
    }

    pub(crate) fn show_code(&mut self, friend: i64) -> Result<()> {
        let f = self.friend(friend)?;
        let code = self.connection_code(&f)?;
        if f.verified.as_deref().is_some_and(|v| !same_code(v, &code)) {
            self.security_code_changed(friend)?;
        }
        self.emit(Event::Code { friend, code });
        Ok(())
    }

    /// An empty code takes the verification back (`/_verify code` without one).
    pub(crate) fn verify(&mut self, friend: i64, code: &str) -> Result<()> {
        let mut f = self.friend(friend)?;
        let ours = self.connection_code(&f)?;
        let given = code.trim();
        let matched = !given.is_empty() && same_code(given, &ours);
        if matched {
            f.verified = Some(ours);
        } else if given.is_empty() {
            f.verified = None;
        }
        self.save_friend(f)?;
        self.state.chats_dirty = true;
        self.emit(Event::Verified { friend, matched });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_is_the_hash_in_decimal_in_groups_of_five() {
        let mut hash = [0u8; 32];
        hash[31] = 1;
        assert_eq!(verification_code(&hash), "1");
        hash[30] = 1;
        assert_eq!(verification_code(&hash), "257");
        assert_eq!(verification_code(&[0u8; 32]), "0");
        assert_eq!(
            verification_code(&[0xff; 32]),
            "11579 20892 37316 19542 35709 85008 68790 78532 69984 66564 05640 39457 58400 79131 29639 935"
        );
    }
}
