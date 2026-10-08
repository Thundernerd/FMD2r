//! The smoke list (`fixtures/smoke/list.toml`): representative upstream modules whose HTTP traffic
//! is recorded once, replayed offline in CI and run live every night.

mod report;
mod smoke;

pub use report::{Classified, EntryResult, Results, StepResult, Verdict, classify, render_report};
pub use smoke::{Entry, Smoke, SmokeError, SmokeList, Step, check_output};
