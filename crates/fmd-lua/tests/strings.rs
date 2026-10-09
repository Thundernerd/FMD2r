//! TStrings objects (`fmd.strings`) (docs/tickets/T04-tstrings-memorystream.md). Expected values
//! were probed from FPC 3.2.2's `TStringList` (FMD2's RTL), with Windows' CRLF `sLineBreak`.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_lua::{Runtime, mlua};

/// Runs `chunk` with a fresh, empty list in the local `s`.
fn run(chunk: &str) {
    let rt = Runtime::new().unwrap();
    rt.exec(&format!("local s = require('fmd.strings').New()\n{chunk}"))
        .unwrap();
}

/// The error message `chunk` raises, run like [`run`].
fn run_err(chunk: &str) -> String {
    let rt = Runtime::new().unwrap();
    rt.exec(&format!("local s = require('fmd.strings').New()\n{chunk}"))
        .unwrap_err()
        .to_string()
}

#[test]
fn add_count_and_zero_based_default_index() {
    run("s.Add('a'); s.Add('b')
         assert(s.Count == 2 and s[0] == 'a' and s[1] == 'b')");
}

#[test]
fn get_and_set_methods_are_zero_based() {
    run("s.Add('a'); s.Add('b')
         assert(s.Get(1) == 'b' and s:Get(0) == 'a')
         s.Set(1, 'B'); s:Set(0, 'A')
         assert(s[0] == 'A' and s[1] == 'B')
         s[1] = 'bb'; assert(s.Get(1) == 'bb')");
}

#[test]
fn strings_array_property() {
    run("s.Add('a'); s.Strings[0] = 'z'; assert(s.Strings[0] == 'z' and s[0] == 'z')");
}

#[test]
fn out_of_bounds_index_raises_fpc_list_error() {
    assert!(run_err("s.Add('a'); local _ = s[1]").contains("List index (1) out of bounds"));
    assert!(run_err("local _ = s.Get(-1)").contains("List index (-1) out of bounds"));
    assert!(run_err("s[0] = 'x'").contains("List index (0) out of bounds"));
}

#[test]
fn non_numeric_default_key_is_index_zero() {
    // `lua_tointeger` turns a key that is not a number into 0.
    run("s.Add('first'); s.Add('second')
         assert(s.Unknown == 'first' and s['1'] == 'second' and s.Get('x') == 'first')
         s.Other = 'changed'; assert(s[0] == 'changed')");
    assert!(run_err("local _ = s.Unknown").contains("List index (0) out of bounds"));
}

#[test]
fn text_setter_splits_on_cr_lf_and_crlf() {
    run(
        r"s.Text = 'x\r\ny\nz'; assert(s.Count == 3 and s[0] == 'x' and s[1] == 'y' and s[2] == 'z')
          s.Text = 'a\rb'; assert(s.Count == 2 and s[1] == 'b')
          s.Text = 'a\n\rb'; assert(s.Count == 3 and s[1] == '')
          s.Text = 'a\r\r\nb'; assert(s.Count == 3 and s[1] == '')
          s.Text = 'a\n'; assert(s.Count == 1)
          s.Text = 'a\n\n'; assert(s.Count == 2 and s[1] == '')
          s.Text = '\n'; assert(s.Count == 1 and s[0] == '')
          s.Text = ''; assert(s.Count == 0)
          s.Text = 'a\0b\nc'; assert(s.Count == 2 and s[0] == 'a\0b')",
    );
}

#[test]
fn text_getter_ends_every_line_with_crlf() {
    run(r"assert(s.Text == '')
          s.Add('x'); s.Add(''); s.Add('z')
          assert(s.Text == 'x\r\n\r\nz\r\n' and s.GetText() == s.Text)");
}

#[test]
fn set_text_method_and_add_text_append_lines() {
    run(r"s:SetText('one'); assert(s.Count == 1 and s[0] == 'one')
          s.AddText('x\r\ny\n'); assert(s.Count == 3 and s[1] == 'x' and s[2] == 'y')");
}

/// Asserts that assigning `input` to `prop` yields `items`, and reading `prop` back yields
/// `output`.
fn assert_parses(prop: &str, setup: &str, input: &str, items: &[&str], output: &str) {
    let rt = Runtime::new().unwrap();
    let lua = rt.lua();
    lua.globals().set("input", input).unwrap();
    rt.exec(&format!(
        "s = require('fmd.strings').New(); {setup}; s.{prop} = input"
    ))
    .unwrap();
    let got: Vec<String> = rt
        .eval::<mlua::Table>("(function() local t = {} for i = 0, s.Count - 1 do t[#t + 1] = s[i] end return t end)()")
        .unwrap()
        .sequence_values()
        .map(Result::unwrap)
        .collect();
    assert_eq!(got, items, "{prop} = {input:?}");
    assert_eq!(
        rt.eval::<String>(&format!("s.{prop}")).unwrap(),
        output,
        "{prop} = {input:?}"
    );
}

#[test]
fn comma_text_follows_fpc_quoting() {
    let cases: &[(&str, &[&str], &str)] = &[
        ("", &[], ""),
        ("a,\"b c\",d", &["a", "b c", "d"], "a,\"b c\",d"),
        ("a b,c", &["a", "b", "c"], "a,b,c"),
        ("  a  ,  b  ", &["a", "b"], "a,b"),
        ("a,,b", &["a", "", "b"], "a,,b"),
        (",", &["", ""], ","),
        ("a,", &["a", ""], "a,"),
        ("\"a\"\"b\",c", &["a\"b", "c"], "\"a\"\"b\",c"),
        ("\"a\"b,c", &["a", "b", "c"], "a,b,c"),
        ("\"abc", &["abc"], "abc"),
        ("a \"b\" c", &["a", "b", "c"], "a,b,c"),
        ("\"\",x", &["", "x"], ",x"),
        ("a\tb", &["a", "b"], "a,b"),
        ("a,  ,b", &["a", "", "b"], "a,,b"),
        (" ", &[], ""),
        ("a\"b,c", &["a\"b", "c"], "\"a\"\"b\",c"),
    ];
    for (input, items, output) in cases {
        assert_parses("CommaText", "", input, items, output);
    }
}

#[test]
fn comma_text_getter_quotes_items_like_fpc() {
    run(
        r#"local function items(...) s.Clear() for _, v in ipairs({...}) do s.Add(v) end return s.CommaText end
           assert(items('') == '""')
           assert(items('', '') == ',')
           assert(items('a,b') == '"a,b"')
           assert(items('"q"') == '"""q"""')
           assert(items('x;y') == 'x;y')
           assert(items('a\t') == '"a\t"')
           assert(items() == '')"#,
    );
}

#[test]
fn delimited_text_uses_delimiter() {
    assert_parses(
        "DelimitedText",
        "s.Delimiter = ';'",
        "a;b c;\"d;e\"",
        &["a", "b", "c", "d;e"],
        "a;b;c;\"d;e\"",
    );
    assert_parses(
        "DelimitedText",
        "s.Delimiter = '|'",
        "x|y|",
        &["x", "y", ""],
        "x|y|",
    );
    assert_parses(
        "DelimitedText",
        "s.Delimiter = ' '",
        "a b  c",
        &["a", "b", "c"],
        "a b c",
    );
    // CommaText ignores the delimiter, and quoting follows the delimiter in use.
    run(r#"s.Delimiter = ';'; s.Add('x;y'); s.Add('a,b')
           assert(s.Delimiter == ';' and s.DelimitedText == '"x;y";a,b' and s.CommaText == 'x;y,"a,b"')"#);
}

#[test]
fn delimiter_keeps_first_character_and_rejects_empty() {
    run("assert(s.Delimiter == ',')
         s.Delimiter = ';;'; assert(s.Delimiter == ';')");
    // `String('')[1]` dereferences nil in FMD2 (baseunits/lua/LuaStrings.pas:119).
    run_err("s.Delimiter = ''");
}

#[test]
fn values_read_and_write_name_value_lines() {
    run("s.Add('a'); s.Add('b')
         s.Values['k'] = 'v'; assert(s.Values['k'] == 'v'); assert(s.IndexOfName('k') == 2)
         assert(s[2] == 'k=v')");
}

#[test]
fn values_match_names_case_insensitively_like_fpc() {
    run(
        "s.Add('Key=v1'); s.Add('noeq'); s.Add('=empty'); s.Add('k2=a=b')
         assert(s.Values['key'] == 'v1' and s.IndexOfName('KEY') == 0)
         assert(s.IndexOfName('') == 2 and s.Values['k2'] == 'a=b')
         assert(s.Values['noeq'] == '' and s.IndexOfName('noeq') == -1)
         s.Values['KEY'] = 'z'; assert(s[0] == 'KEY=z')
         s.Values['new'] = 'n'; assert(s.Count == 5 and s[4] == 'new=n')",
    );
}

#[test]
fn setting_a_value_to_empty_keeps_the_line_like_fpc_3_2() {
    // FPC 3.2.2's `TStrings.SetValue` writes `name=` rather than deleting the line (Delphi
    // deletes it); FMD2 builds with FPC 3.2.2.
    run("s.Add('key=v')
         s.Values['key'] = ''; assert(s.Count == 1 and s[0] == 'key=')
         s.Values['absent'] = ''; assert(s.Count == 2 and s[1] == 'absent=')");
}

#[test]
fn name_value_separator_changes_how_names_are_found() {
    run("assert(s.NameValueSeparator == '=')
         s.Add('k2=a'); s.NameValueSeparator = ':'; s.Add('h:1')
         assert(s.NameValueSeparator == ':' and s.Values['h'] == '1' and s.Values['k2'] == '')
         s.Values['x'] = 'y'; assert(s[2] == 'x:y')");
    run_err("s.NameValueSeparator = ''");
}

#[test]
fn index_of_is_case_insensitive() {
    run("s.Add('a'); s.Add('K2=a=b')
         assert(s.IndexOf('k2=A=B') == 1 and s:IndexOf('A') == 0 and s.IndexOf('zz') == -1)");
}

#[test]
fn sort_orders_ignoring_case() {
    // Plain letters and digits, where FPC's `AnsiCompareText` gives this order both on Linux
    // (probed) and under Windows' locale collation; punctuation is where the two differ.
    run(
        "for _, v in ipairs({'b', 'C', 'ab', 'a', 'B2'}) do s.Add(v) end
         s.Sort()
         assert(s.CommaText == 'a,ab,b,B2,C', s.CommaText)",
    );
}

#[test]
fn delete_removes_by_zero_based_index() {
    run("s.CommaText = 'a,b,c'; s.Delete(1); assert(s.CommaText == 'a,c')");
    assert!(run_err("s.Add('a'); s:Delete(1)").contains("List index (1) out of bounds"));
}

#[test]
fn reverse_reverses_any_length() {
    run("s.Reverse(); assert(s.Count == 0)
         s.CommaText = 'a'; s.Reverse(); assert(s.CommaText == 'a')
         s.CommaText = 'a,b,c,d'; s.Reverse(); assert(s.CommaText == 'd,c,b,a')
         s.CommaText = 'a,b,c'; s:Reverse(); assert(s.CommaText == 'c,b,a')");
}

#[test]
fn ticket_seam_snippet() {
    run(r#"s.Add('a'); s.Add('b')
           assert(s.Count == 2 and s[0] == 'a' and s[1] == 'b')
           assert(s.Get(1) == 'b')
           s.Values['k'] = 'v'; assert(s.Values['k'] == 'v'); assert(s.IndexOfName('k') == 2)
           s.Reverse(); assert(s[0] == 'k=v')
           s.Text = 'x\r\ny\nz'; assert(s.Count == 3)
           s.CommaText = 'a,"b c",d'; assert(s[1] == 'b c')"#);
}

#[test]
fn count_is_read_only_and_get_count_matches() {
    run(
        "s.Add('a'); s.Count = 5; assert(s.Count == 1 and s.GetCount() == 1 and s:GetCount() == 1)",
    );
}

/// Runs `chunk` with a fresh list in `s`, the stream `stream` in `m` and `path` set.
fn run_with_stream(chunk: &str, stream: &fmd_lua::LuaMemoryStream, path: &std::path::Path) {
    let rt = Runtime::new().unwrap();
    let globals = rt.lua().globals();
    globals.set("m", stream.build(rt.lua()).unwrap()).unwrap();
    globals.set("path", path.to_str().unwrap()).unwrap();
    rt.exec(&format!("local s = require('fmd.strings').New()\n{chunk}"))
        .unwrap();
}

#[test]
fn save_to_file_writes_crlf_text_and_load_from_file_reads_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("list.txt");
    let stream = fmd_lua::LuaMemoryStream::new();
    run_with_stream(
        r"s.Add('a\0b'); s.Add('c'); s.SaveToFile(path)
          local t = require('fmd.strings').New(); t:LoadFromFile(path)
          assert(t.Count == 2 and t[0] == 'a\0b' and t[1] == 'c')",
        &stream,
        &path,
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"a\0b\r\nc\r\n");
}

#[test]
fn load_from_file_strips_a_utf8_bom_and_save_writes_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bom.txt");
    std::fs::write(&path, b"\xEF\xBB\xBFbom1\r\nline2").unwrap();
    let stream = fmd_lua::LuaMemoryStream::new();
    run_with_stream(
        "s.LoadFromFile(path); assert(s.Count == 2 and s[0] == 'bom1'); s.SaveToFile(path)",
        &stream,
        &path,
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"\xEF\xBB\xBFbom1\r\nline2\r\n"
    );
}

#[test]
fn load_from_stream_reads_from_the_position_to_the_end() {
    let stream = fmd_lua::LuaMemoryStream::new();
    stream.stream().borrow_mut().write(b"zz\np\nq");
    stream.stream().borrow_mut().set_position(3);
    run_with_stream(
        "s.LoadFromStream(m); assert(s.CommaText == 'p,q')",
        &stream,
        std::path::Path::new(""),
    );
    assert_eq!(stream.stream().borrow().position(), 6);
}

#[test]
fn save_to_stream_writes_text_at_the_position() {
    let stream = fmd_lua::LuaMemoryStream::new();
    stream.stream().borrow_mut().write(b"pre");
    run_with_stream(
        "s.CommaText = 'a,b'; s:SaveToStream(m); assert(m.Size == 9)",
        &stream,
        std::path::Path::new(""),
    );
    assert_eq!(stream.stream().borrow().bytes(), b"prea\r\nb\r\n");
}

#[test]
fn stream_methods_reject_values_that_are_not_streams() {
    assert!(run_err("s.LoadFromStream(s)").contains("MemoryStream"));
    assert!(run_err("s.SaveToStream(nil)").contains("MemoryStream"));
}

#[test]
fn every_lua_strings_pas_member_is_present() {
    // The methods, properties and array properties of baseunits/lua/LuaStrings.pas:222-255.
    run(
        "for _, name in ipairs({'LoadFromFile', 'LoadFromStream', 'SaveToFile', 'SaveToStream',
             'SetText', 'GetText', 'Add', 'AddText', 'Get', 'Set', 'GetCount', 'Sort', 'Clear',
             'Delete', 'IndexOf', 'IndexOfName', 'Reverse'}) do
           assert(type(s[name]) == 'function', name)
         end
         for _, name in ipairs({'Count', 'Text', 'CommaText', 'DelimitedText', 'Delimiter',
             'NameValueSeparator'}) do
           assert(type(s[name]) == 'string' or type(s[name]) == 'number', name)
         end
         assert(type(s.Strings) == 'table' and type(s.Values) == 'table')
         local lib = require('fmd.strings')
         assert(lib.Create().Count == 0 and require('fmd.strings') == lib)",
    );
}

#[test]
fn colon_calls_work_like_dot_calls() {
    run("s:Add('b'); s:AddText('a'); s:Sort(); s:Set(0, 'A'); s:Delete(1)
         assert(s:Get(0) == 'A' and s:GetCount() == 1 and s:IndexOf('a') == 0 and s:GetText() == 'A\\r\\n')
         s:Clear(); assert(s.Count == 0)");
}

#[test]
fn rust_owner_shares_the_list_with_lua() {
    let rt = Runtime::new().unwrap();
    let links = fmd_lua::LuaStrings::new();
    links.list().borrow_mut().add("first");
    rt.lua()
        .globals()
        .set("LINKS", links.build(rt.lua()).unwrap())
        .unwrap();
    rt.exec("assert(LINKS[0] == 'first'); LINKS.Add('second')")
        .unwrap();
    assert_eq!(
        links.list().borrow().items(),
        [b"first".to_vec(), b"second".to_vec()]
    );
}
