//! `zaun update check|apply|on|off` (ADR-0040, CONFIGURATION §6). Nothing is requested while
//! `updates.check` is off.

use std::sync::Arc;

use zaun_core::artwork::http::{HttpClient, UreqTransport};
use zaun_core::clock::{Clock, SystemClock};
use zaun_core::config::PreferencePatch;
use zaun_core::fsx::{Fs, RealFs};
use zaun_core::host::RealRunner;
use zaun_core::model::InstallChannel;
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_core::update::check::fetch_release;
use zaun_core::update::source::TRUSTED_KEYS;
use zaun_core::update::state::{Latest, state_file};
use zaun_core::update::{Install, Release, UpdateDeps, UpdateState, Version, apply, stage};
use zaun_core::{Error, ErrorCode, bail, err};

use crate::cli::{UpdateVerb, exit};
use crate::steam_verbs::gui_running;
use crate::wiring;

/// What the user does to update a copy installed this way.
pub fn instruction(c: InstallChannel) -> &'static str {
    match c {
        InstallChannel::Nix => {
            "Update the flake input that provides Zaun (`nix flake update zaun`), then rebuild (`nixos-rebuild switch` or `home-manager switch`)."
        }
        InstallChannel::Flatpak => "Run `flatpak update`.",
        InstallChannel::Package => "Update Zaun with your system's package manager.",
        InstallChannel::Tarball { owned: true } => {
            "Run `zaun update apply`, or use Settings › About in Zaun."
        }
        InstallChannel::Tarball { owned: false } => {
            "Run install.sh from the new release tarball again, as the user who installed Zaun."
        }
        InstallChannel::Other => "Install the new release the way you installed this copy.",
    }
}

fn channel_name(c: InstallChannel) -> &'static str {
    match c {
        InstallChannel::Nix => "nix",
        InstallChannel::Flatpak => "flatpak",
        InstallChannel::Package => "package",
        InstallChannel::Tarball { owned: true } => "tarball",
        InstallChannel::Tarball { owned: false } => "tarball-system",
        InstallChannel::Other => "other",
    }
}

fn running() -> Result<Version, Error> {
    Version::parse(zaun_core::consts::APP_VERSION)
        .ok_or_else(|| err!(IntTaskFailed, detail = "own version"))
}

struct Ctx {
    paths: AppPaths,
    fs: RealFs,
    clock: Arc<dyn Clock>,
    install: Install,
}

impl Ctx {
    fn new() -> Result<Self, Error> {
        let env = PathEnv::from_process();
        let paths = AppPaths::resolve(&env)
            .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("paths", e))?;
        let fs = RealFs::new();
        let install = wiring::installed(&fs, env.uid());
        Ok(Self {
            paths,
            fs,
            clock: Arc::new(SystemClock::new()),
            install,
        })
    }

    fn consent(&self) -> Result<(), Error> {
        let prefs = wiring::load_prefs(&self.fs, &self.paths, &*self.clock);
        if !prefs.updates.check {
            bail!(UpdChecksOff);
        }
        Ok(())
    }

    fn base(&self) -> HttpClient {
        HttpClient::new(Arc::new(UreqTransport::new()), Arc::clone(&self.clock))
    }

    /// Fetch, verify and remember the newest release.
    fn fetch(&self) -> Result<Release, Error> {
        self.consent()?;
        let http = UpdateDeps::check_client(&self.base());
        let release = fetch_release(&http, TRUSTED_KEYS)?;
        let path = state_file(&self.paths);
        let mut st = UpdateState::load(&self.fs, &path);
        st.last_checked = Some(self.clock.now());
        st.latest = (release.version > running()?).then(|| Latest::of(&release));
        st.save(&self.fs, &path)?;
        Ok(release)
    }
}

/// Run an `update` verb; returns the exit code.
pub fn run(verb: &UpdateVerb) -> Result<u8, Error> {
    let ctx = Ctx::new()?;
    match verb {
        UpdateVerb::Check { json } => check(&ctx, *json),
        UpdateVerb::Apply => apply_now(&ctx),
        UpdateVerb::On => switch(&ctx, true),
        UpdateVerb::Off => switch(&ctx, false),
    }
}

