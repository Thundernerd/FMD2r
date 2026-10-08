//! Calls the `libfmdxpath.so` C ABI (`fmdxpath.h`) through raw FFI.
//!
//! Expected values come from FMD2's engine set-up in `baseunits/XQueryEngineHTML.pas:384-400`.

// A test crate: panicking is how a check fails, including in helpers outside #[test] functions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use std::ffi::c_int;

#[test]
fn xpath_selects_every_matching_node() {
    let doc = Doc::parse("<ul><li>a</li><li> b </li></ul>");
    assert_eq!(doc.eval("//li").count(), 2);
}

#[test]
fn get_is_one_based_and_to_string_trims_like_fmd2() {
    let doc = Doc::parse("<ul><li>a</li><li> b </li></ul>");
    let items = doc.eval("//li");
    assert_eq!(items.get(1).string(), "a");
    // Node-to-string conversion is trimmed by internettools' XQGlobalTrimNodes, which FMD2 leaves at its default (true).
    assert_eq!(items.get(2).string(), "b");
}

#[test]
fn an_invalid_expression_yields_an_empty_value_and_a_last_error() {
    let doc = Doc::parse("<ul><li>a</li></ul>");
    let value = doc.eval("//li[");
    assert_eq!(value.count(), 0);
    let error = take(unsafe { fx_last_error() });
    assert!(error.contains("XPST0003"), "{error}");

    // The next successful evaluation clears it.
    assert_eq!(doc.eval("//li").count(), 1);
    assert_eq!(take(unsafe { fx_last_error() }), "");
}

