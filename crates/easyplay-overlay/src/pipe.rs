//! The pipes to EasyPlay (ADR-0037 §3): a thread reads standard input line by line into the
//! event loop; messages to the app are written to standard output from the loop.
use std::io::{BufRead, Write};

use easyplay_core::chat::overlay::OverlaySnapshot;
use easyplay_core::chat::overlay::protocol::{self, FromOverlay, ToOverlay};
use smithay_client_toolkit::reexports::calloop::LoopHandle;
use smithay_client_toolkit::reexports::calloop::channel::{self, Event};

use crate::app::App;
use crate::look::Look;
use crate::model::Focus;

/// Start reading standard input. End of input ends the overlay (EasyPlay stopped it).
pub fn listen(handle: &LoopHandle<'static, App>) -> Result<(), String> {
    let (tx, rx) = channel::channel::<String>();
    std::thread::Builder::new()
        .name("overlay-stdin".into())
        .spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    handle
        .insert_source(rx, |event, (), app| match event {
            Event::Msg(line) => app.on_line(&line),
            Event::Closed => app.exit = Some(0),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Tell the app.
pub fn say(msg: &FromOverlay) {
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{}", protocol::encode(msg))
        .and_then(|()| out.flush())
        .is_err()
    {
        // EasyPlay closed our output; it closes the input too, which ends the loop.
        tracing::debug!("overlay: output closed");
    }
}

impl App {
    fn on_line(&mut self, line: &str) {
        let msg = match protocol::decode::<ToOverlay>(line) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "overlay: line dropped");
                return;
            }
        };
        match msg {
            ToOverlay::State { state } => self.take_state(*state),
            ToOverlay::Open => self.set_open(true),
            ToOverlay::Close => self.set_open(false),
        }
        self.dirty = true;
    }

    fn take_state(&mut self, state: OverlaySnapshot) {
        crate::words::set_language(&state.look);
        let look = Look::new(&state.look.accent, state.look.high_contrast);
        if look != self.model.look || !self.model.has_state {
            self.model.look = look;
            crate::style::apply(&self.egui, &look);
        }
        self.model.pictures.update(&self.egui, &state);
        let rings_now = state.ringing.is_some() && self.model.snap.ringing.is_none();
        if rings_now && self.model.open.is_on() {
            self.model.focus = Some(Focus::Answer);
        }
        self.model.snap = std::sync::Arc::new(state);
        self.model.rev += 1;
        self.model.has_state = true;
    }

    /// Open or close the panel (from the app, or Escape inside).
    pub fn set_open(&mut self, open: bool) {
        let now = self.now();
        self.model.open.set(open, now);
        if open {
            self.model.focus = Some(if self.model.snap.ringing.is_some() {
                Focus::Answer
            } else {
                Focus::Composer
            });
        } else {
            self.model.pins = false;
            self.model.corner_menu = false;
        }
        crate::layer::set_input(&self.layer, &self.compositor, open);
        self.dirty = true;
    }
}
