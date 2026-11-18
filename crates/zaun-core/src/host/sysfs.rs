//! Sysfs paths the planner masks (BUBBLEWRAP_SANDBOX §4.6).

use std::path::Path;

use super::facts::SysfsFacts;
use crate::fsx::Fs;

/// Resolve each `/sys/class/net/<if>` and record the `…/net` directory containing it.
pub fn probe_sysfs(fs: &dyn Fs) -> SysfsFacts {
    let mut dirs: Vec<_> = fs
        .read_dir(Path::new("/sys/class/net"))
        .map(|entries| {
            entries
                .into_iter()
                .filter_map(|e| fs.canonicalize(&e.path).ok())
                .filter(|p| p.starts_with("/sys/devices") && !p.starts_with("/sys/devices/virtual"))
                .filter_map(|p| p.parent().map(Path::to_path_buf))
                .filter(|p| p.file_name().is_some_and(|n| n == "net"))
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs.dedup();
    SysfsFacts {
        net_device_dirs: dirs,
    }
}
