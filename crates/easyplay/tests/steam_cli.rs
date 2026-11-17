//! Steam games on the command line (ADR-0031): `steam scan|on|off|update|open`, and Steam
//! games in `list`, `show` and `launch`. The fake Steam root lives in the test's own HOME;
//! `steam` on PATH is a stub that records its arguments, so Steam is never started.

#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use easyplay_testkit::TestEnv;
use easyplay_testkit::fixtures::steam_root;

const BIN: &str = env!("CARGO_BIN_EXE_easyplay");

struct World {
    env: TestEnv,
    bin: PathBuf,
    out: PathBuf,
}

impl World {
    fn new() -> Self {
        let env = TestEnv::new().unwrap();
        steam_root::steam_root_on_disk(&env.home());
        let bin = env.root().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let out = env.root().join("steam-args");
        let stub = bin.join("steam");
        std::fs::write(
            &stub,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$STEAM_STUB_OUT\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        Self { env, bin, out }
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut path = std::ffi::OsString::from(self.bin.as_os_str());
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        Command::new(BIN)
            .args(args)
            .env_clear()
            .envs(self.env.envs())
            .env("PATH", path)
            .env("STEAM_STUB_OUT", &self.out)
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(o.status.success(), "{args:?}: {o:?}");
        String::from_utf8(o.stdout).unwrap()
    }

    /// What the stub received (it runs detached: wait for it).
    fn handed(&self, lines: usize) -> Vec<String> {
        for _ in 0..200 {
            if let Ok(t) = std::fs::read_to_string(&self.out) {
                let v: Vec<String> = t.lines().map(str::to_owned).collect();
                if v.len() >= lines {
                    return v;
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("the steam stub was not run");
    }
}

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn scan_reads_nothing_while_off_then_lists_games_and_what_was_left_out() {
    let w = World::new();
    // Off: Steam's files are not read, and nothing is changed.
    let text = w.ok(&["steam", "scan"]);
    assert!(text.contains("easyplay steam on"), "{text}");
    assert!(!text.contains("steam:"), "{text}");
    let v = json(&w.ok(&["steam", "scan", "--json"]));
    assert_eq!(v["enabled"], false);
    assert_eq!(v["games"].as_array().unwrap().len(), 0);
    assert!(
        !w.env.config().join("config.toml").exists(),
        "scan changes nothing"
    );

    w.ok(&["steam", "on"]);
    let text = w.ok(&["steam", "scan"]);
    assert!(text.contains("3 games, 3 other apps left out"), "{text}");
    assert!(text.contains("steam:1174180"), "{text}");
    let v = json(&w.ok(&["steam", "scan", "--json"]));
    assert_eq!(v["enabled"], true);
    assert_eq!(v["games"].as_array().unwrap().len(), 3);
    let reasons: Vec<&str> = v["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["reason"].as_str().unwrap())
        .collect();
    assert_eq!(
        reasons,
        ["redistributable", "compatibility_tool", "runtime"]
    );
}

#[test]
fn the_option_adds_steam_games_to_list_and_show() {
    let w = World::new();
    // Off by default: nothing of Steam's is listed.
    assert!(w.ok(&["list"]).contains("No games yet"));
    assert!(w.ok(&["steam", "on"]).contains("shown"));
    let config = std::fs::read_to_string(w.env.config().join("config.toml")).unwrap();
    assert!(
        config.contains("[steam]") && config.contains("show_games = true"),
        "{config}"
    );

    let text = w.ok(&["list"]);
    for id in ["steam:440", "steam:12210", "steam:1174180"] {
        assert!(text.contains(id), "{id} in {text}");
    }
    assert!(text.contains("Steam: GE-Proton10-25"), "{text}");
    let v = json(&w.ok(&["list", "--json"]));
    let games = v["games"].as_array().unwrap();
    assert_eq!(games.len(), 3);
    let rdr = games
        .iter()
        .find(|g| g["steam_app_id"] == 1_174_180)
        .unwrap();
    assert_eq!(rdr["origin"], "steam");
    assert_eq!(rdr["playtime_source"], "steam");
    assert_eq!(rdr["total_playtime_ms"], 4474 * 60_000);
    assert_eq!(rdr["update"], "waiting");

    let text = w.ok(&["show", "steam:1174180"]);
    assert!(
        text.contains("run by Steam, outside EasyPlay's protection"),
        "{text}"
    );
    assert!(text.contains("(from Steam)"), "{text}");
    let v = json(&w.ok(&["show", "Red Dead Redemption 2", "--json"]));
    assert_eq!(v["protection"], "run_by_steam");
    assert_eq!(v["proton"], "GE-Proton10-25");
    assert_eq!(v["running"], false);

    assert!(w.ok(&["steam", "off"]).contains("not shown"));
    assert!(w.ok(&["list"]).contains("No games yet"));
}

#[test]
fn launch_update_and_open_are_handed_to_steam() {
    let w = World::new();
    w.ok(&["steam", "on"]);
    let text = w.ok(&["launch", "steam:440"]);
    assert!(
        text.contains("Asked Steam to start Team Fortress 2"),
        "{text}"
    );
    assert_eq!(w.handed(1), ["steam://rungameid/440"]);
    let text = w.ok(&["steam", "update", "Red Dead Redemption 2"]);
    assert!(text.contains("Opened Steam's downloads"), "{text}");
    assert_eq!(w.handed(2)[1], "steam://open/downloads");
    w.ok(&["steam", "open", "steam:12210"]);
    assert_eq!(w.handed(3)[2], "steam://nav/games/details/12210");

    // `--wait` and extra arguments are EasyPlay's, not Steam's.
    let o = w.run(&["launch", "steam:440", "--wait"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8(o.stderr).unwrap().contains("EP-STM-007"));
    // An unknown Steam game.
    let o = w.run(&["steam", "update", "steam:999"]);
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn nothing_is_written_under_the_steam_root() {
    let w = World::new();
    let root = w.env.home().join(".local/share/Steam");
    let before = tree(&root);
    w.ok(&["steam", "on"]);
    w.ok(&["list"]);
    w.ok(&["show", "steam:1174180", "--json"]);
    w.ok(&["launch", "steam:440"]);
    w.handed(1);
    assert_eq!(tree(&root), before);
}

/// Every path under `dir` with its size and modification time.
fn tree(dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let md = std::fs::symlink_metadata(e.path()).unwrap();
            if md.is_dir() {
                stack.push(e.path());
            }
            out.push((e.path(), md.len(), md.modified().unwrap()));
        }
    }
    out.sort();
    out
}
