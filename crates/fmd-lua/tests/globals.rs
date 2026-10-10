//! The global helper functions modules call without a `require`
//! (docs/tickets/T05-global-helpers.md). Expected values follow the Pascal sources each test cites.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use fmd_http::TerminateToken;
use fmd_lua::{Globals, Runtime};

fn runtime() -> Runtime {
    let runtime = Runtime::new().unwrap();
    runtime.install_globals(Globals::default()).unwrap();
    runtime
}

/// `Trim` (baseunits/lua/LuaBaseUnit.pas:17-24): FPC `Trim` strips bytes `<= ' '` at both ends;
/// a second argument names the characters to strip instead (`TStringHelper.Trim`).
#[test]
fn trim_strips_control_and_space_bytes_at_both_ends() {
    runtime()
        .exec(
            r#"
            assert(Trim('  a b \t\n') == 'a b')
            assert(Trim('') == '')
            assert(Trim(' \1\31 ') == '')
            assert(Trim('x') == 'x')
            assert(Trim(nil) == '')
            assert(Trim(12) == '12')
            assert(Trim('--a-b--', '-') == 'a-b')
            assert(Trim('-+a+-', '+-') == 'a')
            assert(Trim(' a ', '') == ' a ')
            assert(Trim(' a ', nil) == ' a ')
            assert(Trim(' a\0b ') == 'a')
            "#,
        )
        .unwrap();
}

/// `SeparateLeft`/`SeparateRight` (baseunits/synapse/synautil.pas:1156-1177): split at the first
/// delimiter; a missing (or empty, since FPC's `Pos('')` is 0) delimiter leaves `SeparateLeft`
/// and `SeparateRight` both returning the whole value.
#[test]
fn separate_left_and_right_split_at_the_first_delimiter() {
    runtime()
        .exec(
            r#"
            assert(SeparateLeft('k=v', '=') == 'k' and SeparateRight('k=v', '=') == 'v')
            assert(SeparateLeft('a,b,c', ',') == 'a')
            assert(SeparateRight('a,b,c', ',') == 'b,c')
            assert(SeparateLeft('a::b::c', '::') == 'a')
            assert(SeparateRight('a::b::c', '::') == 'b::c')
            assert(SeparateLeft('abc', '=') == 'abc')
            assert(SeparateRight('abc', '=') == 'abc')
            assert(SeparateLeft('abc', '') == 'abc')
            assert(SeparateRight('abc', '') == 'abc')
            assert(SeparateLeft('=v', '=') == '')
            assert(SeparateRight('k=', '=') == '')
            assert(SeparateLeft('', '=') == '')
            assert(SeparateRight('', '=') == '')
            "#,
        )
        .unwrap();
}

/// `GetBetween(PairBegin, PairEnd, Value)` (baseunits/synapse/synautil.pas:1671-1723): the text
/// after the first `PairBegin` up to its balancing `PairEnd`; the whole value when either pair
/// is missing.
#[test]
fn get_between_returns_the_balanced_text_after_the_first_begin() {
    runtime()
        .exec(
            r#"
            assert(GetBetween('[', ']', 'a[b]c') == 'b')
            assert(GetBetween('[', ']', '[]') == '')
            assert(GetBetween('[', ']', '') == '')
            assert(GetBetween('<<', '>>', 'ab') == 'ab')
            assert(GetBetween('[', ']', 'abc') == 'abc')
            assert(GetBetween('[', ']', 'a[bc') == 'a[bc')
            assert(GetBetween('[', ']', 'a]b[c') == 'a]b[c')
            assert(GetBetween('[', ']', 'x]y[z]') == 'z')
            assert(GetBetween('[', ']', '[a][b]') == 'a')
            assert(GetBetween('(', ')', 'f(a(b)c)d') == 'a(b)c')
            assert(GetBetween('(', ')', '((a)') == '(a)')
            assert(GetBetween('<', '>>', '<<a>>') == '<a>')
            assert(GetBetween('"', '"', 'say "hi" now') == 'hi')
            assert(GetBetween('<div>', '</div>', '<p><div>x</div></p>') == 'x')
            assert(GetBetween('', ']', 'a]b') == 'a]b')
            assert(GetBetween('[', '', 'a[b') == 'a[b')
            "#,
        )
        .unwrap();
}

