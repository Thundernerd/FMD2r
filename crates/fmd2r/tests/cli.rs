// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn fmd2r() -> Command {
    Command::cargo_bin("fmd2r").unwrap()
}

#[test]
fn help_lists_the_top_level_subcommands() {
    fmd2r()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("serve"))
        .stdout(predicate::str::contains("module"))
        .stdout(predicate::str::contains("xpath"));
}

#[test]
fn version_prints_the_crate_version() {
    fmd2r()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn module_init_is_not_implemented_yet() {
    fmd2r()
        .args(["module", "init"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not implemented yet (T15)"))
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn openapi_exports_the_server_document_to_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("openapi.json");
    fmd2r()
        .args(["openapi", "--out"])
        .arg(&out)
        .assert()
        .success();
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert!(doc["openapi"].as_str().unwrap().starts_with("3.1"));
    assert!(doc["paths"]["/api/events"]["get"].is_object());
}
