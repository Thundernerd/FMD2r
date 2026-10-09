//! The queue and its scheduling (`TDownloadManager`, baseunits/uDownloadsManager.pas:1590-2050).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Instant;

use fmd_http::TerminateToken;
use fmd_lua::Module;
use fmd_pack::{RenameContext, custom_rename};
use fmd_store::{ChapterStatus, NewChapter, NewTask, TaskId, TaskStatus};
use tokio::sync::broadcast;

use super::{EngineConfig, EngineError, EngineEvent, NewDownload, Progress, TaskInfo, task};
use crate::modules::ModuleInfo;
use crate::settings::{
    EffectiveLimits, ModuleOverrides, OutputFormat, SaveToSettings, Settings, SymbolMode,
    effective_limits,
};

/// How many events a slow listener may fall behind before it misses some.
const EVENTS_CAPACITY: usize = 1024;

/// The archive extensions a chapter may have been packed under (`FMDSupportedPackedOutputExt`,
/// baseunits/FMDOptions.pas).
const PACKED_EXTENSIONS: [&str; 4] = [".zip", ".cbz", ".pdf", ".epub"];

pub(super) struct Inner {
    pub(super) config: EngineConfig,
    events: broadcast::Sender<EngineEvent>,
    /// `CS_Task`: held while the queue is scanned and tasks are started or stopped.
    state: Mutex<State>,
    /// `isReadyForExit`: ending task threads leave their status alone.
    exiting: AtomicBool,
    progress: Mutex<HashMap<TaskId, ProgressClock>>,
}

#[derive(Default)]
struct State {
    running: HashMap<TaskId, Running>,
}

/// A running task's thread (`TTaskContainer.TaskThread`).
struct Running {
    module: Arc<Module>,
    terminate: TerminateToken,
    thread: Option<JoinHandle<()>>,
    /// `TTaskThread.IsForDelete`: the task is being deleted, its status no longer matters.
    deleting: bool,
}

/// The last progress reported for a task, to throttle and to compute speed.
struct ProgressClock {
    last: Progress,
    at: Instant,
}

