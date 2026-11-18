//! Proton options a game's launch options can set as environment variables (ADR-0027): the
//! names the Launch options tab suggests, each with the value it usually takes and what it
//! does.
//!
//! Read from the `proton` scripts of UMU-Proton 10.0-4 and proton-cachyos 11.0 and from the
//! DXVK and vkd3d-proton documentation. Left out: what a setting of Zaun already sets
//! (renderer, window system, synchronisation, fixes, DLL overrides, HUDs, locale, logging,
//! [`super::env::compat_vars`]), names Zaun reserves or allows only in Developer Mode, and
//! paths, which would name places outside the sandbox.

/// The builds of Proton that read a variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builds {
    /// Every Proton: Valve's, UMU-Proton, GE-Proton, CachyOS.
    Every,
    /// GE-Proton and proton-cachyos only.
    Community,
}

/// One option.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Flag {
    /// Variable name.
    pub name: &'static str,
    /// The value it usually takes.
    pub value: &'static str,
    /// What it does, in one line.
    pub summary: &'static str,
    /// Which builds read it.
    pub builds: Builds,
}

const fn every(name: &'static str, value: &'static str, summary: &'static str) -> Flag {
    Flag {
        name,
        value,
        summary,
        builds: Builds::Every,
    }
}

const fn community(name: &'static str, value: &'static str, summary: &'static str) -> Flag {
    Flag {
        name,
        value,
        summary,
        builds: Builds::Community,
    }
}

/// Every option, most often useful first.
pub const FLAGS: &[Flag] = &[
    every(
        "PROTON_USE_WOW64",
        "1",
        "Run 32-bit Windows programs without 32-bit Linux libraries (new WoW64 mode)",
    ),
    every(
        "DXVK_FRAME_RATE",
        "60",
        "Cap the frame rate of Direct3D 9–11 games",
    ),
    every(
        "VKD3D_FRAME_RATE",
        "60",
        "Cap the frame rate of Direct3D 12 games",
    ),
    every(
        "PROTON_FORCE_LARGE_ADDRESS_AWARE",
        "1",
        "Let 32-bit games use up to 4 GB of memory",
    ),
    every(
        "PROTON_NO_D3D11",
        "1",
        "Hide Direct3D 11, so the game falls back to an older version",
    ),
    every(
        "PROTON_NO_D3D10",
        "1",
        "Hide Direct3D 10, so the game falls back to an older version",
    ),
    every(
        "PROTON_DXVK_D3D8",
        "1",
        "Run Direct3D 8 through DXVK (Vulkan) instead of WineD3D",
    ),
    every(
        "PROTON_HIDE_NVIDIA_GPU",
        "1",
        "Report an NVIDIA graphics card as another make, for games that misbehave on one",
    ),
    every(
        "PROTON_DISABLE_NVAPI",
        "1",
        "Turn off NVIDIA's NVAPI (DLSS, Reflex) for games that crash with it",
    ),
    every(
        "PROTON_FORCE_NVAPI",
        "1",
        "Turn on NVIDIA's NVAPI for games Proton leaves it off for",
    ),
    every(
        "PROTON_CPU_TOPOLOGY",
        "4:0,1,2,3",
        "Show the game fewer processor cores (count:core,core,…), for games that fail on many",
    ),
    every(
        "PROTON_LIMIT_RESOLUTIONS",
        "32",
        "Offer the game only this many screen resolutions, for games that crash on long lists",
    ),
    every(
        "PROTON_OLD_GL_STRING",
        "1",
        "Shorten the OpenGL extension list, for old games that crash on long ones",
    ),
    every(
        "PROTON_HEAP_DELAY_FREE",
        "1",
        "Free memory later, for games that use memory after freeing it",
    ),
    every(
        "PROTON_HEAP_ZERO_MEMORY",
        "1",
        "Clear memory the game allocates, for games that read it before writing",
    ),
    every(
        "PROTON_NO_XIM",
        "1",
        "Don't use X input methods, for crashes or lost keys when typing",
    ),
    every(
        "PROTON_DISABLE_HIDRAW",
        "1",
        "Don't read controllers as raw HID devices, for controllers seen twice or not at all",
    ),
    every(
        "PROTON_USE_XALIA",
        "0",
        "Turn off Xalia, the controller navigation Proton adds to some launchers",
    ),
    every(
        "VKD3D_FEATURE_LEVEL",
        "12_0",
        "Report an older Direct3D 12 feature level, for games that fail on newer ones",
    ),
    every(
        "VKD3D_CONFIG",
        "dxr",
        "Offer Direct3D 12 ray tracing even where vkd3d-proton would not",
    ),
    every(
        "DXVK_LOG_LEVEL",
        "none",
        "Silence DXVK's messages in the game's log",
    ),
    community(
        "PROTON_FSR4_UPGRADE",
        "1",
        "Upgrade the game's FSR 3.1 to FSR 4",
    ),
    community(
        "PROTON_DLSS_UPGRADE",
        "1",
        "Upgrade the game's DLSS to the newest version",
    ),
    community(
        "PROTON_XESS_UPGRADE",
        "1",
        "Upgrade the game's XeSS to the newest version",
    ),
    community(
        "DXVK_ASYNC",
        "1",
        "Build shaders in the background: less stutter, brief missing effects",
    ),
    community(
        "PROTON_DXVK_LOWLATENCY",
        "1",
        "Use the low-latency build of DXVK, where the Proton build has one",
    ),
    community(
        "PROTON_LOCAL_SHADER_CACHE",
        "1",
        "Keep this game's shader cache apart from other games'",
    ),
    community(
        "PROTON_NO_WM_DECORATION",
        "1",
        "Show the game's windows without title bars and borders",
    ),
    community("PROTON_USE_SDL", "1", "Read controllers through SDL"),
    community(
        "PROTON_NVIDIA_LIBS",
        "1",
        "Give the game NVIDIA's own libraries (CUDA, NVENC, OptiX); NVIDIA cards only",
    ),
];

