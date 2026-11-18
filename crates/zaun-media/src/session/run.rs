//! The session thread: gather candidates, write the description, then drive str0m with the
//! network, the microphone and the call flow's requests until the call ends (ADR-0035).
//!
//! str0m is sans-IO: every change to it is followed by draining its output until it asks
//! for a timeout. The thread waits on one channel ([`Inbox`]) for whichever comes first.
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use audiopus::coder::Decoder;
use audiopus::packet::Packet;
use audiopus::{Channels, MutSignals, SampleRate};
use rustls::ClientConfig;
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::format::Codec;
use str0m::media::{Direction, Frequency, MediaKind, MediaTime, Mid};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig};
use zaun_core::chat::media::{MediaConfig, MediaEvent, MediaSink, Negotiation, TrackKind};
use zaun_core::error::ErrorCode;

use super::frame_crypto::FrameCrypto;
use super::inbox::Inbox;
use super::mids::Renames;
use super::relay::Relay;
use crate::audio::backend::{AudioBackend, FRAME};
use crate::audio::{capture, level, playback};
use crate::clock::now;

/// How often the voice levels are reported.
const LEVELS_EVERY: Duration = Duration::from_millis(200);
/// How often a call measures its path (round trip, loss) for the quality bars.
const QUALITY_EVERY: Duration = Duration::from_secs(1);
/// A connection lost this long ends the call.
const LOST_FOR: Duration = Duration::from_secs(20);
/// Decoded frames waiting for the speakers (more are dropped: they would only add delay).
const SPEAKER_QUEUE: usize = 8;

/// Why the session ended early.
pub(crate) enum Fail {
    Error {
        code: ErrorCode,
        detail: String,
    },
    /// Hung up.
    Stopped,
}

impl Fail {
    pub fn media(detail: impl ToString) -> Self {
        Self::Error {
            code: ErrorCode::ChtMediaFailed,
            detail: detail.to_string(),
        }
    }

    pub fn call(detail: impl ToString) -> Self {
        Self::Error {
            code: ErrorCode::ChtCallFailed,
            detail: detail.to_string(),
        }
    }
}

/// What the factory gives every call.
#[derive(Clone)]
pub(crate) struct Options {
    pub audio: Arc<dyn AudioBackend>,
    /// Addresses for host candidates; `None` = the address of the default route.
    pub host_ips: Option<Vec<IpAddr>>,
    pub tls: Option<Arc<ClientConfig>>,
}

pub(crate) struct Session {
    pub rtc: Rtc,
    pub socket: Arc<UdpSocket>,
    /// The host candidate (the socket's address), when direct paths are allowed.
    pub host: Option<SocketAddr>,
    pub relays: Vec<Relay>,
    /// The friend's candidate addresses (each relay permits them).
    pub peers: HashSet<IpAddr>,
    pub sink: MediaSink,
    pub rx: Receiver<Inbox>,
    pub tx: Sender<Inbox>,
    pub stop: Arc<AtomicBool>,
    pub muted: Arc<AtomicBool>,
    pub deafened: bool,
    /// The description went out (relay failures before that are part of gathering).
    pub described: bool,
    /// Requests that came while gathering, handled after the description.
    pub early: Vec<Inbox>,
    pending: Option<SdpPendingOffer>,
    /// Our offer's mids as the friend's app knows them (`mids`); empty when answering.
    renames: Renames,
    /// The voice's mid: the first audio line (the apps' microphone; their second is a
    /// screen's sound, which a call does not play).
    mid: Option<Mid>,
    /// The apps' frame encryption, with the engine's key.
    crypto: Option<FrameCrypto>,
    rtp: u64,
    decoder: Decoder,
    speaker: Option<SyncSender<Vec<i16>>>,
    pub me: u8,
    friend: u8,
    heard: Option<Instant>,
    levels_at: Instant,
    pub(super) connected: bool,
    lost: Option<Instant>,
    /// A stream's video track and notes (ADR-0036); `None` for a call.
    pub(super) video: Option<super::video::VideoPart>,
}

