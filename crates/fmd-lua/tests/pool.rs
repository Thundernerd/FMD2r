//! Running module callbacks on the `WorkerPool`, with fixture modules on disk and a stub HTTP
//! transport (docs/tickets/T14-callback-runner-worker-pool.md, "Seams under test").
//!
//! Expected values come from FMD2's `Do*` callback runners (baseunits/lua/LuaWebsiteModules.pas:
//! 154-465), its per-thread handler (baseunits/lua/LuaWebsiteModuleHandler.pas:33-57) and
//! `TLuaHandler.CallFunction` (baseunits/lua/LuaHandler.pas:134-144).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::collections::VecDeque;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{
    Callback, Invalidate, JobError, Module, ModuleHttpOverrides, ModuleHttpSettings, ModuleLimits,
    ModuleRegistry, PoolConfig, SettingsStoreError, Task, UpdateList, WorkerPool,
};

/// A transport that records every request and answers from a script.
#[derive(Default)]
struct StubTransport {
    requests: Mutex<Vec<WireRequest>>,
    responses: Mutex<VecDeque<WireResponse>>,
    /// Never answer, until the session is terminated.
    hang: AtomicBool,
}

impl Transport for StubTransport {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        self.requests.lock().unwrap().push(request);
        if self.hang.load(Ordering::SeqCst) {
            return Box::pin(std::future::pending());
        }
        let next = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| TransportError("no scripted response left".into()));
        Box::pin(async move { next })
    }
}

/// The ticket's fixture module, with ID `t`.
const T: &str = r#"
function Init() local m = NewWebsiteModule(); m.ID='t'; m.Name='T'; m.RootURL='https://t'; m.OnGetInfo='GetInfo'; m.OnGetNameAndLink='GNL'; m.OnGetPageNumber='GPN' end
calls = 0
function GetInfo() calls = calls + 1; MANGAINFO.Title = 'X'..calls; MANGAINFO.ChapterLinks.Add('/c1'); return no_error end
function GNL() LINKS.Add('/m'..URL); NAMES.Add('n'); return no_error end
function GPN() TASK.PageLinks.Add('p1'); TASK.PageLinks.Add('p2'); return true end
"#;

/// Another module, with ID `u`.
const U: &str = r#"
function Init() local m = NewWebsiteModule(); m.ID='u'; m.Name='U'; m.RootURL='https://u'; m.OnGetInfo='GetInfo' end
function GetInfo() MANGAINFO.Title = 'U'; return no_error end
"#;

/// A lua dir holding `modules`, the modules it loads, and a pool over it.
struct Fixture {
    dir: tempfile::TempDir,
    registry: ModuleRegistry,
    pool: WorkerPool,
    transport: Arc<StubTransport>,
}

impl Fixture {
    fn new(threads: usize, modules: &[(&str, &str)]) -> Fixture {
        Self::with(threads, modules, |_| {})
    }

    fn with(
        threads: usize,
        modules: &[(&str, &str)],
        setup: impl FnOnce(&mut PoolConfig),
    ) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("modules")).unwrap();
        for (name, source) in modules {
            fs::write(dir.path().join("modules").join(name), source).unwrap();
        }
        let report = ModuleRegistry::load_dir(dir.path());
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        let transport = Arc::new(StubTransport::default());
        let http = HttpClient::with_transport(transport.clone()).unwrap();
        let mut config = PoolConfig::new(http);
        config.threads = threads;
        config.lua_dir = dir.path().to_path_buf();
        setup(&mut config);
        Fixture {
            dir,
            registry: report.registry,
            pool: WorkerPool::new(config).unwrap(),
            transport,
        }
    }

    fn module(&self, id: &str) -> &Arc<Module> {
        self.registry.get(id).unwrap()
    }
}

#[test]
fn globals_persist_on_a_worker_until_it_switches_module() {
    let f = Fixture::new(1, &[("T.lua", T), ("U.lua", U)]);
    let title = |id: &str| {
        let reply = f.pool.on(f.module(id)).get_info("/s").wait().unwrap();
        reply.value.info.title
    };

    assert_eq!(title("t"), "X1");
    assert_eq!(title("t"), "X2");
    assert_eq!(title("u"), "U");
    assert_eq!(title("t"), "X1");
}

