//! Smoke run results and the nightly report that classifies them.

use serde::{Deserialize, Serialize};

/// The results of one run over the smoke list, live or replayed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Results {
    pub entries: Vec<EntryResult>,
}

/// One entry's outcome: `module info` on its manga URL, `module pages` on its chapter URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryResult {
    pub name: String,
    pub module_id: String,
    pub info: StepResult,
    pub pages: StepResult,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepResult {
    pub passed: bool,
    /// Why the step failed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// What an entry's live and replay outcomes say about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Both runs passed.
    Pass,
    /// The replay passed but the live run failed: the site, or the upstream module, changed.
    SiteChanged,
    /// The replay failed: FMD2r no longer runs the recorded traffic the way it did.
    Regression,
}

/// An entry's verdict.
#[derive(Debug, Clone)]
pub struct Classified {
    pub name: String,
    pub module_id: String,
    pub verdict: Verdict,
    /// The failed steps, replay first, e.g. `live info failed: status 2`.
    pub failures: Vec<String>,
}

impl EntryResult {
    fn passed(&self) -> bool {
        self.info.passed && self.pages.passed
    }

    /// The failed steps, as `<run> <step> failed: <detail>`.
    fn failures(&self, run: &str) -> impl Iterator<Item = String> {
        [("info", &self.info), ("pages", &self.pages)]
            .into_iter()
            .filter(|(_, step)| !step.passed)
            .map(move |(name, step)| format!("{run} {name} failed: {}", step.detail))
    }
}

/// Classifies every replayed entry; an entry missing from `live` counts as a failed live run.
pub fn classify(live: &Results, replay: &Results) -> Vec<Classified> {
    replay
        .entries
        .iter()
        .map(|replayed| {
            let lived = live.entries.iter().find(|e| e.name == replayed.name);
            let live_passed = lived.is_some_and(EntryResult::passed);
            let mut failures: Vec<String> = replayed.failures("replay").collect();
            match lived {
                Some(lived) => failures.extend(lived.failures("live")),
                None => failures.push("live run missing".to_owned()),
            }
            let verdict = match (replayed.passed(), live_passed) {
                (false, _) => Verdict::Regression,
                (true, false) => Verdict::SiteChanged,
                (true, true) => Verdict::Pass,
            };
            Classified {
                name: replayed.name.clone(),
                module_id: replayed.module_id.clone(),
                verdict,
                failures,
            }
        })
        .collect()
}

/// The nightly report, as Markdown.
pub fn render_report(live: &Results, replay: &Results) -> String {
    let classified = classify(live, replay);
    let count = |verdict| classified.iter().filter(|c| c.verdict == verdict).count();
    let mut out = format!(
        "# Smoke list: nightly run\n\n{} entries: {} passed, {} site/module changed, {} FMD2r regressions\n",
        classified.len(),
        count(Verdict::Pass),
        count(Verdict::SiteChanged),
        count(Verdict::Regression),
    );
    let sections = [
        (
            Verdict::Regression,
            "FMD2r regressions",
            "The replay of the recorded traffic failed: a change in FMD2r broke these.",
        ),
        (
            Verdict::SiteChanged,
            "Site/module changed",
            "The replay passed but the live run failed: the site or its upstream module changed. \
             Re-record the entry once the module works again (scripts/smoke-record.sh).",
        ),
    ];
    for (verdict, heading, explanation) in sections {
        out.push_str(&format!("\n## {heading}\n\n{explanation}\n\n"));
        let mut any = false;
        for entry in classified.iter().filter(|c| c.verdict == verdict) {
            any = true;
            out.push_str(&format!(
                "- `{}` (module {}): {}\n",
                entry.name,
                entry.module_id,
                entry.failures.join("; ")
            ));
        }
        if !any {
            out.push_str("None.\n");
        }
    }
    out
}
