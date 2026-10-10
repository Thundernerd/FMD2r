//! FMD2-DB import of a `<module id>.7z` dump in FMD2's per-site schema
//! (baseunits/DBDataProcess.pas:143-153) into `masterlist`
//! (docs/tickets/T26-discover-list-update.md).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use fmd_core::jobs::Job;
use fmd_core::lists::{
    DbImporter, ImportError, ListEvent, ListEventKind, ListFailureReason, ListJobs, ListUpdater,
    db_url,
};
use fmd_core::settings::SettingsService;
use fmd_http::{
    BoxFuture, HttpClient, TerminateToken, Transport, TransportError, WireRequest, WireResponse,
};
use fmd_lua::{ModuleRegistry, PoolConfig, WorkerPool};
use fmd_store::{AppDb, ListsDb, MangaListing, PageRequest, SearchFilters};

const GOURMET: &str = "598672e8158d4fd781bea8d426534695";
const LATIN1: &str = "201234a2c811487c8542fb7ec2c92b20";

fn archive(id: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/fmd2-db/{id}.7z",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// Serves one body for every URL and records the URLs asked for.
struct Server {
    status: u16,
    body: Vec<u8>,
    urls: Mutex<Vec<String>>,
}

impl Transport for Server {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        self.urls.lock().unwrap().push(request.url);
        let response = WireResponse {
            status: self.status,
            reason: String::new(),
            headers: Vec::new(),
            body: self.body.clone(),
        };
        Box::pin(async move { Ok(response) })
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    lists: ListsDb,
    server: Arc<Server>,
    importer: DbImporter,
}

fn fixture(status: u16, body: Vec<u8>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
    let server = Arc::new(Server {
        status,
        body,
        urls: Mutex::default(),
    });
    let http = HttpClient::with_transport(server.clone()).unwrap();
    let importer = DbImporter::new(http, lists.clone());
    Fixture {
        _dir: dir,
        lists,
        server,
        importer,
    }
}

fn rows(lists: &ListsDb, module: &str) -> Vec<MangaListing> {
    let filters = SearchFilters {
        module_ids: vec![module.into()],
        ..SearchFilters::default()
    };
    let page = PageRequest {
        offset: 0,
        limit: 100,
    };
    let results = lists.masterlist().search("", &filters, page).unwrap();
    results.entries.into_iter().map(|e| e.listing).collect()
}

#[test]
fn an_fmd2_db_dump_becomes_the_modules_list_with_jdn_as_added_jdn() {
    let f = fixture(200, archive(GOURMET));

    let imported = f
        .importer
        .import_archive(GOURMET, &archive(GOURMET))
        .unwrap();

    assert_eq!(imported, 3);
    let rows = rows(&f.lists, GOURMET);
    assert_eq!(rows.len(), 3);
    let goblin = &rows[0];
    assert_eq!(goblin.link, "/project/a-kind-goblins-bird/");
    assert_eq!(goblin.title, "A Kind Goblin’s Bird");
    assert_eq!(goblin.alttitles, "");
    assert_eq!(goblin.authors, "8cat, AB");
    assert_eq!(goblin.artists, "8cat");
    assert_eq!(goblin.genres, "Drama, Fantasy, Historical, Josei, Romance");
    assert_eq!(goblin.status, "1");
    assert_eq!(goblin.summary, "“Won’t you become a Goblin’s bride?”");
    assert_eq!(goblin.numchapter, 26);
    assert_eq!(goblin.added_jdn, 2459252);
    assert_eq!(
        rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        [
            "A Kind Goblin’s Bird",
            "I Became the Villain’s Mother",
            "Lady Baby"
        ]
    );
}

#[test]
fn an_import_replaces_the_modules_list_and_leaves_other_modules_alone() {
    let f = fixture(200, Vec::new());
    let stale = MangaListing {
        link: "/stale".into(),
        title: "Stale".into(),
        ..MangaListing::default()
    };
    f.lists.masterlist().upsert(GOURMET, &stale).unwrap();
    f.lists.masterlist().upsert("other", &stale).unwrap();

    f.importer
        .import_archive(GOURMET, &archive(GOURMET))
        .unwrap();

    assert_eq!(rows(&f.lists, GOURMET).len(), 3);
    assert!(rows(&f.lists, GOURMET).iter().all(|r| r.link != "/stale"));
    assert_eq!(rows(&f.lists, "other").len(), 1);
}

#[test]
fn text_that_is_not_utf8_is_imported_lossily() {
    let f = fixture(200, Vec::new());

    let imported = f.importer.import_archive(LATIN1, &archive(LATIN1)).unwrap();

    assert_eq!(imported, 8);
    let rows = rows(&f.lists, LATIN1);
    let fragtime = rows.iter().find(|r| r.title == "Fragtime").unwrap();
    assert!(fragtime.summary.contains('\u{fffd}'));
    assert_eq!(fragtime.added_jdn, 2458650);
}

#[test]
fn the_dump_is_downloaded_from_the_url_template_with_the_module_id() {
    // `GetDBURL(FModule.ID)` (baseunits/DBUpdater.pas:56-63, :125).
    let template = "https://raw.githubusercontent.com/dazedcat19/FMD2-DB/master/7z/<website>.7z";
    assert_eq!(
        db_url(template, GOURMET),
        format!("https://raw.githubusercontent.com/dazedcat19/FMD2-DB/master/7z/{GOURMET}.7z")
    );
    // `Pos('<website>', DB_URL) <> -1` is always true, so nothing is appended (:58-61).
    assert_eq!(
        db_url("https://mirror.test/db.7z", "x"),
        "https://mirror.test/db.7z"
    );
    assert_eq!(
        db_url("https://m.test/<WEBSITE>.7z", "x"),
        "https://m.test/x.7z"
    );

    let f = fixture(200, archive(GOURMET));
    let mut steps = Vec::new();
    let imported = f
        .importer
        .import(GOURMET, template, &TerminateToken::new(), &mut |s| {
            steps.push(s.to_owned())
        })
        .unwrap();

    assert_eq!(imported, 3);
    // `RS_Downloading`, `RS_Extracting` (baseunits/DBUpdater.pas:123, :166).
    assert_eq!(steps, ["Downloading...", "Extracting..."]);
    assert_eq!(*f.server.urls.lock().unwrap(), [db_url(template, GOURMET)]);
}

/// Keeps one row in `GOURMET`'s list and imports from `f`, returning the error.
fn failed_import(f: &Fixture) -> ImportError {
    let kept = MangaListing {
        link: "/kept".into(),
        ..MangaListing::default()
    };
    f.lists.masterlist().upsert(GOURMET, &kept).unwrap();
    f.importer
        .import(
            GOURMET,
            "https://db.test/<website>.7z",
            &TerminateToken::new(),
            &mut |_| {},
        )
        .unwrap_err()
}

#[test]
fn a_404_means_fmd2_db_has_no_dump_and_leaves_the_list_as_it_was() {
    let f = fixture(404, b"Not Found".to_vec());

    let error = failed_import(&f);

    // `HTTP.ResultCode < 300` fails (baseunits/DBUpdater.pas:125); a 404 is a missing dump.
    assert!(
        matches!(error, ImportError::Download { status: 404, .. }),
        "{error:?}"
    );
    assert_eq!(error.reason(), ListFailureReason::NoDump);
    assert_eq!(rows(&f.lists, GOURMET).len(), 1);
}

#[test]
fn a_server_error_means_fmd2_db_could_not_be_reached() {
    let f = fixture(500, b"Internal Server Error".to_vec());

    let error = failed_import(&f);

    assert_eq!(error.reason(), ListFailureReason::Unreachable, "{error:?}");
    assert_eq!(rows(&f.lists, GOURMET).len(), 1);
}

/// Fails every exchange, as a refused connection does.
struct Offline;

impl Transport for Offline {
    fn send(&self, _: WireRequest) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        Box::pin(async { Err(TransportError("connection refused".into())) })
    }
}

