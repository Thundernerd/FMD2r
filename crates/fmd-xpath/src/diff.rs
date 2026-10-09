//! The differential runner: every corpus entry evaluated on two backends, their results
//! compared in a normalised form (docs/tickets/T35-xpath-differential-corpus.md).

use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Write};

use crate::corpus::{Corpus, Entry, Origin, Step};
use crate::{Document, Kind, XPathEngine, XPathValue};

/// A value as the differential runner compares it: what a module can observe of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    /// Why the value couldn't be computed (the document didn't parse), if it couldn't.
    pub error: Option<String>,
    /// The number of items.
    pub count: i64,
    /// The whole value's string (`XPathString`).
    pub string: String,
    /// Each item.
    pub items: Vec<NormalizedItem>,
}

/// One item of a [`Normalized`] value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedItem {
    pub kind: Kind,
    pub string: String,
    /// The serialised node (outer HTML); empty for anything but a node.
    pub html: String,
}

impl Normalized {
    /// Normalises `value`.
    pub fn of(value: &dyn XPathValue) -> Normalized {
        let count = value.count();
        Normalized {
            error: None,
            count,
            string: value.string(),
            items: (1..=count)
                .map(|i| {
                    let item = value.get(i);
                    NormalizedItem {
                        kind: item.kind(),
                        string: item.string(),
                        html: item.outer_html(),
                    }
                })
                .collect(),
        }
    }

    fn error(error: String) -> Normalized {
        Normalized {
            error: Some(error),
            count: 0,
            string: String::new(),
            items: Vec::new(),
        }
    }
}

impl fmt::Display for Normalized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(error) = &self.error {
            return writeln!(f, "error: {error}");
        }
        writeln!(f, "count: {}", self.count)?;
        writeln!(f, "string: {:?}", self.string)?;
        for (i, item) in self.items.iter().enumerate() {
            writeln!(f, "[{}] {}: {:?}", i + 1, item.kind, item.string)?;
            if !item.html.is_empty() {
                writeln!(f, "    {}", item.html)?;
            }
        }
        Ok(())
    }
}

/// An entry the two backends disagree on.
#[derive(Debug, Clone)]
pub struct Mismatch {
    pub entry: Entry,
    /// The reference backend's result.
    pub expected: Normalized,
    /// The candidate backend's result.
    pub actual: Normalized,
}

/// The outcome of a [`diff`].
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// The number of entries compared.
    pub entries: usize,
    pub mismatches: Vec<Mismatch>,
}

/// The longest normalised result a report shows; the rest is cut.
const SHOWN: usize = 4000;

impl Report {
    /// The report as Markdown: a summary, then the mismatches grouped by [`feature`].
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "# XPath differential report\n\n{} entries, {} mismatches",
            self.entries,
            self.mismatches.len()
        );
        let mut groups: BTreeMap<String, Vec<&Mismatch>> = BTreeMap::new();
        for mismatch in &self.mismatches {
            groups
                .entry(feature(&mismatch.entry))
                .or_default()
                .push(mismatch);
        }
        for (feature, mismatches) in groups {
            let _ = writeln!(out, "\n## {feature} ({})", mismatches.len());
            for mismatch in mismatches {
                let _ = writeln!(out, "\n### {}\n", mismatch.entry);
                for (label, value) in [
                    ("expected", &mismatch.expected),
                    ("actual", &mismatch.actual),
                ] {
                    let _ = writeln!(out, "{label}:\n```\n{}```", cut(&value.to_string()));
                }
            }
        }
        out
    }
}

/// `text` up to [`SHOWN`] bytes, marked when cut.
fn cut(text: &str) -> String {
    if text.len() <= SHOWN {
        return text.to_owned();
    }
    let mut end = SHOWN;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n... ({} bytes)\n", &text[..end], text.len())
}

