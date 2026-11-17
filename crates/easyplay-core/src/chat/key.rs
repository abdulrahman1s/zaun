//! The chat database key (ADR-0033): 32 random bytes, hex, in `chat/key`, mode 0600,
//! made the first time Chat is turned on and read on every start.
//!
//! Games cannot read it: each sandbox binds only its own prefix, never EasyPlay's data
//! folder (`sandbox_validate` checks this). Programs the user runs outside EasyPlay can,
//! as with any desktop chat client; a passphrase lock is ROADMAP R-28.

use std::path::Path;

use crate::error::Result;
use crate::fsx::Fs;

/// The key file's name inside the Chat folder.
pub const KEY_FILE: &str = "key";
/// Random bytes in a key.
pub const KEY_BYTES: usize = 32;

/// Read the key in `dir`, or make one when there is none. `random` fills a buffer with
/// random bytes (the operating system's generator in production).
pub fn load_or_create(
    fs: &dyn Fs,
    dir: &Path,
    random: &dyn Fn(&mut [u8]) -> std::io::Result<()>,
) -> Result<String> {
    let path = dir.join(KEY_FILE);
    if fs.exists(&path) {
        let bytes = fs.read(&path, 4096)?;
        let text = String::from_utf8_lossy(&bytes).trim().to_owned();
        if text.len() == KEY_BYTES * 2 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(text);
        }
        // A key that is not one would make the database unreadable; never replace it.
        bail!(ChtDatabase, reason = "the key file is damaged");
    }
    fs.create_dir_all(dir, 0o700)?;
    let mut raw = [0u8; KEY_BYTES];
    random(&mut raw).map_err(|e| err!(ChtDatabase).with_source(e))?;
    let text: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    fs.atomic_write(&path, text.as_bytes(), 0o600)?;
    Ok(text)
}

/// The operating system's random generator.
pub fn os_random(buf: &mut [u8]) -> std::io::Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = rustix::rand::getrandom(&mut buf[filled..], rustix::rand::GetRandomFlags::empty())?;
        filled += n;
    }
    Ok(())
}
