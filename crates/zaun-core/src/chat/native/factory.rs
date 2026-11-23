//! Starts the helper (ADR-0042).

use std::path::{Path, PathBuf};

use zaun_chat_wire::{Hello, encode_line, features_with};

use super::command::{command, env};
use super::engine::NativeEngine;
use super::from_wire::features;
use super::pipe;
use crate::chat::engine::{Engine, EngineConfig, EngineFactory, EventSink};
use crate::error::Result;
use crate::model::ChatFeatures;
use crate::session::{PipedHelper, spawn_piped_helper};

/// The helper's log, inside Zaun's logs folder.
pub const LOG_NAME: &str = "chat-engine.log";
/// Where the helper's standard error goes when there is no log.
const NO_LOG: &str = "/dev/null";

/// Starts `zaun-chat`.
#[derive(Clone, Debug)]
pub struct NativeFactory {
    pub helper: PathBuf,
    /// `bwrap`; `None` = run unsandboxed (development only).
    pub bwrap: Option<PathBuf>,
    /// The helper's log file; `None` = discarded.
    pub log: Option<PathBuf>,
}

impl EngineFactory for NativeFactory {
    fn available(&self) -> bool {
        self.helper.is_file()
    }

    fn features(&self, relays: &[String]) -> ChatFeatures {
        features(features_with(relays))
    }

    fn start(&self, config: EngineConfig, sink: EventSink) -> Result<Box<dyn Engine>> {
        ensure!(self.available(), ChtUnavailable);
        if self.bwrap.is_none() {
            tracing::warn!("chat: bwrap not found; the chat engine runs without a sandbox");
        }
        let (program, args) = command(&self.helper, self.bwrap.as_deref(), &config.dir);
        let child = spawn_piped_helper(&PipedHelper {
            program: &program,
            args: &args,
            env: &env(&config.dir),
            log: self.log.as_deref().unwrap_or(Path::new(NO_LOG)),
        })
        .map_err(|e| err!(ChtUnavailable).with_source(e))?;
        let pid = child.id();
        let hello = Hello::new(config.dir, config.key, config.name, config.relays);
        let hello = encode_line(&hello).map_err(|e| err!(IntTaskFailed).with_source(e))?;
        let pipes = pipe::start(child, hello, sink)?;
        Ok(Box::new(NativeEngine {
            tx: Some(pipes.tx),
            pid,
            exited: pipes.exited,
        }))
    }
}
