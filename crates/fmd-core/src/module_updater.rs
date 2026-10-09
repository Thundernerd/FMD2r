//! Keeping the Lua tree in sync with upstream, the way FMD2's module updater does
//! (mangadownloader/forms/frmLuaModulesUpdater.pas, baseunits/GitHubRepoV3.pas): the last commit
//! with a conditional ETag request, the tree of that commit diffed by blob SHA against
//! `module_files`, then deletes and downloads.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use fmd_http::{HttpClient, HttpError, HttpSession, TerminateToken};
use fmd_lua::{
    Invalidate, LoadFailure, MemorySettingsStore, Module, ModuleRegistry, ModuleSettingsStore,
    WorkerPool,
};
use fmd_store::{AppDb, EventSeverity, ModuleFile, NewEvent, StoreError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::jobs::{Job, JobError, JobPhase, JobRegistry, JobStatus};
use crate::settings::ModuleUpdaterSettings;

/// `UserAgentCURL` (baseunits/httpsendthread.pas:159), which `TGitHubRepo` sends
/// (baseunits/GitHubRepoV3.pas:121).
const USER_AGENT: &str = "curl/7.70.0";

/// The `settings` key the repository state is stored under (FMD2's `lua_repo.json`,
/// baseunits/FMDOptions.pas:292).
const STATE_KEY: &str = "module_updater.repo";

/// The inbox event kind of the updater's reports.
const EVENT_KIND: &str = "module_update";

/// Downloads running at once when the config does not say.
const DEFAULT_DOWNLOADS: usize = 4;

/// Where the Lua tree comes from: FMD2's `GitHub` block (dist/config.json:8-15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoConfig {
    /// Base URL of the GitHub API.
    pub api_url: String,
    /// Base URL raw files are downloaded from.
    pub download_url: String,
    pub owner: String,
    pub name: String,
    /// The branch (or other ref) to follow.
    pub git_ref: String,
    /// The directory of the repository that holds the Lua tree.
    pub path: String,
}

impl RepoConfig {
    /// The repository in `settings`, at FMD2's API and download URLs (dist/config.json:9-10).
    pub fn from_settings(settings: &ModuleUpdaterSettings) -> RepoConfig {
        RepoConfig {
            api_url: "https://api.github.com/".into(),
            download_url: "https://raw.githubusercontent.com/".into(),
            owner: settings.repo_owner.clone(),
            name: settings.repo_name.clone(),
            // An empty ref means `master` (baseunits/GitHubRepoV3.pas:147-150).
            git_ref: if settings.repo_ref.is_empty() {
                "master".into()
            } else {
                settings.repo_ref.clone()
            },
            path: settings.repo_path.clone(),
        }
    }

    /// `repos/{owner}/{name}/commits?sha={ref}&per_page=1[&path={path}]`
    /// (baseunits/GitHubRepoV3.pas:178-182).
    fn commits_url(&self) -> String {
        let mut url = format!(
            "{}repos/{}/{}/commits?sha={}&per_page=1",
            with_slash(&self.api_url),
            self.owner,
            self.name,
            self.git_ref
        );
        if !self.path.is_empty() {
            url.push_str("&path=");
            url.push_str(&self.path);
        }
        url
    }

    /// `repos/{owner}/{name}/git/trees/{sha}:{path}?recursive=1`
    /// (baseunits/GitHubRepoV3.pas:258).
    fn tree_url(&self, sha: &str) -> String {
        format!(
            "{}repos/{}/{}/git/trees/{sha}:{}?recursive=1",
            with_slash(&self.api_url),
            self.owner,
            self.name,
            self.path
        )
    }

    /// `GetDownloadURL` (baseunits/GitHubRepoV3.pas:342-355), at the synced commit instead of
    /// the ref, so every file of one sync comes from the same tree.
    fn download_url(&self, sha: &str, file: &str) -> String {
        let path = if self.path.is_empty() {
            String::new()
        } else {
            format!("{}/", self.path)
        };
        format!(
            "{}{}/{}/{sha}/{path}{file}",
            with_slash(&self.download_url),
            self.owner,
            self.name
        )
    }
}

/// `AppendURLDelim`: `url` ending with a `/`.
fn with_slash(url: &str) -> String {
    if url.ends_with('/') {
        url.to_owned()
    } else {
        format!("{url}/")
    }
}

/// How a [`ModuleUpdater`] syncs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdaterConfig {
    pub repo: RepoConfig,
    /// Sent as a bearer token to the API, for a higher rate limit.
    pub token: Option<String>,
    /// Keep the loaded version of a module whose updated file fails to load.
    pub keep_last_good: bool,
    /// The Lua tree to sync into.
    pub lua_dir: PathBuf,
    /// Files downloaded at once; at least one.
    pub downloads: usize,
}

impl UpdaterConfig {
    /// The default settings' repository, synced into `lua_dir`.
    pub fn new(lua_dir: impl Into<PathBuf>) -> UpdaterConfig {
        UpdaterConfig::from_settings(&ModuleUpdaterSettings::default(), lua_dir)
    }

