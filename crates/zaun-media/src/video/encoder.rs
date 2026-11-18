//! Which H.264 encoder a share uses (ADR-0036): the GPU's (NVENC, then VA-API) when it opens
//! and encodes a first picture, else OpenH264 in software. A GPU encoder that fails while
//! sharing drops to software for the rest of the share; the share goes on.
use std::sync::Arc;

use zaun_core::model::VideoEncoding;

use super::codec::H264Encoder;
use super::i420::I420;

/// What an encoder is opened for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodeSpec {
    pub width: usize,
    pub height: usize,
    pub fps: u32,
    /// Bits a second ([`bitrate`]).
    pub bitrate: u32,
}

impl EncodeSpec {
    pub fn new(width: usize, height: usize, fps: u32) -> Self {
        Self {
            width,
            height,
            fps,
            bitrate: bitrate(width, height, fps),
        }
    }
}

/// Bits per pixel and frame the bit rate is made of.
const BITS_PER_PIXEL: f64 = 0.07;
/// The bit rate stays within this range (the call relay carries the stream).
pub const BITRATE_RANGE: (u32, u32) = (800_000, 12_000_000);

/// The bit rate for a picture: pixels × frames a second × [`BITS_PER_PIXEL`], within
/// [`BITRATE_RANGE`]. 720p30 ≈ 1.9 Mbit/s, 1080p30 ≈ 4.4, 1080p60 ≈ 8.7, 1440p30 ≈ 7.7,
/// 1440p60 and more: 12 (ADR-0036).
pub fn bitrate(width: usize, height: usize, fps: u32) -> u32 {
    let bits = (width * height) as f64 * f64::from(fps) * BITS_PER_PIXEL;
    // Clamped into range before the cast.
    (bits as u64).clamp(u64::from(BITRATE_RANGE.0), u64::from(BITRATE_RANGE.1)) as u32
}

/// One opened encoder. Its spec does not change: a new size or rate opens a new one.
pub trait VideoEncoder: Send {
    /// Encode `pic` into `out` (replacing it), a keyframe when `key`. `false`: nothing came
    /// out for this picture.
    fn encode(&mut self, pic: &I420, key: bool, out: &mut Vec<u8>) -> Result<bool, String>;
}

/// A kind of encoder that may or may not work on this computer.
pub trait EncoderKind: Send + Sync {
    /// For the log: `h264_nvenc`, `h264_vaapi`, `openh264`.
    fn name(&self) -> &'static str;
    fn encoding(&self) -> VideoEncoding;
    fn open(&self, spec: &EncodeSpec) -> Result<Box<dyn VideoEncoder>, String>;
}

/// OpenH264 in software: always there.
pub struct Software;

impl EncoderKind for Software {
    fn name(&self) -> &'static str {
        "openh264"
    }

    fn encoding(&self) -> VideoEncoding {
        VideoEncoding::Software
    }

    fn open(&self, spec: &EncodeSpec) -> Result<Box<dyn VideoEncoder>, String> {
        Ok(Box::new(H264Encoder::new(spec.fps, spec.bitrate)?))
    }
}

impl VideoEncoder for H264Encoder {
    fn encode(&mut self, pic: &I420, key: bool, out: &mut Vec<u8>) -> Result<bool, String> {
        H264Encoder::encode(self, pic, key, out)
    }
}

/// The kinds to try, in order: the GPU's when `hardware` (and this build has them), then
/// software.
pub fn kinds(hardware: bool) -> Vec<Arc<dyn EncoderKind>> {
    let mut list: Vec<Arc<dyn EncoderKind>> = Vec::new();
    #[cfg(feature = "hwenc")]
    if hardware {
        list.extend(super::hw::kinds());
    }
    #[cfg(not(feature = "hwenc"))]
    let _ = hardware;
    list.push(Arc::new(Software));
    list
}

/// The share's encoder: probed on first use, reopened when the spec changes, dropped to
/// software when a GPU encoder fails.
pub struct Encoders {
    kinds: Vec<Arc<dyn EncoderKind>>,
    /// The kind in use (index into `kinds`) and its encoder.
    open: Option<(usize, EncodeSpec, Box<dyn VideoEncoder>)>,
    /// The first kind still worth trying (those before it failed).
    from: usize,
}

