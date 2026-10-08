//! The worker pool that runs module callbacks: dedicated OS threads, each owning one Lua state
//! cached per module, like FMD2's per-thread `TLuaWebsiteModuleHandler`
//! (baseunits/lua/LuaWebsiteModuleHandler.pas:33-64).

mod callbacks;
mod host_api;
mod pending;
mod worker;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use fmd_http::{HttpClient, HttpSession, TerminateToken};

use crate::module::lock;
use crate::{Module, ModuleHttpSettings, PackageCache, XPathBackend};

pub use callbacks::{
    Answer, Call, Callback, InfoReply, ListReply, MangaInfo, NamesAndLinks, PageCount, Task,
    TaskReply, UpdateList,
};
pub use host_api::missing_host_api;
pub use pending::Pending;

/// Where a module's HTTP overrides come from, given the module a session is created for.
pub type HttpSettingsSource = dyn Fn(&Module) -> Arc<dyn ModuleHttpSettings> + Send + Sync;

/// How a [`WorkerPool`] is set up.
pub struct PoolConfig {
    /// Worker threads; at least one runs.
    pub threads: usize,
    /// The directory holding FMD2's `lua/` tree, for `require` and `fmd.duktape`.
    pub lua_dir: PathBuf,
    /// The client every callback's `HTTP` session is created from.
    pub http: HttpClient,
    /// The module's HTTP overrides for the sessions created for it; none when `None`.
    pub http_settings: Option<Arc<HttpSettingsSource>>,
    /// The XPath backend of `CreateTXQuery`; the runtime's default when `None`.
    pub xpath_backend: Option<XPathBackend>,
}

impl PoolConfig {
    /// One thread per CPU, `lua` in the working directory, no HTTP overrides and the default
    /// XPath backend.
    pub fn new(http: HttpClient) -> PoolConfig {
        PoolConfig {
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
            lua_dir: PathBuf::from("lua"),
            http,
            http_settings: None,
            xpath_backend: None,
        }
    }
}

/// One callback to run on a module.
pub struct Job {
    pub module: Arc<Module>,
    pub call: Call,
    /// The session behind the callback's `HTTP` global; a new one for the module when `None`.
    /// It comes back in [`JobResult::http`].
    pub http: Option<HttpSession>,
    /// Terminating it aborts the callback's HTTP requests, `sleep` and `ExecJS`.
    pub terminate: TerminateToken,
}

/// What a [`Job`] produced.
pub struct JobResult {
    pub answer: Answer,
    /// The session the callback's `HTTP` global used, for the callbacks that set one.
    pub http: Option<HttpSession>,
}

/// What a typed callback method produced: its value and the session its `HTTP` global used.
pub struct Reply<T> {
    pub value: T,
    pub http: Option<HttpSession>,
}

/// Why a job produced no result.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error(transparent)]
    Callback(#[from] CallbackError),
    /// The module declares no function for the callback; FMD2 never runs it then
    /// (`Assigned(...On...)` checks, e.g. baseunits/uData.pas:103).
    #[error("module {module} has no {callback} callback")]
    NoCallback { module: String, callback: Callback },
    #[error("the worker pool has shut down")]
    Closed,
    #[error("blocking wait for a job from inside a tokio runtime")]
    InsideRuntime,
    /// A worker answered with the result of another kind of callback; never happens.
    #[error("the worker answered a different callback")]
    Mismatch,
}

/// A callback that raised an error, or a module chunk that failed while its state was built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("module {module}, {callback} (\"{function}\"): {message}")]
pub struct CallbackError {
    pub module: String,
    pub callback: Callback,
    /// The Lua function the callback names; empty when the module chunk failed.
    pub function: String,
    pub message: String,
    pub traceback: String,
}

/// What [`WorkerPool::invalidate`] marks stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalidate {
    /// The module with this ID.
    Module(String),
    /// Every module, and the files they `require`.
    All,
}

/// A job on its way to a worker, with where its result goes.
struct Envelope {
    job: Job,
    reply: tokio::sync::oneshot::Sender<Result<JobResult, JobError>>,
}

