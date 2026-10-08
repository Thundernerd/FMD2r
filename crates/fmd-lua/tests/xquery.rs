//! `CreateTXQuery`, TXQuery and `IXQValue`, exercised through Lua snippets run on the public
//! runtime (docs/tickets/T08-fmd-xpath-trait-ffi-lua-bindings.md, "Seams under test").
//!
//! Expected values come from baseunits/lua/LuaXQuery.pas, baseunits/lua/LuaIXQValue.pas and
//! baseunits/XQueryEngineHTML.pas.

#![cfg(feature = "xpath-fpc")]
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::rc::Rc;

use fmd_lua::{LuaClass, Runtime, mlua};

/// Stands in for T04's `fmd.strings` until it lands: a list with `Add`, `Count` and the
/// 0-based default index, which is all TXQuery uses of a TStrings (`Add`,
/// baseunits/XQueryEngineHTML.pas:381, :523-524, :541-542).
fn install_strings(runtime: &Runtime) {
    let lua = runtime.lua();
    let new = lua
        .create_function(|lua, ()| {
            LuaClass::new(Rc::new(RefCell::new(Vec::<String>::new())))
                .method("Add", |_, list: &mut Vec<String>, s: String| {
                    list.push(s);
                    Ok(list.len() - 1)
                })
                .read_only_property("Count", |_, list: &mut Vec<String>| Ok(list.len()))
                .default_array_property(
                    |lua, list: &mut Vec<String>, key: mlua::Value| {
                        let i = lua
                            .coerce_integer(key)?
                            .and_then(|i| usize::try_from(i).ok());
                        Ok(i.and_then(|i| list.get(i).cloned()))
                    },
                    |_, _: &mut Vec<String>, _: mlua::Value, _: mlua::Value| Ok(()),
                )
                .build(lua)
                .map_err(mlua::Error::external)
        })
        .unwrap();
    let lib = lua.create_table().unwrap();
    lib.set("New", new).unwrap();
    let preload: mlua::Table = lua.load("package.preload").eval().unwrap();
    preload
        .set(
            "fmd.strings",
            lua.create_function(move |_, ()| Ok(lib.clone())).unwrap(),
        )
        .unwrap();
}

/// Stands in for T04's MemoryStream: a global `NewStream(content)` whose object has the
/// `ToString` method TXQuery reads a stream through.
fn install_stream(runtime: &Runtime) {
    let lua = runtime.lua();
    let new = lua
        .create_function(|lua, content: mlua::LuaString| {
            LuaClass::new(Rc::new(RefCell::new(content.as_bytes().to_vec())))
                .method("ToString", |lua, bytes: &mut Vec<u8>, ()| {
                    lua.create_string(&*bytes)
                })
                .build(lua)
                .map_err(mlua::Error::external)
        })
        .unwrap();
    lua.globals().set("NewStream", new).unwrap();
}

fn run(chunk: &str) {
    let runtime = Runtime::new().unwrap();
    install_strings(&runtime);
    install_stream(&runtime);
    if let Err(e) = runtime.exec(chunk) {
        panic!("{e}");
    }
}

#[test]
fn xpath_count_counts_matching_nodes() {
    run(r#"
        local x = CreateTXQuery('<div><a href="/1">One</a><a href="/2"> </a><a href="/3">Three</a></div>')
        assert(x.XPathCount('//a') == 3)
        assert(x:XPathCount('//a') == 3)
    "#);
}

const LINKS: &str = r#"local x = CreateTXQuery('<div><a href="/1" title="t1">One</a><a href="/2"> </a><a href="/3">Three</a></div>')"#;

/// Runs `chunk` with `x` bound to a TXQuery over three links, the second with blank text.
fn with_links(chunk: &str) {
    run(&format!("{LINKS}\n{chunk}"));
}

#[test]
fn xpath_string_returns_the_first_items_string() {
    with_links(
        r#"
        assert(x.XPathString('//a[1]/@href') == '/1')
        assert(x.XPathString('//a[3]') == 'Three')
        assert(x.XPathString('//nothing') == '')
    "#,
    );
}

#[test]
fn ixqvalue_get_is_one_based_with_node_accessors() {
    with_links(
        r#"
        local v = x.XPath('//a')
        assert(v.Count == 3)
        assert(v.Get(1).GetAttribute('href') == '/1')
        assert(v.Get(1).GetAttribute('missing') == '')
        assert(v.Get(3).ToString() == 'Three')
        assert(v.Get('2').GetAttribute('href') == '/2')    -- lua_tointeger converts numeric strings
        assert(v.Get(0).Count == 0 and v.Get(4).Count == 0)
        assert(v.Get(nil).Count == 0)                       -- one argument: Get(0)
        assert(v.Get(1).OuterHTML() == '<a href="/1" title="t1">One</a>')
        assert(v.Get(1).InnerHTML() == 'One')
        assert(v.Get(2).InnerText() == '')
        -- The ticket writes this as 'json("{\\"k\\":1}")?k', which isn't valid XPath (an XPath
        -- string escapes `"` by doubling it), so internettools, and FMD2, return ''.
        assert(x.XPath([[json('{"k":1}')?k]]).ToString() == '1')
        assert(x.XPath([[json('{"k":{"n":2}}')]]).GetProperty('k').GetProperty('n').ToString() == '2')
    "#,
    );
}

