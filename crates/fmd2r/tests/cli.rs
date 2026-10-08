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

#[test]
fn import_reads_fmd2_userdata_into_the_data_dir_and_prints_the_report() {
    let userdata = tempfile::tempdir().unwrap();
    std::fs::write(
        userdata.path().join("settings.json"),
        r#"{"saveto":{"SaveTo":"C:\\Manga"},"general":{"OneInstanceOnly":true}}"#,
    )
    .unwrap();
    let data = tempfile::tempdir().unwrap();
    let import = |extra: &[&str]| {
        let mut cmd = fmd2r();
        cmd.args(["import", "--from"])
            .arg(userdata.path())
            .arg("--data-dir")
            .arg(data.path())
            .args(["--map-path", "C:\\Manga=/data/manga"])
            .args(extra);
        cmd.assert().success()
    };

    import(&["--dry-run"])
        .stdout(predicate::str::contains("Dry run"))
        .stdout(predicate::str::contains(
            "settings.json → settings: 1 imported",
        ));
    import(&[])
        .stdout(predicate::str::contains(
            "settings.json → settings: 1 imported",
        ))
        .stdout(predicate::str::contains("general/OneInstanceOnly = true"))
        .stdout(predicate::str::contains("downloads.db → tasks: not found"));
    import(&[]).stdout(predicate::str::contains(
        "settings.json → settings: 0 imported, 1 already existing",
    ));
    assert!(data.path().join("app.db").exists());
}

#[test]
fn import_rejects_a_malformed_path_map() {
    fmd2r()
        .args(["import", "--from", ".", "--map-path", "nothing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("expected FROM=TO"));
}