#[allow(clippy::print_stdout)]
fn check(ctx: &Ctx, json: bool) -> Result<u8, Error> {
    let release = ctx.fetch()?;
    let current = running()?;
    let newer = release.version > current;
    let channel = ctx.install.channel;
    if json {
        let v = serde_json::json!({
            "format": 1,
            "current": current.to_string(),
            "available": newer.then(|| release.version.to_string()),
            "date": newer.then(|| release.date.clone()),
            "notes_url": if newer { release.notes_url.clone() } else { None },
            "channel": channel_name(channel),
            "self_updates": channel.self_updates(),
        });
        println!("{v}");
    } else {
        println!("Installed: {current} ({})", channel_name(channel));
        if newer {
            println!("Available: {} ({})", release.version, release.date);
            if let Some(u) = &release.notes_url {
                println!("Notes:     {u}");
            }
            println!("{}", instruction(channel));
        } else {
            println!("Zaun is up to date.");
        }
    }
    Ok(if newer {
        exit::UPDATE_AVAILABLE
    } else {
        exit::OK
    })
}

#[allow(clippy::print_stdout)]
fn apply_now(ctx: &Ctx) -> Result<u8, Error> {
    let layout = ctx
        .install
        .layout()
        .ok_or_else(|| err!(UpdManaged, detail = instruction(ctx.install.channel)))?;
    if gui_running(&ctx.paths) {
        bail!(
            LibBusy,
            detail = "Zaun is open; use Settings › About › Restart now, or quit it first"
        );
    }
    let open = ctx.fs.exists(&ctx.paths.db_file)
        && zaun_core::storage::Db::open_ro(&ctx.paths.db_file)
            .and_then(|mut db| zaun_core::storage::SessionRepo::open_sessions(&mut db))
            .map_or(true, |s| !s.is_empty());
    if open {
        bail!(UpdGameRunning);
    }
    let current = running()?;
    if !ctx.fs.exists(&layout.staged()) {
        let release = ctx.fetch()?;
        if release.version <= current {
            println!("Zaun is up to date.");
            return Ok(exit::OK);
        }
        println!("Downloading Zaun {}…", release.version);
        let files = UpdateDeps::files_client(&ctx.base());
        stage::download(&files, &ctx.fs, &layout, &release, std::env::consts::ARCH)?;
    }
    let runner = RealRunner::new(Arc::clone(&ctx.clock));
    match apply::apply(
        &ctx.fs,
        &runner,
        &layout,
        TRUSTED_KEYS,
        &current,
        std::env::consts::ARCH,
    )? {
        Some(v) => println!("Zaun {v} is installed. It runs the next time you start Zaun."),
        None => println!("Nothing to install."),
    }
    Ok(exit::OK)
}

#[allow(clippy::print_stdout)]
fn switch(ctx: &Ctx, on: bool) -> Result<u8, Error> {
    if gui_running(&ctx.paths) {
        bail!(
            LibBusy,
            detail = "Zaun is open; change \"Check for updates\" in Settings › About"
        );
    }
    let prefs = wiring::load_prefs(&ctx.fs, &ctx.paths, &*ctx.clock);
    let next = prefs
        .apply(
            &PreferencePatch::new().set("updates.check", on),
            &ctx.fs,
            &zaun_core::config::LoadOptions { env_rule: None },
        )
        .map_err(Error::from)?;
    ctx.fs.create_dir_all(&ctx.paths.config_dir, 0o700)?;
    zaun_core::config::save(&ctx.fs, &ctx.paths.config_file, &next).map_err(Error::from)?;
    println!(
        "{}",
        if on {
            "Zaun checks github.com for a newer version once a day."
        } else {
            "Zaun does not check for updates."
        }
    );
    Ok(exit::OK)
}