#[test]
fn get_without_arguments_iterates_the_items() {
    with_links(
        r#"
        local hrefs = {}
        for v in x.XPath('//a').Get() do hrefs[#hrefs + 1] = v.GetAttribute('href') end
        assert(table.concat(hrefs, ',') == '/1,/2,/3')
        local n = 0
        for _ in x.XPath('//nothing').Get() do n = n + 1 end
        assert(n == 0)
        -- A single item iterates as itself.
        n = 0
        for v in x.XPath('count(//a)').Get() do n = n + 1; assert(v.ToString() == '3') end
        assert(n == 1)
    "#,
    );
}

#[test]
fn iterators_of_one_value_share_its_position() {
    // The position lives on the value object (`Current`, baseunits/lua/LuaIXQValue.pas:94),
    // so a second loop over the same value continues where the first stopped.
    with_links(
        r#"
        local v = x.XPath('//a')
        local first = v.Get()
        assert(first().GetAttribute('href') == '/1')
        local second = v.Get()
        assert(second().GetAttribute('href') == '/2')
        assert(first().GetAttribute('href') == '/3')
        assert(first() == nil)
        local n = 0
        for _ in v.Get() do n = n + 1 end
        assert(n == 0)
    "#,
    );
}

#[test]
fn a_context_value_scopes_the_query() {
    run(r#"
        local x = CreateTXQuery('<div id="a"><b>1</b></div><div id="b"><b>2</b><b>3</b></div>')
        local div = x.XPath('//div[@id="b"]')
        assert(x.XPathCount('b', div) == 2)
        -- A sequence's string concatenates its items' strings (internettools).
        assert(x.XPathString('b', div) == '23')
        assert(x.XPath('b', div).Get(2).ToString() == '3')
        -- Arguments past a context are ignored, and so is the context then
        -- (baseunits/lua/LuaXQuery.pas:64-67).
        assert(x.XPathCount('//b', div, 'extra') == 3)
        local ok = pcall(x.XPathCount, 'b', 'not a value')
        assert(not ok)
    "#);
}

#[test]
fn xpath_string_all_trims_skips_empties_and_joins() {
    with_links(
        r#"
        assert(x.XPathStringAll('//a') == 'One, Three')
        assert(x.XPathStringAll('//a', ' | ') == 'One | Three')
        assert(x.XPathStringAll('//a/@href', '') == '/1/2/3')
        assert(x.XPathStringAll('//nothing') == '')
        -- A number separator is a string to `lua_isstring` (baseunits/lua/LuaXQuery.pas:105).
        assert(x.XPathStringAll('//a', 0) == 'One0Three')
        local div = x.XPath('//div')
        assert(x.XPathStringAll('a', '/', div) == 'One/Three')
        -- Neither a string nor a userdata: no result at all (baseunits/lua/LuaXQuery.pas:104-117).
        assert(select('#', x.XPathStringAll('//a', nil)) == 0)
        assert(select('#', x.XPathStringAll('//a', {}, div)) == 0)
        -- Four or more arguments: nothing (baseunits/lua/LuaXQuery.pas:98-134).
        assert(select('#', x.XPathStringAll('//a', ',', div, 1)) == 0)
    "#,
    );
}

#[test]
fn xpath_string_all_trims_with_pascal_trim() {
    // Pascal's Trim strips every character up to ' ', control characters included
    // (baseunits/XQueryEngineHTML.pas:130-137), on the joined result too.
    run(r#"
        local x = CreateTXQuery('<p><i>a</i><i>&#160;</i><i>b&#9;</i></p>')
        assert(x.XPathStringAll('//i', '-') == 'a-\u{a0}-b')
        assert(x.XPathStringAll('//i', ' ') == 'a \u{a0} b')
    "#);
}

#[test]
fn xpath_string_all_fills_a_tstrings() {
    // The TStrings form trims each item but keeps blank ones (baseunits/XQueryEngineHTML.pas:375-382).
    with_links(
        r#"
        local list = require('fmd.strings').New()
        assert(select('#', x.XPathStringAll('//a', list)) == 0)
        assert(list.Count == 3 and list[0] == 'One' and list[1] == '' and list[2] == 'Three')
        local div = x.XPath('//div')
        x.XPathStringAll('a/@href', list, div)
        assert(list.Count == 6 and list[3] == '/1' and list[5] == '/3')
    "#,
    );
}

#[test]
fn xpath_href_all_adds_links_and_trimmed_texts() {
    with_links(
        r#"
        local links, names = require('fmd.strings').New(), require('fmd.strings').New()
        x.XPathHREFAll('//a', links, names)
        assert(links[0] == '/1' and links[1] == '/2' and links[2] == '/3')
        assert(names[0] == 'One' and names[1] == '' and names[2] == 'Three')
        local div = x.XPath('//div')
        x.XPathHREFAll('a[3]', links, names, div)
        assert(links.Count == 4 and links[3] == '/3' and names[3] == 'Three')
        -- Other argument counts do nothing (baseunits/lua/LuaXQuery.pas:142-147).
        x.XPathHREFAll('//a', links)
        assert(links.Count == 4)
    "#,
    );
}

#[test]
fn xpath_href_title_all_adds_links_and_titles() {
    with_links(
        r#"
        local links, titles = require('fmd.strings').New(), require('fmd.strings').New()
        x.XPathHREFTitleAll('//a', links, titles)
        assert(links.Count == 3 and links[0] == '/1' and links[2] == '/3')
        assert(titles[0] == 't1' and titles[1] == '' and titles[2] == '')
        x.XPathHREFTitleAll('a[1]', links, titles, x.XPath('//div'))
        assert(links.Count == 4 and titles[3] == 't1')
    "#,
    );
}

#[test]
fn create_txquery_dispatches_on_its_arguments() {
    // baseunits/lua/LuaXQuery.pas:22-43
    run(r#"
        assert(CreateTXQuery().XPathCount('//a') == 0)
        assert(CreateTXQuery('<a>1</a><a>2</a>').XPathCount('//a') == 2)
        assert(CreateTXQuery(NewStream('<a>1</a>')).XPathString('//a') == '1')
        -- A number is a string to lua_isstring.
        assert(CreateTXQuery(42).XPathString('/') == '42')
        -- One argument of another type: no object at all.
        assert(select('#', CreateTXQuery(nil)) == 0)
        assert(select('#', CreateTXQuery({})) == 0)
        -- Two or more arguments: an empty document.
        assert(CreateTXQuery('<a>1</a>', 'x').XPathCount('//a') == 0)
        -- luaToString goes through a PAnsiChar, so the HTML ends at the first NUL.
        assert(CreateTXQuery('<a>1</a>\0<a>2</a>').XPathCount('//a') == 1)
        -- A stream is read whole (StreamToString, baseunits/XQueryEngineHTML.pas:118-128).
        assert(CreateTXQuery(NewStream('<a>1</a>\0<a>2</a>')).XPathCount('//a') == 2)
    "#);
}

#[test]
fn parse_html_replaces_the_document_unless_empty() {
    // baseunits/lua/LuaXQuery.pas:45-56, baseunits/XQueryEngineHTML.pas:417-426
    run(r#"
        local x = CreateTXQuery('<a>1</a>')
        local old = x.XPath('//a')
        assert(select('#', x.ParseHTML('<a>2</a><a>3</a>')) == 0)
        assert(x.XPathCount('//a') == 2)
        -- Values from the previous document stay usable.
        assert(old.ToString() == '1' and old.Get(1).ToString() == '1')
        x.ParseHTML('')
        assert(x.XPathCount('//a') == 2)
        x.ParseHTML(nil)
        x.ParseHTML()
        assert(x.XPathCount('//a') == 2)
        x.ParseHTML(NewStream('<b>4</b>'))
        assert(x.XPathString('//b') == '4' and x.XPathCount('//a') == 0)
        x:ParseHTML('<i>5</i>')
        assert(x.XPathString('//i') == '5')
    "#);
}

#[test]
fn invalid_expressions_yield_empty_results() {
    with_links(
        r#"
        assert(x.XPathCount('//a[') == 0)
        assert(x.XPathString('//a[') == '')
        assert(x.XPathStringAll('//a[') == '')
        assert(x.XPath('//a[').Count == 0)
        assert(x.XPath('//a[').ToString() == '')
        local n = 0
        for _ in x.XPath('//a[').Get() do n = n + 1 end
        assert(n == 0)
        local links, names = require('fmd.strings').New(), require('fmd.strings').New()
        x.XPathHREFAll('//a[', links, names)
        assert(links.Count == 0)
        -- A missing expression is '' (luaToString of an empty slot), which is invalid too.
        assert(x.XPathCount() == 0)
        -- Runtime errors as well as syntax errors.
        assert(x.XPathString('1 div 0') == '')
    "#,
    );
}

#[test]
fn the_ticket_snippet_runs() {
    run(r#"
        local x = CreateTXQuery('<div><a href="/1">One</a><a href="/2"> </a><a href="/3">Three</a></div>')
        assert(x.XPathCount('//a') == 3)
        assert(x.XPathString('//a[1]/@href') == '/1')
        assert(x.XPathStringAll('//a') == 'One, Three')        -- trimmed, empties skipped
        local n = 0; for v in x.XPath('//a').Get() do n = n + 1 end; assert(n == 3)
        assert(x.XPath('//a').Get(1).GetAttribute('href') == '/1')
        local links, names = require('fmd.strings').New(), require('fmd.strings').New()
        x.XPathHREFAll('//a', links, names); assert(links[0] == '/1')
        assert(x.XPath([[json('{"k":1}')?k]]).ToString() == '1')
        assert(x.XPathCount('//a[') == 0)                       -- errors yield empty
    "#);
}
