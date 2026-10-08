//! Test support for the upstream Lua module corpus (`fixtures/lua`).

mod corpus;

pub use corpus::{CorpusError, CorpusReport, check_each, corpus_root, module_files};
