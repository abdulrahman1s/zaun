//! `MemFs`: an in-memory [`Fs`] with symlinks, mount (device) IDs, error injection and call
//! counting.
//!
//! Semantics follow Linux where tests depend on them: intermediate symlinks are resolved,
//! the last component is followed only by `*_follow` methods (and by `read_dir` and
//! `canonicalize`, as in std), `O_NOFOLLOW` reads of a symlink fail with
//! [`FsErrorKind::Symlink`], renames across devices fail with [`FsErrorKind::CrossDevice`].
//! Modes are stored exactly as given (no umask).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ffi::OsString;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use parking_lot::Mutex;
use zaun_core::fsx::{DirEntry, FileType, Fs, FsError, FsErrorKind, FsOp, FsResult, Metadata};

const MAX_SYMLINKS: usize = 40;
/// Device ID of the root filesystem.
pub const ROOT_DEVICE: u64 = 1;

#[derive(Clone, Debug)]
enum Kind {
    File(Vec<u8>),
    Dir,
    Symlink(PathBuf),
    CharDevice,
    Fifo,
    Socket,
}

#[derive(Clone, Debug)]
struct Node {
    kind: Kind,
    mode: u32,
    uid: u32,
    gid: u32,
    mtime_ns: i64,
    ino: u64,
    rdev: u64,
}

#[derive(Clone, Debug)]
struct Fault {
    op: FsOp,
    prefix: PathBuf,
    kind: FsErrorKind,
    remaining: Option<usize>,
}

#[derive(Debug)]
struct Mount {
    point: PathBuf,
    device: u64,
    fs_type: String,
}

#[derive(Debug)]
struct Inner {
    nodes: BTreeMap<PathBuf, Node>,
    mounts: Vec<Mount>,
    next_ino: u64,
    uid: u32,
    gid: u32,
    now_ns: i64,
    faults: Vec<Fault>,
    calls: HashMap<FsOp, usize>,
    free_space: u64,
    reflink: bool,
}

/// The in-memory filesystem.
#[derive(Debug)]
pub struct MemFs {
    inner: Mutex<Inner>,
}

impl Default for MemFs {
    fn default() -> Self {
        Self::new()
    }
}

fn err(op: FsOp, path: &Path, kind: FsErrorKind) -> FsError {
    FsError::new(op, path, kind)
}

impl Inner {
    fn new_node(&mut self, kind: Kind, mode: u32) -> Node {
        self.next_ino += 1;
        Node {
            kind,
            mode,
            uid: self.uid,
            gid: self.gid,
            mtime_ns: self.now_ns,
            ino: self.next_ino,
            rdev: 0,
        }
    }

    fn device_of(&self, path: &Path) -> u64 {
        self.mounts
            .iter()
            .filter(|m| path.starts_with(&m.point))
            .max_by_key(|m| m.point.components().count())
            .map_or(ROOT_DEVICE, |m| m.device)
    }

    fn check_fault(&mut self, op: FsOp, path: &Path) -> FsResult<()> {
        *self.calls.entry(op).or_insert(0) += 1;
        let normalized = zaun_core::paths::normalize(path);
        if let Some(i) = self
            .faults
            .iter()
            .position(|f| f.op == op && normalized.starts_with(&f.prefix))
        {
            let kind = self.faults[i].kind;
            if let Some(n) = &mut self.faults[i].remaining {
                *n -= 1;
                if *n == 0 {
                    self.faults.remove(i);
                }
            }
            return Err(err(op, path, kind));
        }
        Ok(())
    }

