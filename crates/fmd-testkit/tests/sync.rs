//! `scripts/sync-upstream-lua.sh`, run against a local bare repository instead of GitHub.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/sync-upstream-lua.sh")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A bare repo shaped like dazedcat19/FMD2: the Lua tree under `lua/` next to other files.
/// Returns its `file://` URL and the SHA of the `master` commit.
fn upstream(root: &Path) -> (String, String) {
    let work = root.join("work");
    for (path, contents) in [
        ("lua/modules/Alpha.lua", "function Init() end\n"),
        ("lua/modules/Beta.lua", "function Init() end\n"),
        ("lua/templates/Madara.lua", "local _M = {}\nreturn _M\n"),
        ("lua/utils/json.lua", "return {}\n"),
        ("lua/websitebypass/websitebypass.lua", "return {}\n"),
        ("lua/extras/readme.txt", "extras\n"),
        ("baseunits/uBaseUnit.pas", "unit uBaseUnit;\n"),
        ("README.md", "FMD2\n"),
    ] {
        write(&work.join(path), contents);
    }
    git(&work, &["init", "--quiet", "--initial-branch=master"]);
    git(&work, &["add", "."]);
    git(&work, &["commit", "--quiet", "-m", "upstream"]);
    let sha = git(&work, &["rev-parse", "HEAD"]);
    let bare = root.join("FMD2.git");
    git(root, &["clone", "--quiet", "--bare", "work", "FMD2.git"]);
    (format!("file://{}", bare.display()), sha)
}

fn sync(repo: &str, dest: &Path, extra: &[&str]) {
    let out = Command::new("bash")
        .arg(script())
        .args(["--repo", repo, "--dest"])
        .arg(dest)
        .args(extra)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "sync failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn sync_copies_the_lua_tree_and_records_the_commit() {
    let root = tempfile::tempdir().unwrap();
    let (repo, sha) = upstream(root.path());
    let dest = root.path().join("fixtures/lua");

    sync(&repo, &dest, &[]);

    for dir in ["modules", "templates", "utils", "websitebypass", "extras"] {
        assert!(dest.join(dir).is_dir(), "missing {dir}/");
    }
    assert_eq!(
        fs::read_to_string(dest.join("modules/Alpha.lua")).unwrap(),
        "function Init() end\n"
    );
    assert_eq!(
        fs::read_to_string(dest.join("UPSTREAM_REF")).unwrap(),
        format!("{sha}\n")
    );
    assert!(!dest.join("README.md").exists());
    assert!(!dest.join("baseunits").exists());
    assert!(!dest.join("lua").exists());
}

#[test]
fn resync_at_another_ref_replaces_the_previous_tree() {
    let root = tempfile::tempdir().unwrap();
    let (repo, first) = upstream(root.path());
    let work = root.path().join("work");
    git(&work, &["tag", "v1"]);
    fs::remove_file(work.join("lua/modules/Beta.lua")).unwrap();
    write(&work.join("lua/modules/Gamma.lua"), "function Init() end\n");
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "--quiet", "-m", "next"]);
    git(
        &work,
        &["push", "--quiet", "--tags", "../FMD2.git", "master"],
    );
    let dest = root.path().join("fixtures/lua");

    sync(&repo, &dest, &[]);
    assert!(dest.join("modules/Gamma.lua").exists());
    sync(&repo, &dest, &["--ref", "v1"]);

    assert!(dest.join("modules/Beta.lua").exists());
    assert!(!dest.join("modules/Gamma.lua").exists());
    assert_eq!(
        fs::read_to_string(dest.join("UPSTREAM_REF")).unwrap(),
        format!("{first}\n")
    );
}
