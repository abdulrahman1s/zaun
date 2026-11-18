//! A test sound device: the microphone plays a sine tone in real time, the speakers record
//! what they are given. And a measure of how much of a recording is one tone.
use std::collections::VecDeque;
use std::f64::consts::TAU;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::backend::{AudioBackend, AudioIn, AudioOut, FRAME, RATE};

/// What the speakers keep: the last ten seconds.
const KEEP: usize = RATE as usize * 10;

/// What a [`ToneAudio`]'s speakers were given.
#[derive(Clone, Debug, Default)]
pub struct Recording(Arc<Mutex<VecDeque<i16>>>);

impl Recording {
    pub fn samples(&self) -> Vec<i16> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .copied()
            .collect()
    }
}

/// A microphone playing `freq` Hz, and speakers recording into [`ToneAudio::heard`].
#[derive(Clone, Debug)]
pub struct ToneAudio {
    pub freq: f64,
    pub heard: Recording,
}

impl ToneAudio {
    pub fn new(freq: f64) -> Self {
        Self {
            freq,
            heard: Recording::default(),
        }
    }
}

impl AudioBackend for ToneAudio {
    fn open_input(&self, _device: &str) -> io::Result<Box<dyn AudioIn>> {
        Ok(Box::new(Tone {
            freq: self.freq,
            n: 0,
            next: crate::clock::now(),
        }))
    }

    fn open_output(&self, _device: &str) -> io::Result<Box<dyn AudioOut>> {
        Ok(Box::new(self.heard.clone()))
    }
}

struct Tone {
    freq: f64,
    n: u64,
    next: std::time::Instant,
}

impl AudioIn for Tone {
    fn read(&mut self, frame: &mut [i16; FRAME]) -> io::Result<()> {
        self.next += Duration::from_millis(20);
        let wait = self.next.saturating_duration_since(crate::clock::now());
        std::thread::sleep(wait);
        for s in frame.iter_mut() {
            let t = self.n as f64 / f64::from(RATE);
            // A quarter of full scale: loud, never clipped.
            *s = (0.25 * f64::from(i16::MAX) * (TAU * self.freq * t).sin()) as i16;
            self.n += 1;
        }
        Ok(())
    }
}

impl AudioOut for Recording {
    fn write(&mut self, frame: &[i16]) -> io::Result<()> {
        let mut q = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        q.extend(frame);
        let extra = q.len().saturating_sub(KEEP);
        q.drain(..extra);
        Ok(())
    }
}

/// How much of `samples` is a tone of `freq` Hz: 1.0 for a pure tone, near 0 for silence or
/// other frequencies (Goertzel's amplitude over the RMS amplitude).
pub fn tone_strength(samples: &[i16], freq: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let n = samples.len() as f64;
    let w = TAU * freq / f64::from(RATE);
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for x in samples {
        let s0 = f64::from(*x) + 2.0 * w.cos() * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2;
    let amplitude = 2.0 * power.max(0.0).sqrt() / n;
    let rms = (samples.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / n).sqrt();
    if rms == 0.0 {
        return 0.0;
    }
    amplitude / (rms * std::f64::consts::SQRT_2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_measures_as_itself_and_not_as_another_frequency() {
        let mut t = ToneAudio::new(440.0).open_input("").unwrap();
        let mut all = Vec::new();
        let mut f = [0i16; FRAME];
        for _ in 0..5 {
            t.read(&mut f).unwrap();
            all.extend_from_slice(&f);
        }
        assert!(tone_strength(&all, 440.0) > 0.9);
        assert!(tone_strength(&all, 1000.0) < 0.05);
        assert_eq!(tone_strength(&[0; 960], 440.0), 0.0);
    }
}
