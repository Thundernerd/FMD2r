//! `get_info` against a website that rate-limits (issue #232).
//!
//! FMD2's `HTTP.GET` is true for any response with a body (baseunits/httpsendthread.pas:718), so a
//! module parses a rate-limit page as the series. `get_info` reports such an answer instead.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fmd_core::info::{InfoError, InfoOptions, get_info};
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{Module, ModuleRegistry, PoolConfig, WorkerPool};

/// Cloudflare's "Error 1015" page, as Weeb Central sent it.
const CLOUDFLARE_1015: &str = r#"<!DOCTYPE html>
<html class="no-js" lang="en-US"><head><title>Access denied | site.test used Cloudflare to restrict access</title></head>
<body><div id="cf-wrapper"><div id="cf-error-details" class="cf-error-details-wrapper">
<h1><span class="cf-error-type" data-translate="error">Error</span><span class="cf-error-code">1015</span></h1>
<h2 class="cf-subheadline" data-translate="error_desc">You are being rate limited</h2>
</div></div></body></html>"#;

/// Answers each URL from a table (404 with an empty body for anything else).
#[derive(Default)]
struct Site {
    pages: Mutex<HashMap<String, WireResponse>>,
}

impl Site {
    fn set(&self, path: &str, status: u16, headers: &[(&str, &str)], body: &str) {
        let response = WireResponse {
            status,
            reason: String::new(),
            headers: headers
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect(),
            body: body.as_bytes().to_vec(),
        };
        let url = format!("https://site.test{path}");
        self.pages.lock().unwrap().insert(url, response);
    }
}

impl Transport for Site {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        let response = self
            .pages
            .lock()
            .unwrap()
            .get(&request.url)
            .map(|r| WireResponse {
                status: r.status,
                reason: r.reason.clone(),
                headers: r.headers.clone(),
                body: r.body.clone(),
            })
            .unwrap_or(WireResponse {
                status: 404,
                reason: String::new(),
                headers: Vec::new(),
                body: Vec::new(),
            });
        Box::pin(async move { Ok(response) })
    }
}

/// Like Weeb Central's `GetInfo` (fixtures/lua/modules/WeebCentral.lua:64-77): the title from the
/// first `h1` (by pattern: these tests build `fmd-lua` without XPath), `no_error` whenever the
/// page has a body.
const MODULE: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'site'; m.Name = 'Site'; m.RootURL = 'https://site.test'
  m.OnGetInfo = 'GetInfo'
end

function GetInfo()
  if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then return net_problem end
  MANGAINFO.Title = HTTP.Document.ToString():match('<h1>(.-)</h1>') or ''
  return no_error
end
"#;

struct Fixture {
    _dir: tempfile::TempDir,
    site: Arc<Site>,
    module: Arc<Module>,
    pool: WorkerPool,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let lua = dir.path().join("lua");
        fs::create_dir_all(lua.join("modules")).unwrap();
        fs::write(lua.join("modules/Site.lua"), MODULE).unwrap();
        let report = ModuleRegistry::load_dir(&lua);
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        let site = Arc::new(Site::default());
        let http = HttpClient::with_transport(site.clone()).unwrap();
        let mut config = PoolConfig::new(http);
        config.lua_dir = lua;
        Fixture {
            module: report.registry.get("site").unwrap().clone(),
            pool: WorkerPool::new(config).unwrap(),
            site,
            _dir: dir,
        }
    }

    async fn get_info(&self, link: &str) -> Result<fmd_core::info::MangaInfo, InfoError> {
        get_info(&self.pool, &self.module, link, InfoOptions::default()).await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_429_rate_limit_page_is_rate_limited_with_its_retry_after() {
    let fx = Fixture::new();
    fx.site
        .set("/series/1", 429, &[("Retry-After", "120")], CLOUDFLARE_1015);

    let err = fx.get_info("/series/1").await.unwrap_err();

    assert_eq!(
        err,
        InfoError::RateLimited {
            website: "Site".into(),
            status: 429,
            retry_after: Some(Duration::from_secs(120)),
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_403_cloudflare_error_page_is_rate_limited_without_a_retry_after() {
    let fx = Fixture::new();
    fx.site.set("/series/1", 403, &[], CLOUDFLARE_1015);

    let err = fx.get_info("/series/1").await.unwrap_err();

    assert_eq!(
        err,
        InfoError::RateLimited {
            website: "Site".into(),
            status: 403,
            retry_after: None,
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_403_that_is_not_a_cloudflare_page_goes_to_the_module_as_in_fmd2() {
    let fx = Fixture::new();
    fx.site.set("/series/1", 403, &[], "<h1>Members only</h1>");

    let info = fx.get_info("/series/1").await.unwrap();

    assert_eq!(info.title, "Members only");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_normal_page_still_parses() {
    let fx = Fixture::new();
    fx.site.set("/series/1", 200, &[], "<h1>A Series</h1>");

    let info = fx.get_info("/series/1").await.unwrap();

    assert_eq!(info.title, "A Series");
}
