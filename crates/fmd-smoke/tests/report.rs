//! The nightly report: live and replay results in, a classification per entry out.
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_smoke::{Verdict, classify, render_report};

fn results(json: &str) -> fmd_smoke::Results {
    serde_json::from_str(json).unwrap()
}

/// One entry per outcome combination: both pass, live fails only, replay fails only, both fail.
const LIVE: &str = r#"{ "entries": [
    { "name": "ok",       "module_id": "1", "info": { "passed": true },  "pages": { "passed": true } },
    { "name": "site",     "module_id": "2", "info": { "passed": false, "detail": "status 2" }, "pages": { "passed": true } },
    { "name": "ours",     "module_id": "3", "info": { "passed": true },  "pages": { "passed": true } },
    { "name": "both",     "module_id": "4", "info": { "passed": true },  "pages": { "passed": false, "detail": "no pages" } }
] }"#;
const REPLAY: &str = r#"{ "entries": [
    { "name": "ok",       "module_id": "1", "info": { "passed": true },  "pages": { "passed": true } },
    { "name": "site",     "module_id": "2", "info": { "passed": true },  "pages": { "passed": true } },
    { "name": "ours",     "module_id": "3", "info": { "passed": true },  "pages": { "passed": false, "detail": "snapshot differs" } },
    { "name": "both",     "module_id": "4", "info": { "passed": true },  "pages": { "passed": false, "detail": "snapshot differs" } }
] }"#;

fn verdict_of(name: &str) -> Verdict {
    classify(&results(LIVE), &results(REPLAY))
        .into_iter()
        .find(|c| c.name == name)
        .unwrap()
        .verdict
}

#[test]
fn passing_live_and_replay_is_a_pass() {
    assert_eq!(verdict_of("ok"), Verdict::Pass);
}

#[test]
fn failing_live_with_a_passing_replay_is_a_site_or_module_change() {
    assert_eq!(verdict_of("site"), Verdict::SiteChanged);
}

#[test]
fn failing_replay_is_an_fmd2r_regression() {
    assert_eq!(verdict_of("ours"), Verdict::Regression);
    assert_eq!(verdict_of("both"), Verdict::Regression);
}

/// The list items of `report` under the `## <heading>` section, up to the next section.
fn section<'a>(report: &'a str, heading: &str) -> Vec<&'a str> {
    report
        .lines()
        .skip_while(|l| !l.starts_with(&format!("## {heading}")))
        .skip(1)
        .take_while(|l| !l.starts_with("## "))
        .filter(|l| l.starts_with("- "))
        .collect()
}

#[test]
fn the_report_counts_each_verdict() {
    let report = render_report(&results(LIVE), &results(REPLAY));
    assert!(
        report.contains("4 entries: 1 passed, 1 site/module changed, 2 FMD2r regressions"),
        "{report}"
    );
}

#[test]
fn the_report_lists_failures_under_their_verdict_with_the_failing_step() {
    let report = render_report(&results(LIVE), &results(REPLAY));
    assert_eq!(
        section(&report, "FMD2r regressions"),
        [
            "- `ours` (module 3): replay pages failed: snapshot differs",
            "- `both` (module 4): replay pages failed: snapshot differs; live pages failed: no pages",
        ],
        "{report}"
    );
    assert_eq!(
        section(&report, "Site/module changed"),
        ["- `site` (module 2): live info failed: status 2"],
        "{report}"
    );
}

#[test]
fn an_entry_missing_from_the_live_run_counts_as_failing_live() {
    let live = results(r#"{ "entries": [] }"#);
    let classified = classify(&live, &results(REPLAY));
    let ok = classified.iter().find(|c| c.name == "ok").unwrap();
    assert_eq!(ok.verdict, Verdict::SiteChanged);
    assert_eq!(ok.failures, ["live run missing"]);
}
