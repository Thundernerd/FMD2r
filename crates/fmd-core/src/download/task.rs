//! A running task: the per-chapter pipeline of `TTaskThread.Execute`
//! (baseunits/uDownloadsManager.pas:975-1374).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use fmd_http::{HttpSession, TerminateToken};
use fmd_lua::{Affinity, Caller, HttpModule, JobError, Module, ModuleDef, Reply, create_http};
use fmd_pack::{MAX_IMAGE_FILE_PATH, MagickOptions, PackOptions, RenameContext, custom_rename};
use fmd_store::{ChapterStatus, NewPage, PageStatus, TaskId, TaskPage, TaskStatus};

use super::files::{find_image_file, remove_partial_images};
use super::manager::{Inner, pack_format, rename_options};
use super::{EngineError, EngineEvent, Progress};
use crate::settings::{Settings, StoredModuleHttpSettings};

/// The page-list markers FMD2 keeps in `PageLinks`: no link yet, downloaded, and a link to get
/// at download time (baseunits/uDownloadsManager.pas:349-361, :1048-1061).
pub(super) const WAITING: &str = "W";
pub(super) const DONE: &str = "D";
pub(super) const DYNAMIC: &str = "G";

/// How often finished pages are written to `app.db` while a chapter downloads.
const PAGE_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// What the page threads of a phase do (`TTaskThread.Flag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    /// `CS_GETPAGELINK`: `OnGetImageURL` for every page without a link.
    PageLink,
    /// `CS_DOWNLOAD`: every page not downloaded yet.
    Download,
}

/// The task's state the task and page threads share (`TTaskContainer`): what `TASK` shows a
/// module, plus the counters of the current phase.
pub(super) struct Container {
    pub(super) task: fmd_lua::Task,
    pub(super) chapters_status: Vec<ChapterStatus>,
    /// `WorkCounter`: the next page to hand out.
    pub(super) work_counter: usize,
    /// `DownCounter`: pages done in the current phase.
    pub(super) down_counter: usize,
    pub(super) bytes: u64,
    /// Pages changed since they were last written to `app.db`.
    dirty: BTreeSet<usize>,
    flushed_at: Instant,
}

/// One run of a task thread.
pub(super) struct TaskRun<'a> {
    pub(super) inner: &'a Arc<Inner>,
    pub(super) id: TaskId,
    pub(super) module: &'a Arc<Module>,
    pub(super) def: ModuleDef,
    pub(super) terminate: &'a TerminateToken,
    pub(super) settings: Arc<Settings>,
    title: String,
    save_to: PathBuf,
    /// The chapters as queued. `TASK.ChapterLinks`/`ChapterNames` start as these, but what a
    /// module does to its copies does not move the task's own chapter list.
    chapter_links: Vec<String>,
    chapter_names: Vec<String>,
    /// The manga's link, as queued, for `downloaded_chapters`.
    manga_link: String,
    pub(super) container: Mutex<Container>,
    /// The worker running the task thread's own callbacks, its Lua state.
    affinity: Affinity,
    /// `TTaskThread.HTTP`.
    http: Option<HttpSession>,
    pub(super) chapter: usize,
    pub(super) chapter_link: String,
    pub(super) working_dir: PathBuf,
    /// `CurrentCustomFileName`: the file-name template with everything but `%FILENAME%`
    /// filled in.
    custom_file_name: String,
}

/// Runs task `id` until it finishes, fails or is terminated.
pub(super) fn run(
    inner: &Arc<Inner>,
    id: TaskId,
    module: &Arc<Module>,
    terminate: &TerminateToken,
) {
    let result = TaskRun::load(inner, id, module, terminate).and_then(|mut run| run.execute());
    if let Err(e) = result {
        tracing::error!(target: "fmd_core", "task {}: {e}", id.0);
        if !terminate.is_terminated()
            && let Err(e) = inner.set_status(id, TaskStatus::Failed, Some(&e.to_string()))
        {
            tracing::error!(target: "fmd_core", "task {}: {e}", id.0);
        }
    }
}

