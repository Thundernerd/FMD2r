//! The list update job against a fixture module (docs/tickets/T26-discover-list-update.md).
//!
//! Expected behaviour: `TUpdateListManagerThread.Execute` and `TUpdateListThread`
//! (baseunits/uUpdateThread.pas:173-313, :626-780, :842-903).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};

use fmd_core::lists::{ListProgress, ListUpdater, UpdateOptions};
use fmd_http::{
    BoxFuture, HttpClient, TerminateToken, Transport, TransportError, WireRequest, WireResponse,
};
use fmd_lua::{Module, ModuleRegistry, PoolConfig, WorkerPool};
use fmd_store::{ListsDb, PageRequest, SearchFilters};

/// Answers each URL from a table (404 with an empty body for anything else) and counts requests.
#[derive(Default)]
struct Site {
    pages: Mutex<HashMap<String, String>>,
    requests: Mutex<Vec<String>>,
}

impl Site {
    fn set(&self, path: &str, body: &str) {
        let url = format!("https://site.test{path}");
        self.pages.lock().unwrap().insert(url, body.into());
    }

    fn requested(&self, path: &str) -> usize {
        let url = format!("https://site.test{path}");
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|u| **u == url)
            .count()
    }
}

impl Transport for Site {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        self.requests.lock().unwrap().push(request.url.clone());
        let body = self.pages.lock().unwrap().get(&request.url).cloned();
        let response = WireResponse {
            status: if body.is_some() { 200 } else { 404 },
            reason: String::new(),
            headers: Vec::new(),
            body: body.unwrap_or_default().into_bytes(),
        };
        Box::pin(async move { Ok(response) })
    }
}

/// A module whose directory has as many pages as `/pages` says; page `i` (`/list/i`) holds one
/// `link name` pair per line, and a title's page holds its genres.
const SITE: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'site'; m.Name = 'Site'; m.RootURL = 'https://site.test'; m.Category = 'English'
  m.SortedList = true
  m.OnGetDirectoryPageNumber = 'GetDirectoryPageNumber'
  m.OnGetNameAndLink = 'GetNameAndLink'
  m.OnGetInfo = 'GetInfo'
end

function GetDirectoryPageNumber()
  if not HTTP.GET(MODULE.RootURL .. '/pages') then return net_problem end
  PAGENUMBER = tonumber(HTTP.Document.ToString())
  return no_error
end

function GetNameAndLink()
  if not HTTP.GET(MODULE.RootURL .. '/list/' .. URL) then return net_problem end
  for line in HTTP.Document.ToString():gmatch('[^\n]+') do
    local link, name = line:match('^(%S+) (.+)$')
    LINKS.Add(MODULE.RootURL .. link)
    NAMES.Add(name)
  end
  return no_error
end

function GetInfo()
  if not HTTP.GET(MANGAINFO.URL) then return net_problem end
  MANGAINFO.Title = 'Info ' .. URL
  MANGAINFO.Genres = HTTP.Document.ToString()
  MANGAINFO.ChapterLinks.Add('/c1')
  MANGAINFO.ChapterLinks.Add('/c2')
  return no_error
end
"#;

struct Fixture {
    _dir: tempfile::TempDir,
    site: Arc<Site>,
    module: Arc<Module>,
    lists: ListsDb,
    updater: ListUpdater,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture::with(SITE)
    }

    fn with(source: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("modules")).unwrap();
        fs::write(dir.path().join("modules/Site.lua"), source).unwrap();
        let report = ModuleRegistry::load_dir(dir.path());
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        let module = report.registry.get("site").unwrap().clone();
        let site = Arc::new(Site::default());
        let mut config = PoolConfig::new(HttpClient::with_transport(site.clone()).unwrap());
        config.threads = 2;
        config.lua_dir = dir.path().to_path_buf();
        let pool = Arc::new(WorkerPool::new(config).unwrap());
        let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
        let updater = ListUpdater::new(pool, lists.clone());
        site.set("/pages", "3");
        site.set("/list/0", "/m/1 One\n/m/2 Two");
        site.set("/list/1", "/m/3 Three\n/m/4 Four");
        site.set("/list/2", "/m/5 Five");
        Fixture {
            _dir: dir,
            site,
            module,
            lists,
            updater,
        }
    }

    fn update(&self, options: &UpdateOptions) -> Vec<ListProgress> {
        let mut progress = Vec::new();
        self.updater
            .update(&self.module, options, &TerminateToken::new(), &mut |p| {
                progress.push(p.clone())
            })
            .unwrap();
        progress
    }

    /// `(link, title)` of every stored row, by title.
    fn rows(&self) -> Vec<(String, String)> {
        let page = PageRequest {
            offset: 0,
            limit: 100,
        };
        let results = self
            .lists
            .masterlist()
            .search("", &SearchFilters::default(), page)
            .unwrap();
        results
            .entries
            .into_iter()
            .map(|e| (e.listing.link, e.listing.title))
            .collect()
    }
}

fn without_info() -> UpdateOptions {
    UpdateOptions {
        max_threads: 1,
        no_manga_info: true,
    }
}

fn pairs(rows: &[(&str, &str)]) -> Vec<(String, String)> {
    rows.iter()
        .map(|(l, t)| (l.to_string(), t.to_string()))
        .collect()
}

#[test]
fn a_full_update_stores_every_title_of_every_directory_page_without_its_host() {
    let f = Fixture::new();

    f.update(&without_info());

    // `RemoveHostFromURLsPair` (baseunits/uUpdateThread.pas:240), stored by title.
    assert_eq!(
        f.rows(),
        pairs(&[
            ("/m/5", "Five"),
            ("/m/4", "Four"),
            ("/m/1", "One"),
            ("/m/3", "Three"),
            ("/m/2", "Two"),
        ])
    );
    assert_eq!(f.site.requested("/pages"), 1);
    for page in 0..3 {
        assert_eq!(f.site.requested(&format!("/list/{page}")), 1);
    }
}

