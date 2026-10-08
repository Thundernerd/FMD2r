//! The page threads of a task (`TDownloadThread`, baseunits/uDownloadsManager.pas:303-460) and
//! how the task hands out pages to them (:882-974).

use std::path::Path;

use fmd_http::HttpSession;
use fmd_lua::{Affinity, Caller, Pending, Reply, TaskReply};
use fmd_store::TaskStatus;

use super::files::{find_image_file, save_image};
use super::task::{DONE, DYNAMIC, Phase, TaskRun, WAITING, log_callback_error};

impl TaskRun<'_> {
    /// `CheckOut` (baseunits/uDownloadsManager.pas:969-973): runs page threads for `phase`
    /// until every page is handed out and done. The threads are capped by the task's thread
    /// limit (`GetCurrentLimit`, :868-880) and the number of pages.
    pub(super) fn checkout(&self, phase: Phase) {
        let limit = self.inner.limits(self.module).threads_per_task.max(1);
        let pages = usize::try_from(self.container().task.page_number).unwrap_or(0);
        let threads = usize::try_from(limit).unwrap_or(1).min(pages).max(1);
        std::thread::scope(|scope| {
            for i in 0..threads {
                let spawned = std::thread::Builder::new()
                    .name(format!("fmd-task-{}-page-{i}", self.id.0))
                    .spawn_scoped(scope, || PageThread::new(self).run(phase));
                if let Err(e) = spawned {
                    tracing::warn!(target: "fmd_core", "task {}: page thread: {e}", self.id.0);
                    if i == 0 {
                        PageThread::new(self).run(phase);
                    }
                    break;
                }
            }
        });
        self.page_changed(None, true);
        self.progress(true);
    }

    /// `GetWorkId` (baseunits/uDownloadsManager.pas:902-967): the next page for `phase`, or
    /// `None` when all are handed out or the task is terminated. Pages skipped on the way
    /// count as done.
    fn work_id(&self, phase: Phase) -> Option<usize> {
        if self.terminated() {
            return None;
        }
        let mut c = self.container();
        let page_number = usize::try_from(c.task.page_number).unwrap_or(0);
        if c.work_counter >= page_number {
            return None;
        }
        while c.work_counter < c.task.page_links.len() {
            let i = c.work_counter;
            c.work_counter += 1;
            let wanted = match phase {
                Phase::PageLink => c.task.page_links[i] == WAITING,
                Phase::Download => c.task.page_links[i] != DONE,
            };
            if wanted {
                return Some(i);
            }
            c.down_counter += 1;
        }
        None
    }

    /// `DoSuccess` (baseunits/uDownloadsManager.pas:449-458).
    fn page_done(&self) {
        self.container().down_counter += 1;
        self.progress(false);
    }
}

/// One page thread: its own Lua state and `HTTP` session, kept across its pages.
struct PageThread<'r, 'a> {
    run: &'r TaskRun<'a>,
    affinity: Affinity,
    http: Option<HttpSession>,
}

impl<'r, 'a> PageThread<'r, 'a> {
    fn new(run: &'r TaskRun<'a>) -> Self {
        PageThread {
            run,
            affinity: run.inner.config.pool.affinity(),
            http: None,
        }
    }

    /// `DoPageLink` and `DoDownload` (baseunits/uDownloadsManager.pas:423-447).
    fn run(mut self, phase: Phase) {
        while let Some(work_id) = self.run.work_id(phase) {
            let ok = match phase {
                Phase::PageLink => self.get_link_page(work_id),
                Phase::Download => self.download_image(work_id),
            };
            if ok {
                self.run.page_done();
            }
        }
    }

    fn session(&mut self) -> HttpSession {
        self.http.take().unwrap_or_else(|| self.run.new_session())
    }

    /// Runs a page callback over this thread's session. `TASK` is shared by all page
    /// threads, so only the entries the callback changed are taken back.
    fn task_callback(
        &mut self,
        call: impl FnOnce(Caller<'_>, fmd_lua::Task) -> Pending<Reply<TaskReply>>,
    ) -> bool {
        let before = self.run.container().task.clone();
        let session = self.session();
        let pending = call(
            self.run.caller(self.affinity, Some(session)),
            before.clone(),
        );
        match pending.wait() {
            Ok(reply) => {
                self.http = reply.http;
                merge(&mut self.run.container().task, &before, reply.value.task);
                reply.value.ok
            }
            Err(e) => {
                log_callback_error(self.run.id, &e);
                false
            }
        }
    }

    /// `GetLinkPageFromURL` (baseunits/uDownloadsManager.pas:327-332): `OnGetImageURL` for
    /// page `work_id`, with the chapter's link as `URL`.
    fn get_link_page(&mut self, work_id: usize) -> bool {
        if self.run.def.on_get_image_url.is_none() {
            return false;
        }
        let url = self.run.chapter_link.clone();
        let work = i32::try_from(work_id).unwrap_or(i32::MAX);
        let ok = self.task_callback(|caller, task| caller.get_image_url(task, work, &url));
        self.run.page_changed(Some(work_id), false);
        ok
    }

    /// `TDownloadThread.DownloadImage` (baseunits/uDownloadsManager.pas:334-420).
    ///
    /// With `DynamicPageLink`, a page whose link is not known yet gets it from `OnGetImageURL`
    /// right before it downloads (docs/tickets/T20-download-engine.md); FMD2 instead passes
    /// the marker on to `OnDownloadImage`, which still happens when the module has no
    /// `OnGetImageURL`.
    fn download_image(&mut self, work_id: usize) -> bool {
        let run = self.run;
        let dir = run.working_dir.clone();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            let error = format!("failed to create {}: {e}", dir.display());
            let _ = run
                .inner
                .set_status(run.id, TaskStatus::Failed, Some(&error));
            return false;
        }

        let link = |run: &TaskRun<'_>| {
            run.container()
                .task
                .page_links
                .get(work_id)
                .map(|l| l.trim().to_owned())
                .unwrap_or_default()
        };
        let mut url = link(run);
        if run.def.dynamic_page_link
            && run.def.on_get_image_url.is_some()
            && (url.is_empty() || url == WAITING || url == DYNAMIC)
        {
            self.get_link_page(work_id);
            url = link(run);
        }
        match url.as_str() {
            "" => {
                if let Some(l) = run.container().task.page_links.get_mut(work_id) {
                    *l = WAITING.to_owned();
                }
                return false;
            }
            WAITING => return false,
            DONE => return true,
            _ => {}
        }

