//! `clap` definitions of the command line (CONFIGURATION §6). Validates syntax only; domain
//! rules (game resolution, argument limits) are checked by core.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// `easyplay --version`: version and commit.
pub const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("EASYPLAY_COMMIT"),
    ")"
);

/// Exit codes (CONFIGURATION §6).
pub mod exit {
    /// Success.
    pub const OK: u8 = 0;
    /// The operation failed.
    pub const FAILED: u8 = 1;
    /// Usage error (also clap's own code).
    pub const USAGE: u8 = 2;
    /// Already running, or a lock is held.
    pub const BUSY: u8 = 3;
    /// A host requirement is missing.
    pub const HOST: u8 = 4;
    /// A library migration is pending (start the GUI once).
    pub const MIGRATION: u8 = 5;
}

/// Run Windows games through umu-run inside a per-game Bubblewrap sandbox.
#[derive(Debug, Parser)]
#[command(name = "easyplay", version = VERSION, about, long_about = None)]
pub struct Cli {
    /// Increase log verbosity (-v debug, -vv trace).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Show the sample library instead of yours (UI preview; also `EASYPLAY_PREVIEW=1`).
    #[arg(long, hide = true)]
    pub preview: bool,

    /// Start (or focus) EasyPlay and open the add-game dialog.
    #[arg(long)]
    pub add: bool,