/// Start a call's session thread.
pub(crate) fn spawn(
    config: MediaConfig,
    sink: MediaSink,
    opts: Options,
    tx: Sender<Inbox>,
    rx: Receiver<Inbox>,
    muted: Arc<AtomicBool>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("call-media".into())
        .spawn(move || {
            let stop = Arc::new(AtomicBool::new(false));
            let failed =
                match Session::open(&config, &opts, Arc::clone(&sink), tx, rx, muted, &stop) {
                    Ok(mut s) => {
                        let r = s.run(&config, &opts);
                        s.relays_close();
                        r.err()
                    }
                    Err(f) => Some(f),
                };
            stop.store(true, Ordering::Relaxed);
            if let Some(Fail::Error { code, detail }) = failed {
                sink(MediaEvent::Failed { code, detail });
            }
        })?;
    Ok(())
}

impl Session {
    pub(super) fn open(
        config: &MediaConfig,
        opts: &Options,
        sink: MediaSink,
        tx: Sender<Inbox>,
        rx: Receiver<Inbox>,
        muted: Arc<AtomicBool>,
        stop: &Arc<AtomicBool>,
    ) -> Result<Self, Fail> {
        let host_ip = (!config.relay_only)
            .then(|| host_ip(opts.host_ips.as_deref()))
            .flatten();
        let bind = SocketAddr::new(host_ip.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)), 0);
        let socket = Arc::new(UdpSocket::bind(bind).map_err(Fail::media)?);
        let host = match host_ip {
            Some(_) => Some(socket.local_addr().map_err(Fail::media)?),
            None => None,
        };
        spawn_reader(Arc::clone(&socket), tx.clone(), Arc::clone(stop)).map_err(Fail::media)?;
        let audio = config.tracks.contains(&TrackKind::Audio);
        let video = config.tracks.contains(&TrackKind::Video);
        let (speaker, frames) = sync_channel(SPEAKER_QUEUE);
        // A stream (ADR-0036) opens neither the microphone nor the speakers.
        if audio {
            Self::open_sound(config, opts, &muted, stop, &tx, frames)?;
        }
        let decoder = Decoder::new(SampleRate::Hz48000, Channels::Mono).map_err(Fail::media)?;
        let t = now();
        // A call answers the apps' camera and screen lines too (their answer handler reads
        // all four), with VP8 that is never sent and whose data is dropped.
        let rtc = RtcConfig::new()
            .clear_codecs()
            .enable_opus(audio, false)
            .enable_h264(video)
            .enable_vp8(audio && !video)
            .set_stats_interval(audio.then_some(QUALITY_EVERY))
            .build(t);
        Ok(Self {
            rtc,
            socket,
            host,
            relays: Vec::new(),
            peers: HashSet::new(),
            sink,
            rx,
            tx,
            stop: Arc::clone(stop),
            muted,
            deafened: false,
            described: false,
            early: Vec::new(),
            pending: None,
            renames: Renames::default(),
            mid: None,
            crypto: config.key.as_ref().map(FrameCrypto::new),
            rtp: 0,
            decoder,
            speaker: audio.then_some(speaker),
            me: 0,
            friend: 0,
            heard: None,
            levels_at: t,
            connected: false,
            lost: None,
            video: None,
        })
    }

    /// The microphone and speaker threads of a call.
    fn open_sound(
        config: &MediaConfig,
        opts: &Options,
        muted: &Arc<AtomicBool>,
        stop: &Arc<AtomicBool>,
        tx: &Sender<Inbox>,
        frames: Receiver<Vec<i16>>,
    ) -> Result<(), Fail> {
        let mic = capture::Mic {
            backend: Arc::clone(&opts.audio),
            device: config.audio.mic.clone(),
            denoise: config.audio.noise_suppression,
            volume: config.audio.mic_volume,
            muted: Arc::clone(muted),
            stop: Arc::clone(stop),
        };
        capture::spawn(mic, tx.clone()).map_err(Fail::media)?;
        playback::spawn(
            Arc::clone(&opts.audio),
            config.audio.speakers.clone(),
            frames,
            tx.clone(),
        )
        .map_err(Fail::media)
    }

    pub(super) fn run(&mut self, config: &MediaConfig, opts: &Options) -> Result<(), Fail> {
        let candidates = self.gather(config, opts.tls.as_ref())?;
        if candidates.is_empty() {
            return Err(Fail::call(if config.relay_only {
                "no call relay could be reached"
            } else {
                "no network address for the call"
            }));
        }
        for c in candidates {
            tracing::debug!(candidate = %c.to_sdp_string(), "call: local candidate");
            self.rtc.add_local_candidate(c);
            self.drain()?;
        }
        self.describe(config)?;
        for msg in std::mem::take(&mut self.early) {
            self.on_msg(msg)?;
        }
        loop {
            let timeout = self.drain()?;
            let t = now();
            let deadline = [
                Some(timeout),
                self.relay_deadline(),
                Some(self.levels_at + LEVELS_EVERY),
            ]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(t + LEVELS_EVERY);
            let wait = deadline
                .saturating_duration_since(t)
                .max(Duration::from_millis(1));
            match self.rx.recv_timeout(wait) {
                Ok(msg) => self.on_msg(msg)?,
                Err(RecvTimeoutError::Timeout) => self.on_timeout()?,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    }

    /// The offer, or the answer to the friend's offer.
    fn describe(&mut self, config: &MediaConfig) -> Result<(), Fail> {
        let sdp = match &config.negotiation {
            Negotiation::Offer => {
                let mut change = self.rtc.sdp_api();
                let mut mid = None;
                if config.tracks.contains(&TrackKind::Audio) {
                    mid = Some(change.add_media(
                        MediaKind::Audio,
                        Direction::SendRecv,
                        None,
                        None,
                        None,
                    ));
                }
                // A stream's offer comes from the side that watches (ADR-0036).
                let video = config.tracks.contains(&TrackKind::Video);
                let mut video_mid = None;
                if video {
                    video_mid = Some(change.add_media(
                        MediaKind::Video,
                        Direction::RecvOnly,
                        None,
                        None,
                        None,
                    ));
                    change.add_channel(super::video::CHANNEL.into());
                }
                let (offer, pending) = change
                    .apply()
                    .ok_or_else(|| Fail::media("nothing to offer"))?;
                self.mid = mid;
                if let Some(v) = video_mid {
                    self.video_added(v);
                }
                self.pending = Some(pending);
                let sdp = offer.to_sdp_string();
                if video {
                    sdp
                } else {
                    let (sdp, renames) = Renames::number(&sdp);
                    self.renames = renames;
                    sdp
                }
            }
            Negotiation::Answer(remote) => {
                self.note_peers(remote);
                let offer = SdpOffer::from_sdp_string(remote).map_err(Fail::call)?;
                let answer = self.rtc.sdp_api().accept_offer(offer).map_err(Fail::call)?;
                answer.to_sdp_string()
            }
        };
        self.drain()?;
        self.described = true;
        (self.sink)(MediaEvent::Description(sdp));
        Ok(())
    }

    pub(super) fn on_msg(&mut self, msg: Inbox) -> Result<(), Fail> {
        let t = now();
        match msg {
            Inbox::Udp { from, data } => {
                if let Some(i) = self.relays.iter().position(|r| r.server_is(from)) {
                    self.relay_in(i, &data, t)?;
                } else if let Some(host) = self.host {
                    self.receive(from, host, &data, t)?;
                }
            }
            Inbox::Stream { relay, frame } => match frame {
                Some(f) => {
                    self.relay_in(relay, &f, t)?;
                }
                None => {
                    if let Some(r) = self.relays.get_mut(relay) {
                        r.client.abandon();
                    }
                    self.relay_lost("the connection to the relay closed")?;
                }
            },
            Inbox::Frame { packet, level } => self.send_frame(packet, level, t)?,
            Inbox::Answer(sdp) => {
                let Some(pending) = self.pending.take() else {
                    return Ok(());
                };
                self.note_peers(&sdp);
                let sdp = self.renames.restore(&sdp);
                let answer = SdpAnswer::from_sdp_string(&sdp).map_err(Fail::call)?;
                self.rtc
                    .sdp_api()
                    .accept_answer(pending, answer)
                    .map_err(Fail::call)?;
            }
            Inbox::Candidates(list) => {
                for c in list {
                    match Candidate::from_sdp_string(&c) {
                        Ok(cand) => {
                            self.permit(cand.addr().ip(), t);
                            self.rtc.add_remote_candidate(cand);
                            self.drain()?;
                        }
                        Err(e) => tracing::debug!(error = %e, "call: a candidate not understood"),
                    }
                }
            }
            Inbox::Muted(on) => self.muted.store(on, Ordering::Relaxed),
            Inbox::Deafened(on) => self.deafened = on,
            Inbox::Failed { code, detail } => return Err(Fail::Error { code, detail }),
            Inbox::Stop => return Err(Fail::Stopped),
            Inbox::Video(m) => self.video_msg(m),
        }
        Ok(())
    }

    fn on_timeout(&mut self) -> Result<(), Fail> {
        let t = now();
        self.rtc
            .handle_input(Input::Timeout(t))
            .map_err(Fail::media)?;
        self.relay_ticks(t)?;
        if t >= self.levels_at + LEVELS_EVERY {
            self.levels_at = t;
            if self
                .heard
                .is_none_or(|h| t.saturating_duration_since(h) > LEVELS_EVERY)
            {
                self.friend = 0;
            }
            (self.sink)(MediaEvent::Levels {
                me: self.me,
                friend: self.friend,
            });
        }
        if self
            .lost
            .is_some_and(|l| t.saturating_duration_since(l) > LOST_FOR)
        {
            return Err(Fail::call("the connection was lost"));
        }
        Ok(())
    }

    /// A datagram for str0m: from `source` to our candidate `destination`.
    pub(super) fn receive(
        &mut self,
        source: SocketAddr,
        destination: SocketAddr,
        data: &[u8],
        t: Instant,
    ) -> Result<(), Fail> {
        let Ok(r) = Receive::new(Protocol::Udp, source, destination, data) else {
            return Ok(());
        };
        // A stray or damaged datagram is the network's, not a reason to end the call.
        if let Err(e) = self.rtc.handle_input(Input::Receive(t, r)) {
            tracing::debug!(error = %e, "call: a datagram was refused");
        }
        Ok(())
    }

    fn send_frame(&mut self, packet: Option<Vec<u8>>, level: u8, t: Instant) -> Result<(), Fail> {
        self.me = level;
        let rtp = self.rtp;
        self.rtp += FRAME as u64;
        let (Some(packet), Some(mid), true) = (packet, self.mid, self.connected) else {
            return Ok(());
        };
        let Some(writer) = self.rtc.writer(mid) else {
            return Ok(());
        };
        let Some(pt) = writer
            .payload_params()
            .find(|p| p.spec().codec == Codec::Opus)
            .map(|p| p.pt())
        else {
            return Ok(());
        };
        let packet = match &self.crypto {
            Some(c) => match c.seal(&packet) {
                Some(p) => p,
                None => return Ok(()),
            },
            None => packet,
        };
        let time = MediaTime::new(rtp, Frequency::FORTY_EIGHT_KHZ);
        writer.write(pt, t, time, packet).map_err(Fail::media)?;
        Ok(())
    }

    /// Run str0m until it wants a timeout: send what it sends, act on its events.
    pub(super) fn drain(&mut self) -> Result<Instant, Fail> {
        loop {
            match self.rtc.poll_output().map_err(Fail::media)? {
                Output::Timeout(t) => return Ok(t),
                Output::Transmit(t) => self.transmit(t.source, t.destination, &t.contents),
                Output::Event(e) => self.on_event(e),
            }
        }
    }

    fn transmit(&mut self, source: SocketAddr, destination: SocketAddr, data: &[u8]) {
        if let Some(r) = self
            .relays
            .iter()
            .find(|r| r.client.relayed() == Some(source))
        {
            let wrapped = r.client.send(destination, data);
            r.put(&self.socket, &wrapped);
        } else if Some(source) == self.host {
            if let Err(e) = self.socket.send_to(data, destination) {
                tracing::debug!(error = %e, "call: a datagram was not sent");
            }
        } else {
            // From a relay that is gone: never send it from this computer's own address
            // (with `relay_calls` the friend must not learn it).
            tracing::trace!("call: a datagram of a lost relay was dropped");
        }
    }

    fn on_event(&mut self, e: Event) {
        match e {
            Event::Connected => {
                if !self.connected {
                    tracing::info!("call: media connected");
                    self.connected = true;
                    (self.sink)(MediaEvent::Connected);
                }
            }
            Event::IceConnectionStateChange(s) => {
                tracing::debug!(?s, "call: ICE");
                match s {
                    IceConnectionState::Disconnected if self.connected => {
                        self.lost.get_or_insert_with(now);
                        (self.sink)(MediaEvent::Interrupted);
                    }
                    IceConnectionState::Connected | IceConnectionState::Completed
                        if self.lost.take().is_some() =>
                    {
                        (self.sink)(MediaEvent::Connected);
                    }
                    _ => {}
                }
            }
            Event::MediaAdded(m) if m.kind == MediaKind::Video => self.video_added(m.mid),
            Event::MediaAdded(m) => {
                self.mid.get_or_insert(m.mid);
            }
            Event::MediaData(d) if self.is_video(d.mid) => self.video_data(&d),
            Event::MediaData(d) if Some(d.mid) == self.mid => self.play(&d.data),
            Event::MediaData(_) => {}
            Event::KeyframeRequest(_) => self.keyframe_wanted(),
            Event::ChannelOpen(id, label) => self.channel_open(id, &label),
            Event::ChannelData(d) => self.channel_data(&d),
            Event::PeerStats(s) => {
                if let Some(rtt) = s.rtt {
                    let loss = s
                        .ingress_loss_fraction
                        .unwrap_or(0.0)
                        .max(s.egress_loss_fraction.unwrap_or(0.0));
                    (self.sink)(MediaEvent::Quality {
                        rtt_ms: u16::try_from(rtt.as_millis()).unwrap_or(u16::MAX),
                        loss: (loss.clamp(0.0, 1.0) * 100.0).round() as u8,
                    });
                }
            }
            _ => {}
        }
    }

    /// Decode one packet from the friend and hand it to the speakers.
    fn play(&mut self, data: &[u8]) {
        let opened;
        let data = match &self.crypto {
            Some(c) => {
                let Some(d) = c.open(data) else {
                    tracing::trace!("call: a frame did not decrypt");
                    return;
                };
                opened = d;
                &opened[..]
            }
            None => data,
        };
        let mut pcm = vec![0i16; FRAME * 6];
        let n = match Packet::try_from(data)
            .and_then(|p| MutSignals::try_from(&mut pcm).map(|s| (p, s)))
            .and_then(|(p, s)| self.decoder.decode(Some(p), s, false))
        {
            Ok(n) => n,
            Err(e) => {
                tracing::debug!(error = %e, "call: a packet did not decode");
                return;
            }
        };
        pcm.truncate(n);
        self.heard = Some(now());
        self.friend = if self.deafened { 0 } else { level::level(&pcm) };
        if self.deafened {
            return;
        }
        if let Some(s) = &self.speaker
            && s.try_send(pcm).is_err()
        {
            tracing::trace!("call: the speakers are behind; a frame was dropped");
        }
    }

    /// Permit the addresses of the candidates in a session description.
    fn note_peers(&mut self, sdp: &str) {
        let t = now();
        for line in sdp.lines() {
            if let Some(c) = line.trim().strip_prefix("a=")
                && let Ok(cand) = Candidate::from_sdp_string(c)
            {
                self.permit(cand.addr().ip(), t);
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.speaker = None;
    }
}

/// The address host candidates use: the first given, or the one the system would send from
/// towards the internet (found without sending anything).
fn host_ip(given: Option<&[IpAddr]>) -> Option<IpAddr> {
    if let Some(g) = given {
        return g.first().copied();
    }
    let probe = UdpSocket::bind("0.0.0.0:0").ok()?;
    // TEST-NET-1 (RFC 5737): connecting a UDP socket sends nothing.
    probe.connect("192.0.2.1:9").ok()?;
    let ip = probe.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

/// The UDP socket's reader: every datagram goes to the session.
fn spawn_reader(
    socket: Arc<UdpSocket>,
    tx: Sender<Inbox>,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_millis(200)))?;
    std::thread::Builder::new()
        .name("call-net".into())
        .spawn(move || {
            let mut buf = vec![0u8; 2048];
            while !stop.load(Ordering::Relaxed) {
                match socket.recv_from(&mut buf) {
                    Ok((n, from)) => {
                        let data = buf[..n].to_vec();
                        if tx.send(Inbox::Udp { from, data }).is_err() {
                            return;
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(e) => {
                        tracing::debug!(error = %e, "call: the socket failed");
                        return;
                    }
                }
            }
        })?;
    Ok(())
}
