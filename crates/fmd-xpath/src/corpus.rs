//! The XPath differential corpus (docs/plan.md, "XPath differential tests"): the expressions
//! modules evaluated during smoke runs, with the documents they ran against.
//!
//! A corpus directory holds `entries.jsonl`, one [`Entry`] per line, and `documents/`, each
//! document body once as `<hash>.html`, named by its [`document_hash`](crate::document_hash) in
//! 16 hex digits. A [`CorpusWriter`] records into one through a [`LoggingEngine`](crate::LoggingEngine)
//! hook; [`Corpus::load`] reads it back for [`diff`](crate::diff).

use std::collections::{HashMap, HashSet, hash_map};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::Query;

const ENTRIES: &str = "entries.jsonl";
const DOCUMENTS: &str = "documents";

/// Failure to read or write a corpus.
#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    #[error("cannot access {}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("{}:{line}: {source}", path.display())]
    Parse {
        path: PathBuf,
        line: usize,
        source: serde_json::Error,
    },
    #[error("cannot serialize an entry: {0}")]
    Serialize(serde_json::Error),
}

/// One evaluation: an expression run against a document, or against a value an earlier
/// evaluation produced.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Entry {
    /// The document's [`document_hash`](crate::document_hash).
    #[serde(with = "hex_hash")]
    pub document: u64,
    /// The expression or CSS selector.
    pub expression: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub css: bool,
    /// Where the context value came from; the document itself when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Box<Origin>>,
}

/// Where a value came from: an evaluation's result, then the items and properties taken from it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Origin {
    #[serde(flatten)]
    pub entry: Entry,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<Step>,
}

/// One step from a value to another.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    /// `get(i)`: the 1-based item.
    Item(i64),
    /// `property(name)`.
    Property(String),
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.entry)?;
        for step in &self.path {
            match step {
                Step::Item(i) => write!(f, "[{i}]")?,
                Step::Property(name) => write!(f, ".{name}")?,
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = if self.css { "css" } else { "xpath" };
        write!(f, "{kind} `{}` on {:016x}", self.expression, self.document)?;
        if let Some(context) = &self.context {
            write!(f, " in ({context})")?;
        }
        Ok(())
    }
}

impl Entry {
    /// Every document the entry needs: its own and its contexts'.
    fn documents(&self) -> Vec<u64> {
        let mut documents = vec![self.document];
        let mut context = self.context.as_deref();
        while let Some(origin) = context {
            documents.push(origin.entry.document);
            context = origin.entry.context.as_deref();
        }
        documents
    }
}

fn is_false(b: &bool) -> bool {
    !b
}

/// A document hash as 16 hex digits.
mod hex_hash {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(hash: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{hash:016x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let s = String::deserialize(d)?;
        u64::from_str_radix(&s, 16).map_err(D::Error::custom)
    }
}

fn document_path(dir: &Path, hash: u64) -> PathBuf {
    dir.join(DOCUMENTS).join(format!("{hash:016x}.html"))
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> CorpusError + use<> {
    let path = path.to_owned();
    move |source| CorpusError::Io { path, source }
}

/// A corpus read back: its entries, each once, and the documents they need.
#[derive(Debug, Default)]
pub struct Corpus {
    entries: Vec<Entry>,
    documents: HashMap<u64, Vec<u8>>,
}

impl Corpus {
    /// Reads the corpus in `dir`. A missing directory or entries file is an empty corpus; a
    /// document an entry names that isn't there is an error.
    pub fn load(dir: &Path) -> Result<Corpus, CorpusError> {
        let path = dir.join(ENTRIES);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(source) => return Err(CorpusError::Io { path, source }),
        };
        let mut corpus = Corpus::default();
        let mut seen = HashSet::new();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: Entry = serde_json::from_str(line).map_err(|source| CorpusError::Parse {
                path: path.clone(),
                line: i + 1,
                source,
            })?;
            if !seen.insert(entry.clone()) {
                continue;
            }
            for hash in entry.documents() {
                if let hash_map::Entry::Vacant(slot) = corpus.documents.entry(hash) {
                    let path = document_path(dir, hash);
                    slot.insert(fs::read(&path).map_err(io_error(&path))?);
                }
            }
            corpus.entries.push(entry);
        }
        Ok(corpus)
    }

    /// The entries, in the order they were recorded, each once.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn document(&self, hash: u64) -> Option<&[u8]> {
        self.documents.get(&hash).map(Vec::as_slice)
    }

    pub fn documents(&self) -> impl Iterator<Item = (u64, &[u8])> {
        self.documents
            .iter()
            .map(|(hash, body)| (*hash, body.as_slice()))
    }
}

/// Records evaluations into a corpus directory, skipping entries and documents already there.
/// Clones share one writer, so every worker thread can record through it.
#[derive(Clone)]
pub struct CorpusWriter {
    state: Arc<Mutex<WriterState>>,
}

struct WriterState {
    dir: PathBuf,
    entries: BufWriter<File>,
    seen: HashSet<Entry>,
    documents: HashSet<u64>,
    /// The first error met while recording; [`CorpusWriter::finish`] returns it.
    error: Option<CorpusError>,
}

impl CorpusWriter {
    /// Opens the corpus in `dir`, creating it if needed.
    pub fn open(dir: impl Into<PathBuf>) -> Result<CorpusWriter, CorpusError> {
        let dir = dir.into();
        let documents = dir.join(DOCUMENTS);
        fs::create_dir_all(&documents).map_err(io_error(&documents))?;
        let existing = Corpus::load(&dir)?;
        let path = dir.join(ENTRIES);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(io_error(&path))?;
        Ok(CorpusWriter {
            state: Arc::new(Mutex::new(WriterState {
                documents: existing.documents.keys().copied().collect(),
                seen: existing.entries.into_iter().collect(),
                entries: BufWriter::new(file),
                dir,
                error: None,
            })),
        })
    }

    /// A [`LoggingEngine`](crate::LoggingEngine) hook recording every evaluation.
    pub fn hook(&self) -> impl Fn(&Query) + 'static {
        let writer = self.clone();
        move |query| writer.record(query)
    }

    /// Records one evaluation. A failure doesn't stop the caller; [`finish`](Self::finish)
    /// reports it.
    pub fn record(&self, query: &Query) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.error.is_some() {
            return;
        }
        if let Err(e) = state.record(query) {
            state.error = Some(e);
        }
    }

    /// Flushes what was recorded, or returns the first error met recording it.
    pub fn finish(&self) -> Result<(), CorpusError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = state.error.take() {
            return Err(e);
        }
        let path = state.dir.join(ENTRIES);
        state.entries.flush().map_err(io_error(&path))
    }
}

impl WriterState {
    fn record(&mut self, query: &Query) -> Result<(), CorpusError> {
        if self.documents.insert(query.document_hash) {
            let path = document_path(&self.dir, query.document_hash);
            fs::write(&path, query.document).map_err(io_error(&path))?;
        }
        let entry = Entry {
            document: query.document_hash,
            expression: query.expression.to_owned(),
            css: query.css,
            context: query.context.map(|origin| Box::new(origin.clone())),
        };
        if self.seen.contains(&entry) {
            return Ok(());
        }
        let path = self.dir.join(ENTRIES);
        let line = serde_json::to_string(&entry).map_err(CorpusError::Serialize)?;
        writeln!(self.entries, "{line}").map_err(io_error(&path))?;
        self.seen.insert(entry);
        Ok(())
    }
}

impl Drop for WriterState {
    fn drop(&mut self) {
        // Best effort: `finish` is where flush errors are reported.
        let _ = self.entries.flush();
    }
}
