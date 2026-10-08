//! The shared `fmd-xpath` suite over the `native` backend.

#![cfg(feature = "native")]
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod shared;

shared::suite!(fmd_xpath::native::NativeEngine);

/// A hostile page can't crash the process: deep nesting and deep JSON are handled without
/// recursion (FMD2's engine itself overflows its stack serializing such a page).
#[test]
fn deeply_nested_input_does_not_overflow_the_stack() {
    use fmd_xpath::XPathEngine;

    let html = "<div>".repeat(10_000) + "x";
    let doc = fmd_xpath::native::NativeEngine
        .parse(html.as_bytes())
        .unwrap();
    assert_eq!(doc.eval("count(//div)", None, false).string(), "10000");
    let root = doc.eval("/", None, false);
    let wrapper = "<html><head></head><body></body></html>".len();
    assert_eq!(
        root.outer_html().len(),
        html.len() + "</div>".len() * 10_000 + wrapper
    );
    assert_eq!(root.inner_text(), "x");
    let json = "[".repeat(100_000);
    assert!(
        doc.eval(&format!("json('{json}')"), None, false)
            .is_undefined()
    );
}
