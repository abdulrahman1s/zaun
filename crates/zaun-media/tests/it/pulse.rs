//! The PulseAudio backend against a real sound server (ADR-0035): a microphone and speakers
//! open, frames are read and written.
//!
//! Ignored: it needs a PulseAudio server; a local recipe kept by the maintainer starts a
//! private one (a null sink, its own socket, `PULSE_SERVER`), so the user's sound devices are
//! never opened.

use zaun_media::audio::backend::FRAME;
use zaun_media::{AudioBackend, PulseAudio};

#[test]
#[ignore = "needs a PulseAudio server (see the file's header)"]
fn the_pulseaudio_backend_opens_a_microphone_and_speakers_and_moves_frames() {
    let mut out = PulseAudio.open_output("").expect("speakers");
    let mut input = PulseAudio.open_input("").expect("microphone");
    let tone: Vec<i16> = (0..FRAME)
        .map(|i| ((i as f64 * 0.06).sin() * 8000.0) as i16)
        .collect();
    for _ in 0..10 {
        out.write(&tone).unwrap();
    }
    let mut frame = [0i16; FRAME];
    for _ in 0..10 {
        input.read(&mut frame).unwrap();
    }
    assert!(
        PulseAudio.open_input("no-such-source").is_err(),
        "a missing device fails"
    );
}

#[test]
#[ignore = "needs a PulseAudio server (see the file's header)"]
fn the_server_lists_its_microphones_without_monitors_and_its_speakers() {
    let d = PulseAudio.devices().expect("a device list");
    let inputs: Vec<&str> = d.inputs.iter().map(|x| x.name.as_str()).collect();
    let outputs: Vec<&str> = d.outputs.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(
        inputs,
        vec!["nullmic"],
        "the sink's monitor is no microphone"
    );
    assert_eq!(outputs, vec!["null"]);
    assert!(d.outputs.iter().all(|x| !x.description.is_empty()));
}

#[test]
#[ignore = "needs a PulseAudio server (see the file's header)"]
fn the_mic_test_runs_on_the_real_backend_and_stops_when_dropped() {
    use std::sync::{Arc, Mutex};
    use zaun_core::chat::media::{AudioSettings, MediaFactory, MicTestEvent, MicTestSink};
    let media = zaun_media::RtcMedia::new(Arc::new(PulseAudio));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = Arc::clone(&seen);
    let sink: MicTestSink = Arc::new(move |e| s2.lock().unwrap().push(e));
    let audio = AudioSettings {
        mic: "nullmic".into(),
        speakers: "null".into(),
        noise_suppression: true,
        mic_volume: 80,
    };
    let test = media.mic_test(audio, sink).expect("the test starts");
    std::thread::sleep(std::time::Duration::from_millis(1200));
    drop(test);
    let seen = seen.lock().unwrap().clone();
    assert!(
        !seen
            .iter()
            .any(|e| matches!(e, MicTestEvent::Failed { .. })),
        "{seen:?}"
    );
    let levels = seen
        .iter()
        .filter(|e| matches!(e, MicTestEvent::Level(_)))
        .count();
    assert!(levels >= 15, "{levels} levels in 1.2 s");
}
