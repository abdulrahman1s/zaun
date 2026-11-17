//! Display sockets and the session type (BUBBLEWRAP_SANDBOX §4.7; D-11). Missing pieces
//! become notes, never errors.

use std::path::{Path, PathBuf};

use super::facts::{DisplayFacts, SessionType, WaylandSocket, X11Display};
use crate::fsx::{FileType, Fs};
use crate::paths::PathEnv;

/// `XDG_SESSION_TYPE` → [`SessionType`].
pub fn parse_session_type(value: Option<&str>) -> SessionType {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("wayland") => SessionType::Wayland,
        Some("x11") => SessionType::X11,
        Some("tty") => SessionType::Tty,
        _ => SessionType::Unknown,
    }
}

/// The local display number of a `DISPLAY` value (`:0`, `:1.0`, `unix:0`); `None` for TCP
/// displays (`localhost:10.0`) and garbage.
pub fn parse_display(value: &str) -> Option<u32> {
    let (host, rest) = value.rsplit_once(':')?;
    if !(host.is_empty() || host == "unix") {
        return None;
    }
    let number = rest.split('.').next()?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

fn is_socket(fs: &dyn Fs, path: &Path) -> bool {
    fs.metadata_follow(path)
        .is_ok_and(|m| m.file_type == FileType::Socket)
}

fn env_str<'a>(env: &'a PathEnv, key: &str) -> Option<&'a str> {
    env.get(key)
        .and_then(|v| v.to_str())
        .filter(|v| !v.is_empty())
}

/// The host's runtime directory when it is an absolute path.
pub fn xdg_runtime_dir(env: &PathEnv) -> Option<PathBuf> {
    env_str(env, "XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// Probe display facts; remarks are appended to `notes`.
pub fn probe_display(fs: &dyn Fs, env: &PathEnv, notes: &mut Vec<String>) -> DisplayFacts {
    let xrd = xdg_runtime_dir(env);
    let mut facts = DisplayFacts {
        session_type: parse_session_type(env_str(env, "XDG_SESSION_TYPE")),
        desktop: env_str(env, "XDG_CURRENT_DESKTOP").map(str::to_owned),
        xdg_runtime_dir: xrd.clone(),
        wayland: None,
        x11: None,
    };

    if let Some(name) = env_str(env, "WAYLAND_DISPLAY") {
        let path = if Path::new(name).is_absolute() {
            Some(PathBuf::from(name))
        } else if name.contains('/') {
            None
        } else {
            xrd.as_ref().map(|d| d.join(name))
        };
        match path {
            Some(path) if is_socket(fs, &path) => {
                let inode = fs.metadata_follow(&path).map_or(0, |m| m.inode);
                // Whether the compositor offers security contexts is asked by `probe_all`.
                facts.wayland = Some(WaylandSocket {
                    name: name.to_owned(),
                    path,
                    inode,
                    security_context: false,
                });
            }
            _ => notes.push(format!("Wayland socket {name} not found")),
        }
    }

    if let Some(display) = env_str(env, "DISPLAY") {
        match parse_display(display) {
            Some(number) => {
                let socket = PathBuf::from(format!("/tmp/.X11-unix/X{number}"));
                let filesystem_socket = is_socket(fs, &socket);
                if !filesystem_socket {
                    notes.push(format!(
                        "X11 socket {} not found (abstract socket only: unavailable without network)",
                        socket.display()
                    ));
                }
                let authority = env_str(env, "XAUTHORITY")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .or_else(|| env_str(env, "HOME").map(|h| Path::new(h).join(".Xauthority")))
                    .filter(|p| fs.metadata_follow(p).is_ok_and(|m| m.is_file()));
                if authority.is_none() {
                    notes.push("X11 authority file not found".into());
                }
                facts.x11 = Some(X11Display {
                    number,
                    socket,
                    filesystem_socket,
                    authority,
                });
            }
            None => notes.push(format!("X11 display {display} is not a local display")),
        }
    }

    if !facts.any() {
        notes.push("No display socket found".into());
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_values() {
        assert_eq!(parse_display(":0"), Some(0));
        assert_eq!(parse_display(":1.0"), Some(1));
        assert_eq!(parse_display("unix:12"), Some(12));
        assert_eq!(parse_display("localhost:10.0"), None);
        assert_eq!(parse_display("0"), None);
        assert_eq!(parse_display(":x"), None);
    }

    #[test]
    fn session_types() {
        assert_eq!(parse_session_type(Some("wayland")), SessionType::Wayland);
        assert_eq!(parse_session_type(Some("X11")), SessionType::X11);
        assert_eq!(parse_session_type(None), SessionType::Unknown);
    }
}