#[test]
fn xidel_json_extensions_work() {
    let doc = Doc::parse("<p></p>");
    assert_eq!(doc.eval(r#"json('{"a":[1,2]}')?a?*"#).count(), 2);
}

#[test]
fn css_selectors_evaluate_when_is_css_is_set() {
    let doc = Doc::parse("<div class=x>t</div>");
    assert_eq!(doc.eval_in("div.x", None, true).string(), "t");
}

#[test]
fn a_context_value_scopes_the_evaluation() {
    let doc = Doc::parse("<div id=a><b>1</b></div><div id=b><b>2</b></div>");
    let second = doc.eval("//div[@id='b']");
    assert_eq!(doc.eval_in("b", Some(&second), false).string(), "2");
    assert_eq!(doc.eval_in("b", Some(&second), true).string(), "2");
}

#[test]
fn malformed_html_is_repaired() {
    let doc = Doc::parse("<p>a<p>b");
    assert_eq!(doc.eval("count(//p)").string(), "2");
    assert_eq!(doc.eval("//p").count(), 2);
}

#[test]
fn node_accessors_keep_the_untrimmed_tree() {
    let doc = Doc::parse("<ul><li>a</li><li> b </li></ul>");
    let li = doc.eval("//li[2]");
    // trimText is false (baseunits/XQueryEngineHTML.pas:393), so the text node keeps its spaces.
    assert_eq!(li.inner_html(), " b ");
    assert_eq!(li.outer_html(), "<li> b </li>");
    // innerText is "human readable": TTreeNode.innerText trims and normalizes whitespace itself.
    assert_eq!(li.inner_text(), "b");
}

#[test]
fn get_attribute_reads_a_node_attribute() {
    let doc = Doc::parse(r#"<a href="/x" title="T">t</a>"#);
    let a = doc.eval("//a");
    assert_eq!(a.attribute("href"), "/x");
    assert_eq!(a.attribute("title"), "T");
    assert_eq!(a.attribute("missing"), "");
}

#[test]
fn node_accessors_on_a_non_node_return_empty_with_a_last_error() {
    let doc = Doc::parse("<p></p>");
    let number = doc.eval("1");
    assert_eq!(number.attribute("href"), "");
    assert!(!take(unsafe { fx_last_error() }).is_empty());
    assert_eq!(number.inner_html(), "");
    assert_eq!(number.outer_html(), "");
    assert_eq!(number.inner_text(), "");
}

#[test]
fn get_property_reads_a_json_object_property() {
    let doc = Doc::parse("<p></p>");
    let object = doc.eval(r#"json('{"a":[1,2],"b":"x"}')"#);
    assert_eq!(object.property("b").string(), "x");
    assert_eq!(
        doc.eval_in("?*", Some(&object.property("a")), false)
            .count(),
        2
    );
    assert_eq!(object.property("missing").count(), 0);
    // IXQValue.getProperty: an empty sequence for non-objects.
    assert_eq!(doc.eval("1").property("b").count(), 0);
}

// `fx_kind` from fmdxpath.h.
const FX_KIND_UNDEFINED: c_int = 0;
const FX_KIND_BOOLEAN: c_int = 1;
const FX_KIND_INT64: c_int = 2;
const FX_KIND_NODE: c_int = 4;
const FX_KIND_SEQUENCE: c_int = 5;
const FX_KIND_ARRAY: c_int = 6;
const FX_KIND_DOUBLE: c_int = 7;
const FX_KIND_STRING: c_int = 8;
const FX_KIND_DECIMAL: c_int = 9;
const FX_KIND_OBJECT: c_int = 13;

#[test]
fn kind_reports_the_primary_type() {
    let doc = Doc::parse("<ul><li>a</li><li>b</li></ul>");
    assert_eq!(doc.eval("//li").kind(), FX_KIND_SEQUENCE);
    assert_eq!(doc.eval("//li").get(1).kind(), FX_KIND_NODE);
    assert_eq!(doc.eval("()").kind(), FX_KIND_UNDEFINED);
    assert_eq!(doc.eval("//li[").kind(), FX_KIND_UNDEFINED);
    assert_eq!(doc.eval("true()").kind(), FX_KIND_BOOLEAN);
    assert_eq!(doc.eval("1").kind(), FX_KIND_INT64);
    assert_eq!(doc.eval("1.5").kind(), FX_KIND_DECIMAL);
    assert_eq!(doc.eval("1e0").kind(), FX_KIND_DOUBLE);
    assert_eq!(doc.eval("'s'").kind(), FX_KIND_STRING);
    assert_eq!(doc.eval("[1]").kind(), FX_KIND_ARRAY);
    assert_eq!(doc.eval(r#"json('{"a":1}')"#).kind(), FX_KIND_OBJECT);
}

#[test]
fn values_stay_usable_after_their_document_is_freed() {
    let doc = Doc::parse(r#"<a href="/x">t</a>"#);
    let a = doc.eval("//a");
    drop(doc);
    assert_eq!(a.attribute("href"), "/x");
    assert_eq!(a.get(1).outer_html(), r#"<a href="/x">t</a>"#);
}

#[test]
fn null_handles_are_errors_not_crashes() {
    let null_value = std::ptr::null_mut();
    let expr = "//p";
    let value = unsafe {
        fx_eval(
            std::ptr::null_mut(),
            expr.as_ptr().cast(),
            expr.len(),
            null_value,
            0,
        )
    };
    assert!(!value.is_null(), "fx_eval never returns NULL");
    let value = Value(value);
    assert_eq!(value.count(), 0);
    assert!(!take(unsafe { fx_last_error() }).is_empty());

    unsafe {
        assert_eq!(fx_value_count(null_value), 0);
        assert_eq!(fx_value_kind(null_value), FX_KIND_UNDEFINED);
        assert_eq!(take(fx_value_to_string(null_value)), "");
        assert_eq!(take(fx_value_inner_html(null_value)), "");
        assert_eq!(
            take(fx_value_get_attribute(null_value, expr.as_ptr().cast(), 1)),
            ""
        );
        assert_eq!(Value(fx_value_get(null_value, 1)).count(), 0);
        assert_eq!(
            Value(fx_value_get_property(null_value, expr.as_ptr().cast(), 1)).count(),
            0
        );
        fx_value_free(null_value);
        fx_doc_free(std::ptr::null_mut());
    }
}

/// Divides through `black_box` so the compiler can't fold it; traps if FP exceptions are unmasked.
fn host_float_division() -> f64 {
    std::hint::black_box(0.0_f64) / std::hint::black_box(0.0_f64)
}

#[test]
fn the_library_leaves_the_host_float_environment_alone() {
    assert!(host_float_division().is_nan());
    let doc = Doc::parse("<p>1</p>");
    assert_eq!(doc.eval("1e0 div 0e0").string(), "INF");
    assert!(host_float_division().is_nan());
    std::thread::spawn(|| assert!(host_float_division().is_nan()))
        .join()
        .unwrap();
}

#[test]
fn evaluation_runs_with_fpcs_default_float_exceptions_like_fmd2s_threads() {
    // FMD2 evaluates on FPC threads, whose MXCSR unmasks overflow: the EOverflow becomes an empty value.
    let doc = Doc::parse("<p>1</p>");
    let overflow = doc.eval("1e308 * 10");
    assert_eq!(overflow.count(), 0);
    assert!(take(unsafe { fx_last_error() }).contains("EOverflow"));
    assert_eq!(doc.eval(r#"xs:float("1e40")"#).count(), 0);
    // Observed from internettools built as an FPC program, as FMD2 is.
    assert_eq!(doc.eval(r#"xs:double("1e400")"#).string(), "5.0E-324");
    assert_eq!(doc.eval("0e0 div 0e0").string(), "NaN");
    assert_eq!(doc.eval(r#"(xs:double("1e400"), 1e308 * 10)"#).count(), 0);
    assert!(host_float_division().is_nan());
}

#[test]
fn documents_on_different_threads_evaluate_concurrently() {
    let workers: Vec<_> = (0..8)
        .map(|t| {
            std::thread::spawn(move || {
                for i in 0..200 {
                    let doc = Doc::parse(&format!("<ul><li class=x>{t}-{i}</li><li>y</li></ul>"));
                    assert_eq!(doc.eval("//li").count(), 2);
                    assert_eq!(doc.eval_in("li.x", None, true).string(), format!("{t}-{i}"));
                    // Regexes go through internettools' process-wide FLRE cache.
                    assert_eq!(
                        doc.eval(r#"replace(//li[1], "\d+-", "n")"#).string(),
                        format!("n{i}")
                    );
                    assert_eq!(doc.eval(&format!("//li[matches(., '^{t}-')]")).count(), 1);
                    assert_eq!(doc.eval("//li[").count(), 0);
                    assert!(take(unsafe { fx_last_error() }).contains("XPST0003"));
                    assert_eq!(doc.eval("1e308 * 10").count(), 0);
                }
                unsafe { fx_thread_exit() };
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn text_is_utf8_whatever_the_locale() {
    let doc = Doc::parse("<p title='ü'>héllo ✓</p>");
    assert_eq!(doc.eval("//p").string(), "héllo ✓");
    assert_eq!(doc.eval("//p").attribute("title"), "ü");
    assert_eq!(doc.eval("upper-case(//p)").string(), "HÉLLO ✓");
    assert_eq!(doc.eval("string-length(//p)").string(), "7");
    assert_eq!(doc.eval("//p[contains(., 'é')]").count(), 1);
}
