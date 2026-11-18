//! H.264 on the GPU through FFmpeg's libavcodec (ADR-0036): NVENC (`h264_nvenc`) and VA-API
//! (`h264_vaapi` on the first render node). Pictures are the share's I420 in system memory,
//! uploaded by the encoder (NVENC) or into VA-API surfaces as NV12; DMA-BUF zero copy is not
//! done yet.

mod ffmpeg;
mod planes;

use std::sync::Arc;

use zaun_core::model::VideoEncoding;

use super::encoder::{EncodeSpec, EncoderKind, VideoEncoder};

/// The GPU encoders, in the order they are tried.
pub fn kinds() -> Vec<Arc<dyn EncoderKind>> {
    vec![Arc::new(Nvenc), Arc::new(Vaapi)]
}

/// NVIDIA's NVENC: low-latency preset, constant bit rate, no B-frames.
pub struct Nvenc;

impl EncoderKind for Nvenc {
    fn name(&self) -> &'static str {
        "h264_nvenc"
    }

    fn encoding(&self) -> VideoEncoding {
        VideoEncoding::Hardware
    }

    fn open(&self, spec: &EncodeSpec) -> Result<Box<dyn VideoEncoder>, String> {
        Ok(Box::new(ffmpeg::Encoder::nvenc(spec)?))
    }
}

/// VA-API (AMD, Intel; on NVIDIA it only decodes): the first render node.
pub struct Vaapi;

impl EncoderKind for Vaapi {
    fn name(&self) -> &'static str {
        "h264_vaapi"
    }

    fn encoding(&self) -> VideoEncoding {
        VideoEncoding::Hardware
    }

    fn open(&self, spec: &EncodeSpec) -> Result<Box<dyn VideoEncoder>, String> {
        let node = render_node().ok_or("no render node")?;
        Ok(Box::new(ffmpeg::Encoder::vaapi(spec, &node)?))
    }
}

/// `/dev/dri/renderD128` or the first render node there is.
fn render_node() -> Option<String> {
    let mut nodes: Vec<String> = std::fs::read_dir("/dev/dri")
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("renderD"))
        .collect();
    nodes.sort();
    nodes.first().map(|n| format!("/dev/dri/{n}"))
}
