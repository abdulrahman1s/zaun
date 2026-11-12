//! `build(target, game, proton, options) -> UmuInvocation` (UMU_INTEGRATION §3).
//!
//! The invocation is an argument array (never a shell string) plus the umu-specific
//! variables of table A. The sandbox planner places `args` after `umu-run` in the
//! in-sandbox command and sets the same variables; `launch::planner` asserts that both agree.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};

use super::env::{DEFAULT_GAME_ID, RUNTIME_UPDATE_OFF, disables_runtime_update};
use crate::host::UmuFacts;
use crate::model::{SessionKind, Tool, is_valid_winetricks_verb};

/// Throw-away prefix of Provision sessions (equal to `sandbox::plan::PROVISION_PREFIX`).
pub const PROVISION_PREFIX: &str = "/tmp/easyplay-prefix";

/// What umu runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target<'a> {
    /// A game executable (or an alternate executable of the game).
    Game {
        /// Absolute path, identical inside the sandbox.
        exe: &'a Path,
        /// Launch arguments, appended unchanged.
        args: &'a [String],
    },
    /// An installer; `.msi` packages run through `msiexec /i`.
    Installer {
        /// Absolute path of the installer.
        exe: &'a Path,
        /// Arguments.
        args: &'a [String],
    },
    /// A tool in the game's prefix.
    Tool(&'a Tool),
    /// Create or upgrade the prefix without running a program.
    Prepare,
    /// Install a Proton build and/or runtime with a throw-away prefix.
    Provision,
}

impl Target<'_> {
    /// The session kind this target runs as.
    pub const fn kind(&self) -> SessionKind {
        match self {
            Target::Game { .. } => SessionKind::Game,
            Target::Installer { .. } => SessionKind::Installer,
            Target::Tool(_) => SessionKind::Tool,
            Target::Prepare => SessionKind::Prepare,
            Target::Provision => SessionKind::Provision,
        }
    }
}

/// The game-specific inputs (absent for Provision).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UmuGame<'a> {
    /// The game's prefix (`WINEPREFIX`).
    pub prefix: &'a Path,
    /// `GAMEID`; `None` means `umu-default`.
    pub umu_id: Option<&'a str>,
    /// `STORE`.
    pub store: Option<&'a str>,
}

/// `PROTONPATH`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtonPath {
    /// Absolute path of the build as visible in the sandbox: no download (F-104).
    Path(PathBuf),
    /// A codename or exact build name umu may download (F-124): Provision and network
    /// Prepare only.
    Name(String),
}

impl ProtonPath {
    /// The variable's value.
    pub fn value(&self) -> String {
        match self {
            ProtonPath::Path(p) => p.display().to_string(),
            ProtonPath::Name(n) => n.clone(),
        }
    }
}

/// Options that change the invocation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UmuOptions {
    /// Verbose launch logging (`UMU_LOG=1`).
    pub verbose_log: bool,
    /// A network Prepare session (allows a [`ProtonPath::Name`]).
    pub network_prepare: bool,
}

/// One `umu-run` invocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UmuInvocation {
    /// Session kind.
    pub kind: SessionKind,
    /// Absolute path of `umu-run`.
    pub program: PathBuf,
    /// Arguments after the program.
    #[serde(serialize_with = "lossy_os_vec")]
    pub args: Vec<OsString>,
    /// umu-specific variables (table A subset); everything else comes from the sandbox
    /// planner.
    pub env: BTreeMap<String, String>,
}

impl UmuInvocation {
    /// `[program, args…]`.
    pub fn argv(&self) -> Vec<OsString> {
        let mut v = vec![self.program.clone().into_os_string()];
        v.extend(self.args.iter().cloned());
        v
    }
}

fn lossy_os_vec<S: Serializer>(v: &[OsString], s: S) -> Result<S::Ok, S::Error> {
    s.collect_seq(v.iter().map(|a| a.to_string_lossy()))
}

