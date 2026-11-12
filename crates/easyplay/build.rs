//! Embed the commit for `easyplay --version` (RELEASE_STRATEGY §2.1: version and commit only,
//! no timestamps). `EASYPLAY_COMMIT` overrides detection (Nix and distribution builds without
//! a `.git` directory); otherwise `git rev-parse` is asked; otherwise `unknown`.

fn main() {
    println!("cargo:rerun-if-env-changed=EASYPLAY_COMMIT");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");
    let commit = std::env::var("EASYPLAY_COMMIT")
        .ok()
        .filter(|c| !c.trim().is_empty())
        .or_else(git_commit)
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=EASYPLAY_COMMIT={}", commit.trim());
}

#[allow(clippy::disallowed_methods)] // Build scripts may run git.
fn git_commit() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim();
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_owned())
}