        let file_name = run.file_name(work_id);
        let mut session = self.session();
        session.reset();
        // `AcceptImage` (baseunits/httpsendthread.pas:919-922).
        session.headers_mut().set_value("Accept", "image/webp,*/*");
        self.http = Some(session);

        let def = &run.def;
        if def.on_download_image.is_some() {
            let c = run.container();
            let containers = &c.task.page_container_links;
            if usize::try_from(c.task.page_number).ok() == Some(containers.len())
                && work_id < containers.len()
            {
                url = containers[work_id].clone();
            }
        }
        let work = i32::try_from(work_id).unwrap_or(i32::MAX);

        if def.on_before_download_image.is_some() {
            self.task_callback(|caller, task| caller.before_download_image(task, work, &url));
        }
        let mut ok = if def.on_download_image.is_some() {
            self.task_callback(|caller, task| caller.download_image(task, work, &url))
        } else {
            let mut session = self.session();
            let ok = session.get(&url).unwrap_or(false);
            self.http = Some(session);
            ok
        };
        if ok && let Some(session) = &self.http {
            run.container().bytes += session.document().len() as u64;
        }

        let base = dir.join(&file_name);
        let mut saved = None;
        if ok {
            saved = find_image_file(&base, "");
            if saved.is_none() {
                saved = if def.on_save_image.is_some() {
                    self.save_image(work, &dir, &file_name)
                        .filter(|s| !s.is_empty())
                        .map(Into::into)
                } else {
                    let data = self.http.as_ref().map_or(&[][..], |s| s.document());
                    save_image(data, &dir, &file_name, &run.settings.images)
                };
                ok = saved.is_some();
            }
        }
        let saved = saved.unwrap_or_default();
        if ok {
            ok = Path::new(&saved).is_file();
        }

        if run.terminated() {
            return false;
        }
        if ok {
            if let Some(l) = run.container().task.page_links.get_mut(work_id) {
                *l = DONE.to_owned();
            }
            run.page_changed(Some(work_id), false);
            if def.on_after_image_saved.is_some() {
                let file = saved.to_string_lossy().into_owned();
                let pending = run
                    .caller(self.affinity, None)
                    .after_image_saved(work, &file);
                ok = match pending.wait() {
                    Ok(reply) => reply.value,
                    Err(e) => {
                        log_callback_error(run.id, &e);
                        false
                    }
                };
            }
        }
        ok
    }

    /// `OnSaveImage` with the chapter's directory as `PATH` (with a trailing separator, as
    /// FMD2 builds `CurrentWorkingDir`) and the file name without extension as `FILENAME`.
    fn save_image(&mut self, work: i32, dir: &Path, name: &str) -> Option<String> {
        let mut path = dir.to_string_lossy().into_owned();
        if !path.ends_with('/') {
            path.push('/');
        }
        let session = self.session();
        let pending = self
            .run
            .caller(self.affinity, Some(session))
            .save_image(work, &path, name);
        match pending.wait() {
            Ok(reply) => {
                self.http = reply.http;
                Some(reply.value)
            }
            Err(e) => {
                log_callback_error(self.run.id, &e);
                None
            }
        }
    }
}

/// Takes back into `shared` what a callback changed in its copy of `TASK`: whole lists that
/// changed length, single entries otherwise, and changed numbers and link.
fn merge(shared: &mut fmd_lua::Task, before: &fmd_lua::Task, after: fmd_lua::Task) {
    fn list(shared: &mut Vec<String>, before: &[String], after: Vec<String>) {
        if after.len() != before.len() {
            *shared = after;
            return;
        }
        for (i, (old, new)) in before.iter().zip(after).enumerate() {
            if *old != new
                && let Some(entry) = shared.get_mut(i)
            {
                *entry = new;
            }
        }
    }
    list(&mut shared.page_links, &before.page_links, after.page_links);
    list(
        &mut shared.page_container_links,
        &before.page_container_links,
        after.page_container_links,
    );
    list(&mut shared.file_names, &before.file_names, after.file_names);
    if after.page_number != before.page_number {
        shared.page_number = after.page_number;
    }
    if after.current_max_file_name_length != before.current_max_file_name_length {
        shared.current_max_file_name_length = after.current_max_file_name_length;
    }
    if after.link != before.link {
        shared.link = after.link;
    }
}
