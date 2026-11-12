//! Desktop integration against temporary XDG directories (DESKTOP_INTEGRATION §11): the
//! handler set/restore round trip with a fake `xdg-mime`, rendered shortcuts through
//! `desktop-file-validate` (when installed), and the instance socket with real processes'
//! worth of connections.

#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use easyplay_core::clock::SystemClock;
use easyplay_core::desktop::desktop_entry::{self, GameEntry};
use easyplay_core::desktop::instance::{
    self, Claim, InstanceTransport, Replier, Request, RequestHandler, Response, WireOrigin,
};
use easyplay_core::desktop::mime::{
    DESKTOP_ID, MimeRegistrar, MimeType, Overall, XdgDirs, XdgMimeRegistrar, overall,
};
use easyplay_core::fsx::RealFs;
use easyplay_core::host::RealRunner;
use easyplay_core::ids::GameId;
use easyplay_core::paths::{AppPaths, PathEnv};

/// `xdg-mime` subset over `$XDG_CONFIG_HOME/mimeapps.list` (`[Default Applications]` is the
/// file's last group); `default` edits in place like the real one.
const FAKE_XDG_MIME: &str = r#"#!/bin/sh
f="$XDG_CONFIG_HOME/mimeapps.list"
if [ "$1" = query ] && [ "$2" = default ]; then
  [ -f "$f" ] && awk -v t="$3" 'BEGIN{FS="="} /^\[/{g=($0=="[Default Applications]")} g && $1==t {sub(/^[^=]*=/,""); sub(/;.*/,""); print; exit}' "$f"
  exit 0
fi
if [ "$1" = query ] && [ "$2" = filetype ]; then
  case "$3" in *.exe) echo application/x-ms-dos-executable;; *) echo application/x-ole-storage;; esac
  exit 0
fi
if [ "$1" = default ]; then
  id="$2"; shift 2
  [ -f "$f" ] || printf '[Default Applications]\n' > "$f"
  grep -q '^\[Default Applications\]' "$f" || printf '[Default Applications]\n' >> "$f"
  for t in "$@"; do
    if awk -v t="$t" 'BEGIN{FS="=";r=1} /^\[/{g=($0=="[Default Applications]")} g && $1==t {r=0} END{exit r}' "$f"; then
      awk -v t="$t" -v id="$id" 'BEGIN{FS="=";OFS="="} /^\[/{g=($0=="[Default Applications]")} g && $1==t {print t, id; next} {print}' "$f" > "$f.new" && mv "$f.new" "$f"
    else
      printf '%s=%s\n' "$t" "$id" >> "$f"
    fi
  done
  exit 0
fi
exit 2
"#;