/// Progress reports per task per second, at most.
const PROGRESS_INTERVAL_MS: u128 = 250;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    pub(super) fn new(config: EngineConfig) -> Inner {
        Inner {
            config,
            events: broadcast::channel(EVENTS_CAPACITY).0,
            state: Mutex::default(),
            exiting: AtomicBool::new(false),
            progress: Mutex::default(),
        }
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.events.subscribe()
    }

    pub(super) fn emit(&self, event: EngineEvent) {
        // No listener is not an error.
        let _ = self.events.send(event);
    }

    pub(super) fn settings(&self) -> Arc<Settings> {
        self.config.settings.get()
    }

    pub(super) fn module(&self, id: &str) -> Option<Arc<Module>> {
        (self.config.modules)(id)
    }

    pub(super) fn is_exiting(&self) -> bool {
        self.exiting.load(Ordering::SeqCst)
    }

    /// The limits for `module`'s downloads: its declared ones, the user's overrides and the
    /// global connection settings (`effective_limits`).
    pub(super) fn limits(&self, module: &Module) -> EffectiveLimits {
        let def = module.def();
        let overrides = ModuleOverrides::load(&self.config.db.module_settings(), &def.id)
            .map_err(|e| tracing::warn!(target: "fmd_core", "settings of module {}: {e}", def.id))
            .ok();
        effective_limits(
            &ModuleInfo::from(&def).limits,
            overrides.as_ref(),
            &self.settings().connections,
        )
    }

    /// Sets a task's status and announces it; nothing happens when it already has it
    /// (`TTaskContainer.SetStatus`, baseunits/uDownloadsManager.pas:1376-1382).
    pub(super) fn set_status(
        &self,
        id: TaskId,
        status: TaskStatus,
        error: Option<&str>,
    ) -> Result<(), EngineError> {
        let repo = self.config.db.tasks();
        let task = repo.get(id)?.ok_or(EngineError::NoTask(id))?;
        if task.status == status && task.error.as_deref() == error {
            return Ok(());
        }
        repo.update_status(id, status, error)?;
        if task.status != status {
            self.emit(EngineEvent::Status {
                task: id,
                status,
                chapter: task.current_chapter,
                error: error.map(str::to_owned),
            });
        }
        Ok(())
    }

    /// Reports `progress`, at most every [`PROGRESS_INTERVAL_MS`] unless `force`d.
    pub(super) fn progress(&self, mut progress: Progress, force: bool) {
        let now = Instant::now();
        let mut clocks = lock(&self.progress);
        if let Some(clock) = clocks.get(&progress.task) {
            let elapsed = now.duration_since(clock.at).as_millis();
            if !force && elapsed < PROGRESS_INTERVAL_MS {
                return;
            }
            let bytes = progress.bytes.saturating_sub(clock.last.bytes);
            progress.bytes_per_sec = match elapsed {
                0 => clock.last.bytes_per_sec,
                ms => u64::try_from(u128::from(bytes) * 1000 / ms).unwrap_or(u64::MAX),
            };
        }
        clocks.insert(
            progress.task,
            ProgressClock {
                last: progress,
                at: now,
            },
        );
        drop(clocks);
        self.emit(EngineEvent::Progress(progress));
    }

    /// Queues a new task (`AddToDownload`, mangadownloader/forms/frmMain.pas:2653-2790): the
    /// manga folder and chapter names are made with `CustomRename` from the templates in the
    /// settings, as FMD2 does when it queues chapters.
    pub(super) fn add_task(
        self: &Arc<Self>,
        download: &NewDownload,
    ) -> Result<TaskId, EngineError> {
        let module = self
            .module(&download.module_id)
            .ok_or_else(|| EngineError::NoModule(download.module_id.clone()))?;
        if download.chapters.is_empty() {
            return Err(EngineError::NoChapters);
        }
        let settings = self.settings();
        let website = module.def().name;
        let saveto = &settings.saveto;
        let chapters: Vec<NewChapter> = download
            .chapters
            .iter()
            .map(|chapter| NewChapter {
                link: chapter.link.clone(),
                name: chapter_name(saveto, &website, download, &chapter.title, chapter.number),
                custom_filename: None,
            })
            .collect();
        let status = if settings.general.add_as_stopped {
            TaskStatus::Stopped
        } else {
            TaskStatus::Waiting
        };
        let repo = self.config.db.tasks();
        let task = repo.create(&NewTask {
            module_id: download.module_id.clone(),
            link: download.manga_link.clone(),
            title: download.title.clone(),
            save_to: save_to(saveto, &website, download),
            status,
            enabled: true,
        })?;
        repo.set_chapters(task.id, &chapters)?;
        self.emit(EngineEvent::Added { task: task.id });
        self.emit(EngineEvent::Status {
            task: task.id,
            status,
            chapter: 0,
            error: None,
        });
        self.check_and_active_task()?;
        Ok(task.id)
    }

    /// `CheckAndActiveTask` (baseunits/uDownloadsManager.pas:1784-1833): starts waiting tasks
    /// in queue order while fewer than `max_parallel_tasks` run and their module can take
    /// another (`CanCreateTask`, baseunits/WebsiteModules.pas:414-420).
    pub(super) fn check_and_active_task(self: &Arc<Self>) -> Result<(), EngineError> {
        if self.is_exiting() {
            return Ok(());
        }
        let mut state = lock(&self.state);
        let max = self.settings().connections.max_parallel_tasks;
        let mut count = u32::try_from(state.running.len()).unwrap_or(u32::MAX);
        if count >= max {
            return Ok(());
        }
        for task in self.config.db.tasks().list()? {
            if count >= max {
                break;
            }
            if task.status != TaskStatus::Waiting || state.running.contains_key(&task.id) {
                continue;
            }
            // A task whose module is gone was stopped at startup; one removed since waits.
            let Some(module) = self.module(&task.module_id) else {
                continue;
            };
            if self.can_create_task(&module) {
                self.start_task(&mut state, task.id, module)?;
                count += 1;
            }
        }
        Ok(())
    }

    /// `CanCreateTask` (baseunits/WebsiteModules.pas:414-420).
    fn can_create_task(&self, module: &Module) -> bool {
        let max = self.limits(module).max_tasks;
        max == 0 || module.active_task_count() < i32::try_from(max).unwrap_or(i32::MAX)
    }

    /// `StartTask` (baseunits/uDownloadsManager.pas:1895-1898): a new task thread, which counts
    /// towards its module's active tasks (`TTaskThread.Create`, :461-483).
    fn start_task(
        self: &Arc<Self>,
        state: &mut State,
        id: TaskId,
        module: Arc<Module>,
    ) -> Result<(), EngineError> {
        let terminate = TerminateToken::new();
        let inner = self.clone();
        let token = terminate.clone();
        let thread_module = module.clone();
        module.inc_active_task_count();
        let thread = std::thread::Builder::new()
            .name(format!("fmd-task-{}", id.0))
            .spawn(move || {
                task::run(&inner, id, &thread_module, &token);
                inner.task_ended(id);
            });
        let thread = match thread {
            Ok(thread) => thread,
            Err(e) => {
                module.dec_active_task_count();
                return Err(e.into());
            }
        };
        state.running.insert(
            id,
            Running {
                module,
                terminate,
                thread: Some(thread),
                deleting: false,
            },
        );
        Ok(())
    }

    /// `TTaskThread.Destroy` (baseunits/uDownloadsManager.pas:485-528): a task that ended
    /// neither finished nor failed is Stopped (Disabled when it was disabled meanwhile), unless
    /// it is being deleted or the manager is exiting; then waiting tasks get its slot.
    fn task_ended(self: &Arc<Self>, id: TaskId) {
        let deleting = {
            let mut state = lock(&self.state);
            match state.running.remove(&id) {
                Some(running) => {
                    running.module.dec_active_task_count();
                    running.deleting
                }
                None => false,
            }
        };
        lock(&self.progress).remove(&id);
        if deleting || self.is_exiting() {
            return;
        }
        let ended = || -> Result<(), EngineError> {
            let repo = self.config.db.tasks();
            let Some(task) = repo.get(id)? else {
                return Ok(());
            };
            repo.update_progress(id, task.current_chapter, Some(now_ms()))?;
            if !matches!(
                task.status,
                TaskStatus::Stopped | TaskStatus::Finished | TaskStatus::Failed
            ) {
                let status = if task.enabled {
                    TaskStatus::Stopped
                } else {
                    TaskStatus::Disabled
                };
                self.set_status(id, status, task.error.as_deref())?;
            }
            self.check_and_active_task()
        };
        if let Err(e) = ended() {
            tracing::error!(target: "fmd_core", "task {}: {e}", id.0);
        }
    }

    /// `CheckAndActiveTaskAtStartup` (baseunits/uDownloadsManager.pas:1859-1893). FMD2
    /// resumes Downloading, Preparing and Waiting tasks; a process killed while converting or
    /// packing leaves its task Converting or Compressing, so those resume too
    /// (docs/tickets/T44-download-hard-crash-resume.md).
    pub(super) fn check_and_active_task_at_startup(self: &Arc<Self>) -> Result<(), EngineError> {
        let max = self.settings().connections.max_parallel_tasks;
        let mut started = 0;
        {
            let mut state = lock(&self.state);
            for task in self.config.db.tasks().list()? {
                if !matches!(
                    task.status,
                    TaskStatus::Downloading
                        | TaskStatus::Preparing
                        | TaskStatus::Waiting
                        | TaskStatus::Converting
                        | TaskStatus::Compressing
                ) {
                    continue;
                }
                match self.module(&task.module_id) {
                    None => self.set_status(task.id, TaskStatus::Stopped, None)?,
                    Some(module) if started < max && self.can_create_task(&module) => {
                        self.start_task(&mut state, task.id, module)?;
                        started += 1;
                    }
                    Some(_) => self.set_status(task.id, TaskStatus::Waiting, None)?,
                }
            }
        }
        if started == 0 {
            self.check_and_active_task()?;
        }
        Ok(())
    }

    /// `SetTaskActive` (baseunits/uDownloadsManager.pas:1835-1844), then `CheckAndActiveTask`.
    pub(super) fn start(self: &Arc<Self>, id: TaskId) -> Result<(), EngineError> {
        {
            let state = lock(&self.state);
            let task = self
                .config
                .db
                .tasks()
                .get(id)?
                .ok_or(EngineError::NoTask(id))?;
            if !state.running.contains_key(&id)
                && !matches!(task.status, TaskStatus::Finished | TaskStatus::Waiting)
                && task.enabled
                && self.module(&task.module_id).is_some()
            {
                self.set_status(id, TaskStatus::Waiting, None)?;
            }
        }
        self.check_and_active_task()
    }

    /// `StopTask` (baseunits/uDownloadsManager.pas:1900-1920) on one task, with the queue
    /// locked.
    fn stop_locked(&self, state: &State, id: TaskId) -> Result<(), EngineError> {
        let task = self
            .config
            .db
            .tasks()
            .get(id)?
            .ok_or(EngineError::NoTask(id))?;
        // A task thread shows Waiting until its first chapter starts, so a running task is
        // terminated whatever its status says; FMD2 checks the status first.
        if let Some(running) = state.running.get(&id) {
            running.terminate.terminate();
        } else if task.status == TaskStatus::Waiting {
            self.set_status(id, TaskStatus::Stopped, None)?;
        }
        Ok(())
    }

    pub(super) fn stop(self: &Arc<Self>, id: TaskId) -> Result<(), EngineError> {
        {
            let state = lock(&self.state);
            self.stop_locked(&state, id)?;
        }
        self.check_and_active_task()
    }

    /// `StartAllTasks` (baseunits/uDownloadsManager.pas:1922-1941).
    pub(super) fn start_all(self: &Arc<Self>) -> Result<(), EngineError> {
        {
            let state = lock(&self.state);
            for task in self.config.db.tasks().list()? {
                if task.status != TaskStatus::Finished
                    && self.module(&task.module_id).is_some()
                    && !state.running.contains_key(&task.id)
                    && task.enabled
                {
                    self.set_status(task.id, TaskStatus::Waiting, None)?;
                }
            }
        }
        self.check_and_active_task()
    }

    /// `StopAllTasks` (baseunits/uDownloadsManager.pas:1943-1955): with the queue locked, so a
    /// task that ends meanwhile cannot start one about to be stopped.
    pub(super) fn stop_all(self: &Arc<Self>) -> Result<(), EngineError> {
        let state = lock(&self.state);
        for task in self.config.db.tasks().list()? {
            self.stop_locked(&state, task.id)?;
        }
        Ok(())
    }

    /// `DisableTask`/`EnableTask` (baseunits/uDownloadsManager.pas:2022-2045). FMD2 keeps a
    /// disabled task's status; here it shows as Disabled, and as Stopped once enabled again.
    pub(super) fn set_enabled(
        self: &Arc<Self>,
        id: TaskId,
        enabled: bool,
    ) -> Result<(), EngineError> {
        let repo = self.config.db.tasks();
        let task = repo.get(id)?.ok_or(EngineError::NoTask(id))?;
        if task.enabled == enabled {
            return Ok(());
        }
        {
            let state = lock(&self.state);
            if !enabled {
                self.stop_locked(&state, id)?;
            }
            repo.set_enabled(id, enabled)?;
            self.emit(EngineEvent::Enabled { task: id, enabled });
            // A running task becomes Disabled when its thread ends.
            if !state.running.contains_key(&id) {
                let status = if enabled {
                    TaskStatus::Stopped
                } else {
                    TaskStatus::Disabled
                };
                self.set_status(id, status, None)?;
            }
        }
        self.check_and_active_task()
    }

    /// `RedownloadTask` (baseunits/uDownloadsManager.pas:1846-1857).
    pub(super) fn redownload(self: &Arc<Self>, id: TaskId) -> Result<(), EngineError> {
        {
            let state = lock(&self.state);
            let repo = self.config.db.tasks();
            let task = repo.get(id)?.ok_or(EngineError::NoTask(id))?;
            if state.running.contains_key(&id)
                || task.status == TaskStatus::Waiting
                || !task.enabled
                || self.module(&task.module_id).is_none()
            {
                return Ok(());
            }
            repo.update_progress(id, 0, None)?;
            for chapter in repo.chapters(id)? {
                repo.update_chapter(id, chapter.idx, ChapterStatus::Pending, 0)?;
            }
            self.set_status(id, TaskStatus::Waiting, None)?;
        }
        self.check_and_active_task()
    }

    pub(super) fn reorder(self: &Arc<Self>, ids: &[TaskId]) -> Result<(), EngineError> {
        {
            let _state = lock(&self.state);
            self.config.db.tasks().reorder(ids)?;
        }
        self.emit(EngineEvent::Reordered);
        self.check_and_active_task()
    }

    /// Deletes a task, terminating and waiting for its thread first
    /// (miDownloadDeleteTaskClick, mangadownloader/forms/frmMain.pas:2214-2310).
    pub(super) fn delete(
        self: &Arc<Self>,
        id: TaskId,
        delete_files: bool,
    ) -> Result<(), EngineError> {
        let repo = self.config.db.tasks();
        let task = repo.get(id)?.ok_or(EngineError::NoTask(id))?;
        let chapters = repo.chapters(id)?;
        let thread = {
            let mut state = lock(&self.state);
            state.running.get_mut(&id).and_then(|running| {
                running.deleting = true;
                running.terminate.terminate();
                running.thread.take()
            })
        };
        if let Some(thread) = thread {
            let _ = thread.join();
        }
        if delete_files {
            delete_task_files(Path::new(&task.save_to), &chapters);
        }
        repo.delete(id)?;
        self.emit(EngineEvent::Deleted { task: id });
        self.check_and_active_task()
    }

    pub(super) fn list(self: &Arc<Self>) -> Result<Vec<TaskInfo>, EngineError> {
        let repo = self.config.db.tasks();
        let running: Vec<TaskId> = lock(&self.state).running.keys().copied().collect();
        let progress = lock(&self.progress);
        repo.list()?
            .into_iter()
            .map(|task| {
                Ok(TaskInfo {
                    chapters: repo.chapters(task.id)?,
                    running: running.contains(&task.id),
                    progress: progress.get(&task.id).map(|clock| clock.last),
                    task,
                })
            })
            .collect()
    }

    /// `StopAllDownloadTasksForExit` (baseunits/uDownloadsManager.pas:1957-1977): terminates
    /// every task and waits for it, leaving statuses as they are.
    pub(super) fn shutdown(&self) {
        self.exiting.store(true, Ordering::SeqCst);
        let threads: Vec<JoinHandle<()>> = {
            let mut state = lock(&self.state);
            state
                .running
                .values_mut()
                .filter_map(|running| {
                    running.terminate.terminate();
                    running.thread.take()
                })
                .collect()
        };
        for thread in threads {
            let _ = thread.join();
        }
    }
}

