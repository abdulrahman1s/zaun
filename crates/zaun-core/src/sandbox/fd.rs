//! Descriptor payloads as sealed memfds (BUBBLEWRAP_SANDBOX §3 [V: F-213]).
//!
//! Used by `session::spawn`, never by the planner. Each payload becomes an anonymous file
//! whose size and contents are sealed before it is passed to `bwrap`.

use std::io;
use std::os::fd::OwnedFd;

use rustix::fs::{MemfdFlags, SealFlags, SeekFrom, fcntl_add_seals, memfd_create, seek};

/// Create a sealed, close-on-exec memfd holding `bytes`, positioned at offset 0.
pub fn sealed_memfd(name: &str, bytes: &[u8]) -> io::Result<OwnedFd> {
    let fd = memfd_create(name, MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING)?;
    let mut written = 0;
    while written < bytes.len() {
        let n = rustix::io::write(&fd, &bytes[written..])?;
        if n == 0 {
            return Err(io::Error::from(io::ErrorKind::WriteZero));
        }
        written += n;
    }
    seek(&fd, SeekFrom::Start(0))?;
    fcntl_add_seals(
        &fd,
        SealFlags::SHRINK | SealFlags::GROW | SealFlags::WRITE | SealFlags::SEAL,
    )?;
    Ok(fd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_memfd_is_readable_and_sealed() {
        let fd = sealed_memfd("zaun-test", b"a\0b\0").unwrap();
        let mut buf = [0u8; 8];
        let n = rustix::io::read(&fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"a\0b\0");
        assert!(rustix::io::write(&fd, b"x").is_err());
    }
}