/// Why an invocation cannot be built.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// `umu-run` path is not absolute.
    #[error("umu-run path {0:?} is not absolute")]
    RelativeProgram(PathBuf),
    /// An executable path is not absolute.
    #[error("executable path {0:?} is not absolute")]
    RelativeExecutable(PathBuf),
    /// An argument contains a NUL byte.
    #[error("argument {index} contains a NUL byte")]
    Nul {
        /// Index in `args`.
        index: usize,
    },
    /// A winetricks verb does not match `^[A-Za-z0-9_-]+(=[A-Za-z0-9]*)?$`.
    #[error("invalid winetricks verb {0:?}")]
    InvalidVerb(String),
    /// Winetricks without verbs.
    #[error("winetricks needs at least one verb")]
    NoVerbs,
    /// The kind needs a game (prefix).
    #[error("a {0:?} session needs a game")]
    NeedsGame(SessionKind),
    /// A name instead of a path outside Provision or network Prepare.
    #[error("a {0:?} session needs an absolute compatibility-layer path")]
    NameNotAllowed(SessionKind),
    /// A relative `PROTONPATH`.
    #[error("compatibility-layer path {0:?} is not absolute")]
    RelativeProtonPath(PathBuf),
}

fn is_msi(p: &Path) -> bool {
    p.extension()
        .is_some_and(|e| e.as_bytes().eq_ignore_ascii_case(b"msi"))
}

fn exe_arg(p: &Path) -> Result<OsString, CommandError> {
    if p.is_absolute() {
        Ok(p.as_os_str().to_owned())
    } else {
        Err(CommandError::RelativeExecutable(p.to_path_buf()))
    }
}

fn program_args(target: &Target<'_>) -> Result<Vec<OsString>, CommandError> {
    let strs = |a: &[String]| a.iter().map(OsString::from).collect::<Vec<_>>();
    Ok(match target {
        Target::Game { exe, args } => {
            let mut v = vec![exe_arg(exe)?];
            v.extend(strs(args));
            v
        }
        Target::Installer { exe, args } if is_msi(exe) => {
            let mut v = vec![OsString::from("msiexec"), "/i".into(), exe_arg(exe)?];
            v.extend(strs(args));
            v
        }
        Target::Installer { exe, args } => {
            let mut v = vec![exe_arg(exe)?];
            v.extend(strs(args));
            v
        }
        Target::Tool(tool) => match tool {
            Tool::Winecfg => vec!["winecfg".into()],
            Tool::Regedit => vec!["regedit".into()],
            Tool::ControlPanel => vec!["control".into()],
            Tool::Explorer => vec!["explorer".into()],
            Tool::Winetricks { verbs } => {
                if verbs.is_empty() {
                    return Err(CommandError::NoVerbs);
                }
                let mut v = vec![OsString::from("winetricks")];
                for verb in verbs {
                    if !is_valid_winetricks_verb(verb) {
                        return Err(CommandError::InvalidVerb(verb.clone()));
                    }
                    v.push(verb.into());
                }
                v
            }
            Tool::RunExe { path, args } => {
                let mut v = vec![exe_arg(path)?];
                v.extend(strs(args));
                v
            }
        },
        Target::Prepare | Target::Provision => vec![OsString::new()],
    })
}

/// Build the invocation of §3.
pub fn build(
    target: &Target<'_>,
    game: Option<&UmuGame<'_>>,
    proton: &ProtonPath,
    umu: &UmuFacts,
    options: &UmuOptions,
) -> Result<UmuInvocation, CommandError> {
    let kind = target.kind();
    if !umu.path.is_absolute() {
        return Err(CommandError::RelativeProgram(umu.path.clone()));
    }
    match proton {
        ProtonPath::Path(p) if !p.is_absolute() => {
            return Err(CommandError::RelativeProtonPath(p.clone()));
        }
        ProtonPath::Name(_)
            if !(kind == SessionKind::Provision
                || (kind == SessionKind::Prepare && options.network_prepare)) =>
        {
            return Err(CommandError::NameNotAllowed(kind));
        }
        _ => {}
    }
    let args = program_args(target)?;
    if let Some(index) = args.iter().position(|a| a.as_bytes().contains(&0)) {
        return Err(CommandError::Nul { index });
    }
    let mut env = BTreeMap::new();
    let prefix = match (kind, game) {
        (SessionKind::Provision, _) => PROVISION_PREFIX.to_owned(),
        (_, Some(g)) => g.prefix.display().to_string(),
        (k, None) => return Err(CommandError::NeedsGame(k)),
    };
    env.insert("WINEPREFIX".to_owned(), prefix);
    env.insert("PROTONPATH".to_owned(), proton.value());
    let (umu_id, store) = match (kind, game) {
        (SessionKind::Provision, _) | (_, None) => (None, None),
        (_, Some(g)) => (g.umu_id, g.store),
    };
    env.insert(
        "GAMEID".to_owned(),
        umu_id.unwrap_or(DEFAULT_GAME_ID).to_owned(),
    );
    if let Some(s) = store {
        env.insert("STORE".to_owned(), s.to_owned());
    }
    if disables_runtime_update(kind) {
        env.insert(
            "UMU_RUNTIME_UPDATE".to_owned(),
            RUNTIME_UPDATE_OFF.to_owned(),
        );
    }
    if options.verbose_log {
        env.insert("UMU_LOG".to_owned(), "1".to_owned());
    }
    Ok(UmuInvocation {
        kind,
        program: umu.path.clone(),
        args,
        env,
    })
}