    /// The repository, token and keep-last-good choice of `settings`, synced into `lua_dir`.
    pub fn from_settings(
        settings: &ModuleUpdaterSettings,
        lua_dir: impl Into<PathBuf>,
    ) -> UpdaterConfig {
        UpdaterConfig {
            repo: RepoConfig::from_settings(settings),
            token: settings.github_token.clone().filter(|t| !t.is_empty()),
            keep_last_good: settings.keep_last_good,
            lua_dir: lua_dir.into(),
            downloads: DEFAULT_DOWNLOADS,
        }
    }
}

/// Why a sync stopped.
#[derive(Debug, Error)]
pub enum UpdateError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error("GET {url}: HTTP {code}")]
    Status { url: String, code: i32 },
    #[error("GET {url}: {message}")]
    BadResponse { url: String, message: String },
    #[error("cancelled")]
    Cancelled,
    /// GitHub allows no more API requests until `reset` (Unix seconds).
    #[error("GitHub API rate limit exceeded until {reset}")]
    RateLimited { reset: i64 },
    /// The tree names a path that is absolute or climbs out of the Lua dir.
    #[error("unsafe path in the tree: {0}")]
    UnsafePath(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// What a sync did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// The upstream commit the tree is synced to.
    pub commit: String,
    /// Files written, relative to the Lua dir.
    pub downloaded: Vec<String>,
    /// Files deleted because upstream removed them.
    pub deleted: Vec<String>,
    /// Files that failed to download; they are retried next run.
    pub failed: Vec<String>,
    /// Module files that failed to load after the update.
    pub broken: Vec<String>,
}

/// The repository state kept between runs (`last_commit_sha` and `last_commit_etag`,
/// baseunits/GitHubRepoV3.pas:105-106).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct RepoState {
    last_commit_sha: String,
    last_commit_etag: String,
    /// The SHA each file failing to load was last reported at, so it is reported once per
    /// version.
    failed_init: BTreeMap<String, String>,
    /// The SHA each file using unknown Host API names was last reported at.
    unknown_api: BTreeMap<String, String>,
}

/// The modules currently loaded, swapped whole when an update reloads them. Holders of an
/// earlier registry keep it until they let go.
pub struct LiveModules {
    settings: Arc<dyn ModuleSettingsStore>,
    current: RwLock<Loaded>,
}

/// A registry and the module files that failed to load into it.
struct Loaded {
    registry: Arc<ModuleRegistry>,
    failures: Arc<Vec<LoadFailure>>,
}

impl LiveModules {
    /// Loads every module in `<lua_dir>/modules`, their options and cookies in `settings`. A
    /// file that fails is logged and left out.
    pub fn load(lua_dir: &Path, settings: Arc<dyn ModuleSettingsStore>) -> LiveModules {
        let report = ModuleRegistry::load_dir_with(lua_dir, settings.clone());
        for failure in &report.failures {
            tracing::warn!(target: "fmd_core", "module {}: {}", failure.file.display(), failure.error);
        }
        LiveModules {
            settings,
            current: RwLock::new(Loaded {
                registry: Arc::new(report.registry),
                failures: Arc::new(report.failures),
            }),
        }
    }

    /// The registry in use now.
    pub fn current(&self) -> Arc<ModuleRegistry> {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .registry
            .clone()
    }

    /// The module files that failed to load into [`current`](LiveModules::current), sorted by
    /// path: left out of it, or kept at their earlier version.
    pub fn failures(&self) -> Arc<Vec<LoadFailure>> {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .failures
            .clone()
    }

    fn swap(&self, registry: ModuleRegistry, failures: Vec<LoadFailure>) {
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = Loaded {
            registry: Arc::new(registry),
            failures: Arc::new(failures),
        };
    }
}

/// The upstream commit the Lua tree in `db`'s updater state was last fully synced to; empty
/// before the first sync (`ModuleUpdater::synced_commit` without an updater).
pub fn synced_commit(db: &AppDb) -> Result<String, UpdateError> {
    let state: Option<RepoState> = db.settings().get(STATE_KEY)?;
    Ok(state.map(|s| s.last_commit_sha).unwrap_or_default())
}

/// What [`ModuleUpdater::with_after_sync`] calls.
type AfterSync = dyn Fn(&SyncReport) + Send + Sync;

/// Syncs the Lua tree with upstream and reloads the modules that changed.
pub struct ModuleUpdater {
    config: UpdaterConfig,
    db: AppDb,
    http: HttpClient,
    modules: Arc<LiveModules>,
    /// Told which modules changed, so its workers rebuild their states.
    pool: Option<Arc<WorkerPool>>,
    after_sync: Option<Box<AfterSync>>,
    /// When the API's rate limit resets, in Unix seconds, once a response said none is left.
    rate_limit_reset: Mutex<Option<i64>>,
    /// Terminating it cancels the running sync's requests.
    terminate: Mutex<TerminateToken>,
    /// One sync at a time.
    running: Mutex<()>,
}

/// One answer of the GitHub API.
struct ApiResponse {
    code: i32,
    etag: String,
    body: Vec<u8>,
}

/// One entry of a commit's tree.
#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

#[derive(Deserialize)]
struct Tree {
    tree: Vec<TreeEntry>,
    #[serde(default)]
    truncated: bool,
}

