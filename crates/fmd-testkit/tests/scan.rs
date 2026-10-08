use std::collections::BTreeSet;

use fmd_testkit::scan_host_api_names;

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn finds_object_members_and_fmd_libraries() {
    let source = "local x = HTTP.GET(u); MODULE.Storage['k'] = 1; require 'fmd.crypto'";
    assert_eq!(
        scan_host_api_names(source),
        names(&["HTTP.GET", "MODULE.Storage", "fmd.crypto"])
    );
}

#[test]
fn reports_colon_calls_under_the_dot_name() {
    let source = "HTTP:GET(u)\nif TASK:PageLinks() then end";
    assert_eq!(
        scan_host_api_names(source),
        names(&["HTTP.GET", "TASK.PageLinks"])
    );
}

#[test]
fn recognises_every_require_form() {
    let source = r#"
        local a = require 'fmd.crypto'
        local b = require "fmd.env"
        local c = require("fmd.duktape")
        local d = require ( 'fmd.imagepuzzle' )
        local e = require 'utils.json'
    "#;
    assert_eq!(
        scan_host_api_names(source),
        names(&["fmd.crypto", "fmd.duktape", "fmd.env", "fmd.imagepuzzle"])
    );
}

#[test]
fn ignores_names_inside_comments_and_strings() {
    let source = r#"
        -- HTTP.GET in a line comment
        --[[ MODULE.Storage in a block comment ]]
        --[==[ TASK.PageLinks ]] still in a level-2 comment ]==]
        local s = "MANGAINFO.Title and an escaped \" HTTP.POST"
        local t = 'require "fmd.crypto"'
        local u = [[UPDATELIST.CurrentDirectoryPageNumber]]
        local v = x .. HTTP.HEAD(u)
    "#;
    assert_eq!(scan_host_api_names(source), names(&["HTTP.HEAD"]));
}

#[test]
fn ignores_fields_of_other_tables_that_share_a_host_name() {
    let source = "local t = self.HTTP.GET; local m = cfg:MODULE.Name";
    assert_eq!(scan_host_api_names(source), names(&[]));
}

#[test]
fn finds_host_globals_but_not_local_definitions() {
    let source = r#"
        local x = CreateTXQuery(HTTP.Document)
        local t = Trim(SeparateLeft(x.XPathString('//a'), '|'))
        if PAGENUMBER > 1 then sleep(100) end
        local function GetBetween(a, b, s) return s end
        return no_error, ünïcode
    "#;
    assert_eq!(
        scan_host_api_names(source),
        names(&[
            "CreateTXQuery",
            "HTTP.Document",
            "PAGENUMBER",
            "SeparateLeft",
            "Trim",
            "no_error",
            "sleep",
        ])
    );
}

#[test]
fn table_constructor_keys_are_not_references() {
    let source = "local t = { URL = 1, [2] = 3; sleep = URL == 1 }";
    assert_eq!(scan_host_api_names(source), names(&["URL"]));
}

#[test]
fn hex_literals_do_not_swallow_the_next_operand() {
    let source = "local x = 0xE-HTTP.GET(u) + 1e-5 - 0x1p-2-MODULE.ID";
    assert_eq!(
        scan_host_api_names(source),
        names(&["HTTP.GET", "MODULE.ID"])
    );
}
