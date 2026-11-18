//! Why an update step failed, and its `ZN-UPD` code (ERROR_HANDLING §3).

use crate::artwork::http::HttpError;
use crate::error::{Error, ErrorCode};
use crate::fsx::FsError;

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("update checks are off")]
    ChecksOff,
    #[error("this build has no update key")]
    NotConfigured,
    #[error("the update server could not be reached: {0}")]
    Unreachable(String),
    #[error("the release manifest is not valid: {0}")]
    BadManifest(String),
    #[error("the release signature does not verify")]
    BadSignature,
    #[error("{file}: {why}")]
    BadFile { file: String, why: String },
    #[error("{offered} is not newer than {running}")]
    NotNewer { offered: String, running: String },
    #[error("this copy is not updated by Zaun itself")]
    Managed,
    #[error("a game or tool is running")]
    GameRunning,
    #[error("installing failed: {0}")]
    Install(String),
    #[error("{0} did not start and was rolled back")]
    RolledBack(String),
}

impl UpdateError {
    pub fn code(&self) -> ErrorCode {
        match self {
            UpdateError::ChecksOff => ErrorCode::UpdChecksOff,
            UpdateError::NotConfigured => ErrorCode::UpdNotConfigured,
            UpdateError::Unreachable(_) => ErrorCode::UpdUnreachable,
            UpdateError::BadManifest(_) | UpdateError::BadSignature => ErrorCode::UpdBadSignature,
            UpdateError::BadFile { .. } => ErrorCode::UpdBadFile,
            UpdateError::NotNewer { .. } => ErrorCode::UpdNotNewer,
            UpdateError::Managed => ErrorCode::UpdManaged,
            UpdateError::GameRunning => ErrorCode::UpdGameRunning,
            UpdateError::Install(_) => ErrorCode::UpdInstallFailed,
            UpdateError::RolledBack(_) => ErrorCode::UpdRolledBack,
        }
    }

    pub(super) fn bad_file(file: &str, why: impl Into<String>) -> Self {
        UpdateError::BadFile {
            file: file.to_owned(),
            why: why.into(),
        }
    }
}

impl From<UpdateError> for Error {
    fn from(e: UpdateError) -> Self {
        let code = e.code();
        Error::new(code).with("detail", e.to_string())
    }
}

impl From<HttpError> for UpdateError {
    fn from(e: HttpError) -> Self {
        UpdateError::Unreachable(e.to_string())
    }
}

impl From<FsError> for UpdateError {
    fn from(e: FsError) -> Self {
        UpdateError::Install(e.to_string())
    }
}
