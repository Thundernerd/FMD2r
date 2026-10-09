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
        fn object_constructors_build_and_merge_objects() {
            // As fixtures/lua/modules/Cubari.lua:55 builds chapters.
            let merged = r#"jn:object(object(("chapter_id", "7")), json('{"title":"t"}'))"#;
            assert_eq!(items("", &format!("{merged}()")), ["chapter_id", "title"]);
            assert_eq!(string("", &format!("{merged}.chapter_id")), "7");
            // Odd pairs and duplicate keys are errors.
            assert!(items("", r#"object(("a", 1, "b"))"#).is_empty());
            assert!(items("", r#"jn:object(object(("a", 1)), object(("a", 2)))"#).is_empty());
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
        fn xmlns_attributes_put_elements_in_a_namespace() {
            // `xmlns` and `xmlns:p` declare namespaces as internettools' HTML parser reads them
            // (data/simplehtmltreeparser.pas:2451-2470); they aren't attributes.
            let html = r#"<html lang="en" xmlns="X"><body><div><a>t</a></div></body></html>"#;
            assert_eq!(string(html, "namespace-uri(//a)"), "X");
            assert_eq!(string("<p>x</p>", "namespace-uri(//p)"), "");
            assert_eq!(items(html, "//a"), ["t"]);
            assert_eq!(string(html, "count(/html/@*)"), "1");
            // An undeclared prefix is dropped from the name (data/simplehtmltreeparser.pas:
            // 2466-2477), a declared one kept.
            let vue = r#"<body><a v-bind:title="t">x</a><fb:like>y</fb:like></body>"#;
            assert_eq!(string(vue, "//a/@title"), "t");
            assert_eq!(string(vue, "name(//like)"), "like");
            let fb = r#"<html xmlns:fb="F"><body><fb:like>y</fb:like></body></html>"#;
            assert_eq!(string(fb, "name(//*:like)"), "fb:like");
            assert_eq!(string(fb, "namespace-uri(//*:like)"), "F");
        }

        #[test]
        fn namespaced_nodes_serialize_with_their_declarations() {
            // `serializeNodes` (internettools data/xquery__serialization_nodes.pas:386-700): a
            // serialized node declares the namespaces in scope, and elements outside the HTML
            // namespaces (none, empty, XHTML) are written as XML.
            let outer = |html: &str, expr: &str| {
                let doc = engine().parse(html.as_bytes()).unwrap();
                doc.eval(expr, None, false).outer_html()
            };
            let html = r#"<html lang="en" xmlns="X"><body><br><p title='a"b'>x"y</p><p></p></body></html>"#;
            assert_eq!(
                outer(html, "/html"),
                r#"<html xmlns="X" lang="en"><head/><body><br/><p title="a&quot;b">x&quot;y</p><p/></body></html>"#
            );
            // An element with its parent's namespace is HTML again.
            assert_eq!(
                outer(html, "//body"),
                r#"<body xmlns="X"><br><p title="a&quot;b">x"y</p><p></p></body>"#
            );
            let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><br><p></p></body></html>"#;
            assert_eq!(
                outer(xhtml, "/html"),
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><head></head><body><br><p></p></body></html>"#
            );
            // The element's own namespace first, then its ancestors' declarations.
            let prefixed =
                r#"<html xmlns:b="B" xmlns:a="A" xmlns="X"><body><div>t</div></body></html>"#;
            assert_eq!(
                outer(prefixed, "//div"),
                r#"<div xmlns="X" xmlns:b="B" xmlns:a="A">t</div>"#
            );
            let nested = r#"<html xmlns="X"><body><div xmlns="X">1</div><div xmlns="Y"><a>t</a></div></body></html>"#;
            assert_eq!(
                outer(nested, "//body"),
                r#"<body xmlns="X"><div>1</div><div xmlns="Y"><a>t</a></div></body>"#
            );
            let svg = r#"<body><svg xmlns="http://www.w3.org/2000/svg"><path d="M0"/><text>a"b</text></svg></body>"#;
            assert_eq!(
                outer(svg, "//body"),
                r#"<body><svg xmlns="http://www.w3.org/2000/svg"><path d="M0"/><text>a&quot;b</text></svg></body>"#
            );
            assert_eq!(
                outer(r#"<html xmlns:fb="F"><body><fb:like a=1>y</fb:like></body></html>"#, "//body"),
                r#"<body xmlns:fb="F"><fb:like a="1">y</fb:like></body>"#
            );
            // Only the first child of an inner serialization gets the ancestors' declarations.
            let doc = engine()
                .parse(br#"<html xmlns:og="O"><body><div>1</div><div>2</div></body></html>"#)
                .unwrap();
            assert_eq!(
                doc.eval("//body", None, false).inner_html(),
                r#"<div xmlns:og="O">1</div><div>2</div>"#
            );
        }

        #[test]
        fn string_literals_normalise_line_endings() {
            // CR LF and a lone CR become LF (internettools data/xquery__parse.pas:2653-2656,
            // the default `xqlenXML1` of data/xquery.pas:8384), as in modules that join with
            // a Lua "\r\n" (GenzToons).
            let html = "<p>a</p><p>b</p>";
            assert_eq!(string(html, "string-join(//p, \"\r\n\")"), "a\nb");
            assert_eq!(string("", "'x\ry' || \"\r\r\n\""), "x\ny\n\n");
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
        fn uri_decode_fails_on_a_bad_escape() {
            assert_eq!(string("", "uri-decode('a%20b+c%C3%A9')"), "a b cé");
            // A `%` without two hex digits is an error, even before a multi-byte character.
            assert!(items("", "uri-decode('%aé')").is_empty());
            assert!(items("", "uri-decode('100%')").is_empty());
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
