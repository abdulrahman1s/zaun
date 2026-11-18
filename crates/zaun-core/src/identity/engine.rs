//! Which engine a game is made with (ADR-0028), told by the names of the files beside its
//! program and in its folder, and by the program's version resource. Informational: nothing
//! is decided by it.
//!
//! Only names are read (a directory listing of the program's folder, of the game folder, and
//! of a few named folders such as `bin` or `www`); no file is opened. A rule that needs more
//! than names is not here.

use crate::model::PeInfo;

/// An engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Engine {
    /// Unity.
    Unity,
    /// Unreal Engine.
    Unreal,
    /// Godot.
    Godot,
    /// GameMaker.
    GameMaker,
    /// Ren'Py.
    RenPy,
    /// RPG Maker.
    RpgMaker,
    /// Source (Valve).
    Source,
    /// Source 2 (Valve).
    Source2,
    /// CryEngine.
    CryEngine,
    /// FNA (XNA reimplementation).
    Fna,
    /// MonoGame.
    MonoGame,
    /// Microsoft XNA.
    Xna,
    /// LÖVE.
    Love,
    /// Cocos2d-x.
    Cocos,
    /// Electron.
    Electron,
    /// NW.js.
    NwJs,
}

/// What told the engine apart, beyond the engine itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Variant {
    /// Unity's scripts compiled to native code.
    Il2Cpp,
    /// Unity's scripts run by Mono.
    Mono,
    /// RPG Maker XP.
    Xp,
    /// RPG Maker VX.
    Vx,
    /// RPG Maker VX Ace.
    VxAce,
    /// RPG Maker MV.
    Mv,
    /// RPG Maker MZ.
    Mz,
}

/// An engine and, when the files tell, its variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EngineGuess {
    /// The engine.
    pub engine: Engine,
    /// Its variant.
    pub variant: Option<Variant>,
}

impl EngineGuess {
    const fn plain(engine: Engine) -> Self {
        Self {
            engine,
            variant: None,
        }
    }
}

/// What the rules look at.
pub struct EngineInput<'a> {
    /// The program's file name.
    pub exe_name: &'a str,
    /// The folders above the program, nearest first, up to the game folder (names only).
    pub exe_dirs: &'a [String],
    /// The program's version resource.
    pub pe: &'a PeInfo,
    /// Names beside the program.
    pub siblings: &'a [String],
    /// Names in the game folder, when it is not the program's folder.
    pub root: &'a [String],
    /// Names in a folder beside the program, by name (empty when it is not there).
    pub sub: &'a dyn Fn(&str) -> Vec<String>,
}

fn has(names: &[String], name: &str) -> bool {
    names.iter().any(|n| n.eq_ignore_ascii_case(name))
}