    /// Resolve `path`; intermediate symlinks are followed, the last one only with `follow`.
    /// Returns the resolved path, which may not exist (its parent does).
    fn resolve(&self, op: FsOp, path: &Path, follow: bool) -> FsResult<PathBuf> {
        if !path.is_absolute() {
            return Err(err(op, path, FsErrorKind::InvalidInput));
        }
        let mut cur = PathBuf::from("/");
        let mut queue: VecDeque<OsString> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(n) => Some(n.to_owned()),
                Component::ParentDir => Some("..".into()),
                _ => None,
            })
            .collect();
        let mut links = 0;
        while let Some(comp) = queue.pop_front() {
            if comp == ".." {
                cur.pop();
                continue;
            }
            let next = cur.join(&comp);
            let last = queue.is_empty();
            match self.nodes.get(&next) {
                None if last => return Ok(next),
                None => return Err(err(op, path, FsErrorKind::NotFound)),
                Some(Node {
                    kind: Kind::Symlink(target),
                    ..
                }) if !last || follow => {
                    links += 1;
                    if links > MAX_SYMLINKS {
                        return Err(err(op, path, FsErrorKind::Symlink));
                    }
                    let mut new_queue: VecDeque<OsString> = target
                        .components()
                        .filter_map(|c| match c {
                            Component::Normal(n) => Some(n.to_owned()),
                            Component::ParentDir => Some("..".into()),
                            _ => None,
                        })
                        .collect();
                    new_queue.extend(queue.drain(..));
                    queue = new_queue;
                    if target.is_absolute() {
                        cur = PathBuf::from("/");
                    }
                }
                Some(Node {
                    kind: Kind::Dir, ..
                }) => cur = next,
                Some(_) if last => return Ok(next),
                Some(_) => return Err(err(op, path, FsErrorKind::NotADirectory)),
            }
        }
        Ok(cur)
    }

    fn existing(&self, op: FsOp, path: &Path, follow: bool) -> FsResult<(PathBuf, &Node)> {
        let r = self.resolve(op, path, follow)?;
        match self.nodes.get(&r) {
            Some(n) => Ok((r, n)),
            None => Err(err(op, path, FsErrorKind::NotFound)),
        }
    }

    /// Resolve a path to be created: it must not exist and its parent must be a directory.
    fn creatable(&self, op: FsOp, path: &Path) -> FsResult<PathBuf> {
        let r = self.resolve(op, path, false)?;
        if self.nodes.contains_key(&r) {
            return Err(err(op, path, FsErrorKind::AlreadyExists));
        }
        self.parent_is_dir(op, path, &r)?;
        Ok(r)
    }

    fn parent_is_dir(&self, op: FsOp, path: &Path, r: &Path) -> FsResult<()> {
        let parent = r.parent().unwrap_or(Path::new("/"));
        match self.nodes.get(parent) {
            Some(Node {
                kind: Kind::Dir, ..
            }) => Ok(()),
            Some(_) => Err(err(op, path, FsErrorKind::NotADirectory)),
            None => Err(err(op, path, FsErrorKind::NotFound)),
        }
    }

    fn metadata_of(&self, path: &Path, node: &Node) -> Metadata {
        let (file_type, size) = match &node.kind {
            Kind::File(d) => (FileType::File, d.len() as u64),
            Kind::Dir => (FileType::Dir, 4096),
            Kind::Symlink(t) => (FileType::Symlink, t.as_os_str().len() as u64),
            Kind::CharDevice => (FileType::CharDevice, 0),
            Kind::Fifo => (FileType::Fifo, 0),
            Kind::Socket => (FileType::Socket, 0),
        };
        let nlink = match node.kind {
            Kind::Dir => {
                2 + self
                    .children(path)
                    .filter(|(_, n)| matches!(n.kind, Kind::Dir))
                    .count() as u64
            }
            _ => 1,
        };
        Metadata {
            file_type,
            size,
            mtime_ns: node.mtime_ns,
            inode: node.ino,
            device: self.device_of(path),
            mode: node.mode & 0o7777,
            nlink,
            uid: node.uid,
            gid: node.gid,
            rdev: node.rdev,
        }
    }

    fn children<'a>(&'a self, dir: &'a Path) -> impl Iterator<Item = (&'a PathBuf, &'a Node)> + 'a {
        self.nodes
            .range(dir.to_path_buf()..)
            .skip(1)
            .take_while(move |(p, _)| p.starts_with(dir))
            .filter(move |(p, _)| p.parent() == Some(dir))
    }

    fn file_data(&self, op: FsOp, path: &Path, follow: bool) -> FsResult<Vec<u8>> {
        let (_, node) = self.existing(op, path, follow)?;
        match &node.kind {
            Kind::File(d) => Ok(d.clone()),
            Kind::Dir => Err(err(op, path, FsErrorKind::IsADirectory)),
            Kind::Symlink(_) => Err(err(op, path, FsErrorKind::Symlink)),
            Kind::CharDevice | Kind::Fifo | Kind::Socket => Ok(Vec::new()),
        }
    }
}

