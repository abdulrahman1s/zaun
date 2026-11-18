//! Untrusted image bytes through `ImageCodec` (ART-001): must return `Ok` or `Err` within the
//! limits and never panic past the codec's own `catch_unwind`. Small limits keep each run fast.
#![no_main]

use zaun_core::artwork::{ImageCodec, ImageCrateCodec, ImageLimits};
use zaun_core::artwork::steam::parse_search;
use zaun_core::artwork::thumbs::read_colour_chunk;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = ImageLimits {
        max_bytes: 1 << 20,
        max_pixels: 1 << 20,
        max_side: 4096,
    };
    if let Ok(img) = ImageCrateCodec.decode(data, &limits) {
        assert!(u64::from(img.width) * u64::from(img.height) <= limits.max_pixels);
        assert_eq!(img.pixels.len(), img.width as usize * img.height as usize * 4);
    }
    let _ = read_colour_chunk(data);
    let _ = parse_search(data);
});