#[test]
fn get_name_and_link_sees_the_page_index_as_a_string_url() {
    let f = Fixture::new(1, &[("T.lua", T)]);

    let reply = f
        .pool
        .on(f.module("t"))
        .get_name_and_link(UpdateList::default(), 0)
        .wait()
        .unwrap();

    assert_eq!(reply.value.status, 0);
    assert_eq!(reply.value.links, ["/m0"]);
    assert_eq!(reply.value.names, ["n"]);
}

#[test]
fn get_page_number_fills_the_task_page_links() {
    let f = Fixture::new(1, &[("T.lua", T)]);

    let reply = f
        .pool
        .on(f.module("t"))
        .get_page_number(Task::default(), "/c1")
        .wait()
        .unwrap();

    assert!(reply.value.ok);
    assert_eq!(reply.value.task.page_links, ["p1", "p2"]);
}

#[test]
fn a_callback_that_raises_is_an_error_and_the_worker_goes_on() {
    let raises = r#"
function Init() local m = NewWebsiteModule(); m.ID='r'; m.Name='R'; m.OnGetInfo='GetInfo'; m.OnGetPageNumber='GPN' end
function GetInfo() error('boom') end
function GPN() TASK.PageLinks.Add('p'); return true end
"#;
    let f = Fixture::new(1, &[("R.lua", raises)]);

    let error = f
        .pool
        .on(f.module("r"))
        .get_info("/s")
        .wait()
        .err()
        .unwrap();
    let JobError::Callback(error) = error else {
        panic!("{error:?}")
    };
    assert_eq!(error.module, "r");
    assert_eq!(error.callback, Callback::OnGetInfo);
    assert_eq!(error.function, "GetInfo");
    assert!(error.message.contains("boom"), "{}", error.message);
    assert!(error.traceback.contains("GetInfo"), "{}", error.traceback);

    let reply = f
        .pool
        .on(f.module("r"))
        .get_page_number(Task::default(), "/c");
    assert_eq!(reply.wait().unwrap().value.task.page_links, ["p"]);
}

#[test]
fn every_seventeenth_callback_starts_after_a_full_collection() {
    // Automatic collection is off, so only the pool's full collections run finalizers.
    let gc = r#"
function Init() local m = NewWebsiteModule(); m.ID='g'; m.Name='G'; m.OnGetInfo='GetInfo' end
collectgarbage('stop')
finalized = 0
function GetInfo()
  MANGAINFO.Title = tostring(finalized)
  setmetatable({}, { __gc = function() finalized = finalized + 1 end })
  return no_error
end
"#;
    let f = Fixture::new(1, &[("G.lua", gc)]);
    let finalized = || {
        let reply = f.pool.on(f.module("g")).get_info("/s").wait().unwrap();
        reply.value.info.title
    };

    let seen: Vec<String> = (0..33).map(|_| finalized()).collect();

    // Calls 1-16 see no collection; call 17 sees the garbage of calls 1-16 (the 16 tables plus
    // the MANGAINFO objects) finalized; calls 18-32 see nothing new; call 33 the next 16.
    assert!(seen[..16].iter().all(|s| s == "0"), "{seen:?}");
    assert_eq!(seen[16], "16");
    assert!(seen[17..32].iter().all(|s| s == "16"), "{seen:?}");
    assert_eq!(seen[32], "32");
}

const LIST: &str = r#"
function Init()
  local m = NewWebsiteModule(); m.ID='l'; m.Name='L'
  m.OnBeforeUpdateList='Before'; m.OnGetDirectoryPageNumber='Pages'; m.OnAfterUpdateList='After'
end
function Before() UPDATELIST.CurrentDirectoryPageNumber = 3; UPDATELIST.UpdateStatusText('go'); return true end
function Pages()
  UPDATELIST.UpdateStatusText(math.type(PAGENUMBER)..' '..PAGENUMBER..' '..math.type(WORKPTR)..' '..WORKPTR)
  PAGENUMBER = 7
  return no_error
end
function After() return 'a string' end
"#;

