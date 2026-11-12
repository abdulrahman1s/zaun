//! The Doctor report: host checks, then the compatibility-layer check D-15 with the
//! adoption hint for runtimes umu already installed outside the store.

use crate::consts::APP_VERSION;
use crate::host::{
    Check, CheckStatus, DOCTOR_FORMAT, DoctorContext, DoctorReport, HostFacts, host_checks,
};
use crate::runtime::{RuntimeCatalog, adoptable, catalog_check};

/// D-15, with the hint to adopt umu's runtime when that is what is missing.
pub fn runtime_check(catalog: &RuntimeCatalog) -> Check {
    let mut c = catalog_check(catalog);
    let can_adopt: Vec<&str> = adoptable(&catalog.foreign.slr, &catalog.slr)
        .map(|s| s.variant.as_str())
        .collect();
    let needed: Vec<_> = catalog
        .builds
        .iter()
        .map(|b| b.required_slr)
        .filter(|v| {
            !catalog
                .slr
                .iter()
                .any(|s| s.variant == *v && s.is_complete())
        })
        .collect();
    if !can_adopt.is_empty() && c.status != CheckStatus::Fail {
        let useful = needed.iter().any(|v| can_adopt.contains(&v.as_str()));
        if useful {
            c.status = CheckStatus::Warn;
        }
        c = c.fix(format!(
            "Copy umu's runtime into EasyPlay without downloading: easyplay runtimes adopt ({})",
            can_adopt.join(", ")
        ));
    }
    c
}

/// The ordered checks of `easyplay doctor` and the Doctor screen.
pub fn doctor_report(
    facts: &HostFacts,
    ctx: &DoctorContext<'_>,
    catalog: &RuntimeCatalog,
) -> DoctorReport {
    let mut checks = host_checks(facts, ctx);
    checks.push(runtime_check(catalog));
    DoctorReport {
        format: DOCTOR_FORMAT,
        version: APP_VERSION.to_owned(),
        checks,
        facts: facts.clone(),
    }
}
