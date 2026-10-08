//! Rebuilds the crate when the web UI build (`web/build`, embedded by rust-embed) appears or
//! changes; rust-embed alone does not notice a folder that was missing at the last build.
//!
//! Also records the git commit as `FMD2R_GIT_REVISION` for `GET /api/about`: taken from that
//! environment variable when set (builds without a `.git`, e.g. Docker), otherwise from git.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../web/build");
    println!("cargo:rerun-if-env-changed=FMD2R_GIT_REVISION");
    let revision = std::env::var("FMD2R_GIT_REVISION")
        .ok()
        .or_else(git_revision);
    println!(
        "cargo:rustc-env=FMD2R_GIT_REVISION={}",
        revision.unwrap_or_default()
    );
}

/// The short hash of `HEAD`, rebuilding when `HEAD` or the branch it points at moves.
fn git_revision() -> Option<String> {
    for path in ["HEAD", "refs/heads", "packed-refs"] {
        let git_path = git(&["rev-parse", "--git-path", path])?;
        println!("cargo:rerun-if-changed={git_path}");
    }
    git(&["rev-parse", "--short=12", "HEAD"])
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