#[derive(Deserialize)]
struct Commit {
    sha: String,
}

impl ModuleUpdater {
    pub fn new(
        config: UpdaterConfig,
        db: AppDb,
        http: HttpClient,
        modules: Arc<LiveModules>,
    ) -> ModuleUpdater {
        ModuleUpdater {
            config,
            db,
            http,
            modules,
            pool: None,
            after_sync: None,
            rate_limit_reset: Mutex::new(None),
            terminate: Mutex::default(),
            running: Mutex::new(()),
        }
    }

    /// Invalidates the modules a sync changed in `pool`: each worker rebuilds its state from
    /// the new file when it next runs one, while callbacks already running finish on the old
    /// state.
    pub fn with_pool(mut self, pool: Arc<WorkerPool>) -> ModuleUpdater {
        self.pool = Some(pool);
        self
    }

    /// Calls `hook` after a sync changed files and reloaded the modules, e.g. to write local
    /// settings back into a config file upstream ships.
    pub fn with_after_sync(
        mut self,
        hook: impl Fn(&SyncReport) + Send + Sync + 'static,
    ) -> ModuleUpdater {
        self.after_sync = Some(Box::new(hook));
        self
    }

    /// The modules this updater reloads.
    pub fn modules(&self) -> &Arc<LiveModules> {
        &self.modules
    }

    /// The upstream commit the Lua tree was last fully synced to; empty before the first sync.
    pub fn synced_commit(&self) -> Result<String, UpdateError> {
        synced_commit(&self.db)
    }

    /// The followed ref, e.g. `master`.
    pub fn git_ref(&self) -> &str {
        &self.config.repo.git_ref
    }

    /// Cancels the running sync: its requests stop, files not downloaded yet are retried next
    /// run, and it returns [`UpdateError::Cancelled`].
    pub fn cancel(&self) {
        lock(&self.terminate).terminate();
    }

    /// Runs one sync (`TCheckUpdateThread.DoSync`,
    /// mangadownloader/forms/frmLuaModulesUpdater.pas:769-890). Blocking: run it on a thread
    /// of its own, outside any tokio runtime.
    pub fn sync(&self) -> Result<SyncReport, UpdateError> {
        let _running = lock(&self.running);
        let terminate = lock(&self.terminate).clone();
        let result = self.sync_with(&terminate);
        // Whatever a sync staged is in place or rejected by now, even when it stopped early.
        remove_staging(&self.config.lua_dir);
        // A cancel reaches the sync it was meant for, even one about to start, and no later one.
        *lock(&self.terminate) = TerminateToken::new();
        result
    }

    fn sync_with(&self, terminate: &TerminateToken) -> Result<SyncReport, UpdateError> {
        let stored: RepoState = self.db.settings().get(STATE_KEY)?.unwrap_or_default();
        let rows: BTreeMap<String, ModuleFile> = self
            .db
            .module_files()
            .list()?
            .into_iter()
            .map(|f| (f.path.clone(), f))
            .collect();
        let mut state = stored.clone();
        // First run, or the module tree was wiped: read the whole tree again.
        if rows.is_empty() || has_no_modules(&self.config.lua_dir) {
            state.last_commit_sha.clear();
            state.last_commit_etag.clear();
        }
        let mut next = state.clone();
        let mut upstream = None;
        if let Some(commit) = self.last_commit(&state)? {
            if !commit.etag.is_empty() {
                next.last_commit_etag = commit.etag;
            }
            // `GetUpdate` reads the tree only for a new commit (baseunits/GitHubRepoV3.pas:280-283).
            if !commit.sha.is_empty() && commit.sha != state.last_commit_sha {
                upstream = Some(self.tree(&commit.sha)?);
                next.last_commit_sha = commit.sha;
            }
        }
        let commit = next.last_commit_sha.clone();
        let plan = Plan::new(&rows, upstream.as_ref(), &self.config.lua_dir);
        let mut report = SyncReport {
            commit: commit.clone(),
            ..SyncReport::default()
        };
        for path in &plan.delete {
            self.delete(path)?;
            report.deleted.push(path.clone());
        }
        let to_stage = self.to_stage(&plan.download);
        let results = self.download_all(&commit, &plan.download, &to_stage);
        let rejected = self.validate(
            results
                .iter()
                .filter_map(|r| r.as_ref().ok()?.staged.clone()),
        );
        let mut kept_out = BTreeMap::new();
        for (file, result) in plan.download.iter().zip(results) {
            let committed = result.and_then(|downloaded| match &downloaded.staged {
                Some(staged) => match rejected.get(staged) {
                    Some(error) => {
                        kept_out.insert(
                            file.path.clone(),
                            KeptOut {
                                sha: file.sha.clone(),
                                error: error.clone(),
                            },
                        );
                        Ok(None)
                    }
                    None => {
                        let live = self.config.lua_dir.join(&file.path);
                        std::fs::rename(staged, &live)
                            .map(|()| Some(downloaded.size))
                            .map_err(|source| UpdateError::Io { path: live, source })
                    }
                },
                None => Ok(Some(downloaded.size)),
            });
            match committed {
                Ok(Some(size)) => {
                    self.db.module_files().upsert(&ModuleFile {
                        path: file.path.clone(),
                        sha: file.sha.clone(),
                        last_modified: now_ms(),
                        size,
                    })?;
                    report.downloaded.push(file.path.clone());
                }
                // Downloaded, but its new version stays out of the Lua dir.
                Ok(None) => report.downloaded.push(file.path.clone()),
                Err(e) => {
                    tracing::warn!(target: "fmd_core", "module updater: {}: {e}", file.path);
                    report.failed.push(file.path.clone());
                }
            }
        }
        let changed: Vec<String> = report
            .downloaded
            .iter()
            .chain(&report.deleted)
            .filter(|path| !kept_out.contains_key(*path))
            .cloned()
            .collect();
        if !changed.is_empty() || !kept_out.is_empty() {
            if let Err(e) = self.reload(&changed, &kept_out, &mut next, &mut report) {
                // Keep what was reported so far, and the old commit so the sync is retried.
                let reported = RepoState {
                    failed_init: next.failed_init,
                    unknown_api: next.unknown_api,
                    ..state
                };
                if reported != stored {
                    self.db.settings().set(STATE_KEY, &reported)?;
                }
                return Err(e);
            }
            if let Some(after_sync) = &self.after_sync {
                after_sync(&report);
            }
        }
        // A failed download keeps the old commit, so the next run diffs the tree again and
        // retries it (FMD2 keeps it flagged `fFailedDownload`,
        // mangadownloader/forms/frmLuaModulesUpdater.pas:871-874).
        if !report.failed.is_empty() {
            next.last_commit_sha = state.last_commit_sha;
            next.last_commit_etag = state.last_commit_etag;
        }
        if next != stored {
            self.db.settings().set(STATE_KEY, &next)?;
        }
        if terminate.is_terminated() {
            return Err(UpdateError::Cancelled);
        }
        Ok(report)
    }

