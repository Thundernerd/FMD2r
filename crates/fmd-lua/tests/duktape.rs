//! `fmd.duktape.ExecJS`, exercised through Lua snippets run on the public runtime
//! (docs/tickets/T12-fmd-duktape-execjs.md, "Seams under test"). Expected values are what
//! FMD2's Duktape returns (baseunits/Duktape.pas:77-104, baseunits/lua/LuaDuktape.pas:14-24).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::time::{Duration, Instant};

use fmd_http::TerminateToken;
use fmd_lua::{JsLimits, Runtime};

fn runtime() -> Runtime {
    let runtime = Runtime::new().unwrap();
    runtime.set_lua_dir(fmd_testkit::corpus_root());
    runtime
}

#[test]
fn evaluates_an_expression_to_its_string_value() {
    runtime()
        .exec(
            r#"
            local js = require 'fmd.duktape'
            assert(js.ExecJS('1+2') == '3')
            "#,
        )
        .unwrap();
}

/// `duk_safe_to_string` is JS `ToString`; Duktape.pas:98 then drops a result of `"undefined"`.
#[test]
fn stringifies_the_completion_value_like_duktape() {
    runtime()
        .exec(
            r#"
            local js = require 'fmd.duktape'
            assert(js.ExecJS('var a=[1,2]; a.join("-")') == '1-2')
            assert(js.ExecJS('[1,[2,3],null,undefined]') == '1,2,3,,')
            assert(js.ExecJS('({a:1})') == '[object Object]')
            assert(js.ExecJS('null') == 'null')
            assert(js.ExecJS('true') == 'true')
            assert(js.ExecJS('0.1+0.2') == '0.30000000000000004')
            assert(js.ExecJS('1e21') == '1e+21')
            assert(js.ExecJS('-0') == '0')
            assert(js.ExecJS('1/0') == 'Infinity')
            assert(js.ExecJS('0/0') == 'NaN')
            assert(js.ExecJS('(function f(){})') ~= '')
            assert(js.ExecJS('({toString:function(){return "custom"}})') == 'custom')
            -- ToString throws: Duktape coerces the error instead.
            assert(js.ExecJS('({toString:function(){throw new Error("boom")}})') == 'Error: boom')
            -- Duktape.pas:98-99: "undefined" becomes the empty string, whatever produced it.
            assert(js.ExecJS('undefined') == '')
            assert(js.ExecJS('var x = 1;') == '')
            assert(js.ExecJS('"undefined"') == '')
            assert(js.ExecJS('') == '')
            "#,
        )
        .unwrap();
}

/// A script error is logged and `lua_execjs` pushes nothing (baseunits/lua/LuaDuktape.pas:16-22).
#[test]
fn a_failing_script_returns_no_value() {
    runtime()
        .exec(
            r#"
            local js = require 'fmd.duktape'
            assert(js.ExecJS('throw new Error("x")') == nil)
            assert(select('#', js.ExecJS('throw new Error("x")')) == 0)
            assert(js.ExecJS('throw "plain"') == nil)
            assert(js.ExecJS('var = ;') == nil)
            assert(js.ExecJS('missing.property') == nil)
            -- The worker carries on with the next script.
            assert(js.ExecJS('1+2') == '3')
            "#,
        )
        .unwrap();
}

/// `require` loads JS files from the Lua directory (baseunits/Duktape.pas:106-121).
#[test]
fn require_loads_crypto_js_from_the_lua_directory() {
    runtime()
        .exec(
            r#"
            local js = require 'fmd.duktape'
            assert(js.ExecJS('var C=require("utils/crypto-js.min.js"); C.MD5("a").toString()') == '0cc175b9c0f1b6a831c399e269772661')
            "#,
        )
        .unwrap();
}

/// A runtime whose Lua directory holds the given `(path, source)` JS files.
fn runtime_with_files(files: &[(&str, &str)]) -> (Runtime, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let runtime = Runtime::new().unwrap();
    runtime.set_lua_dir(dir.path());
    (runtime, dir)
}