impl<'a> TaskRun<'a> {
    fn load(
        inner: &'a Arc<Inner>,
        id: TaskId,
        module: &'a Arc<Module>,
        terminate: &'a TerminateToken,
    ) -> Result<TaskRun<'a>, EngineError> {
        let repo = inner.config.db.tasks();
        let stored = repo.get(id)?.ok_or(EngineError::NoTask(id))?;
        let chapters = repo.chapters(id)?;
        let task = fmd_lua::Task {
            chapter_links: chapters.iter().map(|c| c.link.clone()).collect(),
            chapter_names: chapters.iter().map(|c| c.name.clone()).collect(),
            current_download_chapter_ptr: i32::try_from(stored.current_chapter).unwrap_or(0),
            // FMD2 subtracts the working directory from this on Windows
            // (baseunits/uDownloadsManager.pas:766-770); on Linux the limit is per name.
            current_max_file_name_length: i32::try_from(MAX_IMAGE_FILE_PATH).unwrap_or(0),
            link: stored.link.clone(),
            ..fmd_lua::Task::default()
        };
        Ok(TaskRun {
            inner,
            id,
            module,
            def: module.def(),
            terminate,
            settings: inner.settings(),
            title: stored.title,
            save_to: PathBuf::from(stored.save_to),
            chapter_links: chapters.iter().map(|c| c.link.clone()).collect(),
            chapter_names: chapters.iter().map(|c| c.name.clone()).collect(),
            manga_link: stored.link,
            container: Mutex::new(Container {
                task,
                chapters_status: chapters.iter().map(|c| c.status).collect(),
                work_counter: 0,
                down_counter: 0,
                bytes: 0,
                dirty: BTreeSet::new(),
                flushed_at: Instant::now(),
            }),
            affinity: inner.config.pool.affinity(),
            http: None,
            chapter: usize::try_from(stored.current_chapter).unwrap_or(0),
            chapter_link: String::new(),
            working_dir: PathBuf::new(),
            custom_file_name: String::new(),
        })
    }

    pub(super) fn container(&self) -> MutexGuard<'_, Container> {
        self.container.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(super) fn terminated(&self) -> bool {
        self.terminate.is_terminated()
    }

    fn set_status(&self, status: TaskStatus) -> Result<(), EngineError> {
        self.inner.set_status(self.id, status, None)
    }

    /// A callback runner on the worker `affinity` names, tied to the task's terminate token.
    pub(super) fn caller(&self, affinity: Affinity, http: Option<HttpSession>) -> Caller<'_> {
        let caller = self
            .inner
            .config
            .pool
            .on(self.module)
            .with_affinity(affinity)
            .with_terminate(self.terminate.clone());
        match http {
            Some(session) => caller.with_http(session),
            None => caller,
        }
    }

    /// A new session for the module (`TModuleContainer.CreateHTTP`,
    /// baseunits/WebsiteModules.pas:353-379), tied to the task's terminate token.
    pub(super) fn new_session(&self) -> HttpSession {
        let module = HttpModule {
            http: self.module.http().clone(),
            settings: Arc::new(StoredModuleHttpSettings::new(
                self.inner.config.db.clone(),
                self.def.id.clone(),
            )),
        };
        let mut session = create_http(&self.inner.config.http, Some(&module));
        session.set_terminate_token(self.terminate.clone());
        session
    }

    /// Reports the current phase's progress.
    pub(super) fn progress(&self, force: bool) {
        let c = self.container();
        let progress = Progress {
            task: self.id,
            chapter: u32::try_from(self.chapter).unwrap_or(u32::MAX),
            pages_done: u32::try_from(c.down_counter).unwrap_or(u32::MAX),
            pages_total: u32::try_from(c.task.page_number.max(0)).unwrap_or(0),
            bytes: c.bytes,
            bytes_per_sec: 0,
        };
        drop(c);
        self.inner.progress(progress, force);
    }

    /// `GetFileName` (baseunits/uDownloadsManager.pas:530-552): the module's file name for the
    /// page when it named every page, else the page number, put into the file-name template.
    pub(super) fn file_name(&self, work_id: usize) -> String {
        let name = {
            let c = self.container();
            let task = &c.task;
            (task.file_names.len() == task.page_links.len())
                .then(|| task.file_names.get(work_id).cloned())
                .flatten()
        };
        let name = fmd_pack::page_file_name(&self.custom_file_name, name.as_deref(), work_id);
        fmd_pack::fit_file_name(&name, MAX_IMAGE_FILE_PATH)
    }

    /// Marks page `work_id` changed, and writes the changed pages to `app.db` when they were
    /// last written a while ago or `force` is set.
    pub(super) fn page_changed(&self, work_id: Option<usize>, force: bool) {
        let pages = {
            let mut c = self.container();
            if let Some(work_id) = work_id {
                c.dirty.insert(work_id);
            }
            if c.dirty.is_empty() || !(force || c.flushed_at.elapsed() >= PAGE_FLUSH_INTERVAL) {
                return;
            }
            c.flushed_at = Instant::now();
            let dirty = std::mem::take(&mut c.dirty);
            dirty
                .into_iter()
                .filter_map(|i| Some((i, c.task.page_links.get(i)?.clone())))
                .collect::<Vec<_>>()
        };
        let repo = self.inner.config.db.tasks();
        for (idx, link) in pages {
            let (container_url, filename) = {
                let c = self.container();
                let entry = |list: &[String]| list.get(idx).cloned().unwrap_or_default();
                (
                    entry(&c.task.page_container_links),
                    entry(&c.task.file_names),
                )
            };
            let page = TaskPage {
                chapter_idx: u32::try_from(self.chapter).unwrap_or(u32::MAX),
                idx: u32::try_from(idx).unwrap_or(u32::MAX),
                status: page_status(&link),
                url: link,
                container_url,
                filename,
            };
            if let Err(e) = repo.update_page(self.id, &page) {
                tracing::warn!(target: "fmd_core", "task {}: saving page {idx}: {e}", self.id.0);
            }
        }
    }

    /// Writes the current chapter's whole page list to `app.db`.
    fn save_pages(&self) -> Result<(), EngineError> {
        let pages: Vec<NewPage> = {
            let mut c = self.container();
            c.dirty.clear();
            let task = &c.task;
            task.page_links
                .iter()
                .enumerate()
                .map(|(i, link)| NewPage {
                    url: link.clone(),
                    container_url: task
                        .page_container_links
                        .get(i)
                        .cloned()
                        .unwrap_or_default(),
                    filename: task.file_names.get(i).cloned().unwrap_or_default(),
                    status: page_status(link),
                })
                .collect()
        };
        let chapter = u32::try_from(self.chapter).unwrap_or(u32::MAX);
        self.inner
            .config
            .db
            .tasks()
            .set_pages(self.id, chapter, &pages)?;
        Ok(())
    }

    /// The current chapter's pages as they were saved, so a resumed task does not ask the
    /// module for them again.
    fn load_pages(&self) -> Result<(), EngineError> {
        let chapter = u32::try_from(self.chapter).unwrap_or(u32::MAX);
        let pages = self.inner.config.db.tasks().pages(self.id, chapter)?;
        let mut c = self.container();
        let task = &mut c.task;
        task.page_links = pages
            .iter()
            .map(|p| match p.status {
                PageStatus::Downloaded => DONE.to_owned(),
                PageStatus::Waiting if p.url.is_empty() => WAITING.to_owned(),
                PageStatus::Waiting => p.url.clone(),
            })
            .collect();
        task.page_container_links = pages.iter().map(|p| p.container_url.clone()).collect();
        if task.page_container_links.iter().all(String::is_empty) {
            task.page_container_links.clear();
        }
        task.file_names = pages.iter().map(|p| p.filename.clone()).collect();
        if task.file_names.iter().all(String::is_empty) {
            task.file_names.clear();
        }
        Ok(())
    }

    /// Runs a callback of the task thread itself; `TASK` comes back as the module left it.
    fn task_callback(
        &mut self,
        call: impl FnOnce(Caller<'_>, fmd_lua::Task) -> fmd_lua::Pending<Reply<fmd_lua::TaskReply>>,
    ) -> bool {
        let snapshot = self.container().task.clone();
        let http = self.http.take();
        let pending = call(self.caller(self.affinity, http), snapshot);
        match pending.wait() {
            Ok(reply) => {
                self.http = reply.http;
                self.container().task = reply.value.task;
                reply.value.ok
            }
            Err(e) => {
                log_callback_error(self.id, &e);
                false
            }
        }
    }

    /// `TTaskThread.Execute` (baseunits/uDownloadsManager.pas:1104-1374).
    fn execute(&mut self) -> Result<(), EngineError> {
        let dynamic_page_link = self.def.dynamic_page_link;
        let limits = self.inner.limits(self.module);
        self.module
            .http()
            .set_max_connections(limits.max_connections);
        let settings = self.settings.clone();
        let connections = &settings.connections;
        let chapter_count = self.chapter_links.len();
        let mut failed_retry_count = 0;

        if connections.always_start_from_failed_chapters && self.chapter != 0 {
            self.chapter = 0;
        }

        while self.chapter < chapter_count {
            let statuses = self.container().chapters_status.clone();
            while self.chapter < chapter_count
                && statuses[self.chapter] == ChapterStatus::Downloaded
            {
                self.chapter += 1;
            }
            if self.chapter >= chapter_count {
                break;
            }
            if self.terminated() {
                return Ok(());
            }
            self.enter_chapter()?;

            let chapter_name = self.chapter_names[self.chapter].clone();
            self.working_dir = if self.settings.saveto.generate_chapter_folder {
                self.save_to.join(&chapter_name)
            } else {
                self.save_to.clone()
            };
            if let Err(e) = std::fs::create_dir_all(&self.working_dir) {
                // `StatusFailedToCreateDir` (baseunits/uDownloadsManager.pas:717-725).
                let error = format!("failed to create {}: {e}", self.working_dir.display());
                self.inner
                    .set_status(self.id, TaskStatus::Failed, Some(&error))?;
                return Ok(());
            }

            if self.def.on_task_start.is_some() {
                self.task_callback(|caller, task| caller.task_start(task));
            }

            // `CurrentCustomFileName` (baseunits/uDownloadsManager.pas:1168-1178).
            let ctx = RenameContext {
                website: &self.def.name,
                manga: &self.title,
                chapter: &chapter_name,
                // `CR_FILENAME` stays for `GetFileName` to fill in.
                filename: "%FILENAME%",
                ..RenameContext::default()
            };
            let template = match self.settings.saveto.filename_rename.trim() {
                "" => crate::settings::DEFAULT_FILENAME_CUSTOMRENAME,
                template => template,
            };
            self.custom_file_name =
                custom_rename(template, &ctx, &rename_options(&self.settings.saveto));

            self.load_pages()?;
            if self.container().task.page_links.is_empty() {
                self.reset_phase();
                self.set_status(TaskStatus::Preparing)?;
                self.get_page_number();
                if self.terminated() {
                    let mut c = self.container();
                    c.task.page_links.clear();
                    c.task.page_number = 0;
                    return Ok(());
                }
                self.save_pages()?;
            }

            self.recover_interrupted_writes();
            self.check_for_exists(dynamic_page_link);

            {
                let mut c = self.container();
                if c.task.page_links.is_empty() {
                    c.task.page_links.push(WAITING.to_owned());
                }
                c.task.page_number = page_count(&c.task.page_links);
            }
            if !dynamic_page_link && self.check_for_prepare() {
                self.reset_phase();
                self.set_status(TaskStatus::Preparing)?;
                self.checkout(Phase::PageLink);
                if self.terminated() {
                    return Ok(());
                }
                let mut c = self.container();
                for link in &mut c.task.page_links {
                    if link.trim().is_empty() {
                        *link = WAITING.to_owned();
                    }
                }
                drop(c);
                self.save_pages()?;
            }
            if self.terminated() {
                return Ok(());
            }

            let failed = {
                let mut c = self.container();
                c.task.page_number = page_count(&c.task.page_links);
                c.task.page_links.is_empty()
            };
            let failed = if failed {
                tracing::warn!(target: "fmd_core", "task {}: chapter {} has no pages", self.id.0, self.chapter_link);
                true
            } else {
                self.reset_phase();
                self.set_status(TaskStatus::Downloading)?;
                self.checkout(Phase::Download);
                if self.terminated() {
                    return Ok(());
                }
                if self.check_for_finish(dynamic_page_link) {
                    self.set_status(TaskStatus::Converting)?;
                    if self.convert() {
                        self.set_status(TaskStatus::Compressing)?;
                        !self.compress()
                    } else {
                        true
                    }
                } else {
                    true
                }
            };
            if failed {
                self.set_status(TaskStatus::Failed)?;
            }
            self.leave_chapter(failed)?;

            self.chapter += 1;
            if self.chapter == chapter_count
                && failed_retry_count < connections.auto_retry_failed_tasks
            {
                match self.first_failed_chapter() {
                    Some(chapter) => {
                        self.chapter = chapter;
                        failed_retry_count += 1;
                    }
                    None => self.chapter = chapter_count,
                }
            }
            self.save_chapter_pointer()?;

            if std::fs::read_dir(&self.working_dir).is_ok_and(|mut d| d.next().is_none()) {
                let _ = std::fs::remove_dir(&self.working_dir);
            }
        }

        if self.first_failed_chapter().is_some() {
            self.chapter = 0;
            self.save_chapter_pointer()?;
            self.set_status(TaskStatus::Failed)
        } else {
            self.set_status(TaskStatus::Finished)
        }
    }

    /// Records the chapter the task is at.
    fn enter_chapter(&mut self) -> Result<(), EngineError> {
        self.chapter_link = self.chapter_links[self.chapter].clone();
        self.container().task.current_download_chapter_ptr =
            i32::try_from(self.chapter).unwrap_or(i32::MAX);
        self.save_chapter_pointer()
    }

    fn save_chapter_pointer(&self) -> Result<(), EngineError> {
        let chapter = u32::try_from(self.chapter).unwrap_or(u32::MAX);
        self.inner
            .config
            .db
            .tasks()
            .update_progress(self.id, chapter, None)?;
        Ok(())
    }

    /// Marks the chapter downloaded or failed and forgets its pages
    /// (baseunits/uDownloadsManager.pas:1312-1323); a downloaded chapter is recorded in
    /// `downloaded_chapters` (baseunits/DownloadedChaptersDB.pas:67-91).
    fn leave_chapter(&mut self, failed: bool) -> Result<(), EngineError> {
        let status = if failed {
            ChapterStatus::Failed
        } else {
            ChapterStatus::Downloaded
        };
        {
            let mut c = self.container();
            c.chapters_status[self.chapter] = status;
            c.task.page_links.clear();
            c.task.page_container_links.clear();
            c.task.file_names.clear();
        }
        let db = &self.inner.config.db;
        let chapter = u32::try_from(self.chapter).unwrap_or(u32::MAX);
        db.tasks().update_chapter(self.id, chapter, status, 0)?;
        db.tasks().set_pages(self.id, chapter, &[])?;
        if !failed {
            db.downloaded_chapters().mark(
                &self.def.id,
                &self.manga_link,
                &[self.chapter_link.as_str()],
            )?;
        }
        self.inner.emit(EngineEvent::Chapter {
            task: self.id,
            chapter,
            status,
        });
        Ok(())
    }

    /// `FirstFailedChapters` (baseunits/uDownloadsManager.pas:727-734).
    fn first_failed_chapter(&self) -> Option<usize> {
        self.container()
            .chapters_status
            .iter()
            .position(|s| *s == ChapterStatus::Failed)
    }

    /// Starts a phase's counters at 0 (`WorkCounter`, `DownCounter`).
    fn reset_phase(&self) {
        let mut c = self.container();
        c.work_counter = 0;
        c.down_counter = 0;
        drop(c);
        self.progress(true);
    }

    /// `DoGetPageNumber` (baseunits/uDownloadsManager.pas:829-866).
    fn get_page_number(&mut self) {
        self.container().task.page_number = 0;
        if self.def.on_get_page_number.is_some() {
            let url = self.chapter_link.clone();
            self.task_callback(|caller, task| caller.get_page_number(task, &url));
        }
        let terminated = self.terminated();
        let mut c = self.container();
        for link in &mut c.task.page_links {
            *link = link.trim().to_owned();
        }
        if !terminated && c.task.page_number > 0 {
            let pages = usize::try_from(c.task.page_number).unwrap_or(0);
            while c.task.page_links.len() < pages {
                c.task.page_links.push(WAITING.to_owned());
            }
        }
    }

    /// `CheckForExists` (baseunits/uDownloadsManager.pas:1003-1064): marks the pages whose
    /// image, or the chapter's archive, is on disk downloaded, and the others not.
    fn check_for_exists(&self, dynamic_page_link: bool) -> usize {
        let pages = self.container().task.page_links.len();
        let magick = &self.settings.images.imagemagick;
        let magick_ext = if magick.enabled {
            magick.save_as.to_ascii_lowercase()
        } else {
            String::new()
        };
        let archive = self.archive().is_some_and(|path| path.is_file());
        let mut found = 0;
        for i in 0..pages {
            let base = self.working_dir.join(self.file_name(i));
            let exists = archive
                || find_image_file(&base, "").is_some()
                || (!magick_ext.is_empty() && find_image_file(&base, &magick_ext).is_some());
            let mut c = self.container();
            let Some(link) = c.task.page_links.get_mut(i) else {
                break;
            };
            if exists {
                *link = DONE.to_owned();
                found += 1;
            } else if link == DONE {
                *link = if dynamic_page_link { DYNAMIC } else { WAITING }.to_owned();
            }
        }
        found
    }

    /// Where the chapter is packed: `<save to>/<chapter name>`, which `fmd_pack::pack` adds
    /// the format's extension to (baseunits/uDownloadsManager.pas:553-611).
    fn pack_target(&self) -> PathBuf {
        self.save_to.join(&self.chapter_names[self.chapter])
    }

    /// The chapter's archive, when the output format packs chapters
    /// (baseunits/uDownloadsManager.pas:1027-1041).
    fn archive(&self) -> Option<PathBuf> {
        let format = pack_format(self.settings.output.format)?;
        let mut path = self.pack_target().into_os_string();
        path.push(format.extension());
        Some(path.into())
    }

    /// The folder [`TaskRun::compress`] moves the pages into to pack them.
    fn staging_dir(&self) -> PathBuf {
        self.working_dir.join(&self.chapter_names[self.chapter])
    }

    /// Recovers the chapter's pages from a process killed while saving or packing them
    /// (docs/tickets/T44-download-hard-crash-resume.md), before `CheckForExists` looks for
    /// them:
    /// - half-saved pages are removed;
    /// - pages left in the staging folder are put back. An archive on disk is whole, as
    ///   `fmd_pack::pack` only renames it into place once written, so then the pages are what
    ///   was left of removing the packed ones: they go instead of being packed again over the
    ///   archive.
    ///
    /// FMD2 saves and packs in place and has no staging folder.
    fn recover_interrupted_writes(&self) {
        let pages = self.container().task.page_links.len();
        for i in 0..pages {
            remove_partial_images(&self.working_dir.join(self.file_name(i)));
        }
        let Some(archive) = self.archive() else {
            return;
        };
        let staging = self.staging_dir();
        if !staging.is_dir() {
            return;
        }
        let packed = archive.is_file();
        for i in 0..pages {
            let name = self.file_name(i);
            let Some(file) = find_image_file(&staging.join(&name), "") else {
                continue;
            };
            let result = if packed {
                std::fs::remove_file(&file)
            } else if let Some(file_name) = file.file_name() {
                std::fs::rename(&file, self.working_dir.join(file_name))
            } else {
                continue;
            };
            if let Err(e) = result {
                tracing::warn!(target: "fmd_core", "task {}: recovering {}: {e}", self.id.0, file.display());
            }
        }
        // Only an empty folder goes.
        let _ = std::fs::remove_dir(&staging);
    }

    /// `CheckForPrepare` (baseunits/uDownloadsManager.pas:980-1001): whether a page still
    /// lacks its link.
    fn check_for_prepare(&self) -> bool {
        let c = self.container();
        c.task.page_links.is_empty()
            || c.task
                .page_links
                .iter()
                .any(|l| l == WAITING || l.is_empty())
    }

    /// `CheckForFinish` (baseunits/uDownloadsManager.pas:1066-1103): whether every page is on
    /// disk.
    fn check_for_finish(&self, dynamic_page_link: bool) -> bool {
        let pages = self.container().task.page_links.len();
        if pages == 0 {
            return false;
        }
        let missing = pages - self.check_for_exists(dynamic_page_link);
        if missing > 0 {
            tracing::warn!(target: "fmd_core", "task {}: {missing} of {pages} pages of {} missing", self.id.0, self.chapter_link);
        }
        self.save_pages().is_ok() && missing == 0
    }

    /// The saved image of every page, in page order.
    fn page_files(&self, ext: &str) -> Vec<PathBuf> {
        let pages = self.container().task.page_links.len();
        (0..pages)
            .filter_map(|i| find_image_file(&self.working_dir.join(self.file_name(i)), ext))
            .collect()
    }

    /// `TTaskThread.Convert` (baseunits/uDownloadsManager.pas:613-711): ImageMagick, when
    /// enabled; the built-in conversions happen as pages are saved.
    fn convert(&self) -> bool {
        let magick = &self.settings.images.imagemagick;
        if !magick.enabled {
            return true;
        }
        let options = MagickOptions {
            save_as: magick.save_as.to_ascii_lowercase(),
            quality: magick.quality,
            compression: (!magick.compression.eq_ignore_ascii_case("none"))
                .then(|| magick.compression.clone()),
            ..MagickOptions::default()
        };
        match fmd_pack::magick_convert(&self.working_dir, &self.page_files(""), &options) {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(target: "fmd_core", "task {}: ImageMagick: {e}", self.id.0);
                false
            }
        }
    }

    /// `TTaskThread.Compress` (baseunits/uDownloadsManager.pas:553-611): packs the chapter's
    /// page images (:579-590) into `<save to>/<chapter name>` plus the format's extension.
    /// `fmd_pack::pack` takes a whole folder, so the pages are first moved into one of their
    /// own, named after the chapter (the PDF and EPUB title, baseunits/uPacker.pas:186, :225);
    /// other files in the working directory stay out of the archive.
    fn compress(&self) -> bool {
        let Some(format) = pack_format(self.settings.output.format) else {
            return true;
        };
        let magick = &self.settings.images.imagemagick;
        let ext = if magick.enabled {
            magick.save_as.to_ascii_lowercase()
        } else {
            String::new()
        };
        let options = PackOptions {
            pdf_quality: u8::try_from(self.settings.output.pdf_quality.min(100)).unwrap_or(100),
            remove_sources: true,
        };
        let staging = self.staging_dir();
        let packed = std::fs::create_dir_all(&staging).and_then(|()| {
            for file in self.page_files(&ext) {
                if let Some(file_name) = file.file_name() {
                    std::fs::rename(&file, staging.join(file_name))?;
                }
            }
            Ok(())
        });
        let packed = packed
            .map_err(fmd_pack::PackError::from)
            .and_then(|()| fmd_pack::pack(&staging, format, &self.pack_target(), &options));
        // Nothing to pack leaves the folder behind (the archive was already there).
        let _ = std::fs::remove_dir(&staging);
        match packed {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(target: "fmd_core", "task {}: failed to compress: {e}", self.id.0);
                false
            }
        }
    }
}

/// The page count of a page list.
fn page_count(links: &[String]) -> i32 {
    i32::try_from(links.len()).unwrap_or(i32::MAX)
}

fn page_status(link: &str) -> PageStatus {
    if link == DONE {
        PageStatus::Downloaded
    } else {
        PageStatus::Waiting
    }
}

/// A callback that raised an error makes FMD2's `Do*` function return false, after logging
/// it (e.g. baseunits/lua/LuaWebsiteModules.pas:298-301).
pub(super) fn log_callback_error(id: TaskId, error: &JobError) {
    match error {
        JobError::NoCallback { .. } => {}
        error => tracing::warn!(target: "fmd_core", "task {}: {error}", id.0),
    }
}