#[test]
fn a_second_run_of_a_sorted_list_stops_after_the_first_page_holding_a_known_title() {
    let f = Fixture::new();
    f.update(&without_info());
    // The site added a title on top, pushing the rest down a page.
    f.site.set("/list/0", "/m/6 Six\n/m/1 One");
    f.site.set("/list/1", "/m/2 Two\n/m/3 Three");
    f.site.set("/list/2", "/m/4 Four\n/m/5 Five");

    f.update(&without_info());

    // baseunits/uUpdateThread.pas:247-251: page 0 holds a listed link, so `GetNext` hands out
    // no further page (:852-853).
    assert_eq!(f.site.requested("/list/0"), 2);
    assert_eq!(f.site.requested("/list/1"), 1);
    assert_eq!(f.site.requested("/list/2"), 1);
    assert_eq!(
        f.rows(),
        pairs(&[
            ("/m/5", "Five"),
            ("/m/4", "Four"),
            ("/m/1", "One"),
            ("/m/6", "Six"),
            ("/m/3", "Three"),
            ("/m/2", "Two"),
        ])
    );
}

#[test]
fn an_unsorted_list_is_walked_in_full_every_time() {
    let f = Fixture::with(&SITE.replace("m.SortedList = true", "m.SortedList = false"));
    f.update(&without_info());
    f.site.set("/list/2", "/m/5 Five\n/m/6 Six");

    f.update(&without_info());

    for page in 0..3 {
        assert_eq!(f.site.requested(&format!("/list/{page}")), 2);
    }
    assert_eq!(f.rows().len(), 6);
}

#[test]
fn with_info_each_new_title_is_stored_as_its_on_get_info_filled_it_in() {
    let f = Fixture::new();
    f.site.set("/list/0", "/m/1 One");
    f.site.set("/list/1", "");
    f.site.set("/list/2", "");
    f.site.set("/m/1", "Action, Comedy,");

    let options = UpdateOptions {
        max_threads: 1,
        no_manga_info: false,
    };
    f.update(&options);

    let page = PageRequest {
        offset: 0,
        limit: 10,
    };
    let entries = f
        .lists
        .masterlist()
        .search("", &SearchFilters::default(), page)
        .unwrap()
        .entries;
    assert_eq!(entries.len(), 1);
    let listing = &entries[0].listing;
    assert_eq!(listing.link, "/m/1");
    assert_eq!(listing.title, "Info /m/1");
    // Trailing commas trimmed (baseunits/uData.pas:123), chapters counted (:206).
    assert_eq!(listing.genres, "Action, Comedy");
    assert_eq!(listing.numchapter, 2);
    assert_eq!(listing.added_jdn, fmd_core::lists::today_jdn());
}

/// Like DynastyScans (lua/modules/DynastyScans.lua): several directories, no
/// `OnGetDirectoryPageNumber`, the page count learnt from the first page.
const DIRECTORIES: &str = r#"
local dirs = {'a', 'b'}
function Init()
  local m = NewWebsiteModule()
  m.ID = 'site'; m.Name = 'Site'; m.RootURL = 'https://site.test'
  m.TotalDirectory = #dirs
  m.OnGetNameAndLink = 'GetNameAndLink'
end

function GetNameAndLink()
  local dir = dirs[MODULE.CurrentDirectoryIndex + 1]
  UPDATELIST.UpdateStatusText('directory ' .. dir .. ' page ' .. URL)
  LINKS.Add('/' .. dir .. '/' .. URL)
  NAMES.Add(dir .. URL)
  UPDATELIST.CurrentDirectoryPageNumber = 2
  return no_error
end
"#;

#[test]
fn every_directory_is_walked_with_its_index_set_and_the_page_count_the_module_raises() {
    let f = Fixture::with(DIRECTORIES);

    let progress = f.update(&without_info());

    let links: Vec<String> = f.rows().into_iter().map(|(link, _)| link).collect();
    assert_eq!(links, ["/a/0", "/a/1", "/b/0", "/b/1"]);
    let texts: Vec<&str> = progress.iter().map(|p| p.status_text.as_str()).collect();
    assert!(texts.contains(&"directory b page 1"), "{texts:?}");
    assert!(
        texts.contains(&"Looking for new title(s) 2/2..."),
        "{texts:?}"
    );
}

#[test]
fn a_terminated_update_of_a_sorted_list_stores_nothing() {
    let f = Fixture::new();
    let terminate = TerminateToken::new();
    terminate.terminate();

    let outcome = f
        .updater
        .update(&f.module, &without_info(), &terminate, &mut |_| {})
        .unwrap();

    assert!(outcome.cancelled);
    assert_eq!(outcome.added, 0);
    assert!(f.rows().is_empty());
}

#[test]
fn a_link_repeated_within_one_page_of_a_listed_sorted_list_stops_it_too() {
    let f = Fixture::new();
    f.update(&without_info());
    // `mainDataProcess.AddData` keeps a page's new links until its `Rollback`, so the repeat
    // counts as listed (baseunits/uUpdateThread.pas:244-252).
    f.site.set("/list/0", "/m/7 Seven\n/m/7 Seven");

    f.update(&without_info());

    assert_eq!(f.site.requested("/list/1"), 1);
    assert!(
        f.rows()
            .contains(&("/m/7".to_string(), "Seven".to_string()))
    );
}