    /// Loads the module files `changed` touched again and swaps the registry, like
    /// `ScanAndLoadFiles` (baseunits/lua/LuaWebsiteModules.pas:636-656) restricted to them. A
    /// changed file outside `modules/` may be `require`d by any module, so then every module
    /// loads again.
    ///
    /// A file that fails to load is reported to the inbox once per version, and so is each of
    /// `kept_out` (by path): module files whose update failed to load and never
    /// replaced the version that is loaded, which stays, with its `module_files` row, until
    /// upstream next changes it. A module that breaks because a file it `require`s changed has
    /// no earlier version on disk to go back to, so it is dropped.
    fn reload(
        &self,
        changed: &[String],
        kept_out: &BTreeMap<String, KeptOut>,
        state: &mut RepoState,
        report: &mut SyncReport,
    ) -> Result<(), UpdateError> {
        let lua_dir = &self.config.lua_dir;
        let settings = self.modules.settings.clone();
        let everything = changed.iter().any(|p| !is_module_file(p));
        let load = if everything {
            ModuleRegistry::load_dir_with(lua_dir, settings)
        } else {
            let files: Vec<PathBuf> = changed
                .iter()
                .map(|p| lua_dir.join(p))
                .filter(|f| f.is_file())
                .collect();
            ModuleRegistry::load_files_with(lua_dir, &files, settings)
        };
        let reloaded = |file: &Path| everything || changed.iter().any(|p| lua_dir.join(p) == file);
        let old = self.modules.current();
        let from = |file: &Path| -> Vec<Arc<Module>> {
            old.modules()
                .iter()
                .filter(|m| m.def().file == file)
                .cloned()
                .collect()
        };
        let failed: BTreeMap<PathBuf, String> = load
            .failures
            .into_iter()
            .map(|f| (f.file, f.error))
            .collect();
        let mut modules: Vec<Arc<Module>> = old
            .modules()
            .iter()
            .filter(|m| !reloaded(&m.def().file))
            .cloned()
            .collect();
        modules.extend(load.registry.modules().iter().cloned());
        let mut failures: BTreeMap<String, (String, &String)> = BTreeMap::new();
        for (file, error) in &failed {
            let path = relative(lua_dir, file);
            failures.insert(path.clone(), (self.synced_sha(&path)?, error));
        }
        for (path, kept) in kept_out {
            failures.insert(path.clone(), (kept.sha.clone(), &kept.error));
        }
        for (path, (sha, error)) in failures {
            if state.failed_init.get(&path) != Some(&sha) {
                self.report_failure(&path, from(&lua_dir.join(&path)).first(), error)?;
                state.failed_init.insert(path.clone(), sha);
            }
            report.broken.push(path);
        }
        // A reloaded file that loads (or is gone) is reported again when it next breaks.
        state.failed_init.retain(|path, _| {
            let file = lua_dir.join(path);
            !reloaded(&file) || failed.contains_key(&file) || kept_out.contains_key(path)
        });
        let loaded: Vec<&String> = changed
            .iter()
            .filter(|p| is_module_file(p) && !failed.contains_key(&lua_dir.join(p)))
            .collect();
        self.check_host_api(&loaded, load.registry.modules(), state)?;
        let ids: BTreeSet<String> = old
            .modules()
            .iter()
            .chain(load.registry.modules())
            .map(|m| m.def())
            .filter(|def| reloaded(&def.file))
            .map(|def| def.id)
            .collect();
        // The failures of files this reload left alone stand; the rest are this load's.
        let mut live_failures: BTreeMap<PathBuf, String> = self
            .modules
            .failures()
            .iter()
            .filter(|f| !reloaded(&f.file))
            .map(|f| (f.file.clone(), f.error.clone()))
            .collect();
        live_failures.extend(failed);
        for (path, kept) in kept_out {
            live_failures.insert(lua_dir.join(path), kept.error.clone());
        }
        let live_failures = live_failures
            .into_iter()
            .map(|(file, error)| LoadFailure { file, error })
            .collect();
        self.modules
            .swap(ModuleRegistry::from_modules(modules), live_failures);
        if let Some(pool) = &self.pool {
            // A changed `require`d file also drops the pool's cache of them
            // (`LuaPackage.ClearCache`, baseunits/lua/LuaPackage.pas:141-144).
            if everything {
                pool.invalidate(Invalidate::All);
            } else {
                for id in ids {
                    pool.invalidate(Invalidate::Module(id));
                }
            }
        }
        Ok(())
    }

