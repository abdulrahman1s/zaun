//! Timings and limits of one connection (nook smp/client.ts `SmpClientOptions`; Env/SQLite.hs reconnect interval 2 s to 180 s).
use std::time::Duration;

use crate::encoding::{Version, VersionRange};

use super::version::{MAX_VERSION, MIN_VERSION};

#[derive(Debug, Clone)]
pub struct ConnConfig {
    /// How long a command waits for its answer.
    pub command_timeout: Duration,
    /// Idle time between keepalive PINGs; zero turns them off.
    pub ping_interval: Duration,
    /// Bounds TCP, TLS and the SMP handshake together.
    pub connect_timeout: Duration,
    /// Consecutive silent timeouts that make the connection give up.
    pub max_timeouts: u32,
    /// A timeout this much later than scheduled means the process was suspended, so probe instead of judging.
    pub late_after: Duration,
    /// Commands waiting for an answer; more are refused.
    pub max_pending: usize,
    /// Commands queued for the writer; more are refused with `Busy`.
    pub queue_capacity: usize,
    pub reconnect_min: Duration,
    pub reconnect_max: Duration,
    pub versions: VersionRange,
    /// Send our DH key so blocks are sbc-chain encrypted, as the Haskell client does.
    pub block_encryption: bool,
}

impl Default for ConnConfig {
    fn default() -> Self {
        Self {
            command_timeout: Duration::from_secs(15),
            ping_interval: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(20),
            max_timeouts: 2,
            late_after: Duration::from_secs(5),
            max_pending: 1024,
            queue_capacity: 256,
            reconnect_min: Duration::from_secs(2),
            reconnect_max: Duration::from_secs(180),
            versions: VersionRange::new(Version(MIN_VERSION), Version(MAX_VERSION))
                .unwrap_or_else(|| VersionRange::single(Version(MAX_VERSION))),
            block_encryption: true,
        }
    }
}