/// The expression feature a report groups an entry under: a CSS selector, JSON, the first
/// function it calls, or a plain path.
pub fn feature(entry: &Entry) -> String {
    let expr = &entry.expression;
    if entry.css {
        return "css selector".to_owned();
    }
    if expr.contains("json(") || expr.contains("jn:") || expr.contains('?') {
        return "json".to_owned();
    }
    match first_function(expr) {
        Some(name) => format!("{name}()"),
        None => "path".to_owned(),
    }
}

/// The name of the first function call in `expr`, outside string literals; node tests such as
/// `text()` and keywords such as `if (` don't count.
fn first_function(expr: &str) -> Option<&str> {
    const NOT_FUNCTIONS: [&str; 12] = [
        "text",
        "node",
        "comment",
        "element",
        "attribute",
        "if",
        "and",
        "or",
        "div",
        "mod",
        "in",
        "return",
    ];
    let mut quote = None;
    // The current name's start, and the span of a name only spaces have followed since.
    let mut start = None;
    let mut last = None;
    for (i, c) in expr.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                (start, last) = (None, None);
            }
            c if c.is_alphanumeric() || matches!(c, '-' | '_' | ':') => {
                last = None;
                start.get_or_insert(i);
            }
            ' ' => {
                if let Some(s) = start.take() {
                    last = Some((s, i));
                }
            }
            '(' => {
                if let Some((s, e)) = start.map(|s| (s, i)).or(last) {
                    let name = &expr[s..e];
                    if !NOT_FUNCTIONS.contains(&name)
                        && !name.starts_with(|c: char| c.is_ascii_digit())
                    {
                        return Some(name);
                    }
                }
                (start, last) = (None, None);
            }
            _ => (start, last) = (None, None),
        }
    }
    None
}

/// Evaluates every entry of `corpus` on `reference` and `candidate` and reports the entries
/// whose [`Normalized`] results differ.
pub fn diff(corpus: &Corpus, reference: &dyn XPathEngine, candidate: &dyn XPathEngine) -> Report {
    let reference = Backend::parse(corpus, reference);
    let candidate = Backend::parse(corpus, candidate);
    let mut report = Report::default();
    for entry in corpus.entries() {
        report.entries += 1;
        let expected = reference.evaluate(entry);
        let actual = candidate.evaluate(entry);
        if expected != actual {
            report.mismatches.push(Mismatch {
                entry: entry.clone(),
                expected,
                actual,
            });
        }
    }
    report
}

/// A backend with every corpus document parsed.
struct Backend {
    documents: HashMap<u64, Result<Box<dyn Document>, String>>,
}

impl Backend {
    fn parse(corpus: &Corpus, engine: &dyn XPathEngine) -> Backend {
        Backend {
            documents: corpus
                .documents()
                .map(|(hash, body)| (hash, engine.parse(body).map_err(|e| e.to_string())))
                .collect(),
        }
    }

    fn evaluate(&self, entry: &Entry) -> Normalized {
        match self.eval(entry) {
            Ok(value) => Normalized::of(value.as_ref()),
            Err(e) => Normalized::error(e),
        }
    }

    /// The entry's value, its context rebuilt from its origin.
    fn eval(&self, entry: &Entry) -> Result<Box<dyn XPathValue>, String> {
        let document = match self.documents.get(&entry.document) {
            Some(Ok(document)) => document,
            Some(Err(e)) => return Err(e.clone()),
            None => return Err(format!("no document {:016x}", entry.document)),
        };
        let context = entry
            .context
            .as_deref()
            .map(|origin| self.value(origin))
            .transpose()?;
        Ok(document.eval(&entry.expression, context.as_deref(), entry.css))
    }

    /// The value `origin` describes.
    fn value(&self, origin: &Origin) -> Result<Box<dyn XPathValue>, String> {
        let mut value = self.eval(&origin.entry)?;
        for step in &origin.path {
            value = match step {
                Step::Item(i) => value.get(*i),
                Step::Property(name) => value.property(name),
            };
        }
        Ok(value)
    }
}