    /// What to do; without a command EasyPlay starts (or focuses) the application.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// `<GAME>`: a game ID, a unique ID prefix of at least 6 characters, or a name matching
/// exactly one game case-insensitively.
pub type GameArg = String;

/// The verbs.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Open Windows programs: start known ones, set up unknown ones.
    Open {
        /// Programs to open.
        files: Vec<PathBuf>,
    },
    /// Launch a game.
    Launch(LaunchArgs),
    /// Stop a running game.
    Stop {
        /// The game.
        game: GameArg,
        /// Force-stop immediately instead of asking the game to close.
        #[arg(long)]
        force: bool,
    },
    /// Show running games and the latest sessions.
    Status {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// List games.
    List {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
        /// Only running games.
        #[arg(long)]
        running: bool,
        /// Only games with this tag.
        #[arg(long, value_name = "TAG")]
        tag: Option<String>,
    },
    /// Add a program to the library.
    Add {
        /// The program.
        file: PathBuf,
        /// Name of the game.
        #[arg(long)]
        name: Option<String>,
        /// Protection preset.
        #[arg(long, value_enum)]
        preset: Option<PresetArg>,
        /// Do not launch after adding.
        #[arg(long)]
        no_launch: bool,
        /// Compatibility layer: `default`, `umu-proton`, `ge-proton`, a build name, or the
        /// absolute path of a Proton directory.
        #[arg(long, value_name = "RUNTIME")]
        runtime: Option<String>,
    },
    /// Change a game's settings.
    Edit {
        /// The game.
        game: GameArg,
        /// Compatibility layer: `default`, `umu-proton`, `ge-proton`, a build name, or the
        /// absolute path of a Proton directory.
        #[arg(long, value_name = "RUNTIME")]
        runtime: Option<String>,
        /// Keep using the current build of the channel instead of following its updates.
        #[arg(long)]
        no_follow_updates: bool,
    },
    /// Remove a game from the library.
    Remove {
        /// The game.
        game: GameArg,
        /// Keep its private Windows storage (prefix and home).
        #[arg(long)]
        keep_storage: bool,
    },
    /// Show a game's settings.
    Show {
        /// The game.
        game: GameArg,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Inspect sandboxes.
    Sandbox {
        /// What to do.
        #[command(subcommand)]
        command: SandboxCommand,
    },
    /// Export a game's settings as TOML.
    Export {
        /// The game.
        game: GameArg,
        /// Output file (default: the exports folder).
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// Import a game's settings.
    Import {
        /// The exported settings file.
        file: PathBuf,
        /// The program the settings apply to.
        #[arg(long, value_name = "FILE")]
        exe: PathBuf,
    },
    /// Check this system for everything EasyPlay needs.
    Doctor {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Manage compatibility layers.
    Runtimes {
        /// What to do.
        #[command(subcommand)]
        command: RuntimesCommand,
    },
    /// Games installed with Steam (shown when "Show my Steam games" is on).
    Steam {
        /// What to do.
        #[command(subcommand)]
        command: SteamVerb,
    },
    /// Keep EasyPlay running in the background so opened programs start at once.
    ///
    /// Usually started at login by the `easyplay.service` systemd user unit. Waits while an
    /// EasyPlay window started on its own is open.
    Service,
    /// Supervise one session (internal; started by EasyPlay itself).
    #[command(hide = true)]
    Supervise {
        /// The launch plan file.
        #[arg(long, value_name = "FILE")]
        plan: PathBuf,
    },
    /// Write the man page and shell completions (packaging; RELEASE_STRATEGY §7).
    #[command(hide = true, name = "generate-assets")]
    GenerateAssets {
        /// Output directory: `man/man1/easyplay.1` and `completions/` are created in it.
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
    },
}

/// Arguments of `launch`.
#[derive(Debug, Args, PartialEq, Eq)]
pub struct LaunchArgs {
    /// The game.
    pub game: GameArg,
    /// Which program of the game (label or ID; default the primary one).
    #[arg(long, value_name = "LABEL|ID")]
    pub exe: Option<String>,
    /// Wait until the game ends; exit 0 for a normal end, 1 for failure to start, 2 for a crash.
    #[arg(long)]
    pub wait: bool,
    /// Extra arguments for this run only, after `--`.
    #[arg(last = true, value_name = "EXTRA ARGS")]
    pub extra: Vec<String>,
}

/// `sandbox` subcommands.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum SandboxCommand {
    /// Print the sandbox a launch would use.
    Print {
        /// The game.
        game: GameArg,
        /// Output format.
        #[arg(long, value_enum, default_value_t = SandboxFormat::Args)]
        format: SandboxFormat,
        /// Session kind.
        #[arg(long, value_enum, default_value_t = PlanKind::Game)]
        kind: PlanKind,
    },
}

/// `runtimes` subcommands.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum RuntimesCommand {
    /// List installed compatibility layers.
    List,
    /// Check for updates.
    Check,
    /// Verify the runtimes against their manifests (a modified runtime is quarantined).
    Verify,
    /// Download and install a compatibility layer and its runtime (needs the internet once).
    Install {
        /// What to install: `umu-proton` (default), `ge-proton`, an exact build name such as
        /// `GE-Proton10-25`, or the path or name of an installed build whose runtime is
        /// missing.
        #[arg(value_name = "WHAT", default_value = "umu-proton")]
        what: String,
    },
    /// Remove a compatibility layer EasyPlay downloaded (not the newest, not one in use).
    Remove {
        /// The build's name.
        name: String,
    },
    /// Copy the Steam Linux Runtime that umu installed in ~/.local/share/umu into EasyPlay's
    /// store (reflinked where possible; the original is never modified).
    Adopt {
        /// Only this variant (default: every variant installed there and missing here).
        #[arg(long, value_enum)]
        variant: Option<VariantArg>,
    },
}

/// `steam` subcommands (ADR-0031). A Steam game is named like any game, or `steam:<appid>`.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum SteamVerb {
    /// Read Steam's files now and list what was found (changes nothing).
    Scan {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Show the games installed with Steam in EasyPlay.
    On,
    /// Stop showing them (what you set for them is kept).
    Off,
    /// Open Steam's downloads, where the game's update is started.
    Update {
        /// The game.
        game: GameArg,
    },
    /// Show the game in Steam (its properties hold the Proton choice and launch options).
    Open {
        /// The game.
        game: GameArg,
    },
}

/// `runtimes adopt --variant`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum VariantArg {
    /// Steam Linux Runtime 3.0 (sniper).
    Steamrt3,
    /// Steam Linux Runtime 4.0.
    Steamrt4,
}

/// `--preset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum PresetArg {
    /// No internet; only its own folder.
    Offline,
    /// Internet; only its own folder.
    Online,
    /// Internet; fewer restrictions.
    Standard,
}

/// `sandbox print --format`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SandboxFormat {
    /// bwrap arguments, one per line.
    Args,
    /// The plan as JSON.
    Json,
    /// Plain-language access summary.
    Explain,
    /// A POSIX shell script that reproduces the launch by hand (DEVELOPER_MODE §5).
    Shell,
}