impl MemFs {
    /// An empty filesystem containing only `/`, owned by uid/gid 1000.
    pub fn new() -> Self {
        let mut inner = Inner {
            nodes: BTreeMap::new(),
            mounts: Vec::new(),
            next_ino: 1,
            uid: 1000,
            gid: 1000,
            now_ns: 1_794_441_600_000_000_000,
            faults: Vec::new(),
            calls: HashMap::new(),
            free_space: 100 * 1024 * 1024 * 1024,
            reflink: false,
        };
        let root = inner.new_node(Kind::Dir, 0o755);
        inner.nodes.insert(PathBuf::from("/"), root);
        Self {
            inner: Mutex::new(inner),
        }
    }

    // ---- builders (panic on misuse: test setup errors are bugs) -----------------------

    /// Create a directory and its parents (mode 0755).
    pub fn dir(&self, path: impl AsRef<Path>) -> &Self {
        // Already a directory: nothing to do. `create_dir_all` would find that out through
        // `metadata` of every ancestor, whose link count scans everything below it, so a
        // setup loop adding thousands of files to one folder was quadratic (11 s for 10 000).
        {
            let g = self.inner.lock();
            let existing = g.resolve(FsOp::CreateDir, path.as_ref(), true).ok();
            if let Some(Node {
                kind: Kind::Dir, ..
            }) = existing.and_then(|r| g.nodes.get(&r))
            {
                return self;
            }
        }
        self.create_dir_all(path.as_ref(), 0o755)
            .unwrap_or_else(|e| panic!("MemFs::dir {}: {e}", path.as_ref().display()));
        self
    }

    /// Create a file (parents created), replacing an existing one.
    pub fn file(&self, path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> &Self {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            self.dir(parent);
        }
        self.atomic_write(path, data.as_ref(), 0o644)
            .unwrap_or_else(|e| panic!("MemFs::file {}: {e}", path.display()));
        self
    }

    /// Create a symlink `link` → `target` (parents created).
    pub fn link(&self, link: impl AsRef<Path>, target: impl AsRef<Path>) -> &Self {
        let link = link.as_ref();
        if let Some(parent) = link.parent() {
            self.dir(parent);
        }
        self.symlink(target.as_ref(), link)
            .unwrap_or_else(|e| panic!("MemFs::link {}: {e}", link.display()));
        self
    }

