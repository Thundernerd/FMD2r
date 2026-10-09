//! The differential runner (docs/tickets/T35-xpath-differential-corpus.md, "Seams under test"):
//! a corpus recorded through the logging hook, replayed on two backends.

#![cfg(feature = "native")]
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::any::Any;
use std::path::Path;

use fmd_xpath::corpus::{Corpus, CorpusWriter};
use fmd_xpath::native::NativeEngine;
use fmd_xpath::{Document, Kind, LoggingEngine, XPathEngine, XPathValue, diff, document_hash};

const PAGE: &[u8] = b"<ul><li><a href='/1'>One</a></li><li><a href='/2'>Two</a></li></ul><b>x</b>";

/// Records a small corpus into `dir`, as a smoke run's module would: plain expressions, a CSS
/// selector, and expressions run against items of an earlier result.
fn record(dir: &Path) {
    let writer = CorpusWriter::open(dir).unwrap();
    let engine = LoggingEngine::new(NativeEngine, writer.hook());
    let doc = engine.parse(PAGE).unwrap();
    doc.eval("//b", None, false);
    doc.eval("li a", None, true);
    let items = doc.eval("//li", None, false);
    for i in 1..=items.count() {
        doc.eval("a/@href", Some(items.get(i).as_ref()), false);
    }
    // The same expression again is the same entry.
    doc.eval("//b", None, false);
    writer.finish().unwrap();
}

#[test]
fn a_corpus_both_backends_agree_on_passes() {
    let dir = tempfile::tempdir().unwrap();
    record(dir.path());

    let corpus = Corpus::load(dir.path()).unwrap();
    assert_eq!(corpus.entries().len(), 5);
    let report = diff(&corpus, &NativeEngine, &NativeEngine);
    assert_eq!(report.entries, 5);
    assert!(report.mismatches.is_empty(), "{}", report.render());
}

#[test]
fn a_backend_that_differs_on_one_entry_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    record(dir.path());

    let corpus = Corpus::load(dir.path()).unwrap();
    let report = diff(&corpus, &NativeEngine, &Differs("//b"));
    assert_eq!(report.mismatches.len(), 1);
    let mismatch = &report.mismatches[0];
    assert_eq!(mismatch.entry.expression, "//b");
    assert_eq!(mismatch.entry.document, document_hash(PAGE));
    let rendered = report.render();
    assert!(rendered.contains("//b"), "{rendered}");
    assert!(
        rendered.contains(&format!("{:016x}", document_hash(PAGE))),
        "{rendered}"
    );
}

#[test]
fn an_entry_run_against_an_earlier_item_is_replayed_with_that_context() {
    let dir = tempfile::tempdir().unwrap();
    record(dir.path());

    let corpus = Corpus::load(dir.path()).unwrap();
    let report = diff(&corpus, &NativeEngine, &Differs("a/@href"));
    // Both items' entries differ: their contexts were rebuilt from `//li`.
    assert_eq!(report.mismatches.len(), 2);
    for mismatch in &report.mismatches {
        assert!(mismatch.entry.context.is_some());
    }
    let hrefs: Vec<String> = report
        .mismatches
        .iter()
        .map(|m| m.expected.items[0].string.clone())
        .collect();
    assert_eq!(hrefs, ["/1", "/2"]);
}

/// The native backend, except that `expression` yields the string `"different"`.
struct Differs(&'static str);

impl XPathEngine for Differs {
    fn parse(&self, html: &[u8]) -> fmd_xpath::Result<Box<dyn Document>> {
        Ok(Box::new(DiffersDoc(NativeEngine.parse(html)?, self.0)))
    }
}

struct DiffersDoc(Box<dyn Document>, &'static str);

impl Document for DiffersDoc {
    fn eval(&self, expr: &str, context: Option<&dyn XPathValue>, css: bool) -> Box<dyn XPathValue> {
        if expr == self.1 {
            Box::new(Constant)
        } else {
            self.0.eval(expr, context, css)
        }
    }
}

struct Constant;

impl XPathValue for Constant {
    fn count(&self) -> i64 {
        1
    }
    fn get(&self, _: i64) -> Box<dyn XPathValue> {
        Box::new(Constant)
    }
    fn is_undefined(&self) -> bool {
        false
    }
    fn kind(&self) -> Kind {
        Kind::String
    }
    fn string(&self) -> String {
        "different".into()
    }
    fn inner_html(&self) -> String {
        String::new()
    }
    fn outer_html(&self) -> String {
        String::new()
    }
    fn inner_text(&self) -> String {
        String::new()
    }
    fn attribute(&self, _: &str) -> String {
        String::new()
    }
    fn property(&self, _: &str) -> Box<dyn XPathValue> {
        Box::new(Constant)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