/// The option named `name`.
pub fn find(name: &str) -> Option<&'static Flag> {
    FLAGS.iter().find(|f| f.name == name)
}

/// The options matching what was typed, best first, at most `limit`, leaving out names in
/// `taken`. Nothing typed: every option, in catalogue order. Otherwise a name that starts
/// with the text, then one that contains it, then a description that contains every word of
/// it; case is ignored.
pub fn suggest(typed: &str, taken: &[&str], limit: usize) -> Vec<&'static Flag> {
    let typed = typed.trim();
    let upper = typed.to_ascii_uppercase();
    let words: Vec<String> = typed
        .split(|c: char| c.is_whitespace() || c == '_')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let rank = |f: &Flag| -> Option<u8> {
        if typed.is_empty() || f.name.starts_with(&upper) {
            Some(0)
        } else if f.name.contains(&upper) {
            Some(1)
        } else {
            let summary = f.summary.to_lowercase();
            let name = f.name.to_lowercase();
            words
                .iter()
                .all(|w| summary.contains(w.as_str()) || name.contains(w.as_str()))
                .then_some(2)
        }
    };
    let mut out: Vec<(u8, usize, &'static Flag)> = FLAGS
        .iter()
        .enumerate()
        .filter(|(_, f)| !taken.contains(&f.name) && f.name != typed)
        .filter_map(|(i, f)| rank(f).map(|r| (r, i, f)))
        .collect();
    out.sort_by_key(|(r, i, _)| (*r, *i));
    out.into_iter().take(limit).map(|(.., f)| f).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CompatOptions, Renderer, SyncMode, WindowSystem};
    use crate::umu::env::{CompatContext, EnvRule, classify, compat_vars};

    #[test]
    fn every_option_can_be_set_by_anyone_and_no_setting_sets_it() {
        let all = CompatOptions {
            renderer: Renderer::OpenGl,
            window_system: WindowSystem::Wayland,
            sync: SyncMode::NoEsyncNoFsync,
            disable_fixes: true,
            dll_overrides: "d3d9=n".into(),
            mangohud: true,
            dxvk_hud: Some("fps".into()),
            locale: Some("ja_JP.UTF-8".into()),
            proton_log: true,
            wine_debug: Some("+loaddll".into()),
        };
        let cx = CompatContext {
            wayland: true,
            developer_mode: true,
        };
        let set: Vec<&str> = compat_vars(&all, cx).iter().map(|v| v.name).collect();
        let mut seen = std::collections::HashSet::new();
        for f in FLAGS {
            assert_eq!(classify(f.name), EnvRule::Allowed, "{}", f.name);
            assert!(!set.contains(&f.name), "{} has a setting", f.name);
            assert!(seen.insert(f.name), "{} twice", f.name);
            assert!(
                !f.value.is_empty() && !f.summary.ends_with('.'),
                "{}",
                f.name
            );
            assert!(!f.value.starts_with('/'), "{}: no paths", f.name);
        }
    }

    #[test]
    fn suggestions_rank_name_prefix_then_name_then_description() {
        let names = |v: Vec<&Flag>| v.iter().map(|f| f.name).collect::<Vec<_>>();
        assert_eq!(suggest("", &[], 3).len(), 3);
        assert_eq!(suggest("", &[], 3)[0].name, FLAGS[0].name);
        let fr = names(suggest("frame", &[], 10));
        assert!(
            fr.starts_with(&["DXVK_FRAME_RATE", "VKD3D_FRAME_RATE"]),
            "{fr:?}"
        );
        assert_eq!(names(suggest("dxvk_fr", &[], 10)), ["DXVK_FRAME_RATE"]);
        // A description word finds an option whose name does not say it.
        assert!(names(suggest("memory", &[], 10)).contains(&"PROTON_FORCE_LARGE_ADDRESS_AWARE"));
        assert!(names(suggest("ray tracing", &[], 10)).contains(&"VKD3D_CONFIG"));
        // Names already set and the exact name typed are not offered again.
        assert!(!names(suggest("FRAME", &["DXVK_FRAME_RATE"], 10)).contains(&"DXVK_FRAME_RATE"));
        assert!(suggest("PROTON_USE_WOW64", &[], 10).is_empty());
        assert!(suggest("nothing like it", &[], 10).is_empty());
    }
}
