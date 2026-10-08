//! The public `fmd-xpath` API over the `fpc` backend
//! (docs/tickets/T08-fmd-xpath-trait-ffi-lua-bindings.md, "Seams under test").
//!
//! Expected values come from FMD2's engine set-up (baseunits/XQueryEngineHTML.pas:384-400).

#![cfg(feature = "fpc")]
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_xpath::XPathEngine;
use fmd_xpath::fpc::FpcEngine;

#[test]
fn parse_then_eval_counts_matching_nodes() {
    let doc = FpcEngine.parse(b"<ul><li>a</li><li> b </li></ul>").unwrap();
    assert_eq!(doc.eval("//li", None, false).count(), 2);
}

#[test]
fn get_is_one_based_and_out_of_range_is_empty() {
    let doc = FpcEngine.parse(b"<ul><li>a</li><li> b </li></ul>").unwrap();
    let items = doc.eval("//li", None, false);
    assert_eq!(items.get(1).string(), "a");
    // Node strings are trimmed (internettools' XQGlobalTrimNodes, which FMD2 never changes).
    assert_eq!(items.get(2).string(), "b");
    assert!(items.get(3).is_undefined());
    assert!(items.get(0).is_undefined());
    assert!(!items.get(1).is_undefined());
}

#[test]
fn an_invalid_expression_yields_an_empty_value() {
    let doc = FpcEngine.parse(b"<a>x</a>").unwrap();
    let value = doc.eval("//a[", None, false);
    assert_eq!(value.count(), 0);
    assert!(value.is_undefined());
}

#[test]
fn a_context_value_scopes_xpath_and_css() {
    let doc = FpcEngine
        .parse(b"<div id=a><b>1</b></div><div id=b><b class=x>2</b></div>")
        .unwrap();
    let second = doc.eval("//div[@id='b']", None, false);
    assert_eq!(doc.eval("b", Some(second.as_ref()), false).string(), "2");
    assert_eq!(doc.eval("b.x", None, true).string(), "2");
}

#[test]
fn node_accessors_read_the_untrimmed_tree() {
    let doc = FpcEngine
        .parse(b"<p><a href=/x title=T> Go <i>on</i></a></p>")
        .unwrap();
    let a = doc.eval("//a", None, false);
    assert_eq!(a.attribute("href"), "/x");
    assert_eq!(a.attribute("title"), "T");
    assert_eq!(a.attribute("missing"), "");
    assert_eq!(a.inner_html(), " Go <i>on</i>");
    assert_eq!(
        a.outer_html(),
        "<a href=\"/x\" title=\"T\"> Go <i>on</i></a>"
    );
    assert_eq!(a.inner_text(), "Go on");
}

#[test]
fn json_properties_are_values() {
    let doc = FpcEngine.parse(b"").unwrap();
    let object = doc.eval(r#"json('{"k":{"n":2}}')"#, None, false);
    assert_eq!(object.property("k").property("n").string(), "2");
    assert!(object.property("missing").is_undefined());
    assert_eq!(doc.eval(r#"json('{"k":1}')?k"#, None, false).string(), "1");
}

#[test]
fn the_logging_hook_sees_each_expression_with_its_document_hash() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let engine = fmd_xpath::LoggingEngine::new(FpcEngine, move |query: &fmd_xpath::Query| {
        sink.borrow_mut()
            .push((query.expression.to_owned(), query.document_hash, query.css));
    });
    let first = engine.parse(b"<a>1</a>").unwrap();
    let same = engine.parse(b"<a>1</a>").unwrap();
    let other = engine.parse(b"<a>2</a>").unwrap();

    // Logging doesn't change results.
    assert_eq!(first.eval("//a", None, false).string(), "1");
    same.eval("a", None, true);
    other.eval("//a", None, false);

    let seen = seen.borrow();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].0, "//a");
    assert!(!seen[0].2);
    assert_eq!(seen[1].0, "a");
    assert!(seen[1].2);
    // The hash identifies the document's bytes.
    assert_eq!(seen[0].1, seen[1].1);
    assert_ne!(seen[0].1, seen[2].1);
}
