//! The session's own sound server (ADR-0026): PipeWire's PulseAudio server on one socket,
//! playing through the host's PipeWire and refusing to record.
//!
//! The server is `pipewire -c <dir>/`[`SOUND_CONFIG`], started by the supervisor before the
//! sandbox ([`HostAction::StartSoundServer`](super::HostAction::StartSoundServer)). Its rule
//! gives every client the `block-record-stream` quirk, which refuses record streams from
//! microphones and from monitors of what is playing alike. A client sets its own properties,
//! so the rule cannot match on one of them being something: it matches a client whose
//! `pulse.server.type` is missing or anything but `secret`, a random value that only the
//! configuration file holds and the sandbox never sees. Clients cannot load modules (a
//! module could write a source to a file the game can read).

use std::path::Path;

/// File name of the configuration in the server's directory. Not `pipewire-pulse.conf`:
/// PipeWire would add the user's `pipewire-pulse.conf.d` fragments to it.
pub const SOUND_CONFIG: &str = "zaun-sound.conf";

/// Bytes of randomness in the secret.
pub const SECRET_BYTES: usize = 16;

/// `s` as the body of a configuration string.
fn quoted(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The configuration of a server listening on `socket` whose rule spares only a client
/// that names `secret` (hex digits).
pub fn config(socket: &Path, secret: &str) -> String {
    let socket = quoted(&socket.display().to_string());
    let secret = quoted(secret);
    format!(
        r#"# Zaun: a session's sound server that plays and refuses to record (ADR-0026).
context.properties = {{ }}
context.spa-libs = {{
    audio.convert.* = audioconvert/libspa-audioconvert
    support.*       = support/libspa-support
}}
context.modules = [
    {{ name = libpipewire-module-rt args = {{ nice.level = -11 }} flags = [ ifexists nofail ] }}
    {{ name = libpipewire-module-protocol-native }}
    {{ name = libpipewire-module-client-node }}
    {{ name = libpipewire-module-adapter }}
    {{ name = libpipewire-module-metadata }}
    {{ name = libpipewire-module-protocol-pulse }}
]
pulse.properties = {{
    server.address = [ "unix:{socket}" ]
    server.dbus-name = ""
    pulse.allow-module-loading = false
}}
pulse.rules = [
    {{
        matches = [
            {{ pulse.server.type = null }}
            {{ pulse.server.type = "!{secret}" }}
        ]
        actions = {{ quirks = [ block-record-stream ] }}
    }}
]
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_client_but_the_secret_is_refused_recording() {
        let c = config(
            Path::new("/run/user/1000/zaun/sessions/s/sound/native"),
            "ab12",
        );
        assert!(c.contains(
            r#"server.address = [ "unix:/run/user/1000/zaun/sessions/s/sound/native" ]"#
        ));
        assert!(c.contains("pulse.allow-module-loading = false"));
        assert!(c.contains("{ pulse.server.type = null }"));
        assert!(c.contains(r#"{ pulse.server.type = "!ab12" }"#));
        assert!(c.contains("quirks = [ block-record-stream ]"));
        // Only one listening address, and it is not on the network.
        assert_eq!(c.matches("unix:").count(), 1);
        assert!(!c.contains("tcp:"));
    }

    #[test]
    fn strings_are_escaped() {
        let c = config(Path::new(r#"/tmp/a"b\c"#), "x");
        assert!(c.contains(r#""unix:/tmp/a\"b\\c""#));
    }
}
