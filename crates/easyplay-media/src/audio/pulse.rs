//! PulseAudio's simple API, which PipeWire also serves (`pipewire-pulse`): blocking reads and
//! writes on our own threads, devices by source and sink name (ADR-0035 says why not cpal).
use std::io;

use libpulse_binding::def::BufferAttr;
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;

use super::backend::{AudioBackend, AudioIn, AudioOut, FRAME, RATE};

/// The bytes of one frame.
const FRAME_BYTES: u32 = (FRAME * 2) as u32;

/// The system's sound server.
#[derive(Clone, Copy, Debug, Default)]
pub struct PulseAudio;

fn spec() -> Spec {
    Spec {
        format: Format::S16NE,
        channels: 1,
        rate: RATE,
    }
}

fn open(dir: Direction, device: &str, attr: &BufferAttr) -> io::Result<Simple> {
    let dev = (!device.is_empty()).then_some(device);
    let stream = match dir {
        Direction::Record => "Call microphone",
        _ => "Call",
    };
    Simple::new(
        None,
        "EasyPlay",
        dir,
        dev,
        stream,
        &spec(),
        None,
        Some(attr),
    )
    .map_err(|e| io::Error::other(e.to_string().unwrap_or_else(|| format!("{e:?}"))))
}

impl AudioBackend for PulseAudio {
    fn open_input(&self, device: &str) -> io::Result<Box<dyn AudioIn>> {
        let attr = BufferAttr {
            maxlength: u32::MAX,
            tlength: u32::MAX,
            prebuf: u32::MAX,
            minreq: u32::MAX,
            fragsize: FRAME_BYTES,
        };
        Ok(Box::new(PulseIn(open(Direction::Record, device, &attr)?)))
    }

    fn open_output(&self, device: &str) -> io::Result<Box<dyn AudioOut>> {
        // About 80 ms in the server's buffer: late packets wait there, not in ours.
        let attr = BufferAttr {
            maxlength: u32::MAX,
            tlength: FRAME_BYTES * 4,
            prebuf: FRAME_BYTES * 2,
            minreq: FRAME_BYTES,
            fragsize: u32::MAX,
        };
        Ok(Box::new(PulseOut(open(
            Direction::Playback,
            device,
            &attr,
        )?)))
    }

    fn devices(&self) -> io::Result<easyplay_core::model::AudioDevices> {
        super::devices::list()
    }
}

struct PulseIn(Simple);

impl AudioIn for PulseIn {
    fn read(&mut self, frame: &mut [i16; FRAME]) -> io::Result<()> {
        let mut bytes = [0u8; FRAME * 2];
        self.0
            .read(&mut bytes)
            .map_err(|e| io::Error::other(format!("{e:?}")))?;
        let (pairs, _) = bytes.as_chunks::<2>();
        for (s, b) in frame.iter_mut().zip(pairs) {
            *s = i16::from_ne_bytes(*b);
        }
        Ok(())
    }
}

struct PulseOut(Simple);

impl AudioOut for PulseOut {
    fn write(&mut self, frame: &[i16]) -> io::Result<()> {
        let bytes: Vec<u8> = frame.iter().flat_map(|s| s.to_ne_bytes()).collect();
        self.0
            .write(&bytes)
            .map_err(|e| io::Error::other(format!("{e:?}")))
    }
}
