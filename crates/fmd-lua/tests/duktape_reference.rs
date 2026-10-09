//! `fmd.duktape.ExecJS` compared with FMD2's own engine: each Lua snippet runs a script through
//! the public runtime and through `fmd-duktape-ref` (Duktape 2.3.0, as FMD2 bundles it) and
//! expects the same bytes back (docs/tickets/T50-js-engine-verification.md).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fmd_http::{HttpClient, ReplayOptions, ReplayTransport};
use fmd_lua::{Globals, LuaHttp, ModuleRegistry, Runtime};

/// A runtime on the upstream Lua tree whose Lua code also has `same(code)`: asserts that
/// `fmd.duktape.ExecJS(code)` returns what FMD2's `lua_execjs` returns on Duktape
/// (baseunits/lua/LuaDuktape.pas:14-24): the result up to its first NUL, or no value when the
/// script fails.
fn runtime() -> Runtime {
    let runtime = Runtime::new().unwrap();
    let lua_dir = fmd_testkit::corpus_root();
    runtime.set_lua_dir(&lua_dir);
    let lua = runtime.lua();
    let reference = lua
        .create_function(move |lua, code: mlua::LuaString| {
            // luaToString (baseunits/lua/LuaUtils.pas:206-213) reads the code as a C string.
            let code = code.as_bytes();
            let code = code.split(|&b| b == 0).next().unwrap_or_default();
            match fmd_duktape_ref::exec_js(code, &lua_dir) {
                Ok(result) => Ok(Some(lua.create_string(result)?)),
                Err(_) => Ok(None),
            }
        })
        .unwrap();
    lua.globals().set("DuktapeExecJS", reference).unwrap();
    runtime
        .exec(
            r#"
            local function show(s)
              if s == nil then return 'nil' end
              return string.format('%q', s) .. ' [' .. s:gsub('.', function(c) return string.format('%02x', c:byte()) end) .. ']'
            end
            local ExecJS = require('fmd.duktape').ExecJS
            COMPARED = {}
            function same(code)
              local got, want = ExecJS(code), DuktapeExecJS(code)
              COMPARED[#COMPARED + 1] = got or false
              assert(got == want, '\n' .. code .. '\n  QuickJS: ' .. show(got) .. '\n  Duktape: ' .. show(want))
              return got
            end
            "#,
        )
        .unwrap();
    runtime
}

/// Strings in a script and in its result cross as UTF-8 bytes (baseunits/Duktape.pas:92-94,
/// baseunits/lua/LuaDuktape.pas:18); characters outside the BMP are where the engines'
/// string representations differ.
#[test]
fn non_bmp_strings_round_trip_like_duktape() {
    runtime()
        .exec(
            r#"
            same('"héllo wörld 漫画"')
            same('"😀"')
            same('"😀".length')
            same('"a😀b".charCodeAt(1)')
            same('"a😀b".charCodeAt(2)')
            same('"a😀b".charAt(1)')
            same('"\\ud83d\\ude00"')
            same('"\\ud83d\\ude00".length')
            same('"\\ud83d\\ude00" === "😀"')
            same('String.fromCharCode(0xd83d, 0xde00)')
            same('"😀".split("").length')
            same('"x😀y".substring(1, 2)')
            "#,
        )
        .unwrap();
}

/// `fixtures/js`: the pages the module snippets run on.
fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/js")
}

/// A runtime set up to run a callback of the upstream module in `module_file` the way the
/// worker does, with `HTTP` replaying the recording in `fixtures/js/<recording>` and every
/// `ExecJS` the module makes checked with `same`.
fn module_runtime(module_file: &str, recording: &str) -> Runtime {
    let runtime = runtime();
    let lua_dir = fmd_testkit::corpus_root();
    let report = ModuleRegistry::load_file(&lua_dir, &lua_dir.join("modules").join(module_file));
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    runtime.set_module(&report.registry.modules()[0]).unwrap();
    runtime.install_globals(Globals::default()).unwrap();
    let fixtures = fixtures_dir().join(recording);
    let replay = ReplayTransport::open(fixtures, ReplayOptions::default()).unwrap();
    let client = HttpClient::with_transport(Arc::new(replay)).unwrap();
    let http = LuaHttp::new(client.session()).build(runtime.lua()).unwrap();
    runtime.lua().globals().set("HTTP", http).unwrap();
    let source = std::fs::read_to_string(lua_dir.join("modules").join(module_file)).unwrap();
    // acqqcom.lua starts with a UTF-8 BOM, which Lua's file loader skips.
    runtime.exec(source.trim_start_matches('\u{feff}')).unwrap();
    runtime
        .exec(
            r#"
            package.loaded['fmd.duktape'] = { ExecJS = same }
            TASK = { PageLinks = require('fmd.strings').New() }
            "#,
        )
        .unwrap();
    runtime
}