fn write_exe(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Xdg {
    _tmp: tempfile::TempDir,
    env: PathEnv,
    dirs: XdgDirs,
}

fn xdg() -> Xdg {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    write_exe(&bin.join("xdg-mime"), FAKE_XDG_MIME);
    let config = root.join("config");
    let data = root.join("data");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(data.join("applications")).unwrap();
    std::fs::write(
        data.join("applications/wine.desktop"),
        "[Desktop Entry]\nType=Application\nName=Wine Windows Program Loader\nExec=sh %f\n",
    )
    .unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let env = PathEnv::new(1000)
        .with("HOME", &root)
        .with("PATH", path)
        .with("XDG_CONFIG_HOME", &config)
        .with("XDG_DATA_HOME", &data)
        .with("XDG_DATA_DIRS", root.join("nowhere"));
    let dirs = XdgDirs::from_env(&env).unwrap();
    Xdg {
        _tmp: tmp,
        env,
        dirs,
    }
}

fn registrar(x: &Xdg, scratch: Option<PathBuf>) -> XdgMimeRegistrar {
    XdgMimeRegistrar::new(
        Arc::new(RealRunner::new(Arc::new(SystemClock::new()))),
        Arc::new(RealFs::new()),
        &x.env,
        x.dirs.clone(),
        scratch,
    )
}

#[test]
fn handler_set_then_restore_returns_mimeapps_list_to_its_original_bytes() {
    let x = xdg();
    let list = x.dirs.mimeapps_list();
    let original = "# user file\n[Added Associations]\napplication/x-msdownload=wine.desktop;\n\n\
                    [Default Applications]\ntext/plain=org.gnome.TextEditor.desktop\n\
                    application/x-msdownload=wine.desktop\n";
    std::fs::write(&list, original).unwrap();
    let scratch = x.dirs.config_home.parent().unwrap().join("probe");
    let r = registrar(&x, Some(scratch.clone()));
    assert!(r.available());
    let types = r.managed_types();
    // Detected (fake) type names are added to the entry's list.
    assert!(types.contains(&MimeType::parse("application/x-ole-storage").unwrap()));
    assert!(types.contains(&MimeType::parse("application/x-msi").unwrap()));
    assert!(!scratch.exists(), "sample files are cleaned up");

    let before = r.status().unwrap();
    assert_eq!(
        overall(&before),
        Overall::Other("Wine Windows Program Loader".into())
    );
    let prev = r.set_default(&types).unwrap();
    let after = r.status().unwrap();
    assert!(
        after
            .iter()
            .all(|s| s.default.as_deref() == Some(DESKTOP_ID))
    );
    assert_eq!(overall(&after), Overall::EasyPlay);
    assert_eq!(
        prev.entries
            .iter()
            .filter(|(_, p)| p.as_deref() == Some("wine.desktop"))
            .count(),
        1
    );
    // The record survives the JSON round trip through kv.
    let prev: easyplay_core::desktop::mime::PreviousHandlers =
        serde_json::from_str(&serde_json::to_string(&prev).unwrap()).unwrap();
    r.restore(&prev).unwrap();
    assert_eq!(std::fs::read_to_string(&list).unwrap(), original);
}

#[test]
fn handler_restore_without_a_previous_file_removes_it() {
    let x = xdg();
    let list = x.dirs.mimeapps_list();
    let r = registrar(&x, None);
    let types = r.managed_types();
    assert_eq!(overall(&r.status().unwrap()), Overall::Nothing);
    let prev = r.set_default(&types).unwrap();
    assert!(list.exists());
    r.restore(&prev).unwrap();
    assert!(!list.exists());
}

#[test]
fn missing_xdg_mime_disables_the_feature() {
    let x = xdg();
    let env = x.env.clone().with("PATH", "/nonexistent");
    let r = XdgMimeRegistrar::new(
        Arc::new(RealRunner::new(Arc::new(SystemClock::new()))),
        Arc::new(RealFs::new()),
        &env,
        x.dirs.clone(),
        None,
    );
    assert!(!r.available());
    let e = r.status().unwrap_err();
    assert_eq!(e.code.as_str(), "EP-DSK-001");
    assert!(e.context_value("manual").unwrap().contains("Open with"));
}

/// The same round trip with the host's real `xdg-mime` (only with `EASYPLAY_E2E`; the XDG
/// directories are temporary, the user's own files are never touched).
#[test]
fn real_xdg_mime_round_trip() {
    if std::env::var_os("EASYPLAY_E2E").is_none() || find_tool("xdg-mime").is_none() {
        return;
    }
    let x = xdg();
    std::fs::remove_file(x.dirs.config_home.parent().unwrap().join("bin/xdg-mime")).unwrap();
    let env = x
        .env
        .clone()
        .with("XDG_CURRENT_DESKTOP", "X-Generic")
        .with("XDG_DATA_DIRS", x.dirs.data_home.as_os_str());
    std::fs::write(
        x.dirs.data_home.join("applications").join(DESKTOP_ID),
        include_str!("../../../assets/share/applications/io.github.easyplay.EasyPlay.desktop"),
    )
    .unwrap();
    let list = x.dirs.mimeapps_list();
    let original = "[Default Applications]\napplication/x-msdownload=wine.desktop\n";
    std::fs::write(&list, original).unwrap();
    let r = XdgMimeRegistrar::new(
        Arc::new(RealRunner::new(Arc::new(SystemClock::new()))),
        Arc::new(RealFs::new()),
        &env,
        XdgDirs::from_env(&env).unwrap(),
        None,
    );
    let types = r.managed_types();
    let prev = r.set_default(&types).unwrap();
    assert_eq!(overall(&r.status().unwrap()), Overall::EasyPlay);
    eprintln!("after set:\n{}", std::fs::read_to_string(&list).unwrap());
    r.restore(&prev).unwrap();
    let after = std::fs::read_to_string(&list).unwrap();
    eprintln!("after restore:\n{after}");
    // xdg-mime's generic fallback still names EasyPlay for types it merely offers, so the
    // file itself is the check.
    assert_eq!(after, original);
}

fn find_tool(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

#[test]
fn rendered_shortcuts_pass_desktop_file_validate() {
    let Some(tool) = find_tool("desktop-file-validate") else {
        eprintln!("desktop-file-validate not installed; skipped");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let id: GameId = "0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5e".parse().unwrap();
    for (i, name) in [
        "Hollow Knight",
        "Evil\nExec=rm -rf ~",
        "100% %f %U",
        "a;b=c[x]",
        "-flag",
        " lead",
        "back\\slash",
        "\u{202E}exe.txt",
        &"long ".repeat(400),
    ]
    .iter()
    .enumerate()
    {
        let p = tmp.path().join(format!("t{i}.desktop"));
        std::fs::write(
            &p,
            desktop_entry::render(&GameEntry {
                id,
                name,
                icon: None,
            }),
        )
        .unwrap();
        let out = std::process::Command::new(&tool).arg(&p).output().unwrap();
        assert!(
            out.status.success() && out.stdout.is_empty(),
            "{name:?}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}

struct Echo(Mutex<Vec<Request>>);

impl RequestHandler for Echo {
    fn handle(&self, request: Request, reply: &mut dyn Replier) {
        self.0.lock().unwrap().push(request);
        reply.reply(&Response::ok());
    }
}

fn paths_in(dir: &Path) -> AppPaths {
    let env = PathEnv::new(rustix::process::getuid().as_raw())
        .with("HOME", dir)
        .with("XDG_RUNTIME_DIR", dir.join("run"))
        .with("EASYPLAY_DATA_DIR", dir.join("data"))
        .with("EASYPLAY_CONFIG_DIR", dir.join("config"))
        .with("EASYPLAY_CACHE_DIR", dir.join("cache"))
        .with("EASYPLAY_STATE_DIR", dir.join("state"));
    std::fs::create_dir_all(dir.join("run")).unwrap();
    AppPaths::resolve(&env).unwrap()
}

#[test]
fn instance_socket_serves_one_primary_and_rejects_bad_clients() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths_in(tmp.path());
    let uid = rustix::process::getuid().as_raw();
    // A stale socket file from a crashed GUI does not block the start.
    std::fs::create_dir_all(&paths.runtime_dir).unwrap();
    std::fs::write(&paths.instance_socket, b"stale").unwrap();
    let echo = Arc::new(Echo(Mutex::new(Vec::new())));
    let e2 = Arc::clone(&echo);
    let Claim::Primary(server) = instance::claim(&paths, uid, move || e2).unwrap() else {
        panic!("first claim must be primary");
    };
    let mode = std::fs::metadata(&paths.instance_socket)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    // A second claim forwards.
    let Claim::Forward(client) = instance::claim(&paths, uid, || -> Arc<dyn RequestHandler> {
        unreachable!()
    })
    .unwrap() else {
        panic!("second claim must forward");
    };
    assert!(client.ping());
    let r = client
        .send(
            &Request::Open {
                paths: vec!["game.exe".into()],
                origin: WireOrigin::FileManager,
                cwd: Some("/mnt/games".into()),
                activation_token: Some("token".into()),
            },
            &mut |_| {},
        )
        .unwrap();
    assert!(r.ok);
    assert!(matches!(
        &echo.0.lock().unwrap()[0],
        Request::Open { paths, .. } if paths == &[PathBuf::from("/mnt/games/game.exe")]
    ));

    let raw = |bytes: &[u8]| -> String {
        let mut s = UnixStream::connect(&paths.instance_socket).unwrap();
        let _ = s.write_all(bytes);
        let _ = s.shutdown(std::net::Shutdown::Write);
        let mut line = String::new();
        let _ = BufReader::new(&s).read_line(&mut line);
        line
    };
    // Wrong version, unknown op, malformed, oversized, relative path without cwd.
    assert!(raw(b"{\"v\":9,\"op\":\"ping\"}\n").contains("\"ok\":false"));
    assert!(raw(b"{\"v\":1,\"op\":\"rm\"}\n").contains("\"ok\":false"));
    assert!(raw(b"garbage\n").contains("\"ok\":false"));
    assert!(raw(&vec![b'x'; 70 * 1024]).contains("\"ok\":false"));
    assert!(
        raw(b"{\"v\":1,\"op\":\"open\",\"paths\":[\"a.exe\"],\"origin\":\"cli\"}\n")
            .contains("\"ok\":false")
    );
    // A slow client that never finishes its line is dropped after the read timeout and the
    // server keeps serving.
    let slow = UnixStream::connect(&paths.instance_socket).unwrap();
    (&slow).write_all(b"{\"v\":1").unwrap();
    assert!(client.ping());
    drop(slow);
    assert_eq!(echo.0.lock().unwrap().len(), 1);

    drop(server);
    assert!(!paths.instance_socket.exists());
    assert!(!client.ping());
}
