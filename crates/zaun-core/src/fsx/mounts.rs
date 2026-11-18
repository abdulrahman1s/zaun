//! `/proc/self/mountinfo` parsing: which mount a path is on, its filesystem type, and whether
//! two paths share a filesystem.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use super::{Fs, FsResult};

/// One line of `mountinfo` (proc(5)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountInfo {
    /// Mount ID.
    pub mount_id: u32,
    /// Parent mount ID.
    pub parent_id: u32,
    /// Device major number.
    pub major: u32,
    /// Device minor number.
    pub minor: u32,
    /// Root of the mount within the filesystem.
    pub root: PathBuf,
    /// Mount point.
    pub mount_point: PathBuf,
    /// Per-mount options (`rw,noatime`, …).
    pub options: String,
    /// Optional fields (`shared:1`, `master:2`, …).
    pub optional: Vec<String>,
    /// Filesystem type (`ext4`, `btrfs`, `fuse.sshfs`, …).
    pub fs_type: String,
    /// Mount source.
    pub source: String,
    /// Superblock options.
    pub super_options: String,
}

impl MountInfo {
    /// Whether the mount is read-only.
    pub fn is_read_only(&self) -> bool {
        self.options.split(',').any(|o| o == "ro")
    }

    /// Whether the filesystem is a network filesystem on which SQLite WAL is unsafe
    /// (PERSISTENCE §3).
    pub fn is_network_fs(&self) -> bool {
        matches!(
            self.fs_type.as_str(),
            "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "9p" | "fuse.sshfs" | "afs" | "ceph"
        )
    }
}

/// Undo the octal escapes of `mountinfo` (`\040` space, `\011` tab, `\012` newline, `\134`
/// backslash).
fn unescape(field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(field.len());
    let mut i = 0;
    while i < field.len() {
        let octal = (field[i] == b'\\'
            && i + 3 < field.len()
            && field[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b)))
        .then(|| {
            u32::from(field[i + 1] - b'0') * 64
                + u32::from(field[i + 2] - b'0') * 8
                + u32::from(field[i + 3] - b'0')
        })
        .and_then(|v| u8::try_from(v).ok());
        if let Some(v) = octal {
            out.push(v);
            i += 4;
        } else {
            out.push(field[i]);
            i += 1;
        }
    }
    out
}

fn text(field: &[u8]) -> String {
    String::from_utf8_lossy(&unescape(field)).into_owned()
}

fn path(field: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_vec(unescape(field)))
}

fn parse_line(line: &[u8]) -> Option<MountInfo> {
    let fields: Vec<&[u8]> = line
        .split(|b| *b == b' ')
        .filter(|f| !f.is_empty())
        .collect();
    let sep = fields.iter().position(|f| *f == b"-")?;
    if sep < 6 || fields.len() < sep + 3 {
        return None;
    }
    let num = |f: &[u8]| std::str::from_utf8(f).ok()?.parse::<u32>().ok();
    let (major, minor) = {
        let dev = std::str::from_utf8(fields[2]).ok()?;
        let (a, b) = dev.split_once(':')?;
        (a.parse().ok()?, b.parse().ok()?)
    };
    Some(MountInfo {
        mount_id: num(fields[0])?,
        parent_id: num(fields[1])?,
        major,
        minor,
        root: path(fields[3]),
        mount_point: path(fields[4]),
        options: text(fields[5]),
        optional: fields[6..sep].iter().map(|f| text(f)).collect(),
        fs_type: text(fields[sep + 1]),
        source: text(fields[sep + 2]),
        super_options: fields.get(sep + 3).map(|f| text(f)).unwrap_or_default(),
    })
}

/// The mount table, in `mountinfo` order (later entries were mounted later).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MountTable {
    /// Mounts.
    pub mounts: Vec<MountInfo>,
    /// Lines that could not be parsed (ignored).
    pub skipped_lines: usize,
}

/// Upper bound for reading `mountinfo`.
const MOUNTINFO_LIMIT: u64 = 8 * 1024 * 1024;

