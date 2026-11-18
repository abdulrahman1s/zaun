//! The microphone test ("Let's check", ADR-0035): the microphone thread reads 20 ms frames
//! (volume, noise suppression), reports their level about 20 times a second, and hands them
//! to the speaker thread, which plays them half a second late so the user hears themselves.
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::Duration;

use easyplay_core::chat::media::{AudioSettings, MicTest, MicTestEvent, MicTestSink};
use easyplay_core::error::ErrorCode;
use nnnoiseless::DenoiseState;

use super::backend::{AudioBackend, FRAME};
use super::capture::{denoise, scale};
use super::level::level;

/// The playback delay, in frames (25 × 20 ms).
const DELAY: usize = 25;
/// How often the level is reported.
const LEVEL_EVERY: Duration = Duration::from_millis(50);

/// A running test; dropping it stops both threads.
pub(crate) struct Running(Arc<AtomicBool>);

impl MicTest for Running {}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn start(
    backend: &Arc<dyn AudioBackend>,
    audio: AudioSettings,
    sink: MicTestSink,
) -> std::io::Result<Running> {
    let stop = Arc::new(AtomicBool::new(false));
    // Frames waiting for the speakers; the delay itself is kept on the speaker thread.
    let (tx, rx) = sync_channel::<Vec<i16>>(DELAY * 2);
    {
        let (backend, sink, stop) = (Arc::clone(backend), Arc::clone(&sink), Arc::clone(&stop));
        let device = audio.speakers.clone();
        std::thread::Builder::new()
            .name("mic-test-speaker".into())
            .spawn(move || speaker(&*backend, &device, &rx, &sink, &stop))?;
    }
    let (backend, stop2) = (Arc::clone(backend), Arc::clone(&stop));
    std::thread::Builder::new()
        .name("mic-test".into())
        .spawn(move || mic(&*backend, &audio, &tx, &sink, &stop2))?;
    Ok(Running(stop))
}

fn failed(sink: &MicTestSink, code: ErrorCode, detail: String) {
    sink(MicTestEvent::Failed { code, detail });
}

fn mic(
    backend: &dyn AudioBackend,
    audio: &AudioSettings,
    tx: &SyncSender<Vec<i16>>,
    sink: &MicTestSink,
    stop: &AtomicBool,
) {
    let mut input = match backend.open_input(&audio.mic) {
        Ok(i) => i,
        Err(e) => return failed(sink, ErrorCode::ChtNoMicrophone, e.to_string()),
    };
    let mut ns = audio.noise_suppression.then(DenoiseState::new);
    let mut frame = [0i16; FRAME];
    let mut reported = crate::clock::now();
    while !stop.load(Ordering::Relaxed) {
        if let Err(e) = input.read(&mut frame) {
            let detail = format!("the microphone stopped: {e}");
            return failed(sink, ErrorCode::ChtNoMicrophone, detail);
        }
        scale(&mut frame, audio.mic_volume);
        if let Some(ns) = &mut ns {
            denoise(ns, &mut frame);
        }
        let now = crate::clock::now();
        if now.saturating_duration_since(reported) >= LEVEL_EVERY {
            reported = now;
            sink(MicTestEvent::Level(level(&frame)));
        }
        // Full only when the speakers are stuck; the level still matters then.
        let _ = tx.try_send(frame.to_vec());
    }
}

fn speaker(
    backend: &dyn AudioBackend,
    device: &str,
    rx: &Receiver<Vec<i16>>,
    sink: &MicTestSink,
    stop: &AtomicBool,
) {
    let mut out = match backend.open_output(device) {
        Ok(o) => o,
        Err(e) => {
            let detail = format!("the speakers could not be opened: {e}");
            return failed(sink, ErrorCode::ChtMediaFailed, detail);
        }
    };
    let mut delay: VecDeque<Vec<i16>> = VecDeque::with_capacity(DELAY + 1);
    // Ends when the microphone thread ends (its sender goes).
    for frame in rx {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        delay.push_back(frame);
        if delay.len() <= DELAY {
            continue;
        }
        let Some(f) = delay.pop_front() else { continue };
        if let Err(e) = out.write(&f) {
            let detail = format!("the speakers stopped: {e}");
            return failed(sink, ErrorCode::ChtMediaFailed, detail);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::tone::{ToneAudio, tone_strength};
    use std::sync::Mutex;

    #[test]
    fn the_test_reports_levels_and_plays_the_microphone_back_half_a_second_late() {
        let audio = ToneAudio::new(440.0);
        let backend: Arc<dyn AudioBackend> = Arc::new(audio.clone());
        let levels = Arc::new(Mutex::new(Vec::new()));
        let l2 = Arc::clone(&levels);
        let sink: MicTestSink = Arc::new(move |e| l2.lock().unwrap().push(e));
        let running = start(&backend, AudioSettings::default(), sink).unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        drop(running);
        let levels = levels.lock().unwrap().clone();
        let n = levels.len();
        assert!(
            (20..=40).contains(&n),
            "about 20 levels a second: {n} in 1.5 s"
        );
        assert!(
            levels
                .iter()
                .all(|e| matches!(e, MicTestEvent::Level(l) if *l > 50))
        );
        let heard = audio.heard.samples();
        // 1.5 s recorded, the first half second held back: about a second played.
        assert!(
            (30_000..=60_000).contains(&heard.len()),
            "{} samples played",
            heard.len()
        );
        assert!(tone_strength(&heard, 440.0) > 0.9);
        // Stopped: nothing more is played.
        std::thread::sleep(Duration::from_millis(200));
        let after = audio.heard.samples().len();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(audio.heard.samples().len(), after);
    }
}
