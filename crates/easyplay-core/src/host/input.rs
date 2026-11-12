//! Input device classification (SECURITY_MODEL §5.4; D-14).
//!
//! Each `event*`, `js*` and `hidraw*` node is classified from the udev database
//! (`/run/udev/data/c<major>:<minor>`, `E:ID_INPUT_*=1` lines), falling back to the sysfs
//! capability bitmaps. `hidraw` nodes inherit the classes of the input devices under the same
//! HID parent. Readability is decided by [`AccessCheck`] (ACL-aware in production).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::access::AccessCheck;
use super::facts::{InputClass, InputFacts, InputNode, InputNodeKind};
use crate::fsx::{FileType, Fs};

const UDEV_CAP: u64 = 64 * 1024;
const MAX_NODES: usize = 512;

/// Split a Linux `dev_t` into `(major, minor)`.
pub const fn dev_major_minor(dev: u64) -> (u32, u32) {
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff);
    let minor = (dev & 0xff) | ((dev >> 12) & !0xff);
    (major as u32, minor as u32)
}

/// Classes from a udev database entry.
pub fn classes_from_udev(text: &str) -> BTreeSet<InputClass> {
    text.lines()
        .filter_map(|l| l.strip_prefix("E:"))
        .filter_map(|kv| kv.split_once('='))
        .filter(|(_, v)| v.trim() == "1")
        .filter_map(|(k, _)| match k {
            "ID_INPUT_JOYSTICK" => Some(InputClass::Joystick),
            "ID_INPUT_KEYBOARD" => Some(InputClass::Keyboard),
            "ID_INPUT_KEY" => Some(InputClass::Key),
            "ID_INPUT_MOUSE" => Some(InputClass::Mouse),
            "ID_INPUT_TOUCHPAD" => Some(InputClass::Touchpad),
            "ID_INPUT_TOUCHSCREEN" => Some(InputClass::Touchscreen),
            "ID_INPUT_TABLET" => Some(InputClass::Tablet),
            "ID_INPUT_ACCELEROMETER" => Some(InputClass::Accelerometer),
            "ID_INPUT_SWITCH" => Some(InputClass::Switch),
            _ => None,
        })
        .collect()
}

/// Test bit `bit` of a sysfs capability bitmap (space-separated hex words, most significant
/// first, each one `long` wide).
pub fn bitmap_has(bitmap: &str, bit: usize) -> bool {
    let words: Vec<&str> = bitmap.split_whitespace().collect();
    let idx = bit / 64;
    if idx >= words.len() {
        return false;
    }
    let word = words[words.len() - 1 - idx];
    u64::from_str_radix(word, 16).is_ok_and(|w| w & (1u64 << (bit % 64)) != 0)
}

const BTN_MOUSE: usize = 0x110;
const BTN_JOYSTICK: usize = 0x120;
const BTN_GAMEPAD: usize = 0x130;
const KEY_A: usize = 30;
const KEY_SPACE: usize = 57;
const REL_X: usize = 0;
const ABS_X: usize = 0;

/// Classes from the sysfs capability files of an input device (the fallback of udev's
/// `input_id` builtin, simplified).
pub fn classes_from_caps(key: &str, rel: &str, abs: &str) -> BTreeSet<InputClass> {
    let mut c = BTreeSet::new();
    if bitmap_has(abs, ABS_X) && (bitmap_has(key, BTN_JOYSTICK) || bitmap_has(key, BTN_GAMEPAD)) {
        c.insert(InputClass::Joystick);
    }
    if bitmap_has(rel, REL_X) && bitmap_has(key, BTN_MOUSE) {
        c.insert(InputClass::Mouse);
    }
    if bitmap_has(key, KEY_A) && bitmap_has(key, KEY_SPACE) {
        c.insert(InputClass::Keyboard);
    }
    if c.is_empty()
        && key
            .split_whitespace()
            .any(|w| !w.trim_start_matches('0').is_empty())
    {
        c.insert(InputClass::Key);
    }
    c
}

struct Raw {
    node: InputNode,
    /// Canonical sysfs directory of the input device (`…/inputN`) or HID device.
    parent: Option<PathBuf>,
}