/// Runs module callbacks on dedicated OS threads. Each thread owns one Lua state (which, being
/// `!Send`, never leaves it) and keeps it while its jobs target the same module, so globals
/// persist between callbacks of that module on that thread.
pub struct WorkerPool {
    shared: Arc<Shared>,
    sender: Option<mpsc::Sender<Envelope>>,
    threads: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    /// Starts `config.threads` workers (at least one).
    pub fn new(config: PoolConfig) -> std::io::Result<WorkerPool> {
        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));
        let shared = Arc::new(Shared {
            lua_dir: config.lua_dir,
            http: config.http,
            http_settings: config.http_settings,
            xpath_backend: config.xpath_backend,
            package: PackageCache::new(),
            stamps: AtomicU64::new(1),
            stale: Mutex::default(),
            bytecode: Mutex::default(),
        });
        let mut threads = Vec::new();
        for i in 0..config.threads.max(1) {
            let shared = shared.clone();
            let receiver = receiver.clone();
            let thread = std::thread::Builder::new()
                .name(format!("fmd-lua-worker-{i}"))
                .spawn(move || worker::run(&shared, &receiver))?;
            threads.push(thread);
        }
        Ok(WorkerPool {
            shared,
            sender: Some(sender),
            threads,
        })
    }

    /// Queues `job` for the next idle worker.
    pub fn submit(&self, job: Job) -> Pending<JobResult> {
        let (reply, receiver) = tokio::sync::oneshot::channel();
        let terminate = job.terminate.clone();
        if let Some(sender) = &self.sender {
            // A send fails only once every worker is gone; the dropped reply then reads as
            // `Closed`.
            let _ = sender.send(Envelope { job, reply });
        }
        Pending::new(receiver, terminate, Ok)
    }

    /// The typed callbacks of `module`.
    pub fn on(&self, module: &Arc<Module>) -> Caller<'_> {
        Caller {
            pool: self,
            module: module.clone(),
            http: None,
            terminate: TerminateToken::new(),
        }
    }

    /// Marks cached module bytecode and the states built from it stale: each worker rebuilds
    /// its state from the module file when it next runs a job for that module. Jobs already
    /// running finish on the old state.
    pub fn invalidate(&self, what: Invalidate) {
        let stamp = self.shared.stamp();
        let mut stale = lock(&self.shared.stale);
        match what {
            Invalidate::Module(id) => {
                stale.modules.insert(id, stamp);
            }
            Invalidate::All => {
                stale.all = stamp;
                self.shared.package.clear();
            }
        }
    }
}

impl Drop for WorkerPool {
    /// Lets every worker finish its job, then joins them; each closes its Lua state on its own
    /// thread, as FMD2 frees the handler when its thread ends
    /// (baseunits/lua/LuaWebsiteModuleHandler.pas:66-70, :88-95).
    fn drop(&mut self) {
        self.sender = None;
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// The typed callbacks of one module, from [`WorkerPool::on`].
pub struct Caller<'a> {
    pool: &'a WorkerPool,
    module: Arc<Module>,
    http: Option<HttpSession>,
    terminate: TerminateToken,
}

impl Caller<'_> {
    /// Runs the callback's `HTTP` global over `session` instead of a new one.
    pub fn with_http(mut self, session: HttpSession) -> Self {
        self.http = Some(session);
        self
    }

    /// Ties the callback to `token`: terminating it aborts the callback's HTTP requests,
    /// `sleep` and `ExecJS`.
    pub fn with_terminate(mut self, token: TerminateToken) -> Self {
        self.terminate = token;
        self
    }

