//! One libavcodec H.264 encoder (ADR-0036), NVENC or VA-API, fed I420 pictures.
use std::ffi::CString;
use std::sync::Once;

use rsmpeg::avcodec::{AVCodec, AVCodecContext};
use rsmpeg::avutil::{AVDictionary, AVFrame, AVHWDeviceContext};
use rsmpeg::error::RsmpegError;
use rsmpeg::ffi;

use super::planes;
use crate::video::encoder::{EncodeSpec, VideoEncoder};
use crate::video::i420::I420;

pub struct Encoder {
    ctx: AVCodecContext,
    /// The picture in system memory (YUV420P for NVENC, NV12 for VA-API).
    frame: AVFrame,
    /// VA-API: upload `frame` into a surface first.
    vaapi: bool,
    scratch: Vec<u8>,
    pts: i64,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn setup(codec: &AVCodec, spec: &EncodeSpec, pix_fmt: i32) -> Result<AVCodecContext, String> {
    static QUIET: Once = Once::new();
    QUIET.call_once(planes::quiet_logs);
    let (w, h) = (
        i32::try_from(spec.width).map_err(err)?,
        i32::try_from(spec.height).map_err(err)?,
    );
    let fps = i32::try_from(spec.fps.max(1)).map_err(err)?;
    let mut ctx = AVCodecContext::new(codec);
    ctx.set_width(w);
    ctx.set_height(h);
    ctx.set_pix_fmt(pix_fmt);
    ctx.set_time_base(ffi::AVRational { num: 1, den: fps });
    ctx.set_framerate(ffi::AVRational { num: fps, den: 1 });
    ctx.set_bit_rate(i64::from(spec.bitrate));
    // A keyframe every four seconds as well as on request.
    ctx.set_gop_size(fps * 4);
    ctx.set_max_b_frames(0);
    Ok(ctx)
}

fn sw_frame(spec: &EncodeSpec, format: i32) -> Result<AVFrame, String> {
    let mut frame = AVFrame::new();
    frame.set_format(format);
    frame.set_width(i32::try_from(spec.width).map_err(err)?);
    frame.set_height(i32::try_from(spec.height).map_err(err)?);
    frame.get_buffer(0).map_err(err)?;
    Ok(frame)
}

fn opts(pairs: &[(&str, String)]) -> Result<Option<AVDictionary>, String> {
    let mut dict: Option<AVDictionary> = None;
    for (k, v) in pairs {
        let (k, v) = (
            CString::new(*k).map_err(err)?,
            CString::new(v.as_str()).map_err(err)?,
        );
        dict = Some(match dict {
            Some(d) => d.set(&k, &v, 0),
            None => AVDictionary::new(&k, &v, 0),
        });
    }
    Ok(dict)
}

impl Encoder {
    pub fn nvenc(spec: &EncodeSpec) -> Result<Self, String> {
        let codec =
            AVCodec::find_encoder_by_name(c"h264_nvenc").ok_or("libavcodec has no h264_nvenc")?;
        let mut ctx = setup(&codec, spec, ffi::AV_PIX_FMT_YUV420P)?;
        let rate = spec.bitrate.to_string();
        let buf = (spec.bitrate / spec.fps.max(1) * 2).to_string();
        let dict = opts(&[
            ("preset", "p1".into()),
            ("tune", "ull".into()),
            ("zerolatency", "1".into()),
            ("delay", "0".into()),
            ("rc", "cbr".into()),
            ("forced-idr", "1".into()),
            ("maxrate", rate),
            ("bufsize", buf),
        ])?;
        ctx.open(dict).map_err(err)?;
        Ok(Self {
            ctx,
            frame: sw_frame(spec, ffi::AV_PIX_FMT_YUV420P)?,
            vaapi: false,
            scratch: Vec::new(),
            pts: 0,
        })
    }

    pub fn vaapi(spec: &EncodeSpec, node: &str) -> Result<Self, String> {
        let codec =
            AVCodec::find_encoder_by_name(c"h264_vaapi").ok_or("libavcodec has no h264_vaapi")?;
        let node = CString::new(node).map_err(err)?;
        let device = AVHWDeviceContext::create(ffi::AV_HWDEVICE_TYPE_VAAPI, Some(&node), None, 0)
            .map_err(err)?;
        let mut frames = device.hwframe_ctx_alloc();
        {
            let f = frames.data();
            f.format = ffi::AV_PIX_FMT_VAAPI;
            f.sw_format = ffi::AV_PIX_FMT_NV12;
            f.width = i32::try_from(spec.width).map_err(err)?;
            f.height = i32::try_from(spec.height).map_err(err)?;
            f.initial_pool_size = 8;
        }
        frames.init().map_err(err)?;
        let mut ctx = setup(&codec, spec, ffi::AV_PIX_FMT_VAAPI)?;
        ctx.set_hw_frames_ctx(frames);
        let dict = opts(&[("rc_mode", "CBR".into()), ("async_depth", "1".into())])?;
        ctx.open(dict).map_err(err)?;
        Ok(Self {
            ctx,
            frame: sw_frame(spec, ffi::AV_PIX_FMT_NV12)?,
            vaapi: true,
            scratch: Vec::new(),
            pts: 0,
        })
    }

    fn send(&mut self, key: bool) -> Result<(), String> {
        let kind = if key {
            ffi::AV_PICTURE_TYPE_I
        } else {
            ffi::AV_PICTURE_TYPE_NONE
        };
        if self.vaapi {
            let mut hw = AVFrame::new();
            self.ctx
                .hw_frames_ctx_mut()
                .ok_or("no VA-API surfaces")?
                .get_buffer(&mut hw)
                .map_err(err)?;
            hw.hwframe_transfer_data(&self.frame).map_err(err)?;
            hw.set_pts(self.pts);
            hw.set_pict_type(kind);
            self.ctx.send_frame(Some(&hw)).map_err(err)
        } else {
            self.frame.set_pts(self.pts);
            self.frame.set_pict_type(kind);
            self.ctx.send_frame(Some(&self.frame)).map_err(err)
        }
    }
}

impl VideoEncoder for Encoder {
    fn encode(&mut self, pic: &I420, key: bool, out: &mut Vec<u8>) -> Result<bool, String> {
        // The encoder may still hold the last picture's buffer.
        self.frame.make_writable().map_err(err)?;
        if self.vaapi {
            planes::fill_nv12(&mut self.frame, pic, &mut self.scratch)?;
        } else {
            planes::fill_i420(&mut self.frame, pic)?;
        }
        self.send(key)?;
        self.pts += 1;
        out.clear();
        loop {
            match self.ctx.receive_packet() {
                Ok(p) => planes::append_packet(&p, out),
                Err(RsmpegError::EncoderDrainError) => break,
                Err(e) => return Err(err(e)),
            }
        }
        Ok(!out.is_empty())
    }
}