#[test]
fn update_list_callbacks_get_integer_page_globals_and_read_page_number_back() {
    let f = Fixture::new(1, &[("L.lua", LIST)]);
    let on = || f.pool.on(f.module("l"));

    let before = on()
        .before_update_list(UpdateList::default())
        .wait()
        .unwrap();
    assert!(before.value.ok);
    assert_eq!(before.value.list.current_directory_page_number, 3);
    assert_eq!(before.value.list.status_text.as_deref(), Some("go"));

    let pages = on()
        .get_directory_page_number(before.value.list, 1, 2)
        .wait()
        .unwrap();
    assert_eq!(pages.value.status, 0);
    assert_eq!(pages.value.page, 7);
    assert_eq!(pages.value.list.current_directory_page_number, 3);
    assert_eq!(
        pages.value.list.status_text.as_deref(),
        Some("integer 1 integer 2")
    );

    // `lua_toboolean` reads any string as true.
    let after = on()
        .after_update_list(UpdateList::default())
        .wait()
        .unwrap();
    assert!(after.value.ok);
}

#[test]
fn a_callback_the_module_does_not_declare_is_not_run() {
    let f = Fixture::new(1, &[("L.lua", LIST)]);

    let error = f
        .pool
        .on(f.module("l"))
        .get_info("/s")
        .wait()
        .err()
        .unwrap();

    assert!(
        matches!(&error, JobError::NoCallback { module, callback: Callback::OnGetInfo } if module == "l"),
        "{error:?}"
    );
}

const DOWNLOAD: &str = r#"
function Init()
  local m = NewWebsiteModule(); m.ID='d'; m.Name='D'
  m.OnTaskStart='Start'; m.OnGetImageURL='ImageURL'; m.OnBeforeDownloadImage='Before'
  m.OnDownloadImage='Download'; m.OnSaveImage='Save'; m.OnAfterImageSaved='Saved'
end
function Start() TASK.PageNumber = TASK.PageLinks.Count; TASK.Link = 'started'; return true end
function ImageURL() TASK.PageLinks[WORKID] = URL .. ' ' .. math.type(WORKID); return true end
function Before() HTTP.Headers.Values['Referer'] = URL; return false end
function Download() return HTTP.GET(URL) end
function Save() return PATH .. '/' .. FILENAME .. '-' .. WORKID .. '.jpg' end
function Saved() return FILENAME == '/out/001-1.jpg' and WORKID == 1 end
"#;

#[test]
fn download_callbacks_see_their_globals_and_task() {
    let f = Fixture::new(1, &[("D.lua", DOWNLOAD)]);
    let on = || f.pool.on(f.module("d"));
    let task = Task {
        page_links: vec!["a".into(), "b".into()],
        ..Task::default()
    };

    let started = on().task_start(task).wait().unwrap().value;
    assert!(started.ok);
    assert_eq!(started.task.page_number, 2);
    assert_eq!(started.task.link, "started");

    let image = on()
        .get_image_url(started.task, 1, "/p/2")
        .wait()
        .unwrap()
        .value;
    assert!(image.ok);
    assert_eq!(image.task.page_links, ["a", "/p/2 integer"]);

    let before = on()
        .before_download_image(image.task.clone(), 1, "https://d/img")
        .wait()
        .unwrap();
    assert!(!before.value.ok);
    let session = before.http.unwrap();
    assert_eq!(session.headers().value("Referer"), "https://d/img");

    f.transport
        .responses
        .lock()
        .unwrap()
        .push_back(WireResponse {
            status: 200,
            reason: String::new(),
            headers: vec![],
            body: b"img".to_vec(),
        });
    let download = on()
        .with_http(session)
        .download_image(image.task, 1, "https://d/img")
        .wait()
        .unwrap();
    assert!(download.value.ok);
    assert_eq!(download.http.unwrap().document(), b"img");
    let request = f.transport.requests.lock().unwrap()[0].clone();
    assert!(
        request
            .headers
            .contains(&("Referer".to_string(), "https://d/img".to_string())),
        "{:?}",
        request.headers
    );

    let saved = on().save_image(1, "/out", "001").wait().unwrap();
    assert_eq!(saved.value, "/out/001-1.jpg");
    assert!(
        on().after_image_saved(1, "/out/001-1.jpg")
            .wait()
            .unwrap()
            .value
    );
}