    /// Create a character device node (parents created).
    pub fn char_device(&self, path: impl AsRef<Path>, rdev: u64) -> &Self {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            self.dir(parent);
        }
        let mut g = self.inner.lock();
        let mut node = g.new_node(Kind::CharDevice, 0o660);
        node.rdev = rdev;
        g.nodes.insert(path.to_owned(), node);
        drop(g);
        self
    }

    /// Create a FIFO (parents created).
    pub fn fifo(&self, path: impl AsRef<Path>) -> &Self {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            self.dir(parent);
        }
        let mut g = self.inner.lock();
        let node = g.new_node(Kind::Fifo, 0o600);
        g.nodes.insert(path.to_owned(), node);
        drop(g);
        self
    }

    /// Create a Unix socket file (parents created). Creating it again replaces it with a new
    /// inode, as a restarted server does.
    pub fn socket(&self, path: impl AsRef<Path>) -> &Self {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            self.dir(parent);
        }
        let mut g = self.inner.lock();
        let node = g.new_node(Kind::Socket, 0o755);
        g.nodes.insert(path.to_owned(), node);
        drop(g);
        self
    }

    /// Declare a mount: everything at or below `point` has device `device`. The directory is
    /// created when missing.
    pub fn mount(&self, point: impl AsRef<Path>, device: u64, fs_type: &str) -> &Self {
        self.dir(point.as_ref());
        self.inner.lock().mounts.push(Mount {
            point: point.as_ref().to_owned(),
            device,
            fs_type: fs_type.to_owned(),
        });
        self
    }

    /// `mountinfo` text describing the declared mounts (and `/`), for `MountTable::parse`.
    pub fn mountinfo(&self) -> String {
        let g = self.inner.lock();
        let mut out = String::from("1 0 0:1 / / rw - ext4 /dev/root rw\n");
        for (i, m) in g.mounts.iter().enumerate() {
            out.push_str(&format!(
                "{} 1 0:{} / {} rw - {} memfs rw\n",
                i + 2,
                m.device,
                m.point.display().to_string().replace(' ', "\\040"),
                m.fs_type
            ));
        }
        out
    }

    /// Owner of nodes created from now on.
    pub fn set_owner(&self, uid: u32, gid: u32) -> &Self {
        let mut g = self.inner.lock();
        g.uid = uid;
        g.gid = gid;
        drop(g);
        self
    }

    /// Change the owner of an existing node (no following).
    pub fn chown(&self, path: impl AsRef<Path>, uid: u32, gid: u32) -> &Self {
        let mut g = self.inner.lock();
        let node = g
            .nodes
            .get_mut(path.as_ref())
            .unwrap_or_else(|| panic!("MemFs::chown {}: missing", path.as_ref().display()));
        node.uid = uid;
        node.gid = gid;
        drop(g);
        self
    }

    /// Set the modification time of an existing node.
    pub fn set_mtime(&self, path: impl AsRef<Path>, mtime_ns: i64) -> &Self {
        let mut g = self.inner.lock();
        if let Some(n) = g.nodes.get_mut(path.as_ref()) {
            n.mtime_ns = mtime_ns;
        }
        drop(g);
        self
    }

    /// Modification time given to nodes created from now on.
    pub fn set_now(&self, mtime_ns: i64) -> &Self {
        self.inner.lock().now_ns = mtime_ns;
        self
    }

    /// Value returned by `free_space`.
    pub fn set_free_space(&self, bytes: u64) -> &Self {
        self.inner.lock().free_space = bytes;
        self
    }

    /// Value returned by `reflink_supported`.
    pub fn set_reflink(&self, supported: bool) -> &Self {
        self.inner.lock().reflink = supported;
        self
    }

    // ---- error injection and call counting ---------------------------------------------

    /// Make every `op` on `path` or below fail with `kind`.
    pub fn fail(&self, op: FsOp, path: impl AsRef<Path>, kind: FsErrorKind) -> &Self {
        self.inner.lock().faults.push(Fault {
            op,
            prefix: path.as_ref().to_owned(),
            kind,
            remaining: None,
        });
        self
    }

    /// Make the next `times` matching operations fail.
    pub fn fail_times(
        &self,
        op: FsOp,
        path: impl AsRef<Path>,
        kind: FsErrorKind,
        times: usize,
    ) -> &Self {
        if times > 0 {
            self.inner.lock().faults.push(Fault {
                op,
                prefix: path.as_ref().to_owned(),
                kind,
                remaining: Some(times),
            });
        }
        self
    }

    /// Remove all injected faults.
    pub fn clear_faults(&self) {
        self.inner.lock().faults.clear();
    }

    /// Number of calls of `op` so far.
    pub fn calls(&self, op: FsOp) -> usize {
        self.inner.lock().calls.get(&op).copied().unwrap_or(0)
    }

    /// Total number of calls.
    pub fn total_calls(&self) -> usize {
        self.inner.lock().calls.values().sum()
    }

    /// Every path that exists, sorted (assertions on whole trees).
    pub fn paths(&self) -> Vec<PathBuf> {
        self.inner.lock().nodes.keys().cloned().collect()
    }

    /// Content of a file, without following a final symlink; `None` when not a file.
    pub fn contents(&self, path: impl AsRef<Path>) -> Option<Vec<u8>> {
        match &self.inner.lock().nodes.get(path.as_ref())?.kind {
            Kind::File(d) => Some(d.clone()),
            _ => None,
        }
    }
}