fn any(names: &[String], test: impl Fn(&str) -> bool) -> bool {
    names.iter().any(|n| test(&n.to_ascii_lowercase()))
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

/// The engine the input points to, the first rule that matches.
pub fn detect(i: &EngineInput<'_>) -> Option<EngineGuess> {
    use Engine as E;
    let s = i.siblings;
    let exe = i.exe_name.to_ascii_lowercase();
    let stem = stem(i.exe_name);
    let version = |word: &str| {
        [&i.pe.product_name, &i.pe.file_description]
            .into_iter()
            .flatten()
            .any(|v| v.to_ascii_lowercase().contains(word))
    };

    if has(s, "UnityPlayer.dll") || has(s, &format!("{stem}_Data")) {
        let variant = if has(s, "GameAssembly.dll") {
            Some(Variant::Il2Cpp)
        } else if has(s, "MonoBleedingEdge") || has(s, "Mono") {
            Some(Variant::Mono)
        } else {
            None
        };
        return Some(EngineGuess {
            engine: E::Unity,
            variant,
        });
    }
    let in_binaries = i
        .exe_dirs
        .get(1)
        .is_some_and(|d| d.eq_ignore_ascii_case("Binaries"));
    if exe.ends_with("-shipping.exe") || (in_binaries && has(i.root, "Engine")) {
        return Some(EngineGuess::plain(E::Unreal));
    }
    // A bootstrap program in the game folder, beside Unreal's `Engine/Binaries`.
    if has(s, "Engine") && has(&(i.sub)("Engine"), "Binaries") {
        return Some(EngineGuess::plain(E::Unreal));
    }
    if has(s, &format!("{stem}.pck")) || version("godot") {
        return Some(EngineGuess::plain(E::Godot));
    }
    if has(s, "data.win") {
        return Some(EngineGuess::plain(E::GameMaker));
    }
    if has(s, "renpy") && has(s, "lib") {
        return Some(EngineGuess::plain(E::RenPy));
    }
    let rgss = |v, ext: &str, dll: &str| {
        (any(s, |n| {
            n.ends_with(ext) || (n.starts_with(dll) && n.ends_with(".dll"))
        }))
        .then_some(EngineGuess {
            engine: E::RpgMaker,
            variant: Some(v),
        })
    };
    if let Some(g) = rgss(Variant::VxAce, ".rgss3a", "rgss3")
        .or_else(|| rgss(Variant::Vx, ".rgss2a", "rgss2"))
        .or_else(|| rgss(Variant::Xp, ".rgssad", "rgss1"))
    {
        return Some(g);
    }
    let nw = has(s, "nw.dll") || has(s, "nw_elf.dll");
    if nw && has(s, "js") && has(&(i.sub)("js"), "rmmz_core.js") {
        return Some(EngineGuess {
            engine: E::RpgMaker,
            variant: Some(Variant::Mz),
        });
    }
    if nw && has(s, "www") {
        return Some(EngineGuess {
            engine: E::RpgMaker,
            variant: Some(Variant::Mv),
        });
    }
    if has(s, "engine2.dll") {
        return Some(EngineGuess::plain(E::Source2));
    }
    if has(s, "bin") && has(&(i.sub)("bin"), "engine.dll") {
        return Some(EngineGuess::plain(E::Source));
    }
    if has(s, "CrySystem.dll") {
        return Some(EngineGuess::plain(E::CryEngine));
    }
    if i.pe.is_dotnet {
        if has(s, "FNA.dll") {
            return Some(EngineGuess::plain(E::Fna));
        }
        if has(s, "MonoGame.Framework.dll") {
            return Some(EngineGuess::plain(E::MonoGame));
        }
        if any(s, |n| n.starts_with("microsoft.xna.framework")) {
            return Some(EngineGuess::plain(E::Xna));
        }
    }
    if has(s, "love.dll") {
        return Some(EngineGuess::plain(E::Love));
    }
    if has(s, "libcocos2d.dll") {
        return Some(EngineGuess::plain(E::Cocos));
    }
    if has(s, "resources")
        && (has(s, "v8_context_snapshot.bin") || has(s, "snapshot_blob.bin"))
        && has(&(i.sub)("resources"), "app.asar")
    {
        return Some(EngineGuess::plain(E::Electron));
    }
    if nw {
        return Some(EngineGuess::plain(E::NwJs));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn guess(exe: &str, dirs: &[&str], siblings: &[&str], root: &[&str]) -> Option<EngineGuess> {
        guess_with(exe, dirs, siblings, root, &[], &PeInfo::unknown())
    }

    fn guess_with(
        exe: &str,
        dirs: &[&str],
        siblings: &[&str],
        root: &[&str],
        subs: &[(&str, &[&str])],
        pe: &PeInfo,
    ) -> Option<EngineGuess> {
        let subs: Vec<(String, Vec<String>)> = subs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), names(v)))
            .collect();
        let sub = move |n: &str| {
            subs.iter()
                .find(|(k, _)| k == n)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        detect(&EngineInput {
            exe_name: exe,
            exe_dirs: &names(dirs),
            pe,
            siblings: &names(siblings),
            root: &names(root),
            sub: &sub,
        })
    }

    fn engine(g: Option<EngineGuess>) -> Option<(Engine, Option<Variant>)> {
        g.map(|g| (g.engine, g.variant))
    }

    #[test]
    fn unity_and_how_its_scripts_run() {
        let il2cpp = guess(
            "Game.exe",
            &[],
            &[
                "Game.exe",
                "UnityPlayer.dll",
                "GameAssembly.dll",
                "Game_Data",
            ],
            &[],
        );
        assert_eq!(engine(il2cpp), Some((Engine::Unity, Some(Variant::Il2Cpp))));
        let mono = guess("Game.exe", &[], &["Game_Data", "MonoBleedingEdge"], &[]);
        assert_eq!(engine(mono), Some((Engine::Unity, Some(Variant::Mono))));
    }

    #[test]
    fn unreal_by_its_shipping_program_or_its_layout() {
        let shipping = guess("Hollow-Win64-Shipping.exe", &[], &[], &[]);
        assert_eq!(engine(shipping), Some((Engine::Unreal, None)));
        let layout = guess(
            "Hollow.exe",
            &["Win64", "Binaries", "Hollow"],
            &["Hollow.exe"],
            &["Engine", "Hollow", "Hollow.exe"],
        );
        assert_eq!(engine(layout), Some((Engine::Unreal, None)));
        let bootstrap = guess_with(
            "Hollow.exe",
            &[],
            &["Engine", "Hollow", "Hollow.exe"],
            &[],
            &[("Engine", &["Binaries", "Content"])],
            &PeInfo::unknown(),
        );
        assert_eq!(engine(bootstrap), Some((Engine::Unreal, None)));
    }

    #[test]
    fn engines_told_by_one_file() {
        let cases: [(&str, &[&str], Engine); 7] = [
            ("Game.exe", &["Game.pck"], Engine::Godot),
            ("Game.exe", &["data.win", "options.ini"], Engine::GameMaker),
            ("Game.exe", &["renpy", "lib", "game"], Engine::RenPy),
            ("Game.exe", &["engine2.dll"], Engine::Source2),
            ("Game.exe", &["CrySystem.dll"], Engine::CryEngine),
            ("Game.exe", &["love.dll", "SDL2.dll"], Engine::Love),
            ("Game.exe", &["libcocos2d.dll"], Engine::Cocos),
        ];
        for (exe, siblings, want) in cases {
            assert_eq!(
                guess(exe, &[], siblings, &[]).map(|g| g.engine),
                Some(want),
                "{siblings:?}"
            );
        }
    }

    #[test]
    fn rpg_maker_by_its_archives_and_scripts() {
        let vxace = guess("Game.exe", &[], &["Game.rgss3a", "System"], &[]);
        assert_eq!(
            engine(vxace),
            Some((Engine::RpgMaker, Some(Variant::VxAce)))
        );
        let xp = guess("Game.exe", &[], &["RGSS104E.dll"], &[]);
        assert_eq!(engine(xp), Some((Engine::RpgMaker, Some(Variant::Xp))));
        let mz = guess_with(
            "Game.exe",
            &[],
            &["nw.dll", "js", "data"],
            &[],
            &[("js", &["rmmz_core.js", "main.js"])],
            &PeInfo::unknown(),
        );
        assert_eq!(engine(mz), Some((Engine::RpgMaker, Some(Variant::Mz))));
        let mv = guess("Game.exe", &[], &["nw.dll", "www"], &[]);
        assert_eq!(engine(mv), Some((Engine::RpgMaker, Some(Variant::Mv))));
        // NW.js without RPG Maker's folders is NW.js.
        assert_eq!(
            guess("Game.exe", &[], &["nw.dll", "package.nw"], &[]).map(|g| g.engine),
            Some(Engine::NwJs)
        );
    }

    #[test]
    fn source_and_electron_look_into_one_folder() {
        let source = guess_with(
            "hl2.exe",
            &[],
            &["hl2.exe", "bin", "hl2"],
            &[],
            &[("bin", &["engine.dll", "client.dll"])],
            &PeInfo::unknown(),
        );
        assert_eq!(source.map(|g| g.engine), Some(Engine::Source));
        let electron = guess_with(
            "Game.exe",
            &[],
            &["resources", "v8_context_snapshot.bin", "ffmpeg.dll"],
            &[],
            &[("resources", &["app.asar"])],
            &PeInfo::unknown(),
        );
        assert_eq!(electron.map(|g| g.engine), Some(Engine::Electron));
    }

    #[test]
    fn dotnet_frameworks_need_a_dotnet_program() {
        let dotnet = PeInfo {
            is_dotnet: true,
            ..PeInfo::unknown()
        };
        let fna = guess_with("Game.exe", &[], &["FNA.dll"], &[], &[], &dotnet);
        assert_eq!(fna.map(|g| g.engine), Some(Engine::Fna));
        let xna = guess_with(
            "Game.exe",
            &[],
            &["Microsoft.Xna.Framework.Game.dll"],
            &[],
            &[],
            &dotnet,
        );
        assert_eq!(xna.map(|g| g.engine), Some(Engine::Xna));
        assert_eq!(guess("Game.exe", &[], &["FNA.dll"], &[]), None);
    }

    #[test]
    fn godot_by_its_version_resource_and_nothing_by_default() {
        let pe = PeInfo {
            product_name: Some("Godot Engine".into()),
            ..PeInfo::unknown()
        };
        assert_eq!(
            guess_with("x.exe", &[], &[], &[], &[], &pe).map(|g| g.engine),
            Some(Engine::Godot)
        );
        assert_eq!(
            guess("Game.exe", &[], &["Game.exe", "readme.txt"], &[]),
            None
        );
    }
}