fn read_text(fs: &dyn Fs, path: &Path) -> Option<String> {
    fs.read_follow(path, UDEV_CAP)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn sysfs_classes(fs: &dyn Fs, class_dir: &Path) -> BTreeSet<InputClass> {
    let cap =
        |n: &str| read_text(fs, &class_dir.join("device/capabilities").join(n)).unwrap_or_default();
    classes_from_caps(&cap("key"), &cap("rel"), &cap("abs"))
}

fn collect(
    fs: &dyn Fs,
    access: &dyn AccessCheck,
    dir: &Path,
    prefixes: &[(&str, InputNodeKind)],
    sys_class: &str,
    out: &mut Vec<Raw>,
) {
    let Ok(entries) = fs.read_dir(dir) else {
        return;
    };
    for e in entries {
        if out.len() >= MAX_NODES {
            return;
        }
        let Some(name) = e.name.to_str() else {
            continue;
        };
        let Some(kind) = prefixes.iter().find_map(|(p, k)| {
            name.strip_prefix(p)
                .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                .map(|_| *k)
        }) else {
            continue;
        };
        let Ok(md) = fs.metadata(&e.path) else {
            continue;
        };
        if md.file_type != FileType::CharDevice {
            continue;
        }
        let dev = dev_major_minor(md.rdev);
        let class_dir = Path::new(sys_class).join(name);
        let mut classes = read_text(
            fs,
            Path::new(&format!("/run/udev/data/c{}:{}", dev.0, dev.1)),
        )
        .map(|t| classes_from_udev(&t))
        .unwrap_or_default();
        if classes.is_empty() && kind == InputNodeKind::Event {
            classes = sysfs_classes(fs, &class_dir);
        }
        // event/js: …/inputN/eventM → parent …/inputN. hidraw: device → the HID device.
        let parent = match kind {
            InputNodeKind::Hidraw => fs.canonicalize(&class_dir.join("device")).ok(),
            _ => fs
                .canonicalize(&class_dir)
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf)),
        };
        out.push(Raw {
            node: InputNode {
                readable: access.readable(&e.path),
                path: e.path.clone(),
                kind,
                dev,
                classes,
                siblings: Vec::new(),
            },
            parent,
        });
    }
}

/// Probe input facts.
pub fn probe_input(fs: &dyn Fs, access: &dyn AccessCheck) -> InputFacts {
    let mut raw = Vec::new();
    collect(
        fs,
        access,
        Path::new("/dev/input"),
        &[("event", InputNodeKind::Event), ("js", InputNodeKind::Js)],
        "/sys/class/input",
        &mut raw,
    );
    collect(
        fs,
        access,
        Path::new("/dev"),
        &[("hidraw", InputNodeKind::Hidraw)],
        "/sys/class/hidraw",
        &mut raw,
    );

    // Siblings: event and js nodes of the same input device; js nodes inherit event classes.
    let mut by_parent: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    for (i, r) in raw.iter().enumerate() {
        if r.node.kind != InputNodeKind::Hidraw
            && let Some(p) = &r.parent
        {
            by_parent.entry(p.clone()).or_default().push(i);
        }
    }
    for idxs in by_parent.values() {
        let union: BTreeSet<InputClass> = idxs
            .iter()
            .flat_map(|&i| raw[i].node.classes.iter().copied())
            .collect();
        let paths: Vec<PathBuf> = idxs.iter().map(|&i| raw[i].node.path.clone()).collect();
        for &i in idxs {
            if raw[i].node.kind == InputNodeKind::Js {
                raw[i].node.classes.extend(union.iter().copied());
            }
            raw[i].node.siblings = paths
                .iter()
                .filter(|p| **p != raw[i].node.path)
                .cloned()
                .collect();
        }
    }
    // hidraw: union of the classes of input devices below the HID parent.
    for i in 0..raw.len() {
        if raw[i].node.kind != InputNodeKind::Hidraw {
            continue;
        }
        let Some(hid) = raw[i].parent.clone() else {
            continue;
        };
        let classes: BTreeSet<InputClass> = raw
            .iter()
            .filter(|r| r.node.kind == InputNodeKind::Event)
            .filter(|r| r.parent.as_ref().is_some_and(|p| p.starts_with(&hid)))
            .flat_map(|r| r.node.classes.iter().copied())
            .collect();
        raw[i].node.classes.extend(classes);
    }

    let mut nodes: Vec<InputNode> = raw.into_iter().map(|r| r.node).collect();
    nodes.sort_by(|a, b| a.path.cmp(&b.path));
    let keyboards_readable = nodes
        .iter()
        .any(|n| n.readable && !n.is_joystick() && n.classes.iter().any(|c| c.is_sensitive()));
    InputFacts {
        nodes,
        keyboards_readable,
        udev_db: fs.exists(Path::new("/run/udev/data")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_numbers() {
        assert_eq!(dev_major_minor((13 << 8) | 85), (13, 85));
        assert_eq!(dev_major_minor((237 << 8) | 4), (237, 4));
    }

    #[test]
    fn bitmaps() {
        // KEY bitmap of a gamepad: BTN_SOUTH (0x130) in the 5th word from the right.
        let key = "7fdb000000000000 0 0 0 0";
        assert!(bitmap_has(key, BTN_GAMEPAD));
        assert!(!bitmap_has(key, KEY_A));
        assert!(bitmap_has("3", 0) && bitmap_has("3", 1) && !bitmap_has("3", 2));
        assert!(!bitmap_has("", 0));
    }

    #[test]
    fn caps_fallback() {
        let pad = classes_from_caps("7fdb000000000000 0 0 0 0", "0", "3003f");
        assert!(pad.contains(&InputClass::Joystick));
        let kb = classes_from_caps("fffffffffffffffe", "0", "0");
        assert!(kb.contains(&InputClass::Keyboard));
        let mouse = classes_from_caps("1f0000 0 0 0 0", "103", "0");
        assert!(mouse.contains(&InputClass::Mouse));
    }
}
