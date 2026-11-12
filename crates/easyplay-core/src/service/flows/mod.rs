//! The flows of LOGIC_SPECS, as `impl AppService` blocks on the service thread. Slow steps
//! run on the job pool and re-enter as internal messages; prompts park the continuation in
//! [`super::prompts`].

pub mod artwork;
pub mod desktop;
pub mod launch;
pub mod open;
pub mod remove;
pub mod runtime;
pub mod setup;

use super::app_service::AppService;
use super::event::Event;
use super::prompts::Continuation;
use crate::ids::PromptId;
use crate::model::PromptAnswer;

pub use launch::not_installed;

impl AppService {
    /// `AnswerPrompt`: resume the parked flow (a stale or foreign answer is ignored).
    pub(crate) fn answer_prompt(&mut self, prompt: PromptId, answer: PromptAnswer) {
        let Some(parked) = self.prompts.take(prompt, &answer) else {
            tracing::info!(%prompt, "answer for an unknown prompt or not one of its choices");
            return;
        };
        match parked.then {
            c @ (Continuation::OpenChanged { .. } | Continuation::OpenAttach { .. }) => {
                self.resume_open(c, &answer);
            }
            c @ (Continuation::LaunchExeChanged { .. } | Continuation::LaunchRuntime { .. }) => {
                self.resume_launch(c, &answer);
            }
            c @ Continuation::RemoveRunning { .. } => self.resume_remove(c, &answer),
            Continuation::Nothing => {}
        }
        let _ = Event::LibraryChanged;
    }
}
