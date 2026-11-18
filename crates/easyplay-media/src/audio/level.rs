//! How loud a frame is, 0..=100 (−60 dBFS and below is 0).

pub(crate) fn level(frame: &[i16]) -> u8 {
    if frame.is_empty() {
        return 0;
    }
    let sum: f64 = frame.iter().map(|s| f64::from(*s).powi(2)).sum();
    let rms = (sum / frame.len() as f64).sqrt() / f64::from(i16::MAX);
    if rms <= 0.0 {
        return 0;
    }
    let db = 20.0 * rms.log10();
    // Clamped to 0..=100 before the cast.
    ((db + 60.0) / 60.0 * 100.0).clamp(0.0, 100.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_zero_and_full_scale_is_a_hundred() {
        assert_eq!(level(&[0; 960]), 0);
        assert_eq!(level(&[i16::MAX; 960]), 100);
        let quiet: Vec<i16> = (0..960)
            .map(|i| if i % 2 == 0 { 33 } else { -33 })
            .collect();
        assert_eq!(level(&quiet), 0, "−60 dBFS");
    }
}