    /// The blob SHA `module_files` holds for `path`; empty when none.
    fn synced_sha(&self, path: &str) -> Result<String, UpdateError> {
        Ok(self
            .db
            .module_files()
            .get(path)?
            .map(|row| row.sha)
            .unwrap_or_default())
    }

    /// Reports each of the module files `paths` that references Host API names a callback's
    /// Lua state lacks (the T02 scan checked against the T14 state, as the Host API corpus
    /// report does), once per version. `modules` are the modules they declared.
    fn check_host_api(
        &self,
        paths: &[&String],
        modules: &[Arc<Module>],
        state: &mut RepoState,
    ) -> Result<(), UpdateError> {
        let mut referenced = BTreeMap::new();
        for path in paths {
            let file = self.config.lua_dir.join(path);
            let Ok(source) = std::fs::read(&file) else {
                continue;
            };
            let names = fmd_lua::scan_host_api_names(&String::from_utf8_lossy(&source));
            referenced.insert((*path).clone(), (file, names));
        }
        let all: BTreeSet<&str> = referenced
            .values()
            .flat_map(|(_, names)| names.iter().map(String::as_str))
            .collect();
        let missing = match fmd_lua::missing_host_api(all) {
            Ok(missing) => missing,
            Err(e) => {
                tracing::warn!(target: "fmd_core", "module updater: Host API check: {e}");
                return Ok(());
            }
        };
        for (path, (file, names)) in referenced {
            let unknown: Vec<&String> = names.intersection(&missing).collect();
            if unknown.is_empty() {
                state.unknown_api.remove(&path);
                continue;
            }
            let sha = self.synced_sha(&path)?;
            if state.unknown_api.get(&path) == Some(&sha) {
                continue;
            }
            let module = modules.iter().find(|m| m.def().file == file);
            self.push_event(
                EventSeverity::Warning,
                module,
                format!("module {} uses unknown Host API names", file_name(&path)),
                serde_json::json!({ "file": path, "names": unknown }),
            )?;
            state.unknown_api.insert(path, sha);
        }
        Ok(())
    }

    /// Posts "module X failed Init" to the inbox, with FMD2's `DoInit` error
    /// (baseunits/lua/LuaWebsiteModules.pas:473-500).
    fn report_failure(
        &self,
        path: &str,
        module: Option<&Arc<Module>>,
        error: &str,
    ) -> Result<(), UpdateError> {
        self.push_event(
            EventSeverity::Error,
            module,
            format!("module {} failed Init", file_name(path)),
            serde_json::json!({ "file": path, "error": error }),
        )
    }

    /// Posts an updater report about `module` to the inbox.
    fn push_event(
        &self,
        severity: EventSeverity,
        module: Option<&Arc<Module>>,
        title: String,
        body: serde_json::Value,
    ) -> Result<(), UpdateError> {
        self.db.events().push(&NewEvent {
            kind: EVENT_KIND.into(),
            severity,
            module_id: module.map(|m| m.def().id),
            task_id: None,
            title,
            body,
        })?;
        Ok(())
    }

    /// `GetLastCommit` (baseunits/GitHubRepoV3.pas:163-207): the last commit and its ETag, or
    /// `None` when the stored ETag still matches (304 Not Modified).
    fn last_commit(&self, state: &RepoState) -> Result<Option<LastCommit>, UpdateError> {
        let url = self.config.repo.commits_url();
        let response = self.api_get(&url, &state.last_commit_etag)?;
        if response.code == 304 {
            return Ok(None);
        }
        if response.code != 200 {
            return Err(UpdateError::Status {
                url,
                code: response.code,
            });
        }
        let commits: Vec<Commit> = parse(&url, &response.body)?;
        let sha = commits
            .into_iter()
            .next()
            .map(|c| c.sha)
            .unwrap_or_default();
        Ok(Some(LastCommit {
            sha,
            etag: response.etag,
        }))
    }