/// modules/acqqcom.lua:20-35 on a chapter recorded from ac.qq.com. The nonce and `DATA`
/// scripts run as on Duktape. The module no longer finds the packed chapter script (the site
/// now writes `eval(function (p, ...` where it looks for `eval (function(p, ...`), so its third
/// `ExecJS` gets a broken script and fails on both engines; the test then runs that step with
/// the site's current spelling, so the packed script itself is compared too.
#[test]
fn acqqcom_chapter_scripts_evaluate_like_duktape() {
    let runtime = module_runtime("acqqcom.lua", "acqqcom");
    runtime
        .exec(
            r#"
            URL = 'https://ac.qq.com/ComicView/index/id/531490/cid/1'
            assert(getpagenumber() == true)
            assert(#COMPARED == 3)
            local nonce, data = COMPARED[1], COMPARED[2]
            assert(nonce == 'ba75f7f3885d18ae08c1272f530a4b07', nonce)
            assert(data:sub(1, 12) == 'eyJjbb21fcpY', data)
            assert(COMPARED[3] == false)

            local chapter = HTTP.Document.ToString()
            local marker = 'eval(function (p, a, c, k, e, r)'
            local js = '!function(){' .. marker .. GetBetween(marker, '}();', chapter) .. '}();'
            js = 'var W={nonce:"' .. nonce .. '",DATA:"' .. data .. '"};' .. js .. ';JSON.stringify(_v);'
            local v = same(js)
            assert(v:find('"title":"一人之下"', 1, true), v)
            assert(v:find('"url":"https://manhua.acimg.cn/manhua_detail/0/16_12_55_4f9c733239b5f3816f2127deca9ef6f7_7788.jpg/0"', 1, true), v)
            "#,
        )
        .unwrap();
}

/// modules/ReadComicOnline.lua:105-353 on a reader page archived by the Wayback Machine (the
/// site no longer resolves): the module's link decoder runs over the page's scripts as on
/// Duktape.
#[test]
fn readcomiconline_page_decoder_evaluates_like_duktape() {
    let runtime = module_runtime("ReadComicOnline.lua", "readcomiconline");
    runtime
        .exec(
            r#"
            URL = '/Comic/51/Issue-4?id=245214'
            assert(GetPageNumber() == true)
            assert(#COMPARED == 1)
            assert(TASK.PageLinks.Count == 32, TASK.PageLinks.Count)
            assert(TASK.PageLinks[0] == 'https://2.bp.blogspot.com/pw/AP1GczMTQ60bsNhivMYGnABR-BO0bqA-q5DaL_MXseSj1lQOyaLR0I7TDXbqLfllVZbtrJijxERny8WvspwJu_NzYs32K93hDlz3_-wk95r5cZ2Eiz_N2U8=s1600?', TASK.PageLinks[0])
            "#,
        )
        .unwrap();
}

/// websitebypass/cloudflare.lua:31-92 on Cloudflare IUAM ("I'm Under Attack Mode") challenge
/// pages captured in 2020 (fixtures/js/README.md): the challenge script, wrapped in the
/// handler's DOM stand-ins, yields the same `jschl_answer` as on Duktape. The handler cannot
/// solve the third page on either engine.
#[test]
fn cloudflare_iuam_challenges_evaluate_like_duktape() {
    let runtime = runtime();
    let pages = fixtures_dir().join("cloudflare");
    let lua = runtime.lua();
    for (name, expected) in [
        ("js_challenge-27-05-2020.html", "102.7365933239"),
        ("js_challenge1_16_05_2020.html", "-7.5155218245"),
        ("js_challenge2_16_05_2020.html", "NaN"),
    ] {
        let page = std::fs::read(pages.join(name)).unwrap();
        lua.globals()
            .set("PAGE", lua.create_string(page).unwrap())
            .unwrap();
        lua.globals().set("EXPECTED", expected).unwrap();
        runtime
            .exec(
                r#"
                COMPARED = {}
                duktape = { ExecJS = same }
                LOGGER = { SendError = function() end }
                HTTP = { UserAgent = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' }
                local cf = require 'websitebypass.cloudflare'
                local _, answer = cf:IUAMChallengeAnswer(PAGE, 'https://example.com/manga/1')
                assert(#COMPARED == 1)
                assert(answer == EXPECTED, answer)
                "#,
            )
            .unwrap();
    }
}

/// Regular expressions see a non-BMP character as its two surrogates on both engines (no `u`
/// flag in ES5).
#[test]
fn regex_on_astral_characters_matches_duktape() {
    runtime()
        .exec(
            r#"
            same('/^.$/.test("😀")')
            same('/^..$/.test("😀")')
            same('/^[😀]$/.test("😀")')
            same('/^\\S+$/.test("😀")')
            same('/^\\W\\W$/.test("😀")')
            same('/[\\ud83d\\ude00]/.test("\\ude00")')
            same('/\\ud83d\\ude00/.test("😀")')
            same('"a😀b".replace(/😀/, "x")')
            same('"😀😀".replace(/\\ud83d/g, "x")')
            same('"a😀b".match(/[\\ud800-\\udbff]/)[0]')
            same('"😀".search(/\\ude00/)')
            same('"a😀".split(/(?:)/).length')
            same('"x😀y".split("😀").join("|")')
            same('/😀/.source')
            "#,
        )
        .unwrap();
}

/// `JSON.stringify` / `JSON.parse` round trips, which modules use to hand page state back to
/// Lua (modules/ZeroScans.lua:60, modules/DigitalTeam.lua:33).
#[test]
fn json_round_trips_match_duktape() {
    runtime()
        .exec(
            r#"
            same('JSON.stringify("😀")')
            same('JSON.stringify({"😀": "𠀀"})')
            same('JSON.parse("\\"😀\\"").length')
            same('JSON.parse("\\"\\\\ud83d\\\\ude00\\"")')
            same('JSON.stringify(JSON.parse("{\\"a\\":\\"𠀀😀\\"}"))')
            -- Duktape writes lone surrogates as they are and escapes U+2028 and U+2029
            -- (DUK_USE_NONSTD_JSON_ESC_U2028_U2029).
            same('JSON.stringify("\\ud83d")')
            same('JSON.stringify("\\ude00x\\ud800")')
            same('JSON.stringify("a\\u2028b\\u2029c")')
            same('JSON.stringify({"\\u2028": ["\\udfff"]}, null, 1)')
            same('var s = ""; for (var i = 0; i < 0x10000; i++) s += String.fromCharCode(i); JSON.stringify(s)')
            same('JSON.stringify("\\\\ud83d")')
            same('JSON.stringify([1e21, 1e-7, 0.1, -0, NaN, Infinity])')
            same('JSON.stringify({a:undefined,b:function(){},c:NaN,d:new Date(0)})')
            same('JSON.stringify([1,{a:[2]}], null, 2)')
            same('JSON.stringify({a:[{}],b:"x"}, null, "\\t")')
            same('JSON.stringify({a:1,b:2}, ["a"])')
            same('JSON.stringify({a:1,b:2}, function(k, v) { return k === "b" ? undefined : v })')
            same('JSON.stringify({toJSON: function() { return "t" }})')
            same('JSON.parse(" [1, \\"\\\\u00e9\\", {\\"a\\": null}] ")[1]')
            same('JSON.parse("[1,]")')
            same('JSON.parse("01")')
            same('Object.keys(JSON.parse("{\\"b\\":1,\\"a\\":2,\\"1\\":3}")).join()')
            "#,
        )
        .unwrap();
}

/// The `Duktape` built-in's codecs and the Encoding API, which websitebypass/cloudflare.lua:41-42
/// builds the challenge's `btoa`/`atob` on.
#[test]
fn duktape_codecs_match_duktape() {
    runtime()
        .exec(
            r#"
            local cf = [[
function btoa(s) {return Duktape.enc('base64', s);};
function atob(s) {return new TextDecoder().decode(Duktape.dec('base64', s));};
]]
            same(cf .. 'btoa("hello")')
            same(cf .. 'btoa("héllo 漫 😀")')
            same(cf .. 'btoa("")')
            same(cf .. 'atob("aGVsbG8=")')
            same(cf .. 'atob(btoa("héllo 漫 😀"))')
            same(cf .. 'atob("aGVsbG8")')
            same(cf .. 'atob("aGVs bG8=")')
            same(cf .. 'atob("//79")')
            same(cf .. 'atob("!!")')
            same(cf .. 'atob(btoa(String.fromCharCode(0xd800)))')

            same('typeof Duktape')
            same('Duktape.version')
            same('Duktape.enc("hex", "hé")')
            same('Duktape.enc("hex", 12.5)')
            same('Duktape.enc("hex", null)')
            same('Duktape.enc("hex", {})')
            same('Duktape.enc("hex", new Uint8Array([1, 255]))')
            same('Duktape.enc("base64", new Uint8Array([0, 1, 2, 3, 250]).buffer)')
            same('Duktape.enc("base64", new Uint8Array([0, 1, 2, 3, 250]).subarray(1, 3))')
            same('Duktape.enc("hex", new DataView(new Uint8Array([7, 8, 9]).buffer, 1))')
            same('Duktape.enc("hex", new Uint16Array([0x0102]))')
            same('Duktape.enc("nope", "x")')
            same('var b = Duktape.dec("hex", "00ff41"); [typeof b, Object.prototype.toString.call(b), b.length, b[1], b instanceof Uint8Array].join()')
            same('String(Duktape.dec("hex", "0"))')
            same('String(Duktape.dec("hex", "zz"))')
            same('Duktape.enc("hex", Duktape.dec("hex", "ABcd"))')
            same('Duktape.enc("hex", Duktape.dec("base64", "AQID"))')
            same('Duktape.enc("hex", Duktape.dec("base64", "AQ=="))')
            same('Duktape.enc("hex", Duktape.dec("base64", "AQ"))')
            same('Duktape.enc("hex", Duktape.dec("base64", "AQ=")) + "|"')
            same('Duktape.enc("hex", Duktape.dec("base64", "A Q I D"))')
            same('Duktape.enc("hex", Duktape.dec("base64", "AQ==AQ=="))')
            same('Duktape.enc("hex", Duktape.dec("base64", "-_-_"))')
            same('Duktape.enc("hex", Duktape.dec("base64", ""))')
            same('Duktape.enc("hex", Duktape.dec("base64", new Uint8Array([65, 81, 61, 61])))')

            same('var d = new TextDecoder(); [d.encoding, d.fatal, d.ignoreBOM].join()')
            same('new TextDecoder().decode(new Uint8Array([0xef, 0xbb, 0xbf, 0x41]))')
            same('new TextDecoder("utf-8", {ignoreBOM: true}).decode(new Uint8Array([0xef, 0xbb, 0xbf, 0x41])).length')
            same('new TextDecoder().decode(new Uint8Array([0x41, 0xff, 0xc3, 0x42, 0xe6, 0xbc]))')
            same('new TextDecoder().decode(new Uint8Array([0xf0, 0x9f, 0x98, 0x80])).length')
            same('new TextDecoder().decode(new Uint8Array([0xed, 0xa0, 0xbd]))')
            same('new TextDecoder().decode()')
            same('new TextDecoder().decode(new Uint8Array([0x61, 0x62]).buffer)')
            same('try { new TextDecoder("utf-8", {fatal: true}).decode(new Uint8Array([0xff])) } catch (e) { e.name }')
            same('try { new TextDecoder("latin1") } catch (e) { e.name }')
            same('new TextDecoder("UTF8").encoding')
            same('var e = new TextEncoder(); e.encoding')
            same('Duktape.enc("hex", new TextEncoder().encode("é😀\\ud800"))')
            same('new TextEncoder().encode() instanceof Uint8Array')
            same('new TextEncoder().encode().length')
            "#,
        )
        .unwrap();
}