/// `sandbox print --kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum PlanKind {
    /// A game session.
    Game,
    /// A prefix preparation session.
    Prepare,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("easyplay").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn definitions_are_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn verbs_parse() {
        assert_eq!(parse(&[]).command, None);
        assert_eq!(parse(&["-vv"]).verbose, 2);
        assert_eq!(
            parse(&["open", "a.exe", "b.exe"]).command,
            Some(Command::Open {
                files: vec!["a.exe".into(), "b.exe".into()]
            })
        );
        assert_eq!(
            parse(&[
                "launch",
                "hollow",
                "--exe",
                "dx12",
                "--wait",
                "--",
                "-windowed",
                "--x"
            ])
            .command,
            Some(Command::Launch(LaunchArgs {
                game: "hollow".into(),
                exe: Some("dx12".into()),
                wait: true,
                extra: vec!["-windowed".into(), "--x".into()],
            }))
        );
        assert_eq!(
            parse(&["stop", "g", "--force", "-v"]).command,
            Some(Command::Stop {
                game: "g".into(),
                force: true
            })
        );
        assert!(matches!(
            parse(&["list", "--json", "--running", "--tag", "rpg"]).command,
            Some(Command::List {
                json: true,
                running: true,
                tag: Some(_)
            })
        ));
        assert!(matches!(
            parse(&[
                "add",
                "x.exe",
                "--name",
                "X",
                "--preset",
                "online",
                "--no-launch"
            ])
            .command,
            Some(Command::Add {
                preset: Some(PresetArg::Online),
                no_launch: true,
                ..
            })
        ));
        assert_eq!(
            parse(&["sandbox", "print", "g"]).command,
            Some(Command::Sandbox {
                command: SandboxCommand::Print {
                    game: "g".into(),
                    format: SandboxFormat::Args,
                    kind: PlanKind::Game
                }
            })
        );
        assert!(matches!(
            parse(&[
                "sandbox", "print", "g", "--format", "explain", "--kind", "prepare"
            ])
            .command,
            Some(Command::Sandbox {
                command: SandboxCommand::Print {
                    format: SandboxFormat::Explain,
                    kind: PlanKind::Prepare,
                    ..
                }
            })
        ));
        assert!(matches!(
            parse(&["add", "x.exe", "--runtime", "/opt/proton"]).command,
            Some(Command::Add {
                runtime: Some(_),
                ..
            })
        ));
        assert!(matches!(
            parse(&["edit", "g", "--runtime", "ge-proton", "--no-follow-updates"]).command,
            Some(Command::Edit {
                runtime: Some(_),
                no_follow_updates: true,
                ..
            })
        ));
        assert_eq!(
            parse(&["runtimes", "install"]).command,
            Some(Command::Runtimes {
                command: RuntimesCommand::Install {
                    what: "umu-proton".into()
                }
            })
        );
        assert!(matches!(
            parse(&["runtimes", "remove", "GE-Proton10-9"]).command,
            Some(Command::Runtimes {
                command: RuntimesCommand::Remove { .. }
            })
        ));
        assert!(matches!(
            parse(&["export", "g", "-o", "x.toml"]).command,
            Some(Command::Export {
                output: Some(_),
                ..
            })
        ));
        assert!(matches!(
            parse(&["import", "x.toml", "--exe", "g.exe"]).command,
            Some(Command::Import { .. })
        ));
        assert!(matches!(
            parse(&["doctor", "--json"]).command,
            Some(Command::Doctor { json: true })
        ));
        for (v, c) in [
            ("list", RuntimesCommand::List),
            ("check", RuntimesCommand::Check),
            ("verify", RuntimesCommand::Verify),
        ] {
            assert_eq!(
                parse(&["runtimes", v]).command,
                Some(Command::Runtimes { command: c })
            );
        }
        assert_eq!(
            parse(&["status", "--json"]).command,
            Some(Command::Status { json: true })
        );
        assert!(matches!(
            parse(&["supervise", "--plan", "/run/p.json"]).command,
            Some(Command::Supervise { .. })
        ));
    }

    #[test]
    fn usage_errors() {
        for args in [
            &["launch"][..],
            &["add", "x.exe", "--preset", "unsafe"][..],
            &["sandbox", "print", "g", "--format", "yaml"][..],
            &["frobnicate"][..],
        ] {
            let err = Cli::try_parse_from(std::iter::once("easyplay").chain(args.iter().copied()))
                .unwrap_err();
            assert_eq!(err.exit_code(), i32::from(exit::USAGE), "{args:?}");
        }
    }

    /// `--help` of every visible verb, reviewed as snapshots (CLI-001).
    #[test]
    fn help_snapshots() {
        fn walk(cmd: &mut clap::Command, prefix: &str) {
            let name = if prefix.is_empty() {
                "easyplay".to_owned()
            } else {
                format!("{prefix}-{}", cmd.get_name())
            };
            insta::assert_snapshot!(format!("help_{name}"), cmd.render_help().to_string());
            let subs: Vec<clap::Command> = cmd
                .get_subcommands()
                // `runtimes` belongs to runtime provisioning and is reviewed there.
                .filter(|c| !c.is_hide_set() && !matches!(c.get_name(), "help" | "runtimes"))
                .cloned()
                .collect();
            for mut s in subs {
                walk(&mut s, &name);
            }
        }
        let mut cmd = Cli::command();
        cmd.build();
        walk(&mut cmd, "");
    }

    #[test]
    fn supervise_is_hidden() {
        let help = Cli::command().render_help().to_string();
        assert!(!help.contains("supervise"));
        assert!(help.contains("sandbox"));
        assert!(VERSION.starts_with(env!("CARGO_PKG_VERSION")));
    }
}
