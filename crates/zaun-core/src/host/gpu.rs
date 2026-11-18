//! GPU device nodes, vendors and the 32-bit driver hint (BUBBLEWRAP_SANDBOX §4.5; D-12).

use std::path::{Path, PathBuf};

use super::access::AccessCheck;
use super::facts::{DeviceNode, GpuFacts, GpuVendor, LayoutKind};
use super::runner::{CommandRunner, CommandSpec};
use crate::fsx::Fs;

/// Where `ldconfig` may live.
pub const LDCONFIG_CANDIDATES: &[&str] =
    &["/sbin/ldconfig", "/usr/sbin/ldconfig", "/usr/bin/ldconfig"];

/// Libraries whose 32-bit presence means a 32-bit graphics driver is installed.
const DRIVER_LIBS: &[&str] = &[
    "libGLX_mesa.so",
    "libGLX_nvidia.so",
    "libvulkan_",
    "libEGL_mesa.so",
];

/// Map a PCI vendor ID (`0x10de`) to a vendor.
pub fn vendor_from_id(text: &str) -> GpuVendor {
    match text.trim().to_ascii_lowercase().as_str() {
        "0x1002" => GpuVendor::Amd,
        "0x8086" => GpuVendor::Intel,
        "0x10de" => GpuVendor::Nvidia,
        _ => GpuVendor::Other,
    }
}

/// Whether `ldconfig -p` output lists a 32-bit graphics driver library (entries tagged
/// `(libc6)` rather than `(libc6,x86-64)`).
pub fn has_32bit_driver(ldconfig_p: &str) -> bool {
    ldconfig_p.lines().any(|l| {
        let l = l.trim();
        DRIVER_LIBS.iter().any(|lib| l.starts_with(lib)) && l.contains("(libc6)")
    })
}

fn nvidia_node_name(name: &str) -> bool {
    matches!(
        name,
        "nvidiactl" | "nvidia-modeset" | "nvidia-uvm" | "nvidia-uvm-tools" | "nvidia-caps"
    ) || name
        .strip_prefix("nvidia")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Probe GPU facts.
pub fn probe_gpu(
    fs: &dyn Fs,
    access: &dyn AccessCheck,
    runner: &dyn CommandRunner,
    kind: LayoutKind,
) -> GpuFacts {
    let dri = Path::new("/dev/dri");
    let mut facts = GpuFacts {
        dri_present: fs.exists(dri),
        ntsync: fs.exists(Path::new("/dev/ntsync")),
        ..GpuFacts::default()
    };
    if let Ok(entries) = fs.read_dir(dri) {
        for e in entries {
            let Some(name) = e.name.to_str() else {
                continue;
            };
            let node = DeviceNode {
                accessible: access.read_write(&e.path),
                path: e.path.clone(),
            };
            if name.starts_with("renderD") {
                facts.render_nodes.push(node);
            } else if name.starts_with("card") {
                facts.card_nodes.push(node);
            }
        }
    }
    if let Ok(entries) = fs.read_dir(Path::new("/dev")) {
        facts.nvidia_nodes = entries
            .into_iter()
            .filter(|e| e.name.to_str().is_some_and(nvidia_node_name))
            .map(|e| e.path)
            .collect();
    }
    let mut vendors: Vec<GpuVendor> = fs
        .read_dir(Path::new("/sys/class/drm"))
        .map(|entries| {
            entries
                .into_iter()
                .filter(|e| {
                    e.name.to_str().is_some_and(|n| {
                        n.strip_prefix("card")
                            .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
                    })
                })
                .filter_map(|e| fs.read_follow(&e.path.join("device/vendor"), 64).ok())
                .map(|b| vendor_from_id(&String::from_utf8_lossy(&b)))
                .collect()
        })
        .unwrap_or_default();
    vendors.sort();
    vendors.dedup();
    facts.vendors = vendors;
    facts.render_nodes.sort();
    facts.card_nodes.sort();
    facts.nvidia_nodes.sort();

    facts.has_32bit_driver = match kind {
        LayoutKind::Nix => Some(fs.exists(Path::new("/run/opengl-driver-32"))),
        LayoutKind::Guix => None,
        LayoutKind::Fhs => LDCONFIG_CANDIDATES
            .iter()
            .map(PathBuf::from)
            .find(|p| super::exec::is_executable(fs, p))
            .and_then(|ldconfig| runner.run(&CommandSpec::new(ldconfig, &["-p"])).ok())
            .filter(|o| o.success())
            .map(|o| has_32bit_driver(&o.stdout_text())),
    };
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ldconfig_parsing() {
        let out = "1234 libs found in cache `/etc/ld.so.cache'\n\
            \tlibGLX_mesa.so.0 (libc6,x86-64) => /usr/lib/libGLX_mesa.so.0\n\
            \tlibvulkan_radeon.so (libc6,x86-64) => /usr/lib/libvulkan_radeon.so\n";
        assert!(!has_32bit_driver(out));
        let out32 = format!("{out}\tlibGLX_mesa.so.0 (libc6) => /usr/lib32/libGLX_mesa.so.0\n");
        assert!(has_32bit_driver(&out32));
    }

    #[test]
    fn vendor_ids() {
        assert_eq!(vendor_from_id("0x10de\n"), GpuVendor::Nvidia);
        assert_eq!(vendor_from_id("0x1002"), GpuVendor::Amd);
        assert_eq!(vendor_from_id("0x8086"), GpuVendor::Intel);
        assert_eq!(vendor_from_id("0x1af4"), GpuVendor::Other);
    }

    #[test]
    fn nvidia_names() {
        for n in [
            "nvidiactl",
            "nvidia0",
            "nvidia12",
            "nvidia-uvm",
            "nvidia-caps",
        ] {
            assert!(nvidia_node_name(n), "{n}");
        }
        for n in ["nvidia", "nvidiafoo", "nvme0", "nvidia-x"] {
            assert!(!nvidia_node_name(n), "{n}");
        }
    }
}
