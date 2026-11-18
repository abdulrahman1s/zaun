//! The game window's pictures from the portal's PipeWire node (ADR-0036), on their own
//! thread: shared-memory buffers only (no DMA-BUF import), 32-bit RGB formats, each picture
//! converted to I420 at the size the share sends and kept as the latest; the share's thread
//! takes it.
use std::os::fd::OwnedFd;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use pipewire as pw;
use pw::spa;
use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
use pw::spa::pod::Pod;
use pw::stream::{StreamFlags, StreamState};

use crate::video::i420::{I420, PixelOrder, fit};
use crate::video::source::Next;

#[derive(Default)]
struct Latest {
    pic: I420,
    fresh: bool,
    max_height: Option<u32>,
    ended: bool,
    failed: Option<String>,
}

#[derive(Default)]
struct Shared {
    latest: Mutex<Latest>,
    ready: Condvar,
}

impl Shared {
    fn with<R>(&self, f: impl FnOnce(&mut Latest) -> R) -> R {
        let r = f(&mut self.latest.lock().unwrap_or_else(PoisonError::into_inner));
        self.ready.notify_all();
        r
    }
}

/// The node's pictures. Dropping it stops the PipeWire thread.
pub struct PipeWireSource {
    shared: Arc<Shared>,
    quit: pw::channel::Sender<()>,
}

impl PipeWireSource {
    /// Connect to the portal's PipeWire remote `fd` and read node `node`.
    pub fn open(fd: OwnedFd, node: u32) -> Result<Self, String> {
        let shared = Arc::new(Shared::default());
        let (quit, quit_rx) = pw::channel::channel::<()>();
        let (started_tx, started) = std::sync::mpsc::channel();
        let s = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("share-pipewire".into())
            .spawn(move || {
                if let Err(e) = run(fd, node, &s, quit_rx, &started_tx) {
                    let _ = started_tx.send(Err(e.clone()));
                    s.with(|l| l.failed = Some(e));
                }
            })
            .map_err(|e| e.to_string())?;
        started
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "PipeWire did not answer".to_owned())??;
        Ok(Self { shared, quit })
    }

    pub fn next(&mut self, pic: &mut I420, max_height: Option<u32>, timeout: Duration) -> Next {
        let mut l = self
            .shared
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        l.max_height = max_height;
        if !l.fresh && !l.ended && l.failed.is_none() {
            l = match self.shared.ready.wait_timeout(l, timeout) {
                Ok((l, _)) => l,
                Err(p) => p.into_inner().0,
            };
        }
        if let Some(e) = l.failed.take() {
            return Next::Failed(e);
        }
        if l.ended {
            return Next::Ended;
        }
        if !l.fresh {
            return Next::Idle;
        }
        l.fresh = false;
        std::mem::swap(pic, &mut l.pic);
        Next::Frame
    }
}

impl Drop for PipeWireSource {
    fn drop(&mut self) {
        // The thread may be gone already.
        let _ = self.quit.send(());
    }
}

/// What the stream's callbacks keep.
struct Data {
    shared: Arc<Shared>,
    format: VideoInfoRaw,
    order: Option<PixelOrder>,
}

fn run(
    fd: OwnedFd,
    node: u32,
    shared: &Arc<Shared>,
    quit: pw::channel::Receiver<()>,
    started: &std::sync::mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    pw::init();
    let err = |e: pw::Error| e.to_string();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(err)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(err)?;
    let core = context.connect_fd_rc(fd, None).map_err(err)?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "zaun-share",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(err)?;
    let data = Data {
        shared: Arc::clone(shared),
        format: VideoInfoRaw::default(),
        order: None,
    };
    let _listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed(|_, d, _, new| match new {
            StreamState::Error(e) => d.shared.with(|l| l.failed = Some(e)),
            StreamState::Unconnected => d.shared.with(|l| l.ended = true),
            _ => {}
        })
        .param_changed(|_, d, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            if d.format.parse(param).is_err() {
                return;
            }
            d.order = match d.format.format() {
                VideoFormat::BGRx | VideoFormat::BGRA => Some(PixelOrder::Bgrx),
                VideoFormat::RGBx | VideoFormat::RGBA => Some(PixelOrder::Rgbx),
                _ => None,
            };
        })
        .process(|stream, d| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(order) = d.order else {
                tracing::debug!("share: a picture before its format was skipped");
                return;
            };
            let size = d.format.size();
            let (w, h) = (size.width as usize, size.height as usize);
            let Some(plane) = buffer.datas_mut().first_mut() else {
                return;
            };
            let chunk = plane.chunk();
            let (offset, stride) = (chunk.offset() as usize, chunk.stride().max(0) as usize);
            let stride = if stride == 0 { w * 4 } else { stride };
            let Some(bytes) = plane.data() else {
                tracing::debug!("share: a buffer that is not mapped was skipped");
                return;
            };
            let Some(bytes) = bytes.get(offset..) else {
                return;
            };
            d.shared.with(|l| {
                let (tw, th) = fit(w, h, l.max_height);
                if (l.pic.width, l.pic.height) != (tw, th) {
                    l.pic = I420::new(tw, th);
                }
                l.pic.read_32bit(bytes, w, h, stride, order);
                l.fresh = true;
            });
        })
        .register()
        .map_err(err)?;
    let format = format_param()?;
    let mut params = [Pod::from_bytes(&format).ok_or("no format")?];
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node),
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(err)?;
    let ml = mainloop.clone();
    let _quit = quit.attach(mainloop.loop_(), move |()| ml.quit());
    let _ = started.send(Ok(()));
    mainloop.run();
    Ok(())
}

/// The formats asked for: 32-bit RGB in either order, any size, up to 60 frames a second.
fn format_param() -> Result<Vec<u8>, String> {
    use spa::param::format::{FormatProperties, MediaSubtype, MediaType};
    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
        spa::pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        spa::pod::property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA,
        ),
        spa::pod::property!(
            FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1280,
                height: 720
            },
            spa::utils::Rectangle {
                width: 2,
                height: 2
            },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        spa::pod::property!(
            FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 30, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 60, denom: 1 }
        ),
    );
    let (cursor, _) = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok(cursor.into_inner())
}
