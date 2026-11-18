//! H.264 in and out (ADR-0036) through Cisco's OpenH264, built from its source: Constrained
//! Baseline for real-time screen content, Annex B byte streams (what str0m packs into RTP
//! and unpacks again).
use easyplay_core::chat::media::VideoFrame;
use openh264::OpenH264API;
use openh264::decoder::Decoder;
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, RateControlMode, UsageType,
};
use openh264::formats::YUVSource;

use super::i420::{I420, Planes, to_frame};

/// Encodes pictures into H.264 access units.
pub struct H264Encoder {
    enc: Encoder,
}

impl H264Encoder {
    pub fn new(fps: u32, bitrate: u32) -> Result<Self, String> {
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bitrate))
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            // A keyframe every four seconds, so a lost one costs at most that.
            .intra_frame_period(IntraFramePeriod::from_num_frames(fps * 4));
        let enc = Encoder::with_api_config(OpenH264API::from_source(), config)
            .map_err(|e| e.to_string())?;
        Ok(Self { enc })
    }

    /// Encode `pic` into `out` (replacing its contents), a keyframe when `key`. `false`: the
    /// rate control skipped this picture.
    pub fn encode(&mut self, pic: &I420, key: bool, out: &mut Vec<u8>) -> Result<bool, String> {
        if key {
            self.enc.force_intra_frame();
        }
        let bits = self.enc.encode(pic).map_err(|e| e.to_string())?;
        out.clear();
        bits.write_vec(out);
        Ok(!out.is_empty())
    }
}

/// Decodes H.264 access units into page frames.
pub struct H264Decoder {
    dec: Decoder,
}

impl H264Decoder {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            dec: Decoder::new().map_err(|e| e.to_string())?,
        })
    }

    /// Decode one access unit into `out` (its buffer reused). `false`: no picture yet (the
    /// decoder waits for a keyframe or more data).
    pub fn decode(&mut self, unit: &[u8], out: &mut VideoFrame) -> Result<bool, String> {
        let Some(yuv) = self.dec.decode(unit).map_err(|e| e.to_string())? else {
            return Ok(false);
        };
        let (width, height) = yuv.dimensions();
        let (sy, su, sv) = yuv.strides();
        to_frame(
            &Planes {
                width,
                height,
                y: (yuv.y(), sy),
                u: (yuv.u(), su),
                v: (yuv.v(), sv),
            },
            out,
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyplay_core::chat::media::pattern::{shows_test_pattern, test_pattern};

    #[test]
    fn the_test_picture_survives_encoding_and_decoding() {
        let mut enc = H264Encoder::new(30, crate::video::encoder::bitrate(640, 360, 30)).unwrap();
        let mut dec = H264Decoder::new().unwrap();
        let (mut unit, mut out) = (Vec::new(), VideoFrame::black(1, 1));
        let mut shown = 0;
        for n in 0..10 {
            let pic = I420::from_frame(&test_pattern(640, 360, n));
            if !enc.encode(&pic, n == 0, &mut unit).unwrap() {
                continue;
            }
            assert!(unit.starts_with(&[0, 0, 0, 1]), "Annex B");
            if dec.decode(&unit, &mut out).unwrap() {
                assert_eq!((out.width, out.height), (640, 360));
                assert!(shows_test_pattern(&out), "frame {n}");
                shown += 1;
            }
        }
        assert!(shown >= 5, "{shown} frames decoded");
    }

    #[test]
    fn a_new_size_starts_a_new_stream_the_decoder_follows() {
        let mut enc = H264Encoder::new(30, crate::video::encoder::bitrate(1280, 720, 30)).unwrap();
        let mut dec = H264Decoder::new().unwrap();
        let (mut unit, mut out) = (Vec::new(), VideoFrame::black(1, 1));
        for (w, h) in [(320, 180), (1280, 720)] {
            let pic = I420::from_frame(&test_pattern(w, h, 0));
            assert!(enc.encode(&pic, true, &mut unit).unwrap());
            assert!(dec.decode(&unit, &mut out).unwrap());
            assert_eq!((out.width, out.height), (w, h));
        }
    }
}
