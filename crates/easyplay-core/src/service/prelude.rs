//! What every flow file imports: `use crate::service::prelude::*;` (ADR-0034).

#![allow(
    unused_imports,
    reason = "a prelude offers more than any one file uses"
)]

pub(crate) use std::sync::Arc;

pub(crate) use super::app_service::{AppService, Internal};
pub(crate) use super::command::{
    AfterCreate, ChatCommand, Command, GameEdit, LaunchOrigin, OpenOrigin, SteamCommand, StopMode,
};
pub(crate) use super::event::{Event, Notice, QueryResult};
pub(crate) use super::flows::FlowDone;
pub(crate) use super::prompts::Continuation;
pub(crate) use crate::error::{Error, ErrorCode, Result};
pub(crate) use crate::ids::{ExecutableId, GameId, LaunchId, PromptId, QueryId, SessionId};
pub(crate) use crate::jobs::{JobLabel, JobPriority};
pub(crate) use crate::model::{PromptAnswer, PromptKind};
pub(crate) use crate::{bail, ensure, err};
