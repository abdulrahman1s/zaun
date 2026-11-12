//! PulseAudio and PipeWire sockets and ALSA devices (BUBBLEWRAP_SANDBOX §4.7; D-13).

use std::path::{Path, PathBuf};

use super::display::xdg_runtime_dir;
use super::facts::AudioFacts;
use crate::fsx::{FileType, Fs};
use crate::paths::PathEnv;

fn is_socket(fs: &dyn Fs, path: &Path) -> bool {
    fs.metadata_follow(path)
        .is_ok_and(|m| m.file_type == FileType::Socket)
}

/// Probe audio facts; remarks are appended to `notes`.
pub fn probe_audio(fs: &dyn Fs, env: &PathEnv, notes: &mut Vec<String>) -> AudioFacts {
    let xrd = xdg_runtime_dir(env);
    let pulse_from_env = env
        .get("PULSE_SERVER")
        .and_then(|v| v.to_str())
        .and_then(|v| v.strip_prefix("unix:"))
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    let pulse_socket = pulse_from_env
        .or_else(|| xrd.as_ref().map(|d| d.join("pulse/native")))
        .filter(|p| is_socket(fs, p));
    let pipewire_socket = xrd
        .as_ref()
        .map(|d| d.join("pipewire-0"))
        .filter(|p| is_socket(fs, p));
    if pulse_socket.is_none() {
        notes.push("PulseAudio socket not found".into());
    }
    if pipewire_socket.is_none() {
        notes.push("PipeWire socket not found; sound through PulseAudio only".into());
    }
    AudioFacts {
        pulse_socket,
        pipewire_socket,
        alsa_devices: fs.exists(Path::new("/dev/snd")),
    }
}
