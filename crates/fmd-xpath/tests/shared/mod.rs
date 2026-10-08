//! The `fmd-xpath` API suite every backend must pass
//! (docs/tickets/T34-native-xpath-backend.md, "Seams under test"): T08's cases plus the
//! internettools extensions upstream modules use (docs/xpath-extensions.md).
//!
//! Expected values come from FMD2's engine (internettools, configured as in
//! baseunits/XQueryEngineHTML.pas:384-400): the `fpc` backend runs FMD2's own code, so a case
//! passing there pins FMD2's behaviour, and the `native` backend must match it.

/// Expands to the whole suite as `#[test]` functions over the backend `$engine`.
macro_rules! suite {
    ($engine:expr) => {
        use fmd_xpath::XPathEngine;

        fn engine() -> impl XPathEngine {
            $engine
        }

        /// The strings of the items `expr` yields on `html`.
        fn items(html: &str, expr: &str) -> Vec<String> {
            let doc = engine().parse(html.as_bytes()).unwrap();
            let value = doc.eval(expr, None, false);
            (1..=value.count()).map(|i| value.get(i).string()).collect()
        }

        /// The string of what `expr` yields on `html`.
        fn string(html: &str, expr: &str) -> String {
            let doc = engine().parse(html.as_bytes()).unwrap();
            doc.eval(expr, None, false).string()
        }

        /// The strings of the items `expr` yields with the value of `context` as context item.
        fn items_in(html: &str, context: &str, expr: &str) -> Vec<String> {
            let doc = engine().parse(html.as_bytes()).unwrap();
            let context = doc.eval(context, None, false);
            let value = doc.eval(expr, Some(context.as_ref()), false);
            (1..=value.count()).map(|i| value.get(i).string()).collect()
        }

        /// A JSON API response, as modules get one from `HTTP.Document`.
        const API: &str = r#"{"data":{"title":"T","tags":[{"name":"a"},{"name":"b"}],"count":2,"nsfw":false,"note":null},"meta":{"has_next_page":true}}"#;

        #[test]
        fn parse_then_eval_counts_matching_nodes() {
            let doc = engine().parse(b"<ul><li>a</li><li> b </li></ul>").unwrap();
            assert_eq!(doc.eval("//li", None, false).count(), 2);
        }

        #[test]
        fn get_is_one_based_and_out_of_range_is_empty() {
            let doc = engine().parse(b"<ul><li>a</li><li> b </li></ul>").unwrap();
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
            let doc = engine().parse(b"<a>x</a>").unwrap();
            let value = doc.eval("//a[", None, false);
            assert_eq!(value.count(), 0);
            assert!(value.is_undefined());
        }

        #[test]
        fn a_context_value_scopes_xpath_and_css() {
            let doc = engine()
                .parse(b"<div id=a><b>1</b></div><div id=b><b class=x>2</b></div>")
                .unwrap();
            let second = doc.eval("//div[@id='b']", None, false);
            assert_eq!(doc.eval("b", Some(second.as_ref()), false).string(), "2");
            assert_eq!(doc.eval("b.x", None, true).string(), "2");
        }

        #[test]
        fn node_accessors_read_the_untrimmed_tree() {
            let doc = engine()
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
            let doc = engine().parse(b"").unwrap();
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
            let engine = fmd_xpath::LoggingEngine::new(engine(), move |query: &fmd_xpath::Query| {
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

        // Extensions, as inventoried in docs/xpath-extensions.md.

        #[test]
        fn json_parses_text_and_nodes_into_objects_arrays_and_numbers() {
            assert_eq!(string(API, "json(*).data.title"), "T");
            assert_eq!(string("", r#"json('{"k":1.5}').k"#), "1.5");
            // An array's string is its members' strings joined, an object's is empty.
            assert_eq!(items("", "json('[1,2]')"), ["12"]);
            assert_eq!(string("", r#"json('{"k":{"z":1}}').k"#), "");
            assert_eq!(string("", r#"json('{"k":null}').k"#), "null");
            // Liberal syntax: single quotes, bare keys, trailing commas, several values.
            assert_eq!(string("", r#"json("{'a':1}").a"#), "1");
            assert_eq!(string("", "json('{a:1}').a"), "1");
            assert_eq!(items("", "json('[1,2,]')()"), ["1", "2"]);
            assert_eq!(items("", "json('1 2')"), ["1", "2"]);
            assert_eq!(items("", r#"parse-json('{"a":[3]}')?a?*"#), ["3"]);
        }

        #[test]
        fn lookups_read_objects_and_arrays() {
            // The ticket's example.
            assert_eq!(items("", r#"json('{"a":{"b":[1,2]}}')?a?b?*"#), ["1", "2"]);
            assert_eq!(items(API, "json(*)?data?tags?*?name"), ["a", "b"]);
            assert_eq!(items(API, "json(*)?data?tags?2?name"), ["b"]);
            // A lookup on a node is an error, so the whole result is empty...
            assert!(items(API, "string-join(//body?x)").is_empty());
            // ...but on no items it is no items.
            assert_eq!(items(API, "string-join(genres?*, ', ')"), [""]);
            assert_eq!(items_in(API, "json(*)", "?data?title"), ["T"]);
        }

        #[test]
        fn dot_notation_reads_properties_after_a_parenthesis() {
            assert_eq!(items("", r#"json('{"a":{"b":[1,2]}}').a.b()"#), ["1", "2"]);
            assert_eq!(items(API, "json(*).data.tags().name"), ["a", "b"]);
            // Items without the property are skipped.
            assert_eq!(items("", r#"json('[{"n":1},{"n":2},3]')().n"#), ["1", "2"]);
            // `$json.a.b` is the variable `json.a.b` (internettools' "unambiguous" dot
            // notation), so it fails; a parenthesized variable works.
            let json = r#"let $json := json('{"a":{"b":1}}') return "#;
            assert!(items("", &format!("{json}$json.a.b")).is_empty());
            assert_eq!(items("", &format!("{json}($json).a.b")), ["1"]);
            // With a JSON context, `a.b` is one name, which no property has.
            assert!(items_in(API, "json(*)", "data.title").is_empty());
        }

        #[test]
        fn calling_objects_and_arrays_reads_them() {
            assert_eq!(items("", r#"json('{"a":1,"b":2}')()"#), ["a", "b"]);
            assert_eq!(items("", r#"json('{"a":1,"b":2}')("b")"#), ["2"]);
            assert_eq!(items("", "json('[5,6]')(2)"), ["6"]);
            assert_eq!(items("", "json('[5,6]')()"), ["5", "6"]);
        }

        #[test]
        fn jn_functions_list_keys_and_members() {
            assert_eq!(items("", r#"jn:keys(json('{"x":1}'))"#), ["x"]);
            assert_eq!(items("", r#"jn:keys(json('{"z":1,"a":2,"m":3}'))"#), ["z", "a", "m"]);
            assert_eq!(items("", "jn:members(json('[1,[2],3]'))"), ["1", "2", "3"]);
            assert_eq!(items("", "jn:size(json('[1,2]'))"), ["2"]);
        }

        #[test]
        fn steps_on_objects_read_properties() {
            assert_eq!(items_in(API, "json(*)", "data/title"), ["T"]);
            assert_eq!(items_in(API, "json(*)", "data/tags/name"), ["a", "b"]);
            assert_eq!(items(API, "json(*)?data?tags?*/name"), ["a", "b"]);
            assert_eq!(items(API, "json(*)//name"), ["a", "b"]);
            assert_eq!(items_in(API, "json(*)", "data[not(nsfw)]/count"), ["2"]);
            // `true` is a literal (JSONiq), not a step.
            assert_eq!(items_in(API, "json(*)", "meta[has_next_page = true]/has_next_page"), ["true"]);
        }

        #[test]
        fn css_selects_like_internettools() {
            let html = r#"<div class="x"><a href="1">A</a><p><a href="2">B</a></p></div><div class="y"><a href="3">C</a></div>"#;
            // The ticket's example.
            assert_eq!(items(html, "css('div.x > a')"), ["A"]);
            assert_eq!(items(html, "css('div.X a')/@href"), ["1", "2"]);
            assert_eq!(items(html, "css('a:first-child, .y a')"), ["A", "B", "C"]);
            assert_eq!(items(html, "css('div:nth-child(2) a')"), ["C"]);
            assert_eq!(items(html, "css('[href^=\"2\"]')"), ["B"]);
            let doc = engine().parse(html.as_bytes()).unwrap();
            let div = doc.eval("//div[1]", None, false);
            // From a context, selectors start at the context node itself.
            assert_eq!(doc.eval("div", Some(div.as_ref()), true).count(), 1);
            assert_eq!(doc.eval("a", Some(div.as_ref()), true).string(), "AB");
        }

        #[test]
        fn strings_join_and_compare_like_internettools() {
            let html = "<ul><li> One </li><li>two</li></ul>";
            assert_eq!(string(html, "string-join(//li, ', ')"), "One, two");
            // String parameters take a sequence's strings concatenated...
            assert_eq!(string(html, "substring-after(//li, 'O')"), "netwo");
            assert_eq!(string(html, "string-length(//li)"), "6");
            assert_eq!(string(html, "//li || '!'"), "Onetwo!");
            // ...and compare with the case-insensitive "clever" default collation.
            assert_eq!(items(html, "//li[contains(., 'ONE')]"), ["One"]);
            assert_eq!(items(html, "//li[. = 'TWO']"), ["two"]);
            assert_eq!(string("", "'a10' < 'a9'"), "false");
            assert_eq!(string("", "starts-with('Hello', 'he')"), "true");
            // A string compared with a number is converted to one.
            assert_eq!(string("", "'10' = 10"), "true");
            assert_eq!(string("", "3 + '4'"), "7");
            assert_eq!(items(html, "//li ! upper-case(.)"), ["ONE", "TWO"]);
        }

        #[test]
        fn numbers_print_like_internettools() {
            assert_eq!(string("", "1 div 3"), "0.333333333333333333");
            assert_eq!(string("", "0.1 + 0.2"), "0.3");
            assert_eq!(string("", "1e10"), "1.0E10");
            assert_eq!(string("", "xs:double(1) div 3"), "0.3333333333333333");
            assert_eq!(string("", "round(-2.5)"), "-2");
            // Integer division by zero is an error; double division gives INF.
            assert!(items("", "1 div 0").is_empty());
            assert_eq!(string("", "1e0 div 0"), "INF");
        }
    };
}

pub(crate) use suite;
