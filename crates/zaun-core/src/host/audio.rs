//! PulseAudio and PipeWire sockets, the `pipewire` program and ALSA devices
//! (BUBBLEWRAP_SANDBOX §4.7; D-13; ADR-0026).

use std::path::{Path, PathBuf};

use super::display::xdg_runtime_dir;
use super::exec::{find_in, path_dirs};
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
    let pipewire_program = find_in(fs, &path_dirs(env.get("PATH")), "pipewire");
    if pipewire_program.is_none() {
        notes.push("pipewire program not found; sound always includes the microphone".into());
    }
    let snd = Path::new("/dev/snd");
    let snd_nodes = fs
        .read_dir(snd)
        .map(|entries| {
            entries
                .into_iter()
                .filter(|e| e.file_type == FileType::CharDevice)
                .map(|e| e.path)
                .collect()
        })
        .unwrap_or_default();
    AudioFacts {
        pulse_socket,
        pipewire_socket,
        alsa_devices: fs.exists(snd),
        pipewire_program,
        snd_nodes,
    }
}
