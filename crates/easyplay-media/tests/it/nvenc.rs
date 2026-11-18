//! NVIDIA's NVENC through libavcodec (ADR-0036): 60 frames of the test picture encoded on
//! the GPU decode in software and show the picture.
//!
//! Ignored, and built only with `--features hwenc`: it needs an NVIDIA GPU and its driver
//! (`libnvidia-encode`, `libcuda`). Encoding touches no display, sound or capture.
//! `cargo test -p easyplay-media --features hwenc --test it nvenc -- --ignored`

use easyplay_core::chat::media::VideoFrame;
use easyplay_core::chat::media::pattern::{shows_test_pattern, test_pattern};
use easyplay_media::video::codec::H264Decoder;
use easyplay_media::video::encoder::{EncodeSpec, EncoderKind};
use easyplay_media::video::hw::Nvenc;
use easyplay_media::video::i420::I420;

#[test]
#[ignore = "needs an NVIDIA GPU (see the file's header)"]
fn nvenc_encodes_the_test_picture_and_software_decodes_it() {
    let spec = EncodeSpec::new(1920, 1080, 60);
    let mut enc = Nvenc.open(&spec).expect("h264_nvenc opens");
    let mut dec = H264Decoder::new().unwrap();
    let (mut unit, mut out) = (Vec::new(), VideoFrame::black(1, 1));
    let (mut shown, mut bytes) = (0, 0);
    for n in 0..60 {
        let pic = I420::from_frame(&test_pattern(1920, 1080, n));
        // A keyframe at the start and on request halfway, as for a new watcher.
        if !enc.encode(&pic, n == 0 || n == 30, &mut unit).unwrap() {
            continue;
        }
        bytes += unit.len();
        if dec.decode(&unit, &mut out).unwrap() {
            assert_eq!((out.width, out.height), (1920, 1080));
            assert!(shows_test_pattern(&out), "frame {n}");
            shown += 1;
        }
    }
    eprintln!("{shown} of 60 frames decoded, {bytes} bytes");
    assert!(shown >= 55, "{shown} frames decoded");
}