impl MountTable {
    /// Parse `mountinfo` text. Malformed lines are skipped and counted.
    pub fn parse(content: &[u8]) -> MountTable {
        let mut table = MountTable::default();
        for line in content.split(|b| *b == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match parse_line(line) {
                Some(m) => table.mounts.push(m),
                None => table.skipped_lines += 1,
            }
        }
        table
    }

    /// Read and parse `/proc/self/mountinfo`.
    pub fn read(fs: &dyn Fs) -> FsResult<MountTable> {
        let content = fs.read(Path::new("/proc/self/mountinfo"), MOUNTINFO_LIMIT)?;
        Ok(Self::parse(&content))
    }

    /// The mount containing `path` (absolute, canonical): the longest mount point that is a
    /// component-wise prefix; for equal mount points the last one (it covers the others).
    pub fn mount_point_of(&self, path: &Path) -> Option<&MountInfo> {
        let mut best: Option<&MountInfo> = None;
        for m in &self.mounts {
            if path.starts_with(&m.mount_point) {
                let better = match best {
                    None => true,
                    Some(b) => {
                        m.mount_point.components().count() >= b.mount_point.components().count()
                    }
                };
                if better {
                    best = Some(m);
                }
            }
        }
        best
    }

    /// Filesystem type of the mount containing `path`.
    pub fn fs_type_of(&self, path: &Path) -> Option<&str> {
        self.mount_point_of(path).map(|m| m.fs_type.as_str())
    }

