//! The agent's servers, timings and retry intervals (Env/SQLite.hs `defaultAgentConfig`, simplexmq 7.1.4, AGPL-3.0-only).
use std::time::Duration;

use crate::smp::{ConnConfig, SmpServer};

/// `RetryInterval`: the delay stays at `initial` until `increase_after` has passed, then grows by half up to `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryInterval {
    pub initial: Duration,
    pub increase_after: Duration,
    pub max: Duration,
}

#[derive(Debug, Clone)]
pub struct DeliveryConfig {
    /// `messageRetryInterval.riFast`.
    pub fast: RetryInterval,
    /// `riSlow`, used after QUOTA.
    pub slow: RetryInterval,
    /// `messageTimeout`: how long a message is retried after temporary failures.
    pub message_timeout: Duration,
    /// `quotaExceededTimeout`: how long a message waits for room in a full queue.
    pub quota_timeout: Duration,
}

impl Default for DeliveryConfig {
    fn default() -> Self {
        Self {
            fast: RetryInterval {
                initial: Duration::from_secs(2),
                increase_after: Duration::from_secs(10),
                max: Duration::from_secs(120),
            },
            slow: RetryInterval {
                initial: Duration::from_secs(300),
                increase_after: Duration::from_secs(60),
                max: Duration::from_secs(6 * 3600),
            },
            message_timeout: Duration::from_secs(2 * 24 * 3600),
            quota_timeout: Duration::from_secs(7 * 24 * 3600),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Servers our receive queues are created on; the first is used.
    pub servers: Vec<SmpServer>,
    /// The built-in servers, which short links may name by host only.
    pub presets: Vec<SmpServer>,
    pub smp: ConnConfig,
    pub delivery: DeliveryConfig,
    /// How soon a handshake step (NEW, SKEY, LKEY) that failed is sent again while its server is up.
    pub step_retry: Duration,
}

impl AgentConfig {
    #[must_use]
    pub fn new(servers: Vec<SmpServer>) -> Self {
        Self {
            servers,
            presets: Vec::new(),
            smp: ConnConfig::default(),
            delivery: DeliveryConfig::default(),
            step_retry: Duration::from_secs(5),
        }
    }
}

pub(crate) fn ms(d: Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}
