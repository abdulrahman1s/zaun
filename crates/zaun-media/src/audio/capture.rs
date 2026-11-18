//! The microphone thread: 20 ms frames, noise suppression, the level, Opus. Muted, it keeps
//! reading (so the device stays open) and sends only the level, zero.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use audiopus::coder::Encoder;
use audiopus::{Application, Bitrate, Channels, SampleRate};
use nnnoiseless::DenoiseState;
use zaun_core::error::ErrorCode;

use super::backend::{AudioBackend, FRAME};
use super::level::level;
use crate::session::Inbox;

/// Opus at 32 kbit/s: clear speech with room to spare.
const BITRATE: i32 = 32_000;

pub(crate) struct Mic {
    pub backend: Arc<dyn AudioBackend>,
    pub device: String,
    pub denoise: bool,
    /// `chat.mic_volume`: the gain in percent, applied before noise suppression.
    pub volume: u8,
    pub muted: Arc<AtomicBool>,
    pub stop: Arc<AtomicBool>,
}

pub(crate) fn spawn(mic: Mic, tx: Sender<Inbox>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("call-mic".into())
        .spawn(move || run(&mic, &tx))?;
    Ok(())
}

fn failed(tx: &Sender<Inbox>, code: ErrorCode, detail: String) {
    let _ = tx.send(Inbox::Failed { code, detail });
}

fn run(mic: &Mic, tx: &Sender<Inbox>) {
    let mut input = match mic.backend.open_input(&mic.device) {
        Ok(i) => i,
        Err(e) => return failed(tx, ErrorCode::ChtNoMicrophone, e.to_string()),
    };
    let mut encoder = match Encoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip) {
        Ok(e) => e,
        Err(e) => return failed(tx, ErrorCode::ChtMediaFailed, e.to_string()),
    };
    if let Err(e) = encoder.set_bitrate(Bitrate::BitsPerSecond(BITRATE)) {
        tracing::debug!(error = %e, "call: the Opus bitrate was not set");
    }
    let mut ns = mic.denoise.then(DenoiseState::new);
    let mut frame = [0i16; FRAME];
    let mut packet = [0u8; 1500];
    while !mic.stop.load(Ordering::Relaxed) {
        if let Err(e) = input.read(&mut frame) {
            return failed(
                tx,
                ErrorCode::ChtNoMicrophone,
                format!("the microphone stopped: {e}"),
            );
        }
        scale(&mut frame, mic.volume);
        if let Some(ns) = &mut ns {
            denoise(ns, &mut frame);
        }
        let msg = if mic.muted.load(Ordering::Relaxed) {
            Inbox::Frame {
                packet: None,
                level: 0,
            }
        } else {
            let n = match encoder.encode(&frame, &mut packet) {
                Ok(n) => n,
                Err(e) => return failed(tx, ErrorCode::ChtMediaFailed, e.to_string()),
            };
            Inbox::Frame {
                packet: Some(packet[..n].to_vec()),
                level: level(&frame),
            }
        };
        if tx.send(msg).is_err() {
            return;
        }
    }
}

/// The microphone volume: `percent` of each sample (100 leaves the frame as it is).
pub(crate) fn scale(frame: &mut [i16; FRAME], percent: u8) {
    if percent >= 100 {
        return;
    }
    for s in frame.iter_mut() {
        // At most the sample itself, so it stays in range.
        *s = (i32::from(*s) * i32::from(percent) / 100) as i16;
    }
}

/// Noise suppression on one frame, in the denoiser's 10 ms halves (samples kept at the
/// scale of 16-bit integers, as it expects).
pub(crate) fn denoise(ns: &mut DenoiseState<'_>, frame: &mut [i16; FRAME]) {
    let mut inp = [0f32; DenoiseState::FRAME_SIZE];
    let mut out = [0f32; DenoiseState::FRAME_SIZE];
    for half in frame.chunks_mut(DenoiseState::FRAME_SIZE) {
        for (i, s) in inp.iter_mut().zip(half.iter()) {
            *i = f32::from(*s);
        }
        ns.process_frame(&mut out, &inp);
        for (s, o) in half.iter_mut().zip(out) {
            // Clamped into range before the cast.
            *s = o.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn energy(s: &[i16]) -> f64 {
        s.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / s.len() as f64
    }

    #[test]
    fn the_microphone_volume_scales_samples_and_full_volume_leaves_them() {
        let mut f = [0i16; FRAME];
        f[0] = i16::MAX;
        f[1] = i16::MIN;
        f[2] = 1000;
        let whole = f;
        scale(&mut f, 100);
        assert_eq!(f, whole);
        scale(&mut f, 50);
        assert_eq!(&f[..3], &[i16::MAX / 2, i16::MIN / 2, 500]);
        scale(&mut f, 0);
        assert!(f.iter().all(|s| *s == 0));
    }

    #[test]
    fn noise_suppression_never_makes_noise_louder_and_keeps_frames_whole() {
        // RNNoise is trained on real noise; on white noise it takes off a dB or two.
        let mut ns = DenoiseState::new();
        let mut seed: u32 = 1;
        let (mut before, mut after) = (Vec::new(), Vec::new());
        for _ in 0..150 {
            let mut f = [0i16; FRAME];
            for s in &mut f {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *s = ((seed >> 16) as u16 as i16) / 32;
            }
            before.extend_from_slice(&f);
            denoise(&mut ns, &mut f);
            after.extend_from_slice(&f);
        }
        let (b, a) = (energy(&before[48_000..]), energy(&after[48_000..]));
        assert!(a < b && a > 0.0, "noise {b:.0} → {a:.0}");
        assert_eq!(after.len(), before.len());
    }
}