/// Duktape's CommonJS loader (duk_module_duktape_init, baseunits/Duktape.pas:88) with FMD2's
/// `modSearch` (baseunits/Duktape.pas:39-67, 106-121).
#[test]
fn require_resolves_module_ids_like_duktape() {
    let (runtime, _dir) = runtime_with_files(&[
        (
            "utils/a.js",
            "exports.name = 'a:' + require('./b').name + ':' + require.id;",
        ),
        ("utils/b.js", "exports.name = 'b' + require('../top.js').n;"),
        ("top.js", "module.exports = { n: 7 };"),
        ("this.js", "exports.same = (this === exports);"),
        (
            "count.js",
            "globalThis.loads = (globalThis.loads || 0) + 1; exports.n = loads;",
        ),
        ("bare", "exports.ext = 'none';"),
    ]);
    runtime
        .exec(
            r#"
            local js = require 'fmd.duktape'
            -- Relative ids resolve against the requiring module; '.js' is appended when needed.
            assert(js.ExecJS('require("utils/a.js").name') == 'a:b7:utils/a.js')
            assert(js.ExecJS('require("./utils/a").name') == 'a:b7:utils/a')
            assert(js.ExecJS('require("utils//a").name') == 'a:b7:utils/a')
            -- An exact file name wins over the '.js' fallback.
            assert(js.ExecJS('require("bare").ext') == 'none')
            -- `this` is the module's exports.
            assert(js.ExecJS('require("this").same') == 'true')
            -- A module runs once per heap, and every heap starts afresh.
            assert(js.ExecJS('require("count").n + "," + require("count").n') == '1,1')
            assert(js.ExecJS('require("count").n') == '1')
            -- modSearch finds no file: require returns the empty exports.
            assert(js.ExecJS('JSON.stringify(require("missing"))') == '{}')
            -- Ids that cannot be resolved throw.
            assert(js.ExecJS('require("/top")') == nil)
            assert(js.ExecJS('require("../top")') == nil)
            assert(js.ExecJS('require("utils/")') == nil)
            assert(js.ExecJS('require("")') == nil)
            "#,
        )
        .unwrap();
}

/// FMD2's Duktape has no limits; here a runaway script fails like a script error instead of
/// hanging or crashing the worker (docs/tickets/T12-fmd-duktape-execjs.md, "Execution limits").
#[test]
fn runaway_scripts_are_interrupted() {
    let runtime = runtime();
    runtime.set_js_limits(JsLimits {
        time: Duration::from_millis(200),
        memory: 16 * 1024 * 1024,
    });
    let started = Instant::now();
    runtime
        .exec(
            r#"
            local js = require 'fmd.duktape'
            assert(js.ExecJS('while(true){}') == nil)
            -- An interrupt cannot be caught by the script.
            assert(js.ExecJS('for(;;){ try { while(true){} } catch(e) {} }') == nil)
            assert(js.ExecJS('var a=[]; while(true){ a.push(new Array(100000).join("x")) }') == nil)
            assert(js.ExecJS('function f(){ return f() + 1 } f()') == nil)
            -- Each call gets its own budget.
            assert(js.ExecJS('1+2') == '3')
            "#,
        )
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// Terminating the worker's token stops the script it is running.
#[test]
fn terminating_the_worker_interrupts_a_script() {
    let runtime = runtime();
    let token = TerminateToken::new();
    runtime.set_terminate_token(token.clone());
    let terminator = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        token.terminate();
    });
    let started = Instant::now();
    let result: Option<String> = runtime
        .eval("require('fmd.duktape').ExecJS('while(true){}')")
        .unwrap();
    terminator.join().unwrap();
    assert_eq!(result, None);
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// FMD2 hands Duktape the Lua string's bytes as UTF-8 and pushes the result's bytes back
/// (baseunits/lua/LuaDuktape.pas:18, baseunits/Duktape.pas:92-94). JS strings are UTF-16 in
/// between, so non-ASCII text round-trips but `length` counts UTF-16 units.
#[test]
fn passes_strings_through_as_utf8() {
    runtime()
        .exec(
            r#"
            local js = require 'fmd.duktape'
            assert(js.ExecJS('"héllo wörld 漫画"') == 'héllo wörld 漫画')
            assert(js.ExecJS('"漫画".length') == '2')
            assert(js.ExecJS('"\\u00e9" === "é"') == 'true')
            assert(js.ExecJS('"😀"') == '😀')
            assert(js.ExecJS('"😀".length') == '2')
            assert(js.ExecJS('String.fromCharCode(0x41, 0xff)') == 'A\xc3\xbf')
            -- A lone surrogate comes back as its 3-byte encoding, as Duktape stores it.
            assert(js.ExecJS('"\\ud800"') == '\xed\xa0\x80')
            -- Both strings cross as C strings, so they end at the first NUL
            -- (luaToString, baseunits/lua/LuaUtils.pas:206-213; lua_pushstring).
            assert(js.ExecJS('"a\\u0000b"') == 'a')
            assert(js.ExecJS('1+2\0garbage that does not parse') == '3')
            -- Bytes that are not UTF-8 do not decode as source.
            assert(js.ExecJS('"\xff\xfe"') == nil)
            -- Non-string arguments coerce like lua_tolstring; others become ''.
            assert(js.ExecJS(42) == '42')
            assert(js.ExecJS(nil) == '')
            assert(js.ExecJS({}) == '')
            "#,
        )
        .unwrap();
}