/// `MaybeFillHost(Host, URL)` (baseunits/uBaseUnit.pas:943-950): when `SplitURL`
/// (baseunits/httpsendthread.pas:191-276) finds a path but no host, the host without trailing
/// slashes is prefixed to that path; otherwise the URL is returned as is.
#[test]
fn maybe_fill_host_prefixes_urls_without_a_host() {
    runtime()
        .exec(
            r#"
            assert(MaybeFillHost('https://h.com', '/x/y') == 'https://h.com/x/y')
            assert(MaybeFillHost('https://h.com/', '/x') == 'https://h.com/x')
            assert(MaybeFillHost('https://h.com//', '/x') == 'https://h.com/x')
            assert(MaybeFillHost('https://h.com', 'https://o.com/z') == 'https://o.com/z')
            assert(MaybeFillHost('https://h.com', '//cdn.o.com/z') == '//cdn.o.com/z')
            assert(MaybeFillHost('https://h.com', 'x/y') == 'https://h.com/x/y')
            assert(MaybeFillHost('https://h.com', 'page.html') == 'https://h.com/page.html')
            assert(MaybeFillHost('https://h.com', 'a.b.c') == 'a.b.c')
            assert(MaybeFillHost('https://h.com', 'www.o.com/x') == 'www.o.com/x')
            assert(MaybeFillHost('https://h.com', '?page=2') == 'https://h.com/?page=2')
            assert(MaybeFillHost('https://h.com', '  /x ') == 'https://h.com/x')
            assert(MaybeFillHost('https://h.com', '') == '')
            assert(MaybeFillHost('https://h.com', '/') == '/')
            assert(MaybeFillHost('', '/x') == '/x')
            assert(MaybeFillHost('https://h.com', '/a\xe9') == 'https://h.com/a\xe9')
            assert(MaybeFillHost('https://h.com', 'a\xe9/b') == 'https://h.com/a\xe9/b')
            "#,
        )
        .unwrap();
}

/// `MangaInfoStatusIfPos` (baseunits/lua/LuaBaseUnit.pas:32-50, baseunits/uBaseUnit.pas:2793-2850):
/// case-insensitive substring search for the ongoing, completed, hiatus and cancelled strings
/// (defaults at baseunits/uBaseUnit.pas:626-628), in that order, each holding `|`-separated
/// alternatives; codes from baseunits/uBaseUnit.pas:230-233, `RS_InfoStatus_Unknown`
/// (mangadownloader/forms/frmMain.pas:1010) when nothing matches.
#[test]
fn manga_info_status_if_pos_maps_status_text_to_codes() {
    runtime()
        .exec(
            r#"
            assert(MangaInfoStatusIfPos('Status: Ongoing') == '1')
            assert(MangaInfoStatusIfPos('Completed') == '0')
            assert(MangaInfoStatusIfPos('On Hiatus') == '2')
            assert(MangaInfoStatusIfPos('Cancelled') == '3')
            assert(MangaInfoStatusIfPos('Dropped', 'publishing', 'finished', 'hiatus', 'dropped|cancel') == '3')
            assert(MangaInfoStatusIfPos('') == '')
            assert(MangaInfoStatusIfPos(nil) == '')
            assert(MangaInfoStatusIfPos('Licensed') == 'Unknown')
            assert(MangaInfoStatusIfPos('ONGOING') == '1')
            assert(MangaInfoStatusIfPos('ongoing, completed') == '1')
            assert(MangaInfoStatusIfPos('Releasing', 'releasing') == '1')
            assert(MangaInfoStatusIfPos('Ongoing', 'releasing') == 'Unknown')
            assert(MangaInfoStatusIfPos('Finished', 'x', 'Completed|Finished') == '0')
            assert(MangaInfoStatusIfPos('Paused', 'x', 'y', 'paused') == '2')
            assert(MangaInfoStatusIfPos('On Hold', 'Ongoing|Releasing', 'Completed|Finished',
                'Hiatus|On Hold', 'Canceled|Dropped') == '2')
            assert(MangaInfoStatusIfPos('anything', '|', '', '||', '|') == 'Unknown')
            assert(MangaInfoStatusIfPos('ongoing', nil) == 'Unknown')
            assert(select('#', MangaInfoStatusIfPos()) == 0)
            assert(select('#', MangaInfoStatusIfPos('ongoing', 'a', 'b', 'c', 'd', 'ongoing')) == 0)
            "#,
        )
        .unwrap();
}

