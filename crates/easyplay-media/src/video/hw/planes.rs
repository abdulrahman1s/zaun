//! Copying pictures into FFmpeg's frames and encoded bytes out of its packets: the only
//! raw-pointer code of the media crate (the FFmpeg binding hands out the planes and the
//! packet's data only as pointers).
#![allow(
    unsafe_code,
    reason = "FFmpeg's frame planes and packet data are raw pointers"
)]

use rsmpeg::avcodec::AVPacket;
use rsmpeg::avutil::AVFrame;

use crate::video::i420::I420;

/// Copy `rows` rows of `width` bytes, `src` rows `width` apart, into plane `plane` of
/// `frame`. Fails when the frame is not allocated for it.
fn copy_plane(
    frame: &mut AVFrame,
    plane: usize,
    src: &[u8],
    width: usize,
    rows: usize,
) -> Result<(), String> {
    let stride = usize::try_from(frame.linesize[plane]).map_err(|_| "negative stride")?;
    let dst = frame.data[plane];
    if dst.is_null() || stride < width || src.len() < width * rows {
        return Err("the frame does not fit the picture".into());
    }
    for (row, line) in src.chunks_exact(width).take(rows).enumerate() {
        // SAFETY: `dst` is plane `plane` of a frame allocated by `av_frame_get_buffer` for
        // at least `rows` rows of `stride` bytes (checked by the caller's frame size), and
        // `width <= stride`; the source line is `width` bytes; the two do not overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(line.as_ptr(), dst.add(row * stride), width);
        }
    }
    Ok(())
}

/// `pic` into a YUV420P frame of its size.
pub fn fill_i420(frame: &mut AVFrame, pic: &I420) -> Result<(), String> {
    check(frame, pic)?;
    let (w, h) = (pic.width, pic.height);
    copy_plane(frame, 0, &pic.y, w, h)?;
    copy_plane(frame, 1, &pic.u, w / 2, h / 2)?;
    copy_plane(frame, 2, &pic.v, w / 2, h / 2)
}

/// `pic` into an NV12 frame of its size (U and V interleaved), through `scratch`.
pub fn fill_nv12(frame: &mut AVFrame, pic: &I420, scratch: &mut Vec<u8>) -> Result<(), String> {
    check(frame, pic)?;
    let (w, h) = (pic.width, pic.height);
    copy_plane(frame, 0, &pic.y, w, h)?;
    scratch.clear();
    scratch.extend(pic.u.iter().zip(&pic.v).flat_map(|(u, v)| [*u, *v]));
    copy_plane(frame, 1, scratch, w, h / 2)
}

fn check(frame: &AVFrame, pic: &I420) -> Result<(), String> {
    let same = usize::try_from(frame.width).ok() == Some(pic.width)
        && usize::try_from(frame.height).ok() == Some(pic.height);
    if same && pic.y.len() == pic.width * pic.height && pic.u.len() == pic.y.len() / 4 {
        Ok(())
    } else {
        Err("the frame and the picture differ in size".into())
    }
}

/// Append the packet's bytes to `out`.
pub fn append_packet(packet: &AVPacket, out: &mut Vec<u8>) {
    let Ok(len) = usize::try_from(packet.size) else {
        return;
    };
    if packet.data.is_null() || len == 0 {
        return;
    }
    // SAFETY: a packet returned by `avcodec_receive_packet` owns `size` bytes at `data`,
    // valid while the packet lives (it is borrowed for this call).
    let bytes = unsafe { std::slice::from_raw_parts(packet.data, len) };
    out.extend_from_slice(bytes);
}

/// FFmpeg says only what is wrong (its encoders log a lot at the default level).
pub fn quiet_logs() {
    // SAFETY: sets a global integer in libavutil; no pointers involved.
    unsafe { rsmpeg::ffi::av_log_set_level(rsmpeg::ffi::AV_LOG_ERROR as i32) };
}