    /// `GetTree` (baseunits/GitHubRepoV3.pas:243-272): every file (blob) under the path,
    /// by path. An empty or truncated tree is refused rather than taken as "everything was
    /// deleted".
    fn tree(&self, sha: &str) -> Result<BTreeMap<String, String>, UpdateError> {
        let url = self.config.repo.tree_url(sha);
        let response = self.api_get(&url, "")?;
        if response.code != 200 {
            return Err(UpdateError::Status {
                url,
                code: response.code,
            });
        }
        let tree: Tree = parse(&url, &response.body)?;
        if tree.truncated {
            return Err(UpdateError::BadResponse {
                url,
                message: "the tree is truncated".into(),
            });
        }
        let files: BTreeMap<String, String> = tree
            .tree
            .into_iter()
            // Submodules (`commit`) have nothing to download; FMD2 keeps every non-`tree` entry.
            .filter(|e| e.kind == "blob")
            .map(|e| (e.path, e.sha))
            .collect();
        if files.is_empty() {
            return Err(UpdateError::BadResponse {
                url,
                message: "the tree is empty".into(),
            });
        }
        Ok(files)
    }

    /// A GET on the API like `TGitHubRepo`'s session (baseunits/GitHubRepoV3.pas:120-122):
    /// curl's user agent, no redirects, and `If-None-Match` when an ETag is known (:173-176).
    ///
    /// Where FMD2 asks `rate_limit` before a sync and only warns (`CheckRateLimited`, :306-341,
    /// called at mangadownloader/forms/frmLuaModulesUpdater.pas:779-782), this reads the
    /// `X-RateLimit-Remaining` and `X-RateLimit-Reset` headers of every answer and sends
    /// nothing more once none are left, until the reset.
    fn api_get(&self, url: &str, etag: &str) -> Result<ApiResponse, UpdateError> {
        if let Some(at) = *lock(&self.rate_limit_reset)
            && now_ms() / 1000 < at
        {
            return Err(UpdateError::RateLimited { reset: at });
        }
        let mut session = self.session();
        if !etag.is_empty() {
            session.headers_mut().set_value("If-None-Match", etag);
        }
        if let Some(token) = &self.config.token {
            session
                .headers_mut()
                .set_value("Authorization", &format!("Bearer {token}"));
        }
        session.get(url)?;
        let code = session.result_code();
        let headers = session.headers();
        let exhausted = headers.value("X-RateLimit-Remaining").trim() == "0";
        let reset_at = headers
            .value("X-RateLimit-Reset")
            .trim()
            .parse::<i64>()
            .ok();
        *lock(&self.rate_limit_reset) = reset_at.filter(|_| exhausted);
        if exhausted
            && (code == 403 || code == 429)
            && let Some(at) = reset_at
        {
            return Err(UpdateError::RateLimited { reset: at });
        }
        Ok(ApiResponse {
            code,
            etag: headers.value("ETag").trim().to_owned(),
            body: std::mem::take(session.document_mut()),
        })
    }

    fn session(&self) -> HttpSession {
        let mut session = self.http.session();
        session.reset_basic();
        session.set_follow_redirection(false);
        session.set_user_agent(USER_AGENT);
        session.set_terminate_token(lock(&self.terminate).clone());
        session
    }

