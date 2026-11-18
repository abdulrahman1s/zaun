//! Change or remove the user's or a group's picture: the file is read, cropped and shrunk on the
//! job pool, then sent in the profile; the snapshot follows the engine's answer (ADR-0038).
use super::state::ChatDone;
use crate::chat::{Request, picture};
use crate::fsx::FsErrorKind;
use crate::model::PictureCrop;
use crate::service::prelude::*;

impl AppService {
    /// `group` = `None` for the user's own picture.
    pub(super) fn chat_set_picture(
        &mut self,
        group: Option<i64>,
        path: PathBuf,
        crop: Option<PictureCrop>,
    ) -> Result<()> {
        self.chat_picture_target(group)?;
        let (fs, post) = (Arc::clone(&self.deps.fs), self.poster());
        let generation = self.flow.chat.generation;
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("chat-picture"),
            move |_| {
                let result = read_picture(&*fs, &path)
                    .and_then(|bytes| picture::make(&bytes, crop))
                    .map(|p| picture_request(group, Some(p)));
                post(Internal::Flow(Box::new(FlowDone::Chat(
                    ChatDone::Prepared { generation, result },
                ))));
            },
        );
        Ok(())
    }

    /// Read a picture file for the crop step and answer `reply_to`.
    pub(super) fn chat_load_picture(&mut self, path: PathBuf, reply_to: QueryId) {
        let (fs, describe) = (Arc::clone(&self.deps.fs), self.describer(None));
        self.answer_on_pool(
            "chat-picture-source",
            reply_to,
            move |_| match read_picture(&*fs, &path).and_then(|b| picture::source(&path, &b)) {
                Ok(source) => QueryResult::PictureSource(source),
                Err(e) => QueryResult::Failed(Box::new(describe(&e))),
            },
        );
    }

    pub(super) fn chat_clear_picture(&mut self, group: Option<i64>) -> Result<()> {
        self.chat_picture_target(group)?;
        self.chat_request(picture_request(group, None));
        Ok(())
    }

    /// The engine runs and the group is known.
    fn chat_picture_target(&self, group: Option<i64>) -> Result<()> {
        let st = &self.flow.chat;
        ensure!(
            st.engine.is_some(),
            ChtRequestFailed,
            detail = "chat is not running"
        );
        if let Some(g) = group {
            ensure!(
                st.snapshot.group(g).is_some(),
                ChtRequestFailed,
                detail = "no such group",
                group = g
            );
        }
        Ok(())
    }
}

fn picture_request(group: Option<i64>, picture: Option<crate::model::Picture>) -> Request {
    match group {
        Some(group) => Request::SetGroupPicture { group, picture },
        None => Request::SetPicture(picture),
    }
}

/// The file's bytes, at most `picture::MAX_SOURCE` (`ZN-CHT-009` beyond).
fn read_picture(fs: &dyn Fs, path: &Path) -> Result<Vec<u8>> {
    fs.read_follow(path, picture::MAX_SOURCE).map_err(|e| {
        if matches!(e.kind, FsErrorKind::TooLarge { .. }) {
            err!(
                ChtPictureTooLarge,
                detail = "the file is larger than 20 MiB"
            )
        } else {
            e.into()
        }
    })
}