impl Encoders {
    pub fn new(kinds: Vec<Arc<dyn EncoderKind>>) -> Self {
        Self {
            kinds,
            open: None,
            from: 0,
        }
    }

    /// The kind in use: (encoding, name).
    pub fn in_use(&self) -> Option<(VideoEncoding, &'static str)> {
        let (i, _, _) = self.open.as_ref()?;
        self.kinds.get(*i).map(|k| (k.encoding(), k.name()))
    }

    /// Encode `pic` for `spec`, a keyframe when `key` (or when the encoder is new).
    pub fn encode(
        &mut self,
        pic: &I420,
        spec: EncodeSpec,
        key: bool,
        out: &mut Vec<u8>,
    ) -> Result<bool, String> {
        let fresh = self.open.as_ref().is_none_or(|(_, s, _)| *s != spec);
        if fresh {
            self.reopen(spec)?;
        }
        let Some((i, _, enc)) = self.open.as_mut() else {
            return Err("no encoder".into());
        };
        match enc.encode(pic, key || fresh, out) {
            Ok(sent) => Ok(sent),
            Err(e) if *i + 1 < self.kinds.len() => {
                let name = self.kinds[*i].name();
                tracing::warn!(encoder = name, error = %e, "share: the encoder failed; on in software");
                self.from = self.kinds.len() - 1;
                self.open = None;
                self.reopen(spec)?;
                let Some((_, _, enc)) = self.open.as_mut() else {
                    return Err("no encoder".into());
                };
                enc.encode(pic, true, out)
            }
            Err(e) => Err(e),
        }
    }