#[test]
fn account_callbacks_see_the_account_status_constants() {
    let account = r#"
function Init()
  local m = NewWebsiteModule(); m.ID='a'; m.Name='A'
  m.OnLogin='Login'; m.OnAccountState='State'; m.OnCheckSite='Check'
end
function Login()
  return HTTP ~= nil and asUnknown == 0 and asChecking == 1 and asValid == 2 and asInvalid == 3
end
function State() return math.type(asValid) == 'integer' end
function Check() return nil end
"#;
    let f = Fixture::new(1, &[("A.lua", account)]);
    let on = || f.pool.on(f.module("a"));

    let login = on().login().wait().unwrap();
    assert!(login.value);
    assert!(login.http.is_some());
    assert!(on().account_state().wait().unwrap().value);
    assert!(!on().check_site().wait().unwrap().value);
}

#[test]
fn a_callback_that_returns_nothing_reads_what_the_last_one_left_on_the_stack() {
    // FMD2 never clears the stack, so `lua_toboolean(L, -1)` after a callback without results
    // reads the previous callback's result (baseunits/lua/LuaWebsiteModules.pas:449-465).
    let quiet = r#"
function Init()
  local m = NewWebsiteModule(); m.ID='q'; m.Name='Q'; m.OnAccountState='Yes'; m.OnCheckSite='Nothing'
end
function Yes() return true end
function Nothing() end
"#;
    let f = Fixture::new(1, &[("Q.lua", quiet)]);
    let on = || f.pool.on(f.module("q"));

    assert!(!on().check_site().wait().unwrap().value);
    assert!(on().account_state().wait().unwrap().value);
    assert!(on().check_site().wait().unwrap().value);
}

#[test]
fn invalidating_a_module_rebuilds_its_state_from_the_file() {
    let f = Fixture::new(1, &[("T.lua", T), ("U.lua", U)]);
    let title = |id: &str| {
        let reply = f.pool.on(f.module(id)).get_info("/s").wait().unwrap();
        reply.value.info.title
    };
    assert_eq!(title("t"), "X1");
    fs::write(
        f.dir.path().join("modules/T.lua"),
        T.replace("'X'..calls", "'Y'..calls"),
    )
    .unwrap();

    // A rebuilt state runs the cached bytecode until the module is invalidated.
    assert_eq!(title("u"), "U");
    assert_eq!(title("t"), "X1");
    f.pool.invalidate(Invalidate::Module("u".into()));
    assert_eq!(title("t"), "X2");
    f.pool.invalidate(Invalidate::Module("t".into()));
    assert_eq!(title("t"), "Y1");

    fs::write(f.dir.path().join("modules/T.lua"), T).unwrap();
    f.pool.invalidate(Invalidate::All);
    assert_eq!(title("t"), "X1");
}