/// Collects everything a `tracing` fmt subscriber writes.
#[derive(Clone, Default)]
struct LogBuffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Runs `chunk` on a thread named `thread` with globals installed for `module`, returning the
/// fields of each log event it produced.
fn log_of(module: Option<&str>, thread: &str, chunk: &'static str) -> Vec<serde_json::Value> {
    let buffer = LogBuffer::default();
    let writer = buffer.clone();
    let module = module.map(str::to_string);
    std::thread::Builder::new()
        .name(thread.into())
        .spawn(move || {
            let subscriber = tracing_subscriber::fmt()
                .json()
                .with_writer(move || writer.clone())
                .finish();
            tracing::subscriber::with_default(subscriber, || {
                let runtime = Runtime::new().unwrap();
                runtime
                    .install_globals(Globals {
                        module,
                        ..Globals::default()
                    })
                    .unwrap();
                runtime.exec(chunk).unwrap();
            });
        })
        .unwrap()
        .join()
        .unwrap();
    let bytes = buffer.0.lock().unwrap().clone();
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()["fields"].clone())
        .collect()
}

/// `print` (baseunits/lua/LuaBase.pas:53-65) sends every argument to the log as its own line:
/// booleans as `true`/`false`, anything else through `luaToString` (numbers as Lua formats them,
/// other values as empty strings).
#[test]
fn print_logs_each_argument_with_module_and_thread() {
    let events = log_of(
        Some("MangaDex"),
        "worker-3",
        "print('hello', 1, 2.5, true, false, nil, {}, 'a\\0b')",
    );
    let messages: Vec<&str> = events
        .iter()
        .map(|e| e["message"].as_str().unwrap())
        .collect();
    assert_eq!(
        messages,
        ["hello", "1", "2.5", "true", "false", "", "", "a"]
    );
    for event in &events {
        assert_eq!(event["module"], "MangaDex", "{event}");
        assert_eq!(event["thread"], "worker-3", "{event}");
    }
}

/// Without a module the log lines still carry the thread.
#[test]
fn print_without_a_module_logs_the_thread_only() {
    let events = log_of(None, "worker-4", "print('x')");
    assert_eq!(events.len(), 1, "{events:#?}");
    assert_eq!(events[0]["message"], "x");
    assert!(events[0].get("module").is_none(), "{}", events[0]);
    assert_eq!(events[0]["thread"], "worker-4");
}

/// Runs `chunk` on a fresh runtime with `globals`, returning how long it took.
fn time_of(globals: Globals, chunk: &str) -> Duration {
    let runtime = Runtime::new().unwrap();
    runtime.install_globals(globals).unwrap();
    let start = Instant::now();
    runtime.exec(chunk).unwrap();
    start.elapsed()
}

/// `sleep(ms)` (baseunits/lua/LuaBase.pas:67-71) blocks for `lua_tointeger(L, 1)` milliseconds:
/// numeric strings convert, anything else (including non-integral numbers) is 0.
#[test]
fn sleep_blocks_for_the_given_milliseconds() {
    assert!(time_of(Globals::default(), "sleep(150)") >= Duration::from_millis(150));
    assert!(time_of(Globals::default(), "sleep('150')") >= Duration::from_millis(150));
    let instant = time_of(
        Globals::default(),
        "sleep() sleep(nil) sleep('x') sleep(1.5) sleep(-5) sleep({})",
    );
    assert!(instant < Duration::from_millis(100), "{instant:?}");
}

/// `sleep` blocks only the worker that calls it: two workers sleeping at once overlap.
#[test]
fn sleep_blocks_only_the_calling_worker() {
    let start = Instant::now();
    let workers: Vec<_> = (0..2)
        .map(|_| std::thread::spawn(|| time_of(Globals::default(), "sleep(300)")))
        .collect();
    for worker in workers {
        assert!(worker.join().unwrap() >= Duration::from_millis(300));
    }
    let total = start.elapsed();
    assert!(total < Duration::from_millis(550), "{total:?}");
}

/// Terminating the worker's token cuts a `sleep` short.
#[test]
fn sleep_returns_early_when_the_worker_is_terminated() {
    let token = TerminateToken::new();
    let terminator = token.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        terminator.terminate();
    });
    let globals = Globals {
        terminate: Some(token),
        ..Globals::default()
    };
    let elapsed = time_of(globals, "sleep(10000)");
    assert!(elapsed >= Duration::from_millis(100), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
}