    /// Deletes a file upstream removed (`Download`'s delete pass,
    /// mangadownloader/forms/frmLuaModulesUpdater.pas:698-712).
    fn delete(&self, path: &str) -> Result<(), UpdateError> {
        let file = self.config.lua_dir.join(path);
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(UpdateError::Io { path: file, source }),
        }
        self.db.module_files().delete(path)?;
        Ok(())
    }

    /// The paths of `files` that download into the staging dir instead of the Lua dir: with
    /// keep-last-good, the module files on disk whose current version is loaded. Their new version
    /// replaces it only once it loads, so no worker building a state from the file in between
    /// ever runs a version that fails to load.
    fn to_stage(&self, files: &[Wanted]) -> BTreeSet<String> {
        if !self.config.keep_last_good {
            return BTreeSet::new();
        }
        let loaded = self.modules.current();
        files
            .iter()
            .filter(|f| is_module_file(&f.path))
            .filter(|f| {
                let file = self.config.lua_dir.join(&f.path);
                file.is_file() && loaded.modules().iter().any(|m| m.def().file == file)
            })
            .map(|f| f.path.clone())
            .collect()
    }

    /// Loads each staged module file as the scan would from the Lua dir (`DoInit`,
    /// baseunits/lua/LuaWebsiteModules.pas:473-500), in scratch states. Returns the ones that
    /// fail, with their errors naming the file they would replace.
    fn validate(&self, staged: impl Iterator<Item = PathBuf>) -> BTreeMap<PathBuf, String> {
        let staged: Vec<PathBuf> = staged.collect();
        if staged.is_empty() {
            return BTreeMap::new();
        }
        let lua_dir = &self.config.lua_dir;
        let staging = lua_dir.join(STAGING_DIR);
        let load =
            ModuleRegistry::load_files_with(lua_dir, &staged, Arc::new(MemorySettingsStore::new()));
        load.failures
            .into_iter()
            .map(|failure| {
                let live = lua_dir.join(relative(&staging, &failure.file));
                let error = failure.error.replace(
                    &failure.file.display().to_string(),
                    &live.display().to_string(),
                );
                (failure.file, error)
            })
            .collect()
    }

    /// Downloads `files` on at most `downloads` threads at once (FMD2's `TDownloadThread`s,
    /// bounded by `OptionMaxThreads`, mangadownloader/forms/frmLuaModulesUpdater.pas:714-737),
    /// the ones in `to_stage` into the staging dir. Results come back in `files` order: each file's
    /// size, or why it failed.
    fn download_all(
        &self,
        commit: &str,
        files: &[Wanted],
        to_stage: &BTreeSet<String>,
    ) -> Vec<Result<Downloaded, UpdateError>> {
        let next = AtomicUsize::new(0);
        let results = Mutex::new(Vec::with_capacity(files.len()));
        let threads = self.config.downloads.max(1).min(files.len());
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::SeqCst);
                        let Some(file) = files.get(i) else { break };
                        let result =
                            self.download(commit, &file.path, to_stage.contains(&file.path));
                        lock(&results).push((i, result));
                    }
                });
            }
        });
        let mut results = results.into_inner().unwrap_or_else(PoisonError::into_inner);
        results.sort_by_key(|(i, _)| *i);
        results.into_iter().map(|(_, r)| r).collect()
    }

    /// `TDownloadThread.Execute` (mangadownloader/forms/frmLuaModulesUpdater.pas:395-440), but
    /// written to a temp file and renamed over the old one, so a failure never leaves a
    /// half-written module; a `staged` file is written into the staging dir instead.
    fn download(&self, commit: &str, path: &str, staged: bool) -> Result<Downloaded, UpdateError> {
        let live = safe_join(&self.config.lua_dir, path)?;
        let file = if staged {
            self.config.lua_dir.join(STAGING_DIR).join(path)
        } else {
            live
        };
        let url = self.config.repo.download_url(commit, path);
        let mut session = self.session();
        session.get(&url)?;
        if session.result_code() != 200 {
            return Err(UpdateError::Status {
                url,
                code: session.result_code(),
            });
        }
        write_atomically(&file, session.document())?;
        Ok(Downloaded {
            size: session.document().len() as u64,
            staged: staged.then_some(file),
        })
    }
}

/// The module updater as the `modules` background job: each run syncs on a thread of its own.
/// Cheap to clone; clones share the job.
#[derive(Clone)]
pub struct ModuleUpdaterJob {
    inner: Arc<JobInner>,
}

struct JobInner {
    updater: Arc<ModuleUpdater>,
    jobs: JobRegistry,
    status: Mutex<JobStatus>,
}

impl ModuleUpdaterJob {
    /// The job's id in `/api/jobs/{id}`.
    pub const ID: &str = "modules";

    /// The job over `updater`, announcing its changes on `jobs`.
    pub fn new(updater: Arc<ModuleUpdater>, jobs: JobRegistry) -> ModuleUpdaterJob {
        ModuleUpdaterJob {
            inner: Arc::new(JobInner {
                updater,
                jobs,
                status: Mutex::new(JobStatus {
                    phase: JobPhase::Idle,
                    done: 0,
                    total: 0,
                    last_run: None,
                    next_run: None,
                    last_error: None,
                }),
            }),
        }
    }

    /// Records when the scheduler runs the job next, in Unix milliseconds.
    pub fn set_next_run(&self, at: Option<i64>) {
        self.update(|status| status.next_run = at);
    }

    fn update(&self, change: impl FnOnce(&mut JobStatus)) {
        change(&mut lock(&self.inner.status));
        self.inner.jobs.changed(Self::ID);
    }
}

impl Job for ModuleUpdaterJob {
    fn id(&self) -> &str {
        Self::ID
    }

    fn title(&self) -> &str {
        "Module updater"
    }

    fn status(&self) -> JobStatus {
        lock(&self.inner.status).clone()
    }

    fn run(&self) -> Result<(), JobError> {
        {
            let mut status = lock(&self.inner.status);
            if status.phase == JobPhase::Running {
                return Err(JobError::AlreadyRunning);
            }
            status.phase = JobPhase::Running;
            status.done = 0;
            status.total = 0;
            status.last_run = Some(now_ms());
            status.last_error = None;
        }
        self.inner.jobs.changed(Self::ID);
        let job = self.clone();
        let spawned = std::thread::Builder::new()
            .name("fmd-module-updater".into())
            .spawn(move || {
                let result = job.inner.updater.sync();
                job.update(|status| match result {
                    Ok(report) => {
                        status.phase = JobPhase::Done;
                        let done = report.downloaded.len() + report.deleted.len();
                        status.done = done as u64;
                        status.total = (done + report.failed.len()) as u64;
                    }
                    Err(e) => {
                        tracing::warn!(target: "fmd_core", "module updater: {e}");
                        status.phase = JobPhase::Failed;
                        status.last_error = Some(e.to_string());
                    }
                });
            });
        if let Err(e) = spawned {
            self.update(|status| {
                status.phase = JobPhase::Failed;
                status.last_error = Some(e.to_string());
            });
            return Err(JobError::Failed(e.to_string()));
        }
        Ok(())
    }