impl Fs for MemFs {
    fn metadata(&self, path: &Path) -> FsResult<Metadata> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::Metadata, path)?;
        let (r, n) = g.existing(FsOp::Metadata, path, false)?;
        Ok(g.metadata_of(&r, n))
    }

    fn metadata_follow(&self, path: &Path) -> FsResult<Metadata> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::MetadataFollow, path)?;
        let (r, n) = g.existing(FsOp::MetadataFollow, path, true)?;
        Ok(g.metadata_of(&r, n))
    }

    fn read_link(&self, path: &Path) -> FsResult<PathBuf> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::ReadLink, path)?;
        match &g.existing(FsOp::ReadLink, path, false)?.1.kind {
            Kind::Symlink(t) => Ok(t.clone()),
            _ => Err(err(FsOp::ReadLink, path, FsErrorKind::InvalidInput)),
        }
    }

    fn read(&self, path: &Path, limit: u64) -> FsResult<Vec<u8>> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::Read, path)?;
        let data = g.file_data(FsOp::Read, path, false)?;
        if data.len() as u64 > limit {
            return Err(err(FsOp::Read, path, FsErrorKind::TooLarge { limit }));
        }
        Ok(data)
    }

    fn read_follow(&self, path: &Path, limit: u64) -> FsResult<Vec<u8>> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::Read, path)?;
        let data = g.file_data(FsOp::Read, path, true)?;
        if data.len() as u64 > limit {
            return Err(err(FsOp::Read, path, FsErrorKind::TooLarge { limit }));
        }
        Ok(data)
    }

    fn read_range(&self, path: &Path, offset: u64, len: usize) -> FsResult<Vec<u8>> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::Read, path)?;
        let data = g.file_data(FsOp::Read, path, false)?;
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let end = start.saturating_add(len).min(data.len());
        Ok(data[start..end].to_vec())
    }

    fn open_read(&self, path: &Path) -> FsResult<Box<dyn Read + Send>> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::Read, path)?;
        let data = g.file_data(FsOp::Read, path, false)?;
        Ok(Box::new(Cursor::new(data)))
    }

    fn read_dir(&self, path: &Path) -> FsResult<Vec<DirEntry>> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::ReadDir, path)?;
        let (r, n) = g.existing(FsOp::ReadDir, path, true)?;
        if !matches!(n.kind, Kind::Dir) {
            return Err(err(FsOp::ReadDir, path, FsErrorKind::NotADirectory));
        }
        let mut out: Vec<DirEntry> = g
            .children(&r)
            .map(|(p, n)| {
                let name = p.file_name().map(OsString::from).unwrap_or_default();
                DirEntry {
                    path: path.join(&name),
                    name,
                    file_type: match n.kind {
                        Kind::File(_) => FileType::File,
                        Kind::Dir => FileType::Dir,
                        Kind::Symlink(_) => FileType::Symlink,
                        Kind::CharDevice => FileType::CharDevice,
                        Kind::Fifo => FileType::Fifo,
                        Kind::Socket => FileType::Socket,
                    },
                }
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn canonicalize(&self, path: &Path) -> FsResult<PathBuf> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::Canonicalize, path)?;
        Ok(g.existing(FsOp::Canonicalize, path, true)?.0)
    }

    fn create_dir(&self, path: &Path, mode: u32) -> FsResult<()> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::CreateDir, path)?;
        let r = g.creatable(FsOp::CreateDir, path)?;
        let node = g.new_node(Kind::Dir, mode);
        g.nodes.insert(r, node);
        Ok(())
    }

    fn create_dir_all(&self, path: &Path, mode: u32) -> FsResult<()> {
        let mut prefixes: Vec<PathBuf> = path.ancestors().map(Path::to_path_buf).collect();
        prefixes.reverse();
        for p in prefixes {
            if p.as_os_str().is_empty() || p == Path::new("/") {
                continue;
            }
            match self.create_dir(&p, mode) {
                Ok(()) => {}
                Err(e) if e.kind == FsErrorKind::AlreadyExists => {
                    if !self.metadata_follow(&p)?.is_dir() {
                        return Err(err(FsOp::CreateDir, &p, FsErrorKind::NotADirectory));
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn write_new(&self, path: &Path, data: &[u8], mode: u32) -> FsResult<()> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::Write, path)?;
        let r = g.creatable(FsOp::Write, path)?;
        let node = g.new_node(Kind::File(data.to_vec()), mode);
        g.nodes.insert(r, node);
        Ok(())
    }

    fn atomic_write(&self, path: &Path, data: &[u8], mode: u32) -> FsResult<()> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::Write, path)?;
        let r = g.resolve(FsOp::Write, path, false)?;
        if let Some(Node {
            kind: Kind::Dir, ..
        }) = g.nodes.get(&r)
        {
            return Err(err(FsOp::Write, path, FsErrorKind::IsADirectory));
        }
        g.parent_is_dir(FsOp::Write, path, &r)?;
        let node = g.new_node(Kind::File(data.to_vec()), mode);
        g.nodes.insert(r, node);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> FsResult<()> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::Rename, from)?;
        let (rf, node) = g.existing(FsOp::Rename, from, false)?;
        let node = node.clone();
        let rt = g.resolve(FsOp::Rename, to, false)?;
        g.parent_is_dir(FsOp::Rename, to, &rt)?;
        if rf == rt {
            return Ok(());
        }
        if rt.starts_with(&rf) {
            return Err(err(FsOp::Rename, from, FsErrorKind::InvalidInput));
        }
        let to_parent = rt.parent().unwrap_or(Path::new("/")).to_owned();
        if g.device_of(&rf) != g.device_of(&to_parent) {
            return Err(err(FsOp::Rename, from, FsErrorKind::CrossDevice));
        }
        if let Some(existing) = g.nodes.get(&rt) {
            match (&existing.kind, &node.kind) {
                (Kind::Dir, Kind::Dir) => {
                    if g.children(&rt).next().is_some() {
                        return Err(err(FsOp::Rename, to, FsErrorKind::DirectoryNotEmpty));
                    }
                }
                (Kind::Dir, _) => return Err(err(FsOp::Rename, to, FsErrorKind::IsADirectory)),
                (_, Kind::Dir) => return Err(err(FsOp::Rename, to, FsErrorKind::NotADirectory)),
                _ => {}
            }
        }
        let moved: Vec<PathBuf> = g
            .nodes
            .keys()
            .filter(|p| p.starts_with(&rf))
            .cloned()
            .collect();
        for old in moved {
            if let Some(n) = g.nodes.remove(&old) {
                let rel = old.strip_prefix(&rf).unwrap_or(Path::new(""));
                let new = if rel.as_os_str().is_empty() {
                    rt.clone()
                } else {
                    rt.join(rel)
                };
                g.nodes.insert(new, n);
            }
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> FsResult<()> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::RemoveFile, path)?;
        let (r, n) = g.existing(FsOp::RemoveFile, path, false)?;
        if matches!(n.kind, Kind::Dir) {
            return Err(err(FsOp::RemoveFile, path, FsErrorKind::IsADirectory));
        }
        g.nodes.remove(&r);
        Ok(())
    }

    fn remove_dir(&self, path: &Path) -> FsResult<()> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::RemoveDir, path)?;
        let (r, n) = g.existing(FsOp::RemoveDir, path, false)?;
        if !matches!(n.kind, Kind::Dir) {
            return Err(err(FsOp::RemoveDir, path, FsErrorKind::NotADirectory));
        }
        if g.children(&r).next().is_some() {
            return Err(err(FsOp::RemoveDir, path, FsErrorKind::DirectoryNotEmpty));
        }
        if g.mounts.iter().any(|m| m.point == r) || r == Path::new("/") {
            return Err(err(FsOp::RemoveDir, path, FsErrorKind::PermissionDenied));
        }
        g.nodes.remove(&r);
        Ok(())
    }

    fn symlink(&self, target: &Path, link: &Path) -> FsResult<()> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::Symlink, link)?;
        let r = g.creatable(FsOp::Symlink, link)?;
        let node = g.new_node(Kind::Symlink(target.to_owned()), 0o777);
        g.nodes.insert(r, node);
        Ok(())
    }

    fn set_mode(&self, path: &Path, mode: u32) -> FsResult<()> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::SetMode, path)?;
        let (r, n) = g.existing(FsOp::SetMode, path, false)?;
        if matches!(n.kind, Kind::Symlink(_)) {
            return Err(err(FsOp::SetMode, path, FsErrorKind::Symlink));
        }
        if let Some(n) = g.nodes.get_mut(&r) {
            n.mode = mode;
        }
        Ok(())
    }

    fn free_space(&self, path: &Path) -> FsResult<u64> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::FreeSpace, path)?;
        g.existing(FsOp::FreeSpace, path, true)?;
        Ok(g.free_space)
    }

    fn reflink_supported(&self, dir_a: &Path, dir_b: &Path) -> FsResult<bool> {
        let mut g = self.inner.lock();
        g.check_fault(FsOp::Reflink, dir_a)?;
        let (ra, _) = g.existing(FsOp::Reflink, dir_a, true)?;
        let (rb, _) = g.existing(FsOp::Reflink, dir_b, true)?;
        Ok(g.reflink && g.device_of(&ra) == g.device_of(&rb))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaun_core::clock::{MonoInstant, Timestamp};
    use zaun_core::fsx::{
        MountTable, WalkControl, WalkLimits, ensure_private_dir, safe_remove_tree, safe_walk,
    };

    struct Still;
    impl zaun_core::clock::Clock for Still {
        fn now(&self) -> Timestamp {
            Timestamp::EPOCH
        }
        fn mono(&self) -> MonoInstant {
            MonoInstant::ZERO
        }
    }

    #[test]
    fn symlink_resolution() {
        let fs = MemFs::new();
        fs.file("/data/games/a.exe", b"MZ")
            .link("/data/link", "games")
            .link("/abs", "/data/games/a.exe")
            .link("/loop", "/loop");
        assert!(fs.metadata(Path::new("/data/link")).unwrap().is_symlink());
        assert!(
            fs.metadata_follow(Path::new("/data/link"))
                .unwrap()
                .is_dir()
        );
        assert_eq!(fs.read(Path::new("/data/link/a.exe"), 10).unwrap(), b"MZ");
        assert_eq!(
            fs.read(Path::new("/abs"), 10).unwrap_err().kind,
            FsErrorKind::Symlink
        );
        assert_eq!(fs.read_follow(Path::new("/abs"), 10).unwrap(), b"MZ");
        assert_eq!(
            fs.canonicalize(Path::new("/data/link/../link/a.exe"))
                .unwrap(),
            Path::new("/data/games/a.exe")
        );
        assert_eq!(
            fs.metadata_follow(Path::new("/loop")).unwrap_err().kind,
            FsErrorKind::Symlink
        );
        assert_eq!(
            fs.metadata(Path::new("/data/games/a.exe/x"))
                .unwrap_err()
                .kind,
            FsErrorKind::NotADirectory
        );
    }

    #[test]
    fn mounts_and_walks() {
        let fs = MemFs::new();
        fs.file("/r/a", b"1")
            .mount("/r/usb", 7, "vfat")
            .file("/r/usb/b", b"2");
        let md = fs.metadata(Path::new("/r/usb")).unwrap();
        assert_eq!(md.device, 7);
        let s = safe_walk(&fs, Path::new("/r"), WalkLimits::default(), &Still, |_| {
            WalkControl::Continue
        })
        .unwrap();
        assert_eq!(s.entries, 2);
        assert_eq!(s.mount_boundaries, 1);
        let t = MountTable::parse(fs.mountinfo().as_bytes());
        assert_eq!(t.fs_type_of(Path::new("/r/usb/b")), Some("vfat"));
        let r = safe_remove_tree(&fs, Path::new("/"), Path::new("/r")).unwrap();
        assert!(!r.is_complete());
        assert_eq!(r.skipped_mounts, vec![PathBuf::from("/r/usb")]);
        assert!(fs.exists(Path::new("/r/usb/b")));
        assert!(!fs.exists(Path::new("/r/a")));
    }

    #[test]
    fn rename_rules() {
        let fs = MemFs::new();
        fs.file("/a/x", b"1")
            .file("/a/sub/y", b"2")
            .mount("/m", 3, "tmpfs");
        fs.rename(Path::new("/a"), Path::new("/b")).unwrap();
        assert_eq!(fs.contents("/b/sub/y").unwrap(), b"2");
        assert!(!fs.exists(Path::new("/a")));
        assert_eq!(
            fs.rename(Path::new("/b/x"), Path::new("/m/x"))
                .unwrap_err()
                .kind,
            FsErrorKind::CrossDevice
        );
    }

    #[test]
    fn faults_and_counts() {
        let fs = MemFs::new();
        fs.dir("/d");
        fs.fail_times(FsOp::Write, "/d", FsErrorKind::NoSpace, 1);
        assert_eq!(
            fs.atomic_write(Path::new("/d/f"), b"x", 0o600)
                .unwrap_err()
                .kind,
            FsErrorKind::NoSpace
        );
        fs.atomic_write(Path::new("/d/f"), b"x", 0o600).unwrap();
        assert_eq!(fs.calls(FsOp::Write), 2);
        fs.fail(FsOp::Read, "/d", FsErrorKind::PermissionDenied);
        assert!(fs.read(Path::new("/d/f"), 9).is_err());
        assert!(fs.read(Path::new("/d/f"), 9).is_err());
        fs.clear_faults();
        assert_eq!(fs.read(Path::new("/d/f"), 9).unwrap(), b"x");
    }

    #[test]
    fn private_dir_owner_checks() {
        let fs = MemFs::new();
        fs.dir("/tmp");
        ensure_private_dir(&fs, Path::new("/tmp/zaun-1000"), 1000).unwrap();
        fs.dir("/tmp/zaun-1001").chown("/tmp/zaun-1001", 0, 0);
        assert_eq!(
            ensure_private_dir(&fs, Path::new("/tmp/zaun-1001"), 1001)
                .unwrap_err()
                .kind,
            FsErrorKind::Insecure
        );
    }
}
