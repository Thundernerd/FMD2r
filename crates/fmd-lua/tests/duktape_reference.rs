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
    runtime_in(fmd_testkit::corpus_root())
}

/// `runtime()` with `lua_dir` as the Lua directory of both engines.
fn runtime_in(lua_dir: PathBuf) -> Runtime {
    let runtime = Runtime::new().unwrap();
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
            -- A known difference: `code` gives `quickjs` here and `duktape` on Duktape.
            function differs(code, quickjs, duktape)
              local got, want = ExecJS(code), DuktapeExecJS(code)
              assert(got == quickjs, '\n' .. code .. '\n  QuickJS: ' .. show(got))
              assert(want == duktape, '\n' .. code .. '\n  Duktape: ' .. show(want))
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

/// `fixtures/<path>`.
fn fixtures(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
}

/// A runtime set up to run a callback of the upstream module in `module_file` the way the
/// worker does, with `HTTP` replaying the recording in `fixtures/<recording>` and every
/// `ExecJS` the module makes checked with `same`.
fn module_runtime(module_file: &str, recording: &str) -> Runtime {
    let runtime = runtime();
    let lua_dir = fmd_testkit::corpus_root();
    let report = ModuleRegistry::load_file(&lua_dir, &lua_dir.join("modules").join(module_file));
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    runtime.set_module(&report.registry.modules()[0]).unwrap();
    runtime.install_globals(Globals::default()).unwrap();
    let replay = ReplayTransport::open(fixtures(recording), ReplayOptions::default()).unwrap();
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
    let runtime = module_runtime("acqqcom.lua", "js/acqqcom");
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

/// modules/FanFox.lua:91-138 on the smoke list's recording (fixtures/smoke/fanfox): the packed
/// page script and every `chapterfun.ashx` answer evaluate as on Duktape.
#[test]
fn fanfox_page_scripts_evaluate_like_duktape() {
    let runtime = module_runtime("FanFox.lua", "smoke/fanfox/pages");
    runtime
        .exec(
            r#"
            sleep = function() end
            URL = 'https://fanfox.net/manga/kimi_no_na_wa/v02/c004/'
            assert(GetPageNumber() == true)
            assert(#COMPARED == 23, #COMPARED)
            assert(TASK.PageLinks.Count == 44, TASK.PageLinks.Count)
            "#,
        )
        .unwrap();
}

/// modules/ReadComicOnline.lua:105-353 on a reader page archived by the Wayback Machine (the
/// site no longer resolves): the module's link decoder runs over the page's scripts as on
/// Duktape.
#[test]
fn readcomiconline_page_decoder_evaluates_like_duktape() {
    let runtime = module_runtime("ReadComicOnline.lua", "js/readcomiconline");
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
    let pages = fixtures("js/cloudflare");
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

/// The global object and the built-ins have the members Duktape 2.3 has and no others, so
/// scripts that detect features (`typeof Symbol`, `Array.prototype.find || polyfill`) take the
/// same branch. Members Duktape has and QuickJS lacks are in docs/duktape-differences.md.
#[test]
fn builtin_members_match_duktape() {
    runtime()
        .exec(
            r#"
            local inventory = [[
              var g = new Function('return this')();
              var skip = { Buffer: 1, 'Buffer.prototype': 1 };
              var missing = { global: 'Buffer', Duktape: 'Pointer Thread act fin info', 'Error.prototype': 'fileName lineNumber', '%TypedArray%.prototype': 'length' };
              function names(path, o) {
                var drop = (missing[path] || '').split(' ');
                return path + ': ' + Object.getOwnPropertyNames(o).filter(function (k) {
                  return drop.indexOf(k) < 0;
                }).sort().join(' ');
              }
              var out = [names('global', g)];
              Object.getOwnPropertyNames(g).sort().forEach(function (k) {
                var v = g[k];
                if (skip[k] || v === g || v === null || (typeof v !== 'object' && typeof v !== 'function')) return;
                out.push(names(k, v));
                if (typeof v === 'function' && v.prototype) out.push(names(k + '.prototype', v.prototype));
              });
              var ta = Object.getPrototypeOf(Uint8Array);
              out.push(names('%TypedArray%', ta), names('%TypedArray%.prototype', ta.prototype));
            ]]
            same('(function () {' .. inventory .. 'return out.join("\\n") })()')
            same('Object.keys(this).join()')
            same('for (var k in this) {}; Object.keys(new Function("return this")()).join()')
            same('typeof Symbol + typeof Map + typeof Promise + typeof globalThis + typeof atob')
            same('String(new Uint8Array([1, 2]))')
            same('typeof Array.prototype.find + typeof Array.prototype.includes + typeof String.prototype.includes')
            same('typeof Object.assign + typeof Object.entries + typeof String.prototype.padStart')
            same('var b = Uint8Array.allocPlain(3); [b.length, b instanceof Uint8Array].join()')
            same('var p = Uint8Array.plainOf(new Uint8Array([1, 2, 3]).subarray(1)); [p.length, p[0]].join()')
            same('Duktape.gc() + "," + (Duktape.compact(Math) === Math) + "," + typeof Duktape.env')
            -- The build's description: FMD2's DLL (dist/x86_64-win64/libduktape.dll), not the
            -- reference built here.
            assert(require('fmd.duktape').ExecJS('Duktape.env') == 'll u nl p2 a8 x64 windows mingw')
            same('[require.name, typeof require.length, "length" in print, "name" in print].join()')
            "#,
        )
        .unwrap();
}

/// `require` as Duktape's module loader (extras/module-duktape) runs it with FMD2's `modSearch`
/// (baseunits/Duktape.pas:39-67, 106-121).
#[test]
fn require_matches_duktape() {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in [
        (
            "utils/a.js",
            "exports.name = 'a:' + require('./b').name + ':' + require.id;",
        ),
        ("utils/b.js", "exports.name = 'b' + require('../top.js').n;"),
        ("top.js", "module.exports = { n: 7 };"),
        (
            "this.js",
            "exports.same = (this === exports); exports.callee = arguments.callee.name;",
        ),
        (
            "swap.js",
            "var first = exports; module.exports = { first: first === this };",
        ),
        (
            "cycle1.js",
            "exports.early = 1; exports.other = require('cycle2').seen;",
        ),
        ("cycle2.js", "exports.seen = require('cycle1').early;"),
        (
            "throws.js",
            "var g = new Function('return this')(); g.tries = (g.tries || 0) + 1; throw new Error('no');",
        ),
        (
            "module.js",
            "exports.keys = Object.getOwnPropertyNames(module).sort().join(); exports.id = module.id;",
        ),
        ("bare", "exports.ext = 'none';"),
    ] {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    runtime_in(dir.path().to_path_buf())
        .exec(
            r#"
            same('require("utils/a.js").name')
            same('require("./utils/a").name')
            same('require("utils//a").name')
            same('require("bare").ext')
            same('[require("this").same, require("this").callee].join()')
            same('require("swap").first')
            same('require("cycle1").other')
            same('JSON.stringify(require("missing"))')
            same('try { require("throws") } catch (e) {}; try { require("throws") } catch (e) {}; tries')
            same('require("module").keys + "|" + require("module").id')
            same('require("/top")')
            same('require("../top")')
            same('require("utils/")')
            same('require("")')
            same('require(5)')
            same('require()')
            same('[typeof Duktape.modLoaded, Object.getPrototypeOf(Duktape.modLoaded), typeof Duktape.modSearch].join()')
            same('require("top"); Object.keys(Duktape.modLoaded).join()')
            same('Duktape.modSearch = function (id) { return "exports.id = " + JSON.stringify(id) }; require("anything/x").id')
            same('Duktape.modLoaded.fake = { exports: 42 }; require("fake")')
            same('var D = Duktape; Duktape = {}; D.modSearch = function () { return "exports.ok = 1" }; require("q").ok')
            same('[require.name, require.id, Object.getOwnPropertyNames(require).sort().join()].join("|")')
            same('Object.getOwnPropertyDescriptor(this, "require").enumerable')
            "#,
        )
        .unwrap();
}

/// `Function.prototype.toString` gives Duktape's placeholders, not the source text
/// (duk_bi_function_prototype_to_string), which packed scripts may test or hash.
#[test]
fn function_to_string_matches_duktape() {
    runtime()
        .exec(
            r#"
            same('function foo(a, b) { return a + b } foo.toString()')
            same('(function () {}).toString()')
            same('String(function named() {})')
            same('(function f() {}) + ""')
            same('(function () { return arguments.callee.toString() })()')
            same('new Function("a", "return a").toString()')
            same('Math.max.toString()')
            same('String(eval) + String(Date) + String(Function.prototype)')
            same('Function.prototype.toString.call(Math.max)')
            same('(function f() {}).bind(null).toString()')
            same('Math.max.bind(null).toString()')
            same('var b = (function f(a, b) {}).bind(null, 1); [b.name, b.length, typeof b.prototype].join()')
            same('String(print) + String(require) + String(JSON.stringify) + String(Duktape.enc)')
            same('String(TextDecoder) + String(new TextDecoder().decode) + String(Uint8Array.allocPlain)')
            same('String(Object.getOwnPropertyDescriptor(TextEncoder.prototype, "encoding").get)')
            same('String(Function.prototype.toString) + String(Function.prototype.bind)')
            same('[Function.prototype.toString.length, Function.prototype.bind.length, Function.prototype.bind.name].join()')
            same('var fs = [function () {}]; fs[0].name = "x"; String(fs[0])')
            same('var o = { m: function () {} }; Object.defineProperty(o.m, "name", { value: 42 }); String(o.m)')
            same('try { Function.prototype.toString.call({}) } catch (e) { e.name }')
            same('/\\[native code\\]/.test(String(Math.random)) + "," + /\\{\\s*\\[native code\\]\\s*\\}/.test(String(function x(){}))')
            "#,
        )
        .unwrap();
}

/// Dates print and parse as in FMD2's Windows build of Duktape, which has no platform date
/// formatter or parser: everything is Duktape's ISO 8601 form (duk__format_parts_iso8601,
/// duk__parse_string_iso8601_subset), and a time without an offset is UTC.
#[test]
fn dates_match_duktape() {
    runtime()
        .exec(
            r#"
            same('var d = new Date(2020, 5, 7, 8, 9, 10, 11); [d.toString(), d.toDateString(), d.toTimeString(), d.toLocaleString(), d.toLocaleDateString(), d.toLocaleTimeString(), d.toUTCString(), d.toGMTString(), d.toISOString(), d.toJSON(), String(d), d + "", JSON.stringify([d])].join("|")')
            same('var d = new Date(2021, 0, 31, 23, 59, 59, 999); [d.toString(), d.toTimeString()].join("|")')
            same('[new Date(NaN).toString(), new Date(NaN).toUTCString(), new Date(NaN).toDateString(), new Date(NaN).toLocaleTimeString()].join("|")')
            same('try { new Date(NaN).toISOString() } catch (e) { e.name }')
            same('[new Date(-1e14).toUTCString(), new Date(8.64e15).toUTCString(), new Date(Date.UTC(-5, 0, 1)).toUTCString(), new Date(Date.UTC(10000, 0, 1)).toUTCString(), new Date(Date.UTC(99, 11, 31)).toUTCString()].join("|")')
            same('Date.prototype.toGMTString === Date.prototype.toUTCString')
            same('typeof Date() + "," + (Date().length === new Date().toString().length)')
            same('try { Date.prototype.toString.call({}) } catch (e) { e.name }')
            same('[Date.length, Date.name, Date.prototype.constructor === Date, new Date(0) instanceof Date, Object.prototype.toString.call(new Date(0))].join()')
            same('[Date.UTC(2020, 0), Date.UTC(2020), Date.UTC(99, 0, 1), Date.UTC(2020, 13, 40, 25, 61, 61, 1001)].join()')
            same('[new Date(2020, 0).getTime() === new Date(2020, 0, 1, 0, 0, 0, 0).getTime(), new Date(0).getTime(), new Date("0").getTime(), new Date(true).getTime(), new Date(null).getTime(), new Date(undefined).getTime()].join()')
            local parses = {
              '2020-01-02', '2020-01', '2020', '+002020-01-02T00:00:00Z', '-000001-01-01T00:00:00Z',
              '2020-01-02T03:04:05', '2020-01-02 03:04:05', '2020-01-02T03:04:05Z', '2020-01-02T03:04Z',
              '2020-01-02T03Z', '2020-01-02T03:04:05.5+01:00', '2020-01-02T03:04:05.123456Z',
              '2020-01-02T03:04:05+0100', '2020-01-02T03:04:05+01', '2020-01-02T03:04:05-01:30',
              '2020-01-02T24:00:00Z', '2020-13-02', '2020-02-30', '2020-01-02T03:04:05.Z',
              '2020-01-02T03:04:05Zx', '2020/01/02', 'Jan 2, 2020', 'Thu, 02 Jan 2020 03:04:05 GMT', '',
              ' 2020-01-02', '2020-01-02 ', '1234567890', '12345678901', '2020-01-02T03:04:05 +01:00',
              '2020-06-07 08:09:10.011+02:00', '2020-06-07 06:09:10.011Z', 'Invalid Date', '2020-1-2',
              '0099-01-01T00:00:00Z', '0000-01-01', '2020-01-02T03:04:05.1234567891Z',
            }
            for _, s in ipairs(parses) do
              same('[Date.parse(' .. string.format('%q', s) .. '), new Date(' .. string.format('%q', s) .. ').getTime()].join()')
            end
            same('var d = new Date(2020, 5, 7, 8, 9, 10, 11); [Date.parse(d.toString()), Date.parse(d.toUTCString()), Date.parse(d.toISOString()), new Date(d).getTime(), new Date(d.toString()).getTime()].join()')
            same('new Date({ valueOf: function () { return 5 }, toString: function () { return "2020-01-01" } }).getTime()')
            same('new Date({ toString: function () { return "2020-01-01" } }).getTime()')
            same('Date.parse({ toString: function () { return "2020-01-01" } })')
            "#,
        )
        .unwrap();
}

/// The differences that remain, pinned on both engines; docs/duktape-differences.md says why
/// no module observes them.
#[test]
fn known_differences_from_duktape() {
    runtime()
        .exec(
            r#"
            -- Node.js's Buffer binding.
            differs('typeof Buffer', '', 'function')
            -- Error messages and stack text.
            differs('try { undefined_var } catch (e) { e.message }', 'undefined_var is not defined', "identifier 'undefined_var' undefined")
            -- ES2015+ syntax parses here and fails on Duktape.
            differs('let a = 1; a', '1', nil)
            differs('(() => 3)()', '3', nil)
            differs('/a/y.test("a")', 'true', nil)
            -- Decimal literals past 2^53: Duktape's conversion is off by one unit.
            differs('9007199254740993', '9007199254740992', '9007199254740994')
            -- String.fromCharCode above U+FFFF: QuickJS keeps the low 16 bits, Duktape the code point.
            differs('String.fromCharCode(0x1F600)', '\xef\x98\x80', '\xf0\x9f\x98\x80')
            -- RegExp source: Duktape escapes "/" inside a class too.
            differs('/[/]/.source', '[/]', '[\\/]')
            -- Name inference (ES2015) for anonymous function expressions.
            differs('var f = function () {}; f.name', 'f', '')
            -- Own properties of instances.
            differs('Object.getOwnPropertyNames(function f() {}).sort().join()', 'length,name,prototype', 'fileName,length,name,prototype')
            differs('Object.getOwnPropertyNames(new Uint8Array(1)).join()', '0', '0,length')
            differs('(function () { "use strict"; return Object.getOwnPropertyNames(arguments).sort().join() })()', 'callee,length', 'callee,caller,length')
            -- Duktape passes the property name to getters (DUK_USE_NONSTD_GETTER_KEY_ARGUMENT);
            -- "undefined" comes back as '' (baseunits/Duktape.pas:98-99).
            differs('Object.defineProperty({}, "x", { get: function (k) { return typeof k } }).x', '', 'string')
            "#,
        )
        .unwrap();
}