    fn submit<T: 'static>(self, call: Call, map: fn(Answer) -> Option<T>) -> Pending<Reply<T>> {
        let job = Job {
            module: self.module,
            call,
            http: self.http,
            terminate: self.terminate,
        };
        self.pool.submit(job).map(move |result| {
            let value = map(result.answer).ok_or(JobError::Mismatch)?;
            Ok(Reply {
                value,
                http: result.http,
            })
        })
    }

    /// `DoBeforeUpdateList` (baseunits/lua/LuaWebsiteModules.pas:154-170).
    pub fn before_update_list(self, list: UpdateList) -> Pending<Reply<ListReply>> {
        self.submit(Call::BeforeUpdateList { list }, |answer| match answer {
            Answer::BeforeUpdateList(reply) => Some(reply),
            _ => None,
        })
    }

    /// `DoGetDirectoryPageNumber` (baseunits/lua/LuaWebsiteModules.pas:190-217), with
    /// `PAGENUMBER` starting as `page` and `WORKPTR` as `work_ptr`.
    pub fn get_directory_page_number(
        self,
        list: UpdateList,
        page: i32,
        work_ptr: i32,
    ) -> Pending<Reply<PageCount>> {
        let call = Call::GetDirectoryPageNumber {
            list,
            page,
            work_ptr,
        };
        self.submit(call, |answer| match answer {
            Answer::GetDirectoryPageNumber(reply) => Some(reply),
            _ => None,
        })
    }

    /// `DoAfterUpdateList` (baseunits/lua/LuaWebsiteModules.pas:172-188).
    pub fn after_update_list(self, list: UpdateList) -> Pending<Reply<ListReply>> {
        self.submit(Call::AfterUpdateList { list }, |answer| match answer {
            Answer::AfterUpdateList(reply) => Some(reply),
            _ => None,
        })
    }

    /// `DoGetNameAndLink` (baseunits/lua/LuaWebsiteModules.pas:219-243) for the directory
    /// page `page_index`.
    pub fn get_name_and_link(
        self,
        list: UpdateList,
        page_index: i32,
    ) -> Pending<Reply<NamesAndLinks>> {
        let call = Call::GetNameAndLink { list, page_index };
        self.submit(call, |answer| match answer {
            Answer::GetNameAndLink(reply) => Some(reply),
            _ => None,
        })
    }

    /// `DoGetInfo` (baseunits/lua/LuaWebsiteModules.pas:245-265) for the manga at `url`.
    pub fn get_info(self, url: &str) -> Pending<Reply<InfoReply>> {
        let call = Call::GetInfo { url: url.into() };
        self.submit(call, |answer| match answer {
            Answer::GetInfo(reply) => Some(reply),
            _ => None,
        })
    }

    /// `DoTaskStart` (baseunits/lua/LuaWebsiteModules.pas:267-283).
    pub fn task_start(self, task: Task) -> Pending<Reply<TaskReply>> {
        self.submit(Call::TaskStart { task }, |answer| match answer {
            Answer::TaskStart(reply) => Some(reply),
            _ => None,
        })
    }

    /// `DoGetPageNumber` (baseunits/lua/LuaWebsiteModules.pas:285-304) for the chapter at
    /// `url`.
    pub fn get_page_number(self, task: Task, url: &str) -> Pending<Reply<TaskReply>> {
        let call = Call::GetPageNumber {
            task,
            url: url.into(),
        };
        self.submit(call, |answer| match answer {
            Answer::GetPageNumber(reply) => Some(reply),
            _ => None,
        })
    }

    /// `DoGetImageURL` (baseunits/lua/LuaWebsiteModules.pas:306-326) for page `work_id` at
    /// `url`.
    pub fn get_image_url(self, task: Task, work_id: i32, url: &str) -> Pending<Reply<TaskReply>> {
        let call = Call::GetImageUrl {
            task,
            work_id,
            url: url.into(),
        };
        self.submit(call, |answer| match answer {
            Answer::GetImageUrl(reply) => Some(reply),
            _ => None,
        })
    }

    /// `DoBeforeDownloadImage` (baseunits/lua/LuaWebsiteModules.pas:328-348) for page
    /// `work_id`, whose image is at `url`.
    pub fn before_download_image(
        self,
        task: Task,
        work_id: i32,
        url: &str,
    ) -> Pending<Reply<TaskReply>> {
        let call = Call::BeforeDownloadImage {
            task,
            work_id,
            url: url.into(),
        };
        self.submit(call, |answer| match answer {
            Answer::BeforeDownloadImage(reply) => Some(reply),
            _ => None,
        })
    }

    /// `DoDownloadImage` (baseunits/lua/LuaWebsiteModules.pas:350-370) for page `work_id`,
    /// whose image is at `url`; the image is the returned session's document.
    pub fn download_image(self, task: Task, work_id: i32, url: &str) -> Pending<Reply<TaskReply>> {
        let call = Call::DownloadImage {
            task,
            work_id,
            url: url.into(),
        };
        self.submit(call, |answer| match answer {
            Answer::DownloadImage(reply) => Some(reply),
            _ => None,
        })
    }

    /// `DoSaveImage` (baseunits/lua/LuaWebsiteModules.pas:372-391): saves the image in the
    /// session's document as `name` (without extension) in the directory `path`.
    pub fn save_image(self, work_id: i32, path: &str, name: &str) -> Pending<Reply<String>> {
        let call = Call::SaveImage {
            work_id,
            path: path.into(),
            name: name.into(),
        };
        self.submit(call, |answer| match answer {
            Answer::SaveImage(saved) => Some(saved),
            _ => None,
        })
    }

    /// `DoAfterImageSaved` (baseunits/lua/LuaWebsiteModules.pas:393-410) for the file
    /// `file_name`.
    pub fn after_image_saved(self, work_id: i32, file_name: &str) -> Pending<Reply<bool>> {
        let call = Call::AfterImageSaved {
            work_id,
            file_name: file_name.into(),
        };
        self.submit(call, |answer| match answer {
            Answer::AfterImageSaved(ok) => Some(ok),
            _ => None,
        })
    }

    /// `DoLogin` (baseunits/lua/LuaWebsiteModules.pas:412-429).
    pub fn login(self) -> Pending<Reply<bool>> {
        self.submit(Call::Login, |answer| match answer {
            Answer::Login(ok) => Some(ok),
            _ => None,
        })
    }

    /// `DoAccountState` (baseunits/lua/LuaWebsiteModules.pas:431-447).
    pub fn account_state(self) -> Pending<Reply<bool>> {
        self.submit(Call::AccountState, |answer| match answer {
            Answer::AccountState(ok) => Some(ok),
            _ => None,
        })
    }

    /// `DoCheckSite` (baseunits/lua/LuaWebsiteModules.pas:449-465).
    pub fn check_site(self) -> Pending<Reply<bool>> {
        self.submit(Call::CheckSite, |answer| match answer {
            Answer::CheckSite(ok) => Some(ok),
            _ => None,
        })
    }
}

/// What every worker shares.
struct Shared {
    lua_dir: PathBuf,
    http: HttpClient,
    http_settings: Option<Arc<HttpSettingsSource>>,
    xpath_backend: Option<XPathBackend>,
    package: PackageCache,
    /// The source of stamps: states, bytecode and invalidations are ordered by them.
    stamps: AtomicU64,
    stale: Mutex<Stale>,
    /// Each module file's bytecode (FMD2's `Container.ByteCode`).
    bytecode: Mutex<HashMap<PathBuf, Bytecode>>,
}

/// A module file's compiled chunk and the stamp it was compiled at.
struct Bytecode {
    stamp: u64,
    code: Arc<[u8]>,
}

/// When modules were last invalidated.
#[derive(Default)]
struct Stale {
    all: u64,
    modules: HashMap<String, u64>,
}

impl Shared {
    fn stamp(&self) -> u64 {
        self.stamps.fetch_add(1, Ordering::SeqCst)
    }

    /// Whether something of module `id` made at `stamp` was invalidated since.
    fn is_stale(&self, id: &str, stamp: u64) -> bool {
        let stale = lock(&self.stale);
        let since = stale.modules.get(id).copied().unwrap_or(0).max(stale.all);
        stamp <= since
    }
}
