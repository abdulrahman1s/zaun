use std::path::{Path, PathBuf};

use crate::chat::native::command::{command, env};

fn args(bwrap: Option<&str>) -> (PathBuf, Vec<String>) {
    let bwrap = bwrap.map(PathBuf::from);
    let (program, args) = command(
        Path::new("/opt/zaun/libexec/zaun-chat"),
        bwrap.as_deref(),
        Path::new("/home/u/.local/share/zaun/chat"),
    );
    let args = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    (program, args)
}

#[test]
fn the_helper_sees_only_its_data_folder_writable_and_never_the_home_folder() {
    let (program, args) = args(Some("/usr/bin/bwrap"));
    assert_eq!(program, PathBuf::from("/usr/bin/bwrap"));
    let writable: Vec<&String> = args
        .windows(2)
        .filter(|w| w[0] == "--bind")
        .map(|w| &w[1])
        .collect();
    assert_eq!(writable, vec!["/home/u/.local/share/zaun/chat"]);
    assert!(!args.iter().any(|a| a == "/home" || a == "/home/u"));
    for flag in [
        "--unshare-all",
        "--share-net",
        "--die-with-parent",
        "--new-session",
    ] {
        assert!(args.iter().any(|a| a == flag), "{flag}");
    }
    assert_eq!(args.last().unwrap(), "/opt/zaun/libexec/zaun-chat");
}

#[test]
fn the_helper_gets_no_library_bind_and_no_library_environment() {
    let (_, args) = args(Some("/usr/bin/bwrap"));
    let ro: Vec<&String> = args
        .windows(3)
        .filter(|w| w[0] == "--ro-bind")
        .map(|w| &w[1])
        .collect();
    assert_eq!(ro, vec!["/opt/zaun/libexec/zaun-chat"]);
    for (k, _) in env(Path::new("/d")) {
        let k = k.to_string_lossy().into_owned();
        assert!(
            !k.starts_with("ZAUN_SIMPLEX") && k != "LD_LIBRARY_PATH",
            "{k}"
        );
    }
}

#[test]
fn without_bwrap_the_helper_runs_directly() {
    let (program, args) = args(None);
    assert_eq!(program, PathBuf::from("/opt/zaun/libexec/zaun-chat"));
    assert!(args.is_empty());
}