/// The JS that upstream modules build around page scripts, on recorded-style inputs. Expected
/// values are the decoded payloads, which the inputs were made from.
#[test]
fn evaluates_upstream_module_snippets() {
    runtime()
        .exec(
            r#"
            local js = require 'fmd.duktape'

            -- templates/Madara.lua:175-183. The protector data was encrypted with
            -- `openssl enc -aes-256-cbc -md md5 -pass pass:6f1d2c3b4a -S 0123456789abcdef`.
            local script = [==[var wpmangaprotectornonce = '6f1d2c3b4a';
var chapter_data = '{"ct":"6GQLimXyG0wkqO7IuqSY9udUWOP/SFrOIBuAA7Vnyxg3NUtWTcSSFXscIBllWnkfbGGbUWLH9mFBk/0a8PFNKglD9OF3y8A3MywgANkkA7Y=","iv":"464219cc1e359e8d129bab3723756188","s":"0123456789abcdef"}';]==]
            local images = js.ExecJS(script .. [[

			var CryptoJS = require("utils/crypto-js.min.js");
			var CryptoJSAesJson = require("utils/cryptojs-aes-format.js");
			JSON.parse(CryptoJS.AES.decrypt(chapter_data, wpmangaprotectornonce, { format: CryptoJSAesJson }).toString(CryptoJS.enc.Utf8));

			]])
            assert(images == '["https://a.example/1.jpg","https://a.example/2.jpg"]', images)

            -- modules/FanFox.lua:100: a Dean Edwards packed script that sets `guidkey`.
            local packed = [==[eval(function(p,a,c,k,e,d){e=function(c){return c.toString(36)};if(!''.replace(/^/,String)){while(c--){d[c.toString(a)]=k[c]||c.toString(a)}k=[function(e){return d[e]}];e=function(){return'\\w+'};c=1};while(c--){if(k[c]){p=p.replace(new RegExp('\\b'+e(c)+'\\b','g'),k[c])}}return p}('0 1=\'2\';',3,3,'var|guidkey|abc123def456ghi789'.split('|'),0,{}))]==]
            local key = js.ExecJS('var $=function(){return{val:function(){}}},newImgs,guidkey;' .. packed .. ';newImgs||guidkey;')
            assert(key == 'abc123def456ghi789', key)

            -- modules/FanFox.lua:117: chapterfun.ashx defines `d`, an array of page links.
            local pages = js.ExecJS('var pix="//img.example/c1";var d=[pix+"/1.jpg",pix+"/2.jpg"];' .. ';d;')
            assert(pages == '//img.example/c1/1.jpg,//img.example/c1/2.jpg', pages)

            -- modules/ZeroScans.lua:59-60: the page's state object, re-serialized as JSON.
            local state = ([==[window.__ZEROSCANS__ = {data:{details:{name:"Ñame 漫",genres:[{name:"Action"}],rating:4.50}}};]==]):gsub('window.', '')
            local json = js.ExecJS(state .. ';JSON.stringify(__ZEROSCANS__);')
            assert(json == '{"data":{"details":{"name":"Ñame 漫","genres":[{"name":"Action"}],"rating":4.5}}}', json)

            -- modules/acqqcom.lua:21: a script that writes to `window`.
            local nonce = js.ExecJS('var window={};' .. [==[eval("window[\"no\"+\"nce\"] = \"3f9a\" + (1+1);")]==] .. ';window.nonce;')
            assert(nonce == '3f9a2', nonce)
            "#,
        )
        .unwrap();
}

/// `duk_peval` (baseunits/Duktape.pas:93) runs the code as non-strict global code, which packed
/// scripts rely on: `eval("var x=...")` declares a global.
#[test]
fn runs_code_as_non_strict_global_code() {
    runtime()
        .exec(
            r#"
            local js = require 'fmd.duktape'
            assert(js.ExecJS([[var g; eval("var g='x'"); g]]) == 'x')
            assert(js.ExecJS([[eval("var h=1"); typeof h]]) == 'number')
            assert(js.ExecJS('(function(){ return this === undefined })()') == 'false')
            assert(js.ExecJS('undeclared = 5; undeclared') == '5')
            "#,
        )
        .unwrap();
}