    /// Whether `a` and `b` are on the same filesystem (same device number). Note that a
    /// rename across two bind mounts of one filesystem still fails with `EXDEV`; compare
    /// `mount_point_of(..).mount_id` for that.
    pub fn is_same_fs(&self, a: &Path, b: &Path) -> bool {
        match (self.mount_point_of(a), self.mount_point_of(b)) {
            (Some(x), Some(y)) => x.major == y.major && x.minor == y.minor,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARCH: &str = "\
22 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
23 22 0:21 / /proc rw,nosuid,nodev,noexec,relatime shared:5 - proc proc rw
24 22 0:22 / /sys rw,nosuid,nodev,noexec,relatime shared:6 - sysfs sysfs rw
25 22 0:5 / /dev rw,nosuid,relatime shared:2 - devtmpfs dev rw,size=8126464k,mode=755
40 22 259:3 / /home rw,relatime shared:30 - ext4 /dev/nvme0n1p3 rw
41 22 0:35 / /tmp rw,nosuid,nodev shared:31 - tmpfs tmpfs rw,size=8G
90 40 8:17 / /home/sam/Games\\040Drive rw,relatime shared:40 - xfs /dev/sdb1 rw,attr2
";

    const FEDORA_BTRFS: &str = "\
60 1 0:30 /root / rw,relatime shared:1 - btrfs /dev/nvme0n1p3 rw,seclabel,compress=zstd:1,ssd,subvol=/root
88 60 0:30 /home /home rw,relatime shared:44 - btrfs /dev/nvme0n1p3 rw,seclabel,compress=zstd:1,ssd,subvol=/home
";

    const UBUNTU_SNAP: &str = "\
25 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw,errors=remount-ro
500 25 7:3 / /snap/core22/1380 ro,nodev,relatime shared:200 - squashfs /dev/loop3 ro,errors=continue
501 25 0:50 / /run/user/1000 rw,nosuid,nodev,relatime shared:300 - tmpfs tmpfs rw,size=1600000k,mode=700,uid=1000,gid=1000
502 25 0:51 / /mnt/nas rw,relatime shared:301 - nfs4 nas:/export rw,vers=4.2
";

    const NIXOS: &str = "\
23 1 0:23 / / rw,relatime shared:1 - ext4 /dev/disk/by-label/nixos rw
26 23 0:23 /nix/store /nix/store ro,relatime shared:3 - ext4 /dev/disk/by-label/nixos rw
";

    const DEBIAN_SSHFS: &str = "\
21 1 254:1 / / rw,relatime shared:1 - ext4 /dev/mapper/root rw
120 21 0:60 / /home/sam/remote rw,nosuid,nodev,relatime shared:70 - fuse.sshfs sam@host: rw,user_id=1000,group_id=1000
";

    const OVERMOUNT: &str = "\
1 0 8:1 / / rw - ext4 /dev/sda1 rw
2 1 0:40 / /mnt rw - tmpfs tmpfs rw
3 1 8:5 / /mnt rw - vfat /dev/sda5 rw
bad line here
";

    #[test]
    fn parses_fields_and_escapes() {
        let t = MountTable::parse(ARCH.as_bytes());
        assert_eq!(t.mounts.len(), 7);
        assert_eq!(t.skipped_lines, 0);
        let games = &t.mounts[6];
        assert_eq!(games.mount_point, Path::new("/home/sam/Games Drive"));
        assert_eq!(games.fs_type, "xfs");
        assert_eq!((games.major, games.minor), (8, 17));
        assert_eq!(games.optional, vec!["shared:40".to_owned()]);
        assert_eq!(games.super_options, "rw,attr2");
        assert!(!games.is_read_only());
    }

    #[test]
    fn longest_prefix_wins() {
        let t = MountTable::parse(ARCH.as_bytes());
        assert_eq!(
            t.fs_type_of(Path::new("/home/sam/Games Drive/x/y.exe")),
            Some("xfs")
        );
        assert_eq!(t.fs_type_of(Path::new("/home/sam/Games")), Some("ext4"));
        assert_eq!(
            t.mount_point_of(Path::new("/home/sam"))
                .unwrap()
                .mount_point,
            Path::new("/home")
        );
        assert_eq!(t.fs_type_of(Path::new("/usr/bin")), Some("ext4"));
        assert_eq!(t.fs_type_of(Path::new("/tmp/x")), Some("tmpfs"));
        // Component-wise, not string prefix.
        assert_eq!(t.fs_type_of(Path::new("/homes")), Some("ext4"));
        assert!(t.is_same_fs(Path::new("/home/a"), Path::new("/home/b")));
        assert!(!t.is_same_fs(Path::new("/home/a"), Path::new("/tmp/b")));
    }

    #[test]
    fn btrfs_subvolumes_share_a_device() {
        let t = MountTable::parse(FEDORA_BTRFS.as_bytes());
        assert_eq!(t.mounts[1].root, Path::new("/home"));
        assert!(t.is_same_fs(Path::new("/home/sam"), Path::new("/var/lib")));
    }

    #[test]
    fn network_and_readonly_detection() {
        let t = MountTable::parse(UBUNTU_SNAP.as_bytes());
        assert!(
            t.mount_point_of(Path::new("/snap/core22/1380/usr"))
                .unwrap()
                .is_read_only()
        );
        assert!(
            t.mount_point_of(Path::new("/mnt/nas/games"))
                .unwrap()
                .is_network_fs()
        );
        let d = MountTable::parse(DEBIAN_SSHFS.as_bytes());
        assert!(
            d.mount_point_of(Path::new("/home/sam/remote/x"))
                .unwrap()
                .is_network_fs()
        );
        let n = MountTable::parse(NIXOS.as_bytes());
        assert!(
            n.mount_point_of(Path::new("/nix/store/abc"))
                .unwrap()
                .is_read_only()
        );
    }

    #[test]
    fn later_mount_covers_earlier_and_bad_lines_are_skipped() {
        let t = MountTable::parse(OVERMOUNT.as_bytes());
        assert_eq!(t.skipped_lines, 1);
        assert_eq!(t.fs_type_of(Path::new("/mnt/x")), Some("vfat"));
    }

    #[test]
    fn unescape_cases() {
        assert_eq!(unescape(b"a\\040b"), b"a b");
        assert_eq!(unescape(b"a\\134b"), b"a\\b");
        assert_eq!(unescape(b"tab\\011"), b"tab\t");
        assert_eq!(unescape(b"bad\\04"), b"bad\\04");
        assert_eq!(unescape(b"\\377"), [0xff]);
    }
}
