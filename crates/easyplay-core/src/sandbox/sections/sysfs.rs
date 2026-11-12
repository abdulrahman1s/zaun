//! §4.6 `/sys`: five read-only device trees, then masks over them.

use super::{Effective, mk};
use crate::host::HostFacts;
use crate::model::NetworkAccess;
use crate::sandbox::plan::{PlanBuilder, PolicyField};

/// The bound sysfs trees.
pub const SYSFS_TREES: [&str; 5] = [
    "/sys/block",
    "/sys/bus",
    "/sys/class",
    "/sys/dev",
    "/sys/devices",
];

/// §4.6.
pub fn emit(host: &HostFacts, eff: &Effective, b: &mut PlanBuilder) {
    for t in SYSFS_TREES {
        b.mount(mk::ro_same(t), "device information for drivers", None);
    }
    if eff.mask_net() {
        let field = if eff.network == NetworkAccess::None {
            PolicyField::Network
        } else {
            PolicyField::MaskHardwareIds
        };
        b.mount(
            mk::tmpfs("/sys/class/net", None, None),
            "hide network interfaces",
            Some(field.clone()),
        );
        let mut dirs = host.sysfs.net_device_dirs.clone();
        dirs.sort();
        dirs.dedup();
        for d in dirs {
            b.mount(
                mk::tmpfs(&d, None, None),
                "hide network interface addresses",
                Some(field.clone()),
            );
        }
        b.mount(
            mk::tmpfs("/sys/devices/virtual/net", None, None),
            "hide virtual network interfaces",
            Some(field),
        );
    }
    if eff.mask_hardware_ids {
        for d in ["/sys/devices/virtual/dmi", "/sys/class/dmi"] {
            b.mount(
                mk::tmpfs(d, None, None),
                "hide board and product identifiers",
                Some(PolicyField::MaskHardwareIds),
            );
        }
    }
}
