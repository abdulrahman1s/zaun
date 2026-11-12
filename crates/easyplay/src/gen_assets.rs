//! `easyplay generate-assets --out DIR` (hidden): the man pages and shell completions,
//! generated from the `clap` definitions (CLI-004, RELEASE_STRATEGY §7). Packages call it at
//! build time and install the result:
//!
//! ```text
//! DIR/man/man1/easyplay.1, easyplay-<verb>.1 …     → share/man/man1/
//! DIR/completions/easyplay.bash                    → share/bash-completion/completions/easyplay
//! DIR/completions/_easyplay                        → share/zsh/site-functions/_easyplay
//! DIR/completions/easyplay.fish                    → share/fish/vendor_completions.d/easyplay.fish
//! ```

use std::path::Path;

use clap::CommandFactory;
use clap_complete::Shell;
use easyplay_core::{Error, ErrorCode};

use crate::cli::{Cli, exit};

fn io(e: std::io::Error) -> Error {
    Error::new(ErrorCode::FsWriteDenied).with_source(e)
}

/// The command definition as shipped (hidden verbs stay out of the pages).
fn command() -> clap::Command {
    Cli::command().version(env!("CARGO_PKG_VERSION"))
}

/// Write everything under `out`.
#[allow(clippy::print_stdout)]
pub fn run(out: &Path) -> Result<u8, Error> {
    let man = out.join("man/man1");
    let comp = out.join("completions");
    std::fs::create_dir_all(&man).map_err(io)?;
    std::fs::create_dir_all(&comp).map_err(io)?;
    clap_mangen::generate_to(command(), &man).map_err(io)?;
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        let mut cmd = command();
        let p = clap_complete::generate_to(shell, &mut cmd, "easyplay", &comp).map_err(io)?;
        println!("{}", p.display());
    }
    println!("{}", man.display());
    Ok(exit::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_pages_and_completions() {
        let tmp = tempfile::tempdir().unwrap();
        run(tmp.path()).unwrap();
        let page = std::fs::read_to_string(tmp.path().join("man/man1/easyplay.1")).unwrap();
        assert!(page.contains(".TH easyplay 1"), "{page}");
        assert!(page.contains("launch"));
        assert!(!page.contains("supervise"));
        assert!(tmp.path().join("man/man1/easyplay-launch.1").exists());
        assert!(!tmp.path().join("man/man1/easyplay-supervise.1").exists());
        for f in ["easyplay.bash", "_easyplay", "easyplay.fish"] {
            let t = std::fs::read_to_string(tmp.path().join("completions").join(f)).unwrap();
            assert!(t.contains("sandbox"), "{f}");
        }
    }
}