/// Deletes the chapters' folders and archives in `save_to`, then `save_to` when empty
/// (mangadownloader/forms/frmMain.pas:2264-2285).
/// Files that cannot be removed are logged and left; the task is deleted anyway, as in FMD2.
fn delete_task_files(save_to: &Path, chapters: &[fmd_store::TaskChapter]) {
    let removed = |path: &Path, result: std::io::Result<()>| {
        if let Err(e) = result {
            tracing::warn!(target: "fmd_core", "deleting {}: {e}", path.display());
        }
    };
    for chapter in chapters {
        let dir = save_to.join(&chapter.name);
        if dir.is_dir() {
            removed(&dir, std::fs::remove_dir_all(&dir));
        }
        for ext in PACKED_EXTENSIONS {
            let mut file = dir.as_os_str().to_owned();
            file.push(ext);
            let file = PathBuf::from(file);
            if file.is_file() {
                removed(&file, std::fs::remove_file(&file));
            }
        }
    }
    // `RemoveDirUTF8`: only an empty folder goes, so failing here is expected.
    let _ = std::fs::remove_dir(save_to);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// `CustomRename`'s options from the save-to settings.
pub(crate) fn rename_options(saveto: &SaveToSettings) -> fmd_pack::RenameOptions {
    fmd_pack::RenameOptions {
        symbols: match saveto.illegal_chars {
            SymbolMode::Posix => fmd_pack::SymbolMode::Posix,
            SymbolMode::Windows => fmd_pack::SymbolMode::Windows,
        },
        replace_unicode: saveto
            .replace_unicode
            .then(|| saveto.replace_unicode_with.clone()),
        pad_volume: if saveto.convert_digit_volume {
            saveto.digit_volume_length as usize
        } else {
            0
        },
        pad_chapter: if saveto.convert_digit_chapter {
            saveto.digit_chapter_length as usize
        } else {
            0
        },
    }
}

/// A chapter's name: the chapter template with `%NUMBERING%` as four digits
/// (mangadownloader/forms/frmMain.pas:2666-2675).
fn chapter_name(
    saveto: &SaveToSettings,
    website: &str,
    download: &NewDownload,
    title: &str,
    number: u32,
) -> String {
    let numbering = format!("{number:04}");
    let ctx = RenameContext {
        website,
        manga: &download.title,
        author: &download.authors,
        artist: &download.artists,
        chapter: title,
        numbering: &numbering,
        filename: "",
    };
    custom_rename(&saveto.chapter_rename, &ctx, &rename_options(saveto))
}

/// The task's directory: the given or default download directory, plus the manga folder when
/// generated and not already part of it, without trailing dots
/// (mangadownloader/forms/frmMain.pas:2685-2710).
pub(crate) fn save_to(saveto: &SaveToSettings, website: &str, download: &NewDownload) -> String {
    let mut dir = match download.save_to.trim() {
        "" => saveto.default_dir.clone(),
        dir => dir.to_owned(),
    };
    if saveto.generate_manga_folder {
        let ctx = RenameContext {
            website,
            manga: &download.title,
            author: &download.authors,
            artist: &download.artists,
            ..RenameContext::default()
        };
        let folder = custom_rename(&saveto.manga_rename, &ctx, &rename_options(saveto));
        if !dir.contains(&folder) {
            dir = Path::new(&dir).join(folder).to_string_lossy().into_owned();
        }
    }
    dir.trim_end_matches('.').to_owned()
}

/// Whether the output format packs chapters into a file.
pub(super) fn pack_format(format: OutputFormat) -> Option<fmd_pack::PackFormat> {
    match format {
        OutputFormat::Folder => None,
        OutputFormat::Zip => Some(fmd_pack::PackFormat::Zip),
        OutputFormat::Cbz => Some(fmd_pack::PackFormat::Cbz),
        OutputFormat::Pdf => Some(fmd_pack::PackFormat::Pdf),
        OutputFormat::Epub => Some(fmd_pack::PackFormat::Epub),
    }
}
