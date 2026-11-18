//! Programmatic Windows shortcut files (MS-SHLLINK) for `identity::lnk` tests.

fn put16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

/// A shortcut as an installer writes it.
#[derive(Clone, Debug, Default)]
pub struct LnkSpec {
    /// `LinkInfo` local base path (`C:\…`).
    pub local_base_path: Option<String>,
    /// Write the Unicode variant of the local base path too.
    pub unicode_link_info: bool,
    /// Relative path string.
    pub relative_path: Option<String>,
    /// Working directory string.
    pub working_dir: Option<String>,
    /// Arguments string.
    pub arguments: Option<String>,
    /// Include a (dummy) ID list first.
    pub id_list: bool,
    /// Strings in UTF-16 (`IsUnicode`).
    pub unicode: bool,
}

/// The shortcut bytes.
pub fn build(spec: &LnkSpec) -> Vec<u8> {
    let mut flags = 0u32;
    if spec.id_list {
        flags |= 0x01;
    }
    if spec.local_base_path.is_some() {
        flags |= 0x02;
    }
    if spec.relative_path.is_some() {
        flags |= 0x08;
    }
    if spec.working_dir.is_some() {
        flags |= 0x10;
    }
    if spec.arguments.is_some() {
        flags |= 0x20;
    }
    if spec.unicode {
        flags |= 0x80;
    }
    let mut b = Vec::new();
    put32(&mut b, 0x4c);
    b.extend_from_slice(&[
        0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x46,
    ]);
    put32(&mut b, flags);
    b.resize(0x4c, 0);
    if spec.id_list {
        put16(&mut b, 4);
        b.extend_from_slice(&[2, 0, 0, 0]);
    }
    if let Some(base) = &spec.local_base_path {
        let header: u32 = if spec.unicode_link_info { 0x24 } else { 0x1c };
        let mut info = vec![0u8; header as usize];
        let base_off = info.len() as u32;
        info.extend_from_slice(base.as_bytes());
        info.push(0);
        let suffix_off = info.len() as u32;
        info.push(0);
        let mut ubase_off = 0;
        if spec.unicode_link_info {
            ubase_off = info.len() as u32;
            for w in base.encode_utf16().chain(std::iter::once(0)) {
                info.extend_from_slice(&w.to_le_bytes());
            }
        }
        let size = info.len() as u32;
        info[0..4].copy_from_slice(&size.to_le_bytes());
        info[4..8].copy_from_slice(&header.to_le_bytes());
        info[8..12].copy_from_slice(&1u32.to_le_bytes());
        info[16..20].copy_from_slice(&base_off.to_le_bytes());
        info[24..28].copy_from_slice(&suffix_off.to_le_bytes());
        if spec.unicode_link_info {
            info[28..32].copy_from_slice(&ubase_off.to_le_bytes());
        }
        b.extend_from_slice(&info);
    }
    for s in [&spec.relative_path, &spec.working_dir, &spec.arguments]
        .into_iter()
        .flatten()
    {
        if spec.unicode {
            let w: Vec<u16> = s.encode_utf16().collect();
            put16(&mut b, w.len() as u16);
            for x in w {
                b.extend_from_slice(&x.to_le_bytes());
            }
        } else {
            put16(&mut b, s.len() as u16);
            b.extend_from_slice(s.as_bytes());
        }
    }
    put32(&mut b, 0); // terminal block
    b
}
