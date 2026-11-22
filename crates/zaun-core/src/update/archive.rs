//! Unpack a bundle folder (ADR-0041 §3): a tar of plain files in one folder, already
//! verified against its signed hash. Anything else in it (a link, a device, a subfolder, a
//! path with `/`) refuses the whole folder, so unpacking can only create files inside it.

use std::io::Read;

use super::UpdateError;
use super::source::{MAX_FILE, MAX_FOLDER_FILES};

const BLOCK: usize = 512;

/// One file of the archive.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
}

/// Read every entry of `tar`, calling `each` with it in archive order.
pub fn unpack(
    mut tar: impl Read,
    mut each: impl FnMut(Entry) -> Result<(), UpdateError>,
) -> Result<usize, UpdateError> {
    let mut count = 0;
    let mut long_name: Option<String> = None;
    loop {
        let mut header = [0u8; BLOCK];
        if !read_block(&mut tar, &mut header)? || header.iter().all(|&b| b == 0) {
            return Ok(count);
        }
        let size = octal(&header[124..136])?;
        if size > MAX_FILE {
            return Err(bad("a file is too large"));
        }
        let data = read_data(&mut tar, size)?;
        match header[156] {
            // GNU long name: the next header's name.
            b'L' => long_name = Some(text(&data)?),
            // Directory: only the archive's own `./`.
            b'5' => {
                if !matches!(
                    name_of(&header, long_name.take())?.as_str(),
                    "." | "./" | ""
                ) {
                    return Err(bad("a subfolder"));
                }
            }
            b'0' | 0 => {
                let name = name_of(&header, long_name.take())?;
                let name = name.strip_prefix("./").unwrap_or(&name).to_owned();
                if !plain_name(&name) {
                    return Err(bad("a file name"));
                }
                count += 1;
                if count > MAX_FOLDER_FILES {
                    return Err(bad("too many files"));
                }
                each(Entry { name, data })?;
            }
            _ => return Err(bad("an entry that is not a plain file")),
        }
    }
}

fn bad(what: &str) -> UpdateError {
    UpdateError::bad_file("folder", format!("the archive has {what}"))
}

/// A whole block, `false` at a clean end of the stream.
fn read_block(r: &mut impl Read, block: &mut [u8; BLOCK]) -> Result<bool, UpdateError> {
    let mut got = 0;
    while got < BLOCK {
        match r.read(&mut block[got..]) {
            Ok(0) if got == 0 => return Ok(false),
            Ok(0) => return Err(bad("a cut header")),
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(UpdateError::bad_file("folder", e.to_string())),
        }
    }
    Ok(true)
}

/// `size` bytes, then the padding to the next block.
fn read_data(r: &mut impl Read, size: u64) -> Result<Vec<u8>, UpdateError> {
    let padded = size.div_ceil(BLOCK as u64) * BLOCK as u64;
    let mut data = Vec::new();
    r.take(padded)
        .read_to_end(&mut data)
        .map_err(|e| UpdateError::bad_file("folder", e.to_string()))?;
    if data.len() as u64 != padded {
        return Err(bad("a cut file"));
    }
    data.truncate(size as usize);
    Ok(data)
}

fn octal(field: &[u8]) -> Result<u64, UpdateError> {
    let s = std::str::from_utf8(field).map_err(|_| bad("a bad size"))?;
    let s = s.trim_matches(|c: char| c == '\0' || c == ' ');
    u64::from_str_radix(s, 8).map_err(|_| bad("a bad size"))
}

fn text(bytes: &[u8]) -> Result<String, UpdateError> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec()).map_err(|_| bad("a name that is not UTF-8"))
}

fn name_of(header: &[u8; BLOCK], long: Option<String>) -> Result<String, UpdateError> {
    if let Some(n) = long {
        return Ok(n);
    }
    let name = text(&header[0..100])?;
    // POSIX ustar: a prefix field joins the name with `/`, so it can only name a subfolder.
    // (GNU tar, which the release workflow uses, keeps other fields there.)
    if &header[257..263] == b"ustar\0" && header[345] != 0 {
        return Err(bad("a subfolder"));
    }
    Ok(name)
}

/// Letters, digits and `._+-`, not starting with a dot: no separator, no `..`.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(name: &str, kind: u8, size: usize) -> Vec<u8> {
        let mut h = vec![0u8; BLOCK];
        h[..name.len()].copy_from_slice(name.as_bytes());
        let size = format!("{size:011o}\0");
        h[124..136].copy_from_slice(size.as_bytes());
        h[156] = kind;
        h[257..263].copy_from_slice(b"ustar ");
        h
    }

    fn entry(out: &mut Vec<u8>, name: &str, kind: u8, data: &[u8]) {
        out.extend(header(name, kind, data.len()));
        out.extend(data);
        out.resize(out.len().div_ceil(BLOCK) * BLOCK, 0);
    }

    fn names(tar: &[u8]) -> Result<Vec<String>, UpdateError> {
        let mut got = Vec::new();
        unpack(tar, |e| {
            got.push(e.name);
            Ok(())
        })?;
        Ok(got)
    }

    #[test]
    fn plain_files_and_long_names_unpack() {
        let long = "libHS".to_owned() + &"x".repeat(120) + ".so";
        let mut tar = Vec::new();
        entry(&mut tar, "./", b'5', b"");
        entry(&mut tar, "./libsimplex.so", b'0', b"elf");
        entry(
            &mut tar,
            "././@LongLink",
            b'L',
            format!("./{long}\0").as_bytes(),
        );
        entry(&mut tar, "./truncated", b'0', b"hs");
        tar.extend([0u8; 2 * BLOCK]);
        assert_eq!(names(&tar).unwrap(), vec!["libsimplex.so".to_owned(), long]);
    }

    #[test]
    fn links_subfolders_and_escapes_refuse_the_folder() {
        for (name, kind) in [
            ("./link", b'2'),
            ("./sub/", b'5'),
            ("../escape", b'0'),
            ("sub/file", b'0'),
            ("/etc/passwd", b'0'),
            (".hidden", b'0'),
        ] {
            let mut tar = Vec::new();
            entry(&mut tar, name, kind, b"x");
            assert!(names(&tar).is_err(), "{name} was accepted");
        }
    }

    #[test]
    fn a_cut_archive_is_refused() {
        let mut tar = Vec::new();
        entry(&mut tar, "file", b'0', &[1u8; 600]);
        tar.truncate(BLOCK + 100);
        assert!(names(&tar).is_err());
    }
}