    /// Open the first kind, from the one in use, that opens and encodes a first picture.
    fn reopen(&mut self, spec: EncodeSpec) -> Result<(), String> {
        let start = self.open.as_ref().map_or(self.from, |(i, _, _)| *i);
        self.open = None;
        let probe = I420::new(spec.width, spec.height);
        let mut scratch = Vec::new();
        let mut last = String::from("no encoder");
        for i in start..self.kinds.len() {
            let kind = &self.kinds[i];
            let tried = kind
                .open(&spec)
                .and_then(|mut enc| enc.encode(&probe, true, &mut scratch).map(|_| enc));
            match tried {
                Ok(enc) => {
                    tracing::info!(
                        encoder = kind.name(),
                        width = spec.width,
                        height = spec.height,
                        fps = spec.fps,
                        bitrate = spec.bitrate,
                        "share: encoder chosen"
                    );
                    self.from = i;
                    self.open = Some((i, spec, enc));
                    return Ok(());
                }
                Err(e) => {
                    tracing::info!(encoder = kind.name(), error = %e, "share: encoder not usable");
                    last = e;
                }
            }
        }
        Err(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// An encoder kind that fails to open, or fails after `works` pictures.
    struct Fake {
        name: &'static str,
        opens: bool,
        works: usize,
        opened: Mutex<Vec<EncodeSpec>>,
    }

    impl Fake {
        fn new(name: &'static str, opens: bool, works: usize) -> Arc<Self> {
            Arc::new(Self {
                name,
                opens,
                works,
                opened: Mutex::new(Vec::new()),
            })
        }
    }

    struct FakeEnc(usize);

    impl VideoEncoder for FakeEnc {
        fn encode(&mut self, _pic: &I420, key: bool, out: &mut Vec<u8>) -> Result<bool, String> {
            if self.0 == 0 {
                return Err("the device went away".into());
            }
            self.0 -= 1;
            out.clear();
            out.push(u8::from(key));
            Ok(true)
        }
    }

    impl EncoderKind for Fake {
        fn name(&self) -> &'static str {
            self.name
        }
        fn encoding(&self) -> VideoEncoding {
            if self.name == "soft" {
                VideoEncoding::Software
            } else {
                VideoEncoding::Hardware
            }
        }
        fn open(&self, spec: &EncodeSpec) -> Result<Box<dyn VideoEncoder>, String> {
            self.opened.lock().unwrap().push(*spec);
            if self.opens {
                Ok(Box::new(FakeEnc(self.works)))
            } else {
                Err("no such device".into())
            }
        }
    }

    fn list(k: &[Arc<Fake>]) -> Vec<Arc<dyn EncoderKind>> {
        k.iter()
            .map(|f| Arc::clone(f) as Arc<dyn EncoderKind>)
            .collect()
    }

    #[test]
    fn the_first_encoder_that_opens_and_encodes_is_chosen_in_order() {
        let (nv, va, soft) = (
            Fake::new("nv", false, 0),
            Fake::new("va", true, 100),
            Fake::new("soft", true, 100),
        );
        let mut e = Encoders::new(list(&[Arc::clone(&nv), Arc::clone(&va), Arc::clone(&soft)]));
        let (pic, mut out) = (I420::new(64, 36), Vec::new());
        assert!(
            e.encode(&pic, EncodeSpec::new(64, 36, 30), false, &mut out)
                .unwrap()
        );
        assert_eq!(out, vec![1], "a new encoder starts with a keyframe");
        assert_eq!(e.in_use(), Some((VideoEncoding::Hardware, "va")));
        assert_eq!(nv.opened.lock().unwrap().len(), 1);
        assert!(soft.opened.lock().unwrap().is_empty());
    }

    #[test]
    fn an_encoder_that_fails_its_first_picture_is_skipped() {
        let (nv, soft) = (Fake::new("nv", true, 0), Fake::new("soft", true, 100));
        let mut e = Encoders::new(list(&[nv, soft]));
        let mut out = Vec::new();
        e.encode(
            &I420::new(64, 36),
            EncodeSpec::new(64, 36, 30),
            false,
            &mut out,
        )
        .unwrap();
        assert_eq!(e.in_use(), Some((VideoEncoding::Software, "soft")));
    }

    #[test]
    fn a_gpu_encoder_that_fails_while_sharing_drops_to_software_and_the_share_goes_on() {
        // Works for the probe and two pictures, then fails.
        let (nv, soft) = (Fake::new("nv", true, 3), Fake::new("soft", true, 100));
        let mut e = Encoders::new(list(&[Arc::clone(&nv), Arc::clone(&soft)]));
        let (pic, spec, mut out) = (I420::new(64, 36), EncodeSpec::new(64, 36, 30), Vec::new());
        for _ in 0..2 {
            e.encode(&pic, spec, false, &mut out).unwrap();
        }
        assert_eq!(e.in_use().unwrap().1, "nv");
        assert!(e.encode(&pic, spec, false, &mut out).unwrap());
        assert_eq!(out, vec![1], "the software encoder starts with a keyframe");
        assert_eq!(e.in_use().unwrap().1, "soft");
        // A new size later stays in software: the GPU is not tried again in this share.
        e.encode(
            &I420::new(128, 72),
            EncodeSpec::new(128, 72, 30),
            false,
            &mut out,
        )
        .unwrap();
        assert_eq!(e.in_use().unwrap().1, "soft");
        assert_eq!(nv.opened.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_new_size_or_rate_reopens_the_same_kind_with_a_keyframe() {
        let nv = Fake::new("nv", true, 100);
        let mut e = Encoders::new(list(&[Arc::clone(&nv), Fake::new("soft", true, 100)]));
        let mut out = Vec::new();
        e.encode(
            &I420::new(64, 36),
            EncodeSpec::new(64, 36, 30),
            false,
            &mut out,
        )
        .unwrap();
        e.encode(
            &I420::new(64, 36),
            EncodeSpec::new(64, 36, 30),
            false,
            &mut out,
        )
        .unwrap();
        assert_eq!(out, vec![0]);
        e.encode(
            &I420::new(64, 36),
            EncodeSpec::new(64, 36, 60),
            false,
            &mut out,
        )
        .unwrap();
        assert_eq!(out, vec![1]);
        let opened = nv.opened.lock().unwrap();
        assert_eq!(
            opened.iter().map(|s| s.fps).collect::<Vec<_>>(),
            vec![30, 60]
        );
    }

    #[test]
    fn the_bit_rate_grows_with_pixels_and_frames_within_its_range() {
        assert_eq!(bitrate(1280, 720, 30) / 100_000, 19);
        assert_eq!(bitrate(1920, 1080, 60) / 100_000, 87);
        assert_eq!(bitrate(2560, 1440, 60), BITRATE_RANGE.1);
        assert_eq!(bitrate(320, 180, 30), BITRATE_RANGE.0);
        assert!(bitrate(1920, 1080, 60) > bitrate(1920, 1080, 30));
    }
}