#[test]
fn a_job_can_be_awaited_but_not_waited_for_inside_a_runtime() {
    let f = Fixture::new(1, &[("T.lua", T)]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    let reply = runtime.block_on(f.pool.on(f.module("t")).get_info("/s"));
    assert_eq!(reply.unwrap().value.info.title, "X1");

    let waited = runtime.block_on(async { f.pool.on(f.module("t")).get_info("/s").wait() });
    assert!(
        matches!(waited, Err(JobError::InsideRuntime)),
        "{:?}",
        waited.err()
    );
}

#[test]
fn terminating_a_job_aborts_its_http_request() {
    let hangs = r#"
function Init() local m = NewWebsiteModule(); m.ID='h'; m.Name='H'; m.OnGetInfo='GetInfo' end
function GetInfo()
  MANGAINFO.Title = tostring(HTTP.GET('https://h/x')) .. ' ' .. tostring(HTTP.Terminated)
  return no_error
end
"#;
    let f = Fixture::new(1, &[("H.lua", hangs)]);
    f.transport.hang.store(true, Ordering::SeqCst);

    let pending = f.pool.on(f.module("h")).get_info("/s");
    while f.transport.requests.lock().unwrap().is_empty() {
        std::thread::sleep(Duration::from_millis(5));
    }
    pending.terminate();

    assert_eq!(pending.wait().unwrap().value.info.title, "false true");
}

#[test]
fn module_limits_are_what_the_module_object_holds() {
    let limited = r#"
function Init()
  local m = NewWebsiteModule(); m.ID='m'; m.Name='M'; m.OnTaskStart='Start'
  m.MaxTaskLimit = 2; m.MaxThreadPerTaskLimit = 3; m.MaxConnectionLimit = 4
end
function Start() MODULE.MaxTaskLimit = 5; return true end
"#;
    let f = Fixture::new(1, &[("M.lua", limited)]);
    let module = f.module("m");
    let declared = ModuleLimits {
        max_task_limit: 2,
        max_thread_per_task_limit: 3,
        max_connection_limit: 4,
    };
    assert_eq!(module.limits(), declared);

    // A callback may change them, as FMD2's `MODULE` writes the container's fields
    // (baseunits/lua/LuaWebsiteModules.pas:1001-1003).
    f.pool
        .on(module)
        .task_start(Task::default())
        .wait()
        .unwrap();
    let changed = ModuleLimits {
        max_task_limit: 5,
        ..declared
    };
    assert_eq!(module.limits(), changed);
}

struct UserAgent;

impl ModuleHttpSettings for UserAgent {
    fn http_overrides(&self) -> Option<ModuleHttpOverrides> {
        Some(ModuleHttpOverrides {
            user_agent: "UA/1".into(),
            ..ModuleHttpOverrides::default()
        })
    }

    fn clear_cookies(&self) -> Result<(), SettingsStoreError> {
        Ok(())
    }

    fn store_bypass(&self, _: &str, _: &str) -> Result<(), SettingsStoreError> {
        Ok(())
    }
}

#[test]
fn the_http_global_is_a_session_with_the_module_http_settings() {
    let gets = r#"
function Init() local m = NewWebsiteModule(); m.ID='s'; m.Name='S'; m.OnGetInfo='GetInfo' end
function GetInfo() HTTP.GET('https://s/x'); return no_error end
"#;
    let f = Fixture::with(1, &[("S.lua", gets)], |config| {
        config.http_settings = Some(Arc::new(|module: &Module| {
            assert_eq!(module.def().id, "s");
            Arc::new(UserAgent)
        }));
    });

    let reply = f.pool.on(f.module("s")).get_info("/s").wait().unwrap();

    assert!(reply.http.is_some());
    let request = f.transport.requests.lock().unwrap()[0].clone();
    assert!(
        request
            .headers
            .contains(&("User-Agent".to_string(), "UA/1".to_string())),
        "{:?}",
        request.headers
    );
}

#[test]
fn jobs_spread_over_every_worker_each_with_its_own_state() {
    let f = Fixture::new(4, &[("T.lua", T), ("U.lua", U)]);

    let pending: Vec<_> = (0..40)
        .map(|i| {
            f.pool
                .on(f.module(if i % 2 == 0 { "t" } else { "u" }))
                .get_info("/s")
        })
        .collect();
    let titles: Vec<String> = pending
        .into_iter()
        .map(|p| p.wait().unwrap().value.info.title)
        .collect();

    for (i, title) in titles.iter().enumerate() {
        let module_title = if i % 2 == 0 {
            title.starts_with('X')
        } else {
            title == "U"
        };
        assert!(module_title, "{titles:?}");
    }
}

#[test]
fn jobs_with_the_same_affinity_share_one_worker_and_its_globals() {
    // Like one FMD2 task thread running a whole task's callbacks in its own state.
    let f = Fixture::new(4, &[("T.lua", T)]);
    let worker = f.pool.affinity();

    let pending: Vec<_> = (0..12)
        .map(|_| {
            f.pool
                .on(f.module("t"))
                .with_affinity(worker)
                .get_info("/s")
        })
        .collect();
    let titles: Vec<String> = pending
        .into_iter()
        .map(|p| p.wait().unwrap().value.info.title)
        .collect();

    let expected: Vec<String> = (1..=12).map(|i| format!("X{i}")).collect();
    assert_eq!(titles, expected);
}

#[test]
fn the_pool_and_its_jobs_cross_threads_but_lua_states_do_not() {
    fn send_sync<T: Send + Sync>() {}
    fn send<T: Send>() {}
    send_sync::<WorkerPool>();
    send::<fmd_lua::Pending<fmd_lua::JobResult>>();
    send::<fmd_lua::Job>();
    // `Runtime` (one Lua state) is `!Send`: see the `compile_fail` example on its docs.
}
