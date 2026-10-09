//! `fmd2r xpath eval` (docs/tickets/T35-xpath-differential-corpus.md, "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;

const PAGE: &str =
    r#"<ul><li><a href="/1" title="first">One</a></li><li><a href="/2">Two</a></li></ul>"#;

fn page(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("page.html");
    std::fs::write(&path, PAGE).unwrap();
    path
}

fn eval(backend: &str, file: &Path, expr: &str) -> String {
    let out = Command::cargo_bin("fmd2r")
        .unwrap()
        .args(["xpath", "eval", "--backend", backend])
        .arg(file)
        .arg(expr)
        .assert()
        .success();
    String::from_utf8(out.get_output().stdout.clone()).unwrap()
}

#[test]
fn eval_prints_the_normalised_result() {
    let dir = tempfile::tempdir().unwrap();
    let file = page(dir.path());

    // As FMD2's engine prints it: a sequence's string joins its items with nothing between.
    assert_eq!(
        eval("native", &file, "//a"),
        "count: 2\n\
         string: \"OneTwo\"\n\
         [1] node: \"One\"\n    <a href=\"/1\" title=\"first\">One</a>\n\
         [2] node: \"Two\"\n    <a href=\"/2\">Two</a>\n"
    );
    assert_eq!(
        eval("native", &file, "count(//li)"),
        "count: 1\nstring: \"2\"\n[1] int64: \"2\"\n"
    );
}

#[test]
fn eval_runs_css_selectors() {
    let dir = tempfile::tempdir().unwrap();
    let file = page(dir.path());

    Command::cargo_bin("fmd2r")
        .unwrap()
        .args(["xpath", "eval", "--css", "--backend", "native"])
        .arg(&file)
        .arg("li > a[title]")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("count: 1\n"));
}

#[cfg(feature = "xpath-fpc")]
#[test]
fn eval_prints_the_same_on_both_backends() {
    let dir = tempfile::tempdir().unwrap();
    let file = page(dir.path());

    for expr in [
        "//a",
        "//a/@href",
        "count(//li)",
        "string-join(//a, '|')",
        "//missing",
    ] {
        assert_eq!(
            eval("fpc", &file, expr),
            eval("native", &file, expr),
            "{expr}"
        );
    }
}

#[cfg(not(feature = "xpath-fpc"))]
#[test]
fn eval_on_a_backend_left_out_of_the_build_fails() {
    let dir = tempfile::tempdir().unwrap();
    let file = page(dir.path());

    Command::cargo_bin("fmd2r")
        .unwrap()
        .args(["xpath", "eval", "--backend", "fpc"])
        .arg(&file)
        .arg("//a")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not built in"));
}
