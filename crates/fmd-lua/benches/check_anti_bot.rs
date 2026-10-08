//! What the anti-bot hook adds to every request of a module: `____CheckAntiBot` over upstream's
//! `checkantibot.lua` (docs/tickets/T30-anti-bot.md, acceptance: under 50 µs typical).
//!
//! Times `HTTP.GET` answered 200 by an in-memory transport, with and without the hook; the
//! difference is the check. Run with `cargo bench -p fmd-lua --bench check_anti_bot`.

// A bench is test code (CODING_STANDARDS.md); clippy only exempts `#[test]` fns.
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{
    HttpModule, LuaHttp, ModuleHttpOverrides, ModuleHttpSettings, ModuleRegistry, Runtime,
    SettingsStoreError, create_http,
};

struct Ok200;

impl Transport for Ok200 {
    fn send(&self, _: WireRequest) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        Box::pin(async {
            Ok(WireResponse {
                status: 200,
                reason: "OK".into(),
                headers: vec![
                    ("Content-Type".into(), "text/html".into()),
                    ("Server".into(), "cloudflare".into()),
                ],
                body: b"<html>a page</html>".to_vec(),
            })
        })
    }
}

struct NoSettings;

impl ModuleHttpSettings for NoSettings {
    fn http_overrides(&self) -> Option<ModuleHttpOverrides> {
        None
    }
    fn clear_cookies(&self) -> Result<(), SettingsStoreError> {
        Ok(())
    }
    fn store_bypass(&self, _: &str, _: &str) -> Result<(), SettingsStoreError> {
        Ok(())
    }
}

/// The mean time of one `HTTP.GET` over `rounds`.
fn time_gets(rt: &Runtime, rounds: u32) -> Duration {
    rt.exec("for i = 1, 200 do HTTP.GET('https://site.test/') end")
        .unwrap();
    let chunk = format!("for i = 1, {rounds} do HTTP.GET('https://site.test/') end");
    let start = Instant::now();
    rt.exec(&chunk).unwrap();
    start.elapsed() / rounds
}

fn main() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("websitebypass")).unwrap();
    std::fs::create_dir_all(dir.path().join("modules")).unwrap();
    for file in ["checkantibot.lua", "websitebypass.lua"] {
        std::fs::copy(
            fmd_testkit::corpus_root().join("websitebypass").join(file),
            dir.path().join("websitebypass").join(file),
        )
        .unwrap();
    }
    std::fs::write(
        dir.path().join("modules/site.lua"),
        "function Init() local m = NewWebsiteModule(); m.ID = 'site'; m.Name = 'Site' end",
    )
    .unwrap();
    let module = ModuleRegistry::load_dir(dir.path())
        .registry
        .get("site")
        .unwrap()
        .clone();
    let client = HttpClient::with_transport(Arc::new(Ok200)).unwrap();
    let settings: Arc<dyn ModuleHttpSettings> = Arc::new(NoSettings);
    let http_module = HttpModule {
        http: module.http().clone(),
        settings: settings.clone(),
    };

    let runtime = |hook: bool| {
        let rt = Runtime::new().unwrap();
        rt.set_lua_dir(dir.path());
        rt.set_module(&module).unwrap();
        let session = create_http(&client, Some(&http_module));
        let http = if hook {
            LuaHttp::with_website_bypass(session, module.clone(), settings.clone())
        } else {
            LuaHttp::new(session)
        };
        rt.lua()
            .globals()
            .set("HTTP", http.build(rt.lua()).unwrap())
            .unwrap();
        rt
    };
    let (plain, hooked) = (runtime(false), runtime(true));
    let rounds = 20_000;
    let mut best = Duration::MAX;
    for _ in 0..5 {
        let without = time_gets(&plain, rounds);
        let with = time_gets(&hooked, rounds);
        let cost = with.saturating_sub(without);
        println!("GET without hook {without:?}, with hook {with:?}: CheckAntiBot ~{cost:?}");
        best = best.min(cost);
    }
    println!("CheckAntiBot: ~{best:?} per request (best of 5)");
}
