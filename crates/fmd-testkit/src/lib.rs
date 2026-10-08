//! Test support for the upstream Lua module corpus (`fixtures/lua`).

mod corpus;
mod scan;

pub use corpus::{CorpusError, CorpusReport, check_each, corpus_root, module_files};
pub use scan::scan_host_api_names;
