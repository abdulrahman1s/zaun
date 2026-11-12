//! Programmatic PE files for `identity::pe` tests (no binaries are committed).
//!
//! [`PeBuilder`] writes a minimal, valid PE32+ (or PE32) image with optional version strings,
//! icons, imports, a `.bind` section and a CLR directory. [`malformed_corpus`] derives broken
//! variants from it: truncations, cyclic and exploding resource trees, bogus offsets.

/// File alignment used by the builder.
const FILE_ALIGN: u32 = 0x200;
/// Section alignment used by the builder.
const SECT_ALIGN: u32 = 0x1000;

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const RT_VERSION: u32 = 16;

fn align(v: u32, a: u32) -> u32 {
    v.div_ceil(a) * a
}

fn put16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn set16(b: &mut [u8], off: usize, v: u16) {
    b[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn set32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn pad4(b: &mut Vec<u8>) {
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
}

fn utf16z(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for w in s.encode_utf16().chain(std::iter::once(0)) {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out
}

/// One `VS_VERSIONINFO`-style node.
fn version_node(
    key: &str,
    value: &[u8],
    value_len: u16,
    text: bool,
    children: &[Vec<u8>],
) -> Vec<u8> {
    let mut b = vec![0, 0];
    put16(&mut b, value_len);
    put16(&mut b, u16::from(text));
    b.extend_from_slice(&utf16z(key));
    pad4(&mut b);
    b.extend_from_slice(value);
    pad4(&mut b);
    for c in children {
        b.extend_from_slice(c);
        pad4(&mut b);
    }
    let len = u16::try_from(b.len()).expect("version node too large");
    set16(&mut b, 0, len);
    b
}

/// A `VS_VERSIONINFO` resource with the given strings in table `040904B0`.
pub fn version_resource(strings: &[(&str, &str)]) -> Vec<u8> {
    let entries: Vec<Vec<u8>> = strings
        .iter()
        .map(|(k, v)| {
            let val = utf16z(v);
            version_node(k, &val, u16::try_from(val.len() / 2).unwrap(), true, &[])
        })
        .collect();
    let table = version_node("040904B0", &[], 0, true, &entries);
    let sfi = version_node("StringFileInfo", &[], 0, true, &[table]);
    let translation = [0x09, 0x04, 0xB0, 0x04];
    let var = version_node("Translation", &translation, 4, false, &[]);
    let vfi = version_node("VarFileInfo", &[], 0, true, &[var]);
    let mut fixed = Vec::new();
    put32(&mut fixed, 0xFEEF_04BD);
    put32(&mut fixed, 0x0001_0000);
    for _ in 0..11 {
        put32(&mut fixed, 0);
    }
    version_node("VS_VERSION_INFO", &fixed, 52, false, &[sfi, vfi])
}

/// One resource: `(type, id, bytes)`.
pub type Resource = (u32, u32, Vec<u8>);

/// Lay out a resource section at RVA `rva` (type → id → language 0x409 → data).
pub fn resource_section(rva: u32, resources: &[Resource]) -> Vec<u8> {
    let mut types: Vec<u32> = resources.iter().map(|r| r.0).collect();
    types.sort_unstable();
    types.dedup();
    let dir_size = |n: usize| 16 + 8 * n as u32;
    // Offsets: root, type dirs, name dirs, data entries, data.
    let mut off = dir_size(types.len());
    let mut type_dirs = Vec::new();
    for t in &types {
        let n = resources.iter().filter(|r| r.0 == *t).count();
        type_dirs.push(off);
        off += dir_size(n);
    }
    let mut name_dirs = Vec::new();
    for _ in resources {
        name_dirs.push(off);
        off += dir_size(1);
    }
    let mut data_entries = Vec::new();
    for _ in resources {
        data_entries.push(off);
        off += 16;
    }
    let mut data_offs = Vec::new();
    for r in resources {
        off = align(off, 8);
        data_offs.push(off);
        off += u32::try_from(r.2.len()).unwrap();
    }
    let mut b = vec![0u8; off as usize];
    let write_dir = |b: &mut Vec<u8>, at: u32, entries: &[(u32, u32)]| {
        let at = at as usize;
        set16(b, at + 14, u16::try_from(entries.len()).unwrap());
        for (i, (name, target)) in entries.iter().enumerate() {
            set32(b, at + 16 + i * 8, *name);
            set32(b, at + 16 + i * 8 + 4, *target);
        }
    };
    let root: Vec<(u32, u32)> = types
        .iter()
        .zip(&type_dirs)
        .map(|(t, d)| (*t, 0x8000_0000 | d))
        .collect();
    write_dir(&mut b, 0, &root);
    for (t, d) in types.iter().zip(&type_dirs) {
        let entries: Vec<(u32, u32)> = resources
            .iter()
            .enumerate()
            .filter(|(_, r)| r.0 == *t)
            .map(|(i, r)| (r.1, 0x8000_0000 | name_dirs[i]))
            .collect();
        write_dir(&mut b, *d, &entries);
    }
    for (i, r) in resources.iter().enumerate() {
        write_dir(&mut b, name_dirs[i], &[(0x409, data_entries[i])]);
        let de = data_entries[i] as usize;
        set32(&mut b, de, rva + data_offs[i]);
        set32(&mut b, de + 4, u32::try_from(r.2.len()).unwrap());
        let d = data_offs[i] as usize;
        b[d..d + r.2.len()].copy_from_slice(&r.2);
    }
    b
}

/// An icon image declared by the builder.
#[derive(Clone, Debug)]
pub struct IconSpec {
    /// Width (256 is written as 0).
    pub width: u32,
    /// Height.
    pub height: u32,
    /// Bits per pixel.
    pub bit_count: u16,
    /// Image bytes (content is not validated by the inspector).
    pub data: Vec<u8>,
}

/// Builder of a minimal PE file.
#[derive(Clone, Debug)]
pub struct PeBuilder {
    machine: u16,
    pe32: bool,
    dll: bool,
    subsystem: u16,
    timestamp: u32,
    strings: Vec<(String, String)>,
    icons: Vec<IconSpec>,
    imports: Vec<String>,
    delay_imports: Vec<String>,
    bind: bool,
    clr: bool,
    raw_resources: Option<Vec<u8>>,
    resource_padding: usize,
}

impl Default for PeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

struct Sect {
    name: [u8; 8],
    data: Vec<u8>,
    va: u32,
}

impl PeBuilder {
    /// An x86-64 GUI executable with nothing else.
    pub fn new() -> Self {
        Self {
            machine: 0x8664,
            pe32: false,
            dll: false,
            subsystem: 2,
            timestamp: 0x5F00_0000,
            strings: Vec::new(),
            icons: Vec::new(),
            imports: Vec::new(),
            delay_imports: Vec::new(),
            bind: false,
            clr: false,
            raw_resources: None,
            resource_padding: 0,
        }
    }

    /// 32-bit x86 PE32 image.
    #[must_use]
    pub fn x86(mut self) -> Self {
        self.machine = 0x14c;
        self.pe32 = true;
        self
    }

    /// Machine field.
    #[must_use]
    pub fn machine(mut self, m: u16) -> Self {
        self.machine = m;
        self
    }

    /// Mark as DLL.
    #[must_use]
    pub fn dll(mut self) -> Self {
        self.dll = true;
        self
    }

    /// Subsystem field (2 GUI, 3 console, 1 native).
    #[must_use]
    pub fn subsystem(mut self, s: u16) -> Self {
        self.subsystem = s;
        self
    }

    /// Link timestamp.
    #[must_use]
    pub fn timestamp(mut self, t: u32) -> Self {
        self.timestamp = t;
        self
    }

    /// Add a version string (`ProductName`, `CompanyName`, …).
    #[must_use]
    pub fn string(mut self, key: &str, value: &str) -> Self {
        self.strings.push((key.to_owned(), value.to_owned()));
        self
    }

    /// Add an icon of `len` bytes of filler.
    #[must_use]
    pub fn icon(mut self, width: u32, height: u32, bit_count: u16, len: usize) -> Self {
        let data = (0..len).map(|i| (i % 251) as u8).collect();
        self.icons.push(IconSpec {
            width,
            height,
            bit_count,
            data,
        });
        self
    }

    /// Import a DLL.
    #[must_use]
    pub fn import(mut self, dll: &str) -> Self {
        self.imports.push(dll.to_owned());
        self
    }

    /// Delay-load a DLL.
    #[must_use]
    pub fn delay_import(mut self, dll: &str) -> Self {
        self.delay_imports.push(dll.to_owned());
        self
    }

    /// Add a `.bind` section (SteamStub).
    #[must_use]
    pub fn bind_section(mut self) -> Self {
        self.bind = true;
        self
    }

    /// Set a CLR directory (.NET).
    #[must_use]
    pub fn clr(mut self) -> Self {
        self.clr = true;
        self
    }

    /// Use these bytes as the resource section verbatim.
    #[must_use]
    pub fn raw_resources(mut self, bytes: Vec<u8>) -> Self {
        self.raw_resources = Some(bytes);
        self
    }

    /// Append `n` zero bytes to the resource section (large-section tests).
    #[must_use]
    pub fn resource_padding(mut self, n: usize) -> Self {
        self.resource_padding = n;
        self
    }

    /// The RVA the resource section will have (for [`resource_section`] with raw bytes).
    pub const RSRC_RVA: u32 = 0x2000;

    fn resources(&self) -> Vec<Resource> {
        let mut r = Vec::new();
        if !self.strings.is_empty() {
            let s: Vec<(&str, &str)> = self
                .strings
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            r.push((RT_VERSION, 1, version_resource(&s)));
        }
        if !self.icons.is_empty() {
            let mut group = Vec::new();
            put16(&mut group, 0);
            put16(&mut group, 1);
            put16(&mut group, u16::try_from(self.icons.len()).unwrap());
            for (i, ic) in self.icons.iter().enumerate() {
                group.push(if ic.width >= 256 { 0 } else { ic.width as u8 });
                group.push(if ic.height >= 256 { 0 } else { ic.height as u8 });
                group.push(0);
                group.push(0);
                put16(&mut group, 1);
                put16(&mut group, ic.bit_count);
                put32(&mut group, u32::try_from(ic.data.len()).unwrap());
                put16(&mut group, u16::try_from(i + 1).unwrap());
                r.push((RT_ICON, u32::try_from(i + 1).unwrap(), ic.data.clone()));
            }
            r.push((RT_GROUP_ICON, 1, group));
        }
        r
    }

    fn import_section(names: &[String], stride: usize, name_off: usize, va: u32) -> Vec<u8> {
        let table = (names.len() + 1) * stride;
        let mut b = vec![0u8; table];
        for (i, n) in names.iter().enumerate() {
            let rva = va + u32::try_from(b.len()).unwrap();
            set32(&mut b, i * stride + name_off, rva);
            if stride == 32 {
                set32(&mut b, i * stride, 1); // attributes: RVA-based
            }
            b.extend_from_slice(n.as_bytes());
            b.push(0);
        }
        b
    }

    /// The file bytes.
    pub fn build(&self) -> Vec<u8> {
        let mut sections = vec![Sect {
            name: *b".text\0\0\0",
            data: vec![0xC3; 16],
            va: 0x1000,
        }];
        let mut rsrc = match &self.raw_resources {
            Some(raw) => raw.clone(),
            None => {
                let res = self.resources();
                if res.is_empty() {
                    Vec::new()
                } else {
                    resource_section(Self::RSRC_RVA, &res)
                }
            }
        };
        let has_rsrc = !rsrc.is_empty() || self.resource_padding > 0;
        rsrc.extend(std::iter::repeat_n(0u8, self.resource_padding));
        if has_rsrc {
            sections.push(Sect {
                name: *b".rsrc\0\0\0",
                data: rsrc,
                va: Self::RSRC_RVA,
            });
        }
        let mut next_va = align(
            Self::RSRC_RVA + sections.last().map_or(0, |s| s.data.len() as u32).max(1),
            SECT_ALIGN,
        )
        .max(Self::RSRC_RVA + SECT_ALIGN);
        let mut import_dir = (0, 0);
        if !self.imports.is_empty() {
            let data = Self::import_section(&self.imports, 20, 12, next_va);
            import_dir = (next_va, ((self.imports.len() + 1) * 20) as u32);
            let len = data.len() as u32;
            sections.push(Sect {
                name: *b".idata\0\0",
                data,
                va: next_va,
            });
            next_va = align(next_va + len, SECT_ALIGN);
        }
        let mut delay_dir = (0, 0);
        if !self.delay_imports.is_empty() {
            let data = Self::import_section(&self.delay_imports, 32, 4, next_va);
            delay_dir = (next_va, ((self.delay_imports.len() + 1) * 32) as u32);
            let len = data.len() as u32;
            sections.push(Sect {
                name: *b".didat\0\0",
                data,
                va: next_va,
            });
            next_va = align(next_va + len, SECT_ALIGN);
        }
        if self.bind {
            sections.push(Sect {
                name: *b".bind\0\0\0",
                data: vec![0; 32],
                va: next_va,
            });
            next_va = align(next_va + 32, SECT_ALIGN);
        }
        let opt_size: u16 = if self.pe32 { 96 + 128 } else { 112 + 128 };
        let headers_len = 0x40 + 4 + 20 + u32::from(opt_size) + 40 * sections.len() as u32;
        let size_of_headers = align(headers_len, FILE_ALIGN);

        let mut f = vec![0u8; size_of_headers as usize];
        f[0..2].copy_from_slice(b"MZ");
        set32(&mut f, 0x3c, 0x40);
        f[0x40..0x44].copy_from_slice(b"PE\0\0");
        let coff = 0x44;
        set16(&mut f, coff, self.machine);
        set16(&mut f, coff + 2, sections.len() as u16);
        set32(&mut f, coff + 4, self.timestamp);
        set16(&mut f, coff + 16, opt_size);
        set16(
            &mut f,
            coff + 18,
            0x0022 | if self.dll { 0x2000 } else { 0 },
        );
        let opt = coff + 20;
        set16(&mut f, opt, if self.pe32 { 0x10b } else { 0x20b });
        set32(&mut f, opt + 16, 0x1000); // entry point
        set32(&mut f, opt + 32, SECT_ALIGN);
        set32(&mut f, opt + 36, FILE_ALIGN);
        set32(&mut f, opt + 56, next_va); // SizeOfImage
        set32(&mut f, opt + 60, size_of_headers);
        set16(&mut f, opt + 68, self.subsystem);
        let (count_off, dirs_off) = if self.pe32 { (92, 96) } else { (108, 112) };
        set32(&mut f, opt + count_off, 16);
        let dirs = opt + dirs_off;
        if has_rsrc {
            let len = sections
                .iter()
                .find(|s| s.va == Self::RSRC_RVA)
                .map_or(0, |s| s.data.len());
            set32(&mut f, dirs + 2 * 8, Self::RSRC_RVA);
            set32(&mut f, dirs + 2 * 8 + 4, len as u32);
        }
        set32(&mut f, dirs + 8, import_dir.0);
        set32(&mut f, dirs + 8 + 4, import_dir.1);
        set32(&mut f, dirs + 13 * 8, delay_dir.0);
        set32(&mut f, dirs + 13 * 8 + 4, delay_dir.1);
        if self.clr {
            set32(&mut f, dirs + 14 * 8, 0x1000);
            set32(&mut f, dirs + 14 * 8 + 4, 72);
        }
        let mut table = opt + opt_size as usize;
        let mut raw_ptr = size_of_headers;
        let mut body = Vec::new();
        for s in &sections {
            let raw_size = align(s.data.len() as u32, FILE_ALIGN);
            f[table..table + 8].copy_from_slice(&s.name);
            set32(&mut f, table + 8, s.data.len() as u32);
            set32(&mut f, table + 12, s.va);
            set32(&mut f, table + 16, raw_size);
            set32(&mut f, table + 20, raw_ptr);
            set32(&mut f, table + 36, 0x4000_0040);
            let mut d = s.data.clone();
            d.resize(raw_size as usize, 0);
            body.extend_from_slice(&d);
            raw_ptr += raw_size;
            table += 40;
        }
        f.extend_from_slice(&body);
        f
    }
}

/// A typical game executable: version strings, three icons, network imports.
pub fn sample_game() -> Vec<u8> {
    PeBuilder::new()
        .string("ProductName", "Hollow Knight")
        .string("FileDescription", "Hollow Knight")
        .string("CompanyName", "Team Cherry")
        .string("OriginalFilename", "hollow_knight.exe")
        .string("FileVersion", "1.5.78.11833")
        .icon(16, 16, 32, 300)
        .icon(256, 256, 32, 2000)
        .icon(48, 48, 32, 900)
        .import("KERNEL32.dll")
        .import("WS2_32.dll")
        .delay_import("winhttp.dll")
        .build()
}

/// A resource section whose root directory has one entry pointing back at the root.
pub fn cyclic_resources() -> Vec<u8> {
    let mut b = vec![0u8; 64];
    set16(&mut b, 14, 1);
    set32(&mut b, 16, RT_VERSION);
    set32(&mut b, 20, 0x8000_0000);
    b
}

/// A resource section declaring `n` entries in its root (each a self-referencing directory).
pub fn exploding_resources(n: u16) -> Vec<u8> {
    let mut b = vec![0u8; 16 + 8 * usize::from(n)];
    set16(&mut b, 14, n);
    for i in 0..usize::from(n) {
        set32(&mut b, 16 + i * 8, i as u32);
        set32(&mut b, 16 + i * 8 + 4, 0x8000_0000);
    }
    b
}

/// Small deterministic generator for byte flips.
fn lcg(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state >> 33
}

/// Broken PE files with a label each: every one must yield an error or partial info, never a
/// panic or a hang.
pub fn malformed_corpus() -> Vec<(String, Vec<u8>)> {
    let good = sample_game();
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for cut in [
        0,
        1,
        2,
        63,
        64,
        0x44,
        0x50,
        0x100,
        0x200,
        0x400,
        good.len() / 2,
        good.len() - 1,
    ] {
        out.push((
            format!("truncated at {cut}"),
            good[..cut.min(good.len())].to_vec(),
        ));
    }
    let mut lfanew = good.clone();
    set32(&mut lfanew, 0x3c, 0xFFFF_FFF0);
    out.push(("e_lfanew past end".into(), lfanew));
    let mut nsec = good.clone();
    set16(&mut nsec, 0x46, 0xFFFF);
    out.push(("65535 sections".into(), nsec));
    let mut optsz = good.clone();
    set16(&mut optsz, 0x44 + 16, 0xFFFF);
    out.push(("huge optional header".into(), optsz));
    let mut magic = good.clone();
    set16(&mut magic, 0x58, 0x1234);
    out.push(("bad optional magic".into(), magic));
    out.push((
        "cyclic resource directory".into(),
        PeBuilder::new().raw_resources(cyclic_resources()).build(),
    ));
    out.push((
        "exploding resource directory".into(),
        PeBuilder::new()
            .raw_resources(exploding_resources(60_000))
            .build(),
    ));
    let mut bogus_count = vec![0u8; 32];
    set16(&mut bogus_count, 12, 0xFFFF);
    set16(&mut bogus_count, 14, 0xFFFF);
    out.push((
        "entry count beyond section".into(),
        PeBuilder::new().raw_resources(bogus_count).build(),
    ));
    let mut v = version_resource(&[("ProductName", "X")]);
    set16(&mut v, 0, 0xFFFF);
    out.push((
        "version length too large".into(),
        PeBuilder::new()
            .raw_resources(resource_section(PeBuilder::RSRC_RVA, &[(RT_VERSION, 1, v)]))
            .build(),
    ));
    let mut group = vec![0u8; 6];
    set16(&mut group, 2, 1);
    set16(&mut group, 4, 0xFFFF);
    out.push((
        "icon group with huge count".into(),
        PeBuilder::new()
            .raw_resources(resource_section(
                PeBuilder::RSRC_RVA,
                &[(RT_GROUP_ICON, 1, group)],
            ))
            .build(),
    ));
    let mut state = 0x0123_4567_89AB_CDEF_u64;
    for i in 0..64 {
        let mut b = good.clone();
        for _ in 0..8 {
            let pos = (lcg(&mut state) as usize) % b.len().min(0x1200);
            b[pos] = lcg(&mut state) as u8;
        }
        out.push((format!("random flips #{i}"), b));
    }
    out
}