#[test]
fn a_connection_error_means_fmd2_db_could_not_be_reached() {
    let mut f = fixture(200, Vec::new());
    let http = HttpClient::with_transport(Arc::new(Offline)).unwrap();
    f.importer = DbImporter::new(http, f.lists.clone());

    let error = failed_import(&f);

    assert_eq!(error.reason(), ListFailureReason::Unreachable, "{error:?}");
    assert_eq!(rows(&f.lists, GOURMET).len(), 1);
}

#[test]
fn a_body_that_is_not_a_7z_archive_is_a_bad_archive() {
    let f = fixture(200, b"<html>rate limited</html>".to_vec());

    let error = failed_import(&f);

    assert!(matches!(error, ImportError::Archive(_)), "{error:?}");
    assert_eq!(error.reason(), ListFailureReason::BadArchive);
    assert_eq!(rows(&f.lists, GOURMET).len(), 1);
}

#[test]
fn an_empty_download_is_a_bad_archive() {
    // `HTTP.GET` is false on an empty body (baseunits/DBUpdater.pas:125).
    let f = fixture(200, Vec::new());

    let error = failed_import(&f);

    assert_eq!(error.reason(), ListFailureReason::BadArchive, "{error:?}");
    assert_eq!(rows(&f.lists, GOURMET).len(), 1);
}

/// Runs `ListJobs::import_db` of a website that can build its own list against a 404 and
/// returns the job's last error once it ended.
fn failed_import_job_message(f: Fixture) -> String {
    let lua = f._dir.path().join("lua/modules");
    std::fs::create_dir_all(&lua).unwrap();
    std::fs::write(
        lua.join("Site.lua"),
        "function Init()\n  local m = NewWebsiteModule()\n  m.ID = 'site'; m.Name = 'Site'; \
         m.RootURL = 'https://site.test'; m.OnGetNameAndLink = 'GetNameAndLink'\nend\n",
    )
    .unwrap();
    let report = ModuleRegistry::load_dir(&f._dir.path().join("lua"));
    let module = report.registry.get("site").unwrap().clone();
    let http = HttpClient::with_transport(f.server.clone()).unwrap();
    let mut config = PoolConfig::new(http);
    config.threads = 1;
    let pool = Arc::new(WorkerPool::new(config).unwrap());
    let db = AppDb::open(f._dir.path().join("app.db")).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let jobs = ListJobs::new(
        ListUpdater::new(pool, f.lists.clone()),
        f.importer,
        Arc::new(SettingsService::load(db).unwrap()),
        move |id: &str| (id == "site").then(|| module.clone()),
        move |event: ListEvent| {
            let _ = tx.send(event.kind);
        },
    );
    jobs.import_db("site").unwrap();
    while rx.recv_timeout(Duration::from_secs(10)).unwrap() != ListEventKind::Failed {}
    jobs.status().last_error.unwrap()
}

#[test]
fn a_missing_dump_reads_as_no_ready_made_list_without_naming_fmd2_db() {
    let f = fixture(404, b"Not Found".to_vec());

    let last_error = failed_import_job_message(f);
    // The details after it keep the URL, which names the upstream project.
    let (message, _details) = last_error.split_once("\n\nDetails:").unwrap();

    assert!(
        message.starts_with(
            "There is no ready-made list for Site yet. Use Update list to build it from the website."
        ),
        "{message}"
    );
    assert!(!message.contains("FMD2-DB"), "{message}");
}
