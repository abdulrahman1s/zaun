//! `OpenFolder`: only folders Zaun knows (DESKTOP_INTEGRATION §8).
use super::DesktopDone;
use crate::desktop::KnownPath;
use crate::model::SaveDataLocation;
use crate::service::command::FolderRequest;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn open_folder(&mut self, target: FolderRequest) -> Result<()> {
        let path = self.folder_path(&target)?;
        let opener = self.desktop().map(|d| Arc::clone(&d.opener));
        let fs = Arc::clone(&self.deps.fs);
        self.desktop_job("desktop-open", move |_| {
            if !fs.metadata_follow(&path).is_ok_and(|m| m.is_dir()) {
                return DesktopDone::Failed(err!(FsReadDenied, path = path.display()));
            }
            match opener.map(|o| o.open(&KnownPath::folder(path))) {
                Some(Err(e)) => DesktopDone::Failed(e),
                _ => DesktopDone::Done,
            }
        })
    }

    fn folder_path(&mut self, target: &FolderRequest) -> Result<PathBuf> {
        Ok(match target {
            FolderRequest::DataFolder => self.deps.paths.data_dir.clone(),
            FolderRequest::Backups => self.deps.paths.backups_dir.clone(),
            FolderRequest::Session(s) => self.deps.paths.session_dir(*s),
            FolderRequest::Prefix(id) => self.deps.paths.prefix_dir(*id),
            FolderRequest::GameFolder(id) => {
                let g = self.lib.game(*id)?;
                match g.root {
                    Some(r) => r,
                    None => g
                        .primary()
                        .and_then(|e| e.path.parent().map(PathBuf::from))
                        .ok_or_else(|| err!(FsReadDenied))?,
                }
            }
            FolderRequest::SaveData(id) => {
                let g = self.lib.game(*id)?;
                match &g.policy.save_data {
                    SaveDataLocation::External { dir } => dir.clone(),
                    SaveDataLocation::Internal => self.internal_save_dir(*id),
                }
            }
            FolderRequest::Granted(p) => {
                ensure!(
                    self.is_granted(p),
                    FsReadDenied,
                    detail = "not a folder Zaun manages"
                );
                p.clone()
            }
        })
    }

    /// The Wine user folder inside the prefix (`steamuser` when it exists).
    fn internal_save_dir(&self, id: GameId) -> PathBuf {
        let users = self.deps.paths.prefix_dir(id).join("drive_c/users");
        let user = users.join("steamuser");
        if self.deps.fs.exists(&user) {
            user
        } else {
            users
        }
    }

    /// Only a folder some game was granted (the user chose it).
    fn is_granted(&mut self, p: &PathBuf) -> bool {
        let snap = self.lib.snapshot();
        let ids: Vec<GameId> = snap.games.iter().map(|g| g.id).collect();
        ids.into_iter().any(|id| {
            self.lib.game(id).is_ok_and(|g| {
                g.policy
                    .folders
                    .iter()
                    .any(|f| f.path == *p || f.canonical == *p)
            })
        })
    }
}