    fn cancel(&self) -> Result<(), JobError> {
        if self.status().phase != JobPhase::Running {
            return Err(JobError::NotRunning);
        }
        self.inner.updater.cancel();
        Ok(())
    }
}

/// What `GetLastCommit` read.
struct LastCommit {
    sha: String,
    etag: String,
}

/// A module file whose downloaded update failed to load, so it never replaced the loaded one.
struct KeptOut {
    /// The blob SHA of the update.
    sha: String,
    /// Why it failed to load.
    error: String,
}

/// A file written by a download.
struct Downloaded {
    size: u64,
    /// Where it was staged, when it was not written into place.
    staged: Option<PathBuf>,
}

/// A file to download.
struct Wanted {
    path: String,
    sha: String,
}

/// What a sync changes (`SyncRepos`, mangadownloader/forms/frmLuaModulesUpdater.pas:588-684).
#[derive(Default)]
struct Plan {
    delete: Vec<String>,
    download: Vec<Wanted>,
}

impl Plan {
    /// Diffs `upstream` (when a new tree was read) against the synced `rows` by SHA: new and
    /// changed files download, files gone upstream are deleted. A synced file missing from
    /// `lua_dir` downloads again (mangadownloader/forms/frmLuaModulesUpdater.pas:791-806).
    fn new(
        rows: &BTreeMap<String, ModuleFile>,
        upstream: Option<&BTreeMap<String, String>>,
        lua_dir: &Path,
    ) -> Plan {
        let mut plan = Plan::default();
        let wanted: BTreeMap<&String, &String> = match upstream {
            Some(upstream) => upstream.iter().collect(),
            None => rows.iter().map(|(path, row)| (path, &row.sha)).collect(),
        };
        for (path, sha) in wanted {
            let synced = rows.get(path).is_some_and(|row| &row.sha == sha);
            if !synced || !lua_dir.join(path).is_file() {
                plan.download.push(Wanted {
                    path: path.clone(),
                    sha: sha.clone(),
                });
            }
        }
        if let Some(upstream) = upstream {
            plan.delete = rows
                .keys()
                .filter(|path| !upstream.contains_key(*path))
                .cloned()
                .collect();
        }
        plan
    }
}

/// Whether `path` (relative to the Lua dir) is a file the module scan loads: a `*.lua` or
/// `*.luac` directly in `modules/` (baseunits/lua/LuaWebsiteModules.pas:644).
fn is_module_file(path: &str) -> bool {
    let Some(name) = path.strip_prefix("modules/") else {
        return false;
    };
    let extension = Path::new(name).extension();
    !name.contains('/')
        && extension
            .is_some_and(|e| e.eq_ignore_ascii_case("lua") || e.eq_ignore_ascii_case("luac"))
}

/// `file` relative to `lua_dir`, with `/` separators like the tree's paths.
fn relative(lua_dir: &Path, file: &Path) -> String {
    let relative = file.strip_prefix(lua_dir).unwrap_or(file);
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The last component of `path` (`ExtractFileName`, as `DoInit` names the file it reports,
/// baseunits/lua/LuaWebsiteModules.pas:497).
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether `<lua_dir>/modules` is missing or holds nothing: the tree was never synced, or was
/// wiped, and a sync reads it all again (the first-run bootstrap).
pub fn has_no_modules(lua_dir: &Path) -> bool {
    std::fs::read_dir(lua_dir.join("modules")).map_or(true, |mut entries| entries.next().is_none())
}

/// `root/path`, refusing a `path` that is absolute or climbs out of `root`.
fn safe_join(root: &Path, path: &str) -> Result<PathBuf, UpdateError> {
    use std::path::Component;
    let relative = Path::new(path);
    let plain = relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)));
    if path.is_empty() || !plain {
        return Err(UpdateError::UnsafePath(path.to_owned()));
    }
    Ok(root.join(relative))
}

/// The directory of the Lua dir that module updates are staged in until they load.
const STAGING_DIR: &str = ".fmd2r-staging";

/// Removes the staging dir and whatever a sync left in it.
fn remove_staging(lua_dir: &Path) {
    match std::fs::remove_dir_all(lua_dir.join(STAGING_DIR)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(target: "fmd_core", "module updater: {STAGING_DIR}: {e}"),
    }
}

/// Writes `bytes` to a temp file next to `file`, then renames it over `file`.
fn write_atomically(file: &Path, bytes: &[u8]) -> Result<(), UpdateError> {
    let io = |source| UpdateError::Io {
        path: file.to_owned(),
        source,
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = file.with_file_name(format!(".{name}.fmd2r-download"));
    let written = std::fs::write(&temp, bytes).and_then(|()| std::fs::rename(&temp, file));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written.map_err(io)
}

fn parse<T: serde::de::DeserializeOwned>(url: &str, body: &[u8]) -> Result<T, UpdateError> {
    serde_json::from_slice(body).map_err(|e| UpdateError::BadResponse {
        url: url.to_owned(),
        message: e.to_string(),
    })
}

/// Locks `mutex`; what it guards stays consistent even if a holder panicked.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Now, in Unix milliseconds.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