/// The arguments as display strings (tests, inspector).
pub fn display_args(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|a| OsStr::new(a).to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn umu() -> UmuFacts {
        UmuFacts {
            path: "/usr/bin/umu-run".into(),
            version: None,
            closure: Vec::new(),
            wrapper: None,
        }
    }

    const PFX: &str = "/home/sam/.local/share/easyplay/games/G/prefix";
    const PROTON: &str = "/home/sam/.local/share/Steam/compatibilitytools.d/GE-Proton10-25";

    fn game() -> UmuGame<'static> {
        UmuGame {
            prefix: Path::new(PFX),
            umu_id: None,
            store: None,
        }
    }

    fn path() -> ProtonPath {
        ProtonPath::Path(PROTON.into())
    }

    fn args(t: &Target<'_>) -> Vec<String> {
        let inv = build(t, Some(&game()), &path(), &umu(), &UmuOptions::default()).unwrap();
        display_args(&inv.args)
    }

    #[test]
    fn game_path_is_one_verbatim_argument() {
        let hostile = Path::new("/mnt/games/My \"Game\" — 日本/-dash 'x'/$HOME\\a.exe");
        let launch = vec![
            "-windowed".to_owned(),
            "two words".to_owned(),
            String::new(),
        ];
        let inv = build(
            &Target::Game {
                exe: hostile,
                args: &launch,
            },
            Some(&game()),
            &path(),
            &umu(),
            &UmuOptions::default(),
        )
        .unwrap();
        assert_eq!(inv.args[0], hostile.as_os_str());
        assert_eq!(
            display_args(&inv.args[1..]),
            vec!["-windowed", "two words", ""]
        );
        assert_eq!(inv.argv()[0], OsString::from("/usr/bin/umu-run"));
        assert_eq!(inv.kind, SessionKind::Game);
    }

    #[test]
    fn rows_of_section_3() {
        let none: [String; 0] = [];
        let exe = Path::new("/g/setup.exe");
        assert_eq!(
            args(&Target::Installer {
                exe,
                args: &["/S".to_owned()]
            }),
            vec!["/g/setup.exe", "/S"]
        );
        assert_eq!(
            args(&Target::Installer {
                exe: Path::new("/g/pkg.MSI"),
                args: &none
            }),
            vec!["msiexec", "/i", "/g/pkg.MSI"]
        );
        let tools = [
            (Tool::Winecfg, vec!["winecfg"]),
            (Tool::Regedit, vec!["regedit"]),
            (Tool::ControlPanel, vec!["control"]),
            (Tool::Explorer, vec!["explorer"]),
            (
                Tool::Winetricks {
                    verbs: vec!["vcrun2019".into(), "fontsmooth=rgb".into()],
                },
                vec!["winetricks", "vcrun2019", "fontsmooth=rgb"],
            ),
            (
                Tool::RunExe {
                    path: "/g/patch me.exe".into(),
                    args: vec!["--x".into()],
                },
                vec!["/g/patch me.exe", "--x"],
            ),
        ];
        for (tool, expected) in tools {
            assert_eq!(args(&Target::Tool(&tool)), expected, "{tool:?}");
        }
        assert_eq!(args(&Target::Prepare), vec![""]);
    }

    #[test]
    fn environment_per_kind() {
        let g = UmuGame {
            prefix: Path::new(PFX),
            umu_id: Some("umu-1245620"),
            store: Some("gog"),
        };
        let inv = build(
            &Target::Prepare,
            Some(&g),
            &path(),
            &umu(),
            &UmuOptions {
                verbose_log: true,
                ..UmuOptions::default()
            },
        )
        .unwrap();
        let e = |k: &str| inv.env.get(k).map(String::as_str);
        assert_eq!(e("WINEPREFIX"), Some(PFX));
        assert_eq!(e("PROTONPATH"), Some(PROTON));
        assert_eq!(e("GAMEID"), Some("umu-1245620"));
        assert_eq!(e("STORE"), Some("gog"));
        assert_eq!(e("UMU_RUNTIME_UPDATE"), Some("0"));
        assert_eq!(e("UMU_LOG"), Some("1"));
        assert_eq!(e("PROTON_VERB"), None);

        let inv = build(
            &Target::Provision,
            None,
            &ProtonPath::Name("GE-Proton".into()),
            &umu(),
            &UmuOptions::default(),
        )
        .unwrap();
        assert_eq!(display_args(&inv.args), vec![""]);
        let e = |k: &str| inv.env.get(k).map(String::as_str);
        assert_eq!(e("WINEPREFIX"), Some(PROVISION_PREFIX));
        assert_eq!(e("PROTONPATH"), Some("GE-Proton"));
        assert_eq!(e("GAMEID"), Some("umu-default"));
        assert_eq!(e("UMU_RUNTIME_UPDATE"), None);
        assert_eq!(e("UMU_LOG"), None);
        for k in inv.env.keys() {
            assert!(!super::super::env::is_never_set(k));
        }
    }

    #[test]
    fn rejections() {
        let o = UmuOptions::default();
        let none: [String; 0] = [];
        let bad_verb = Tool::Winetricks {
            verbs: vec!["vcrun; rm -rf".into()],
        };
        let cases: Vec<(Target<'_>, Option<UmuGame<'_>>, ProtonPath, CommandError)> = vec![
            (
                Target::Tool(&bad_verb),
                Some(game()),
                path(),
                CommandError::InvalidVerb("vcrun; rm -rf".into()),
            ),
            (
                Target::Game {
                    exe: Path::new("rel.exe"),
                    args: &none,
                },
                Some(game()),
                path(),
                CommandError::RelativeExecutable("rel.exe".into()),
            ),
            (
                Target::Prepare,
                None,
                path(),
                CommandError::NeedsGame(SessionKind::Prepare),
            ),
            (
                Target::Game {
                    exe: Path::new("/a.exe"),
                    args: &none,
                },
                Some(game()),
                ProtonPath::Name("GE-Proton".into()),
                CommandError::NameNotAllowed(SessionKind::Game),
            ),
            (
                Target::Prepare,
                Some(game()),
                ProtonPath::Path("rel".into()),
                CommandError::RelativeProtonPath("rel".into()),
            ),
        ];
        for (t, g, p, err) in cases {
            assert_eq!(build(&t, g.as_ref(), &p, &umu(), &o), Err(err));
        }
        let empty = Tool::Winetricks { verbs: Vec::new() };
        assert_eq!(
            build(&Target::Tool(&empty), Some(&game()), &path(), &umu(), &o),
            Err(CommandError::NoVerbs)
        );
        let nul = vec!["a\0b".to_owned()];
        assert_eq!(
            build(
                &Target::Game {
                    exe: Path::new("/a.exe"),
                    args: &nul
                },
                Some(&game()),
                &path(),
                &umu(),
                &o
            ),
            Err(CommandError::Nul { index: 1 })
        );
        // A network Prepare may download.
        assert!(
            build(
                &Target::Prepare,
                Some(&game()),
                &ProtonPath::Name("UMU-Proton".into()),
                &umu(),
                &UmuOptions {
                    network_prepare: true,
                    ..o
                }
            )
            .is_ok()
        );
    }
}
