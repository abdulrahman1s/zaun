//! `TestEnv`: temporary XDG roots for tests that touch the real filesystem.

use std::ffi::OsString;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use zaun_core::consts::{ENV_CACHE_DIR, ENV_CONFIG_DIR, ENV_DATA_DIR, ENV_STATE_DIR};
use zaun_core::paths::{AppPaths, PathEnv};

/// A temporary home with `config`, `data`, `cache`, `state` and `run` roots. Process
/// environment variables are never modified (that is `unsafe` in edition 2024); use
/// [`TestEnv::path_env`] in-process and [`TestEnv::envs`] for spawned binaries.
pub struct TestEnv {
    root: tempfile::TempDir,
    uid: u32,
}

impl TestEnv {
    /// Create the directories (mode 0700).
    pub fn new() -> std::io::Result<Self> {
        let root = tempfile::Builder::new().prefix("zaun-test-").tempdir()?;
        let uid = std::fs::metadata(root.path())?.uid();
        for d in ["home", "config", "data", "cache", "state", "run"] {
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(root.path().join(d))?;
        }
        Ok(Self { root, uid })
    }

    /// The temporary root.
    pub fn root(&self) -> &Path {
        self.root.path()
    }
    /// `$HOME`.
    pub fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }
    /// `ZAUN_CONFIG_DIR`.
    pub fn config(&self) -> PathBuf {
        self.root.path().join("config")
    }
    /// `ZAUN_DATA_DIR`.
    pub fn data(&self) -> PathBuf {
        self.root.path().join("data")
    }
    /// `ZAUN_CACHE_DIR`.
    pub fn cache(&self) -> PathBuf {
        self.root.path().join("cache")
    }
    /// `ZAUN_STATE_DIR`.
    pub fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }
    /// `XDG_RUNTIME_DIR`.
    pub fn run(&self) -> PathBuf {
        self.root.path().join("run")
    }
    /// The current user ID.
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// Environment variables pointing every root into the temporary directory.
    pub fn envs(&self) -> Vec<(OsString, OsString)> {
        vec![
            ("HOME".into(), self.home().into()),
            ("XDG_RUNTIME_DIR".into(), self.run().into()),
            (ENV_CONFIG_DIR.into(), self.config().into()),
            (ENV_DATA_DIR.into(), self.data().into()),
            (ENV_CACHE_DIR.into(), self.cache().into()),
            (ENV_STATE_DIR.into(), self.state().into()),
        ]
    }

    /// A [`PathEnv`] with [`TestEnv::envs`].
    pub fn path_env(&self) -> PathEnv {
        let mut env = PathEnv::new(self.uid);
        for (k, v) in self.envs() {
            env.set(k, v);
        }
        env
    }

    /// Resolved application paths.
    pub fn paths(&self) -> AppPaths {
        // Every root is absolute by construction.
        AppPaths::resolve(&self.path_env()).unwrap_or_else(|e| panic!("TestEnv paths: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_point_into_the_temp_root() {
        let env = TestEnv::new().unwrap();
        let p = env.paths();
        assert_eq!(p.data_dir, env.data());
        assert_eq!(p.config_dir, env.config());
        assert_eq!(p.runtime_dir, env.run().join("zaun"));
        assert!(!p.runtime_dir_is_fallback);
        assert!(p.db_file.starts_with(env.root()));
        assert!(!p.uses_default_roots(&env.path_env()));
    }
}
