//! §4.15 Environment (UMU_INTEGRATION §4): fixed variables, host pass-through,
//! compatibility options, then global and per-game user variables. Sockets, `HOME`,
//! `WINEPREFIX`, `PROTONPATH` and `PRESSURE_VESSEL_VARIABLE_DIR` are set by the sections that
//! create what they point to.

use super::Effective;
use crate::host::HostFacts;
use crate::model::{PolicyException, Renderer, SandboxPolicy, SessionKind, SyncMode, WindowSystem};
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{EnvOrigin, PlanBuilder};
use crate::sandbox::validate::{UserVarRule, classify_user_var};

/// Host variables copied when set (table B, all groups but the layout-specific one).
pub const HOST_PASSTHROUGH: &[&str] = &[
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "TZ",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "XDG_SESSION_TYPE",
    "DESKTOP_SESSION",
    "XCURSOR_THEME",
    "XCURSOR_SIZE",
    "DRI_PRIME",
    "MESA_VK_DEVICE_SELECT",
    "__NV_PRIME_RENDER_OFFLOAD",
    "__NV_PRIME_RENDER_OFFLOAD_PROVIDER",
    "__GLX_VENDOR_LIBRARY_NAME",
    "__VK_LAYER_NV_optimus",
];

/// §4.15.
pub fn emit(
    policy: &SandboxPolicy,
    ctx: &SandboxContext<'_>,
    host: &HostFacts,
    eff: &Effective,
    b: &mut PlanBuilder,
) {
    let fixed = EnvOrigin::Fixed;
    b.env("USER", ctx.user_name, fixed.clone());
    b.env("LOGNAME", ctx.user_name, fixed.clone());
    let mut path = if host.layout.path_env.is_empty() {
        "/usr/local/bin:/usr/bin:/bin".to_owned()
    } else {
        host.layout.path_env.clone()
    };
    if let Some(dir) = ctx.umu.path.parent() {
        let dir = dir.display().to_string();
        if !path.split(':').any(|p| p == dir) {
            path.push(':');
            path.push_str(&dir);
        }
    }
    b.env("PATH", path, fixed.clone());
    let game_id = ctx.game.and_then(|g| g.umu_id).unwrap_or("umu-default");
    b.env("GAMEID", game_id, fixed.clone());
    if let Some(store) = ctx.game.and_then(|g| g.store) {
        b.env("STORE", store, fixed.clone());
    }
    // Without it an offline launch hangs on the runtime update check (F-226).
    if ctx.kind != SessionKind::Provision {
        b.env("UMU_RUNTIME_UPDATE", "0", fixed.clone());
    }
    if ctx.launch.verbose_log {
        b.env("UMU_LOG", "1", fixed.clone());
    }
    // The outer sandbox already decides what "home" is (the private home), so the inner
    // container may share it (TECHNICAL_RESEARCH F-223).
    b.env("PRESSURE_VESSEL_SHARE_HOME", "1", fixed.clone());
    // GStreamer debug output floods the session log (F-224).
    b.env("GST_DEBUG", "0", fixed);

    // Table B.
    let home = ctx.home.display().to_string();
    let layout_names: Vec<&str> = host
        .layout
        .passthrough_env
        .iter()
        .map(String::as_str)
        .collect();
    for name in HOST_PASSTHROUGH.iter().copied().chain(layout_names) {
        if let Some(value) = ctx.host_env.get(name) {
            if value.contains(&home) {
                b.note(
                    "note.env.home_value",
                    format!("{name} was not passed: it refers to your home folder"),
                );
                continue;
            }
            b.env(name, value.clone(), EnvOrigin::Host);
        }
    }

    // Table C.
    let c = ctx.launch.compat;
    let opt = |n: &str| EnvOrigin::Option(n.to_owned());
    if c.renderer == Renderer::OpenGl {
        b.env("PROTON_USE_WINED3D", "1", opt("renderer"));
    }
    if c.window_system == WindowSystem::Wayland && eff.wayland {
        b.env("PROTON_ENABLE_WAYLAND", "1", opt("window_system"));
    }
    match c.sync {
        SyncMode::Default => {}
        SyncMode::NoFsync => b.env("PROTON_NO_FSYNC", "1", opt("sync")),
        SyncMode::NoEsyncNoFsync => {
            b.env("PROTON_NO_FSYNC", "1", opt("sync"));
            b.env("PROTON_NO_ESYNC", "1", opt("sync"));
        }
    }
    if c.disable_fixes {
        b.env("PROTONFIXES_DISABLE", "1", opt("disable_fixes"));
    }
    if !c.dll_overrides.is_empty() {
        b.env(
            "WINEDLLOVERRIDES",
            c.dll_overrides.clone(),
            opt("dll_overrides"),
        );
    }
    if c.mangohud {
        b.env("MANGOHUD", "1", opt("mangohud"));
    }
    if let Some(hud) = &c.dxvk_hud {
        b.env("DXVK_HUD", hud.clone(), opt("dxvk_hud"));
    }
    if let Some(locale) = &c.locale {
        b.env("LC_ALL", locale.clone(), opt("locale"));
    }
    if let Some(debug) = &c.wine_debug
        && ctx.developer_mode
    {
        b.env("WINEDEBUG", debug.clone(), opt("wine_debug"));
    }

    // Table D, then exceptions. Reserved names were rejected by validation; they are skipped
    // here as well and the invariants assert their absence.
    let user_sources = [
        (ctx.launch.global_env, EnvOrigin::Global),
        (ctx.launch.game_env, EnvOrigin::Game),
    ];
    for (vars, origin) in user_sources {
        for v in vars {
            match classify_user_var(&v.name) {
                UserVarRule::Allowed => b.env(v.name.clone(), v.value.clone(), origin.clone()),
                UserVarRule::Dangerous if ctx.developer_mode => {
                    b.env(v.name.clone(), v.value.clone(), origin.clone());
                }
                _ => {}
            }
        }
    }
    for e in &policy.exceptions {
        if let PolicyException::PassEnv { name, .. } = e {
            let rule = classify_user_var(name);
            let ok = rule == UserVarRule::Allowed
                || (rule == UserVarRule::Dangerous && ctx.developer_mode);
            if ok && let Some(value) = ctx.host_env.get(name) {
                b.env(name.clone(), value.clone(), EnvOrigin::Exception);
            }
        }
    }
}
