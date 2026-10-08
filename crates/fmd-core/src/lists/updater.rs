//! The list update of one module, reproducing `TUpdateListManagerThread.Execute` and its
//! `TUpdateListThread` workers (baseunits/uUpdateThread.pas:173-313, :626-780, :842-903).

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use fmd_http::TerminateToken;
use fmd_lua::{JobError, Module, ModuleDef, Pending, Reply, UpdateList, WorkerPool};
use fmd_store::{ListsDb, MangaListing, StoreError};
use thiserror::Error;

use super::info::{listing, remove_host};
use super::today_jdn;

/// `INFORMATION_NOT_FOUND` (baseunits/WebsiteModules.pas:21).
const INFORMATION_NOT_FOUND: u8 = 2;

/// How a list update runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOptions {
    /// Workers when the module declares no `MaxThreadPerTaskLimit`
    /// (`OptionMaxUpdateListThreads`, `connections.max_update_list_threads`).
    pub max_threads: u32,
    /// Store new titles with their listed name only, without running `OnGetInfo` on each
    /// (`OptionUpdateListNoMangaInfo`, `update_lists.no_manga_info`).
    pub no_manga_info: bool,
}

/// The step a list update is at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListPhase {
    Preparing,
    /// `CS_DIRECTORY_COUNT`: one `OnGetDirectoryPageNumber` per directory.
    DirectoryCount,
    /// `CS_DIRECTORY_PAGE`: one `OnGetNameAndLink` per directory page.
    DirectoryPages,
    /// `CS_INFO`: one `OnGetInfo` per new title.
    Info,
    Saving,
}

/// Where a list update is, as FMD2's update-list status bar shows it
/// (baseunits/uUpdateThread.pas:607-624, :862-897).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListProgress {
    pub phase: ListPhase,
    /// Work items of the phase handed out so far.
    pub done: u64,
    /// Work items of the phase; grows when a module raises `UPDATELIST.CurrentDirectoryPageNumber`.
    pub total: u64,
    /// FMD2's status text, or the module's own from `UPDATELIST.UpdateStatusText`.
    pub status_text: String,
}

/// What a list update did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateOutcome {
    /// Titles added to the list.
    pub added: u64,
    /// Whether a sorted list stopped at a page holding an already listed title.
    pub stopped_early: bool,
    /// Whether the update was terminated.
    pub cancelled: bool,
}

/// Why a list update stopped.
#[derive(Debug, Error)]
pub enum ListError {
    #[error("lists.db: {0}")]
    Store(#[from] StoreError),
    #[error("worker pool: {0}")]
    Pool(JobError),
}

/// Updates modules' lists in `lists.db` by running their update-list callbacks on a
/// [`WorkerPool`].
pub struct ListUpdater {
    pool: Arc<WorkerPool>,
    lists: ListsDb,
}

/// The state of one update, shared by its phases.
struct Run<'a> {
    module: &'a Arc<Module>,
    def: ModuleDef,
    terminate: &'a TerminateToken,
    progress: &'a mut dyn FnMut(&ListProgress),
    phase: ListPhase,
    /// `FCurrentGetInfoLimit`: the work items of the current phase.
    limit: i32,
    /// `workPtr`: the work items handed out.
    handed_out: i32,
    /// The listed links (`mainDataProcess`).
    known: HashSet<String>,
    /// The new titles, as `(link, name)` in the order found (`tempDataProcess`).
    found: Vec<(String, String)>,
    found_links: HashSet<String>,
    /// `isFinishSearchingForNewManga`.
    finished: bool,
    stopped_early: bool,
    rows: Vec<MangaListing>,
}

impl Run<'_> {
    fn report(&mut self, status_text: String) {
        let progress = ListProgress {
            phase: self.phase,
            done: u64::try_from(self.handed_out).unwrap_or(0),
            total: u64::try_from(self.limit).unwrap_or(0),
            status_text,
        };
        (self.progress)(&progress);
    }

    /// Reports the module's `UpdateStatusText` and takes a raised
    /// `CurrentDirectoryPageNumber`, which only ever grows the current phase
    /// (`SetCurrentDirectoryPageNumber`, baseunits/uUpdateThread.pas:451-460).
    fn read_back(&mut self, list: UpdateList) {
        if list.current_directory_page_number > self.limit {
            self.limit = list.current_directory_page_number;
        }
        if let Some(text) = list.status_text {
            self.report(text);
        }
    }

    /// The `UPDATELIST` a callback sees: `CurrentDirectoryPageNumber` reads the current
    /// phase's limit (baseunits/uUpdateThread.pas:118).
    fn list(&self) -> UpdateList {
        UpdateList {
            current_directory_page_number: self.limit,
            status_text: None,
        }
    }
}

/// A failed callback is logged and its work item skipped, as FMD2's workers catch and log
/// exceptions (e.g. baseunits/uUpdateThread.pas:207-210); a pool that is gone ends the update.
fn callback_result<T>(module: &str, result: Result<T, JobError>) -> Result<Option<T>, ListError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error @ (JobError::Callback(_) | JobError::NoCallback { .. })) => {
            tracing::warn!(target: "fmd_core", "list update of {module}: {error}");
            Ok(None)
        }
        Err(error) => Err(ListError::Pool(error)),
    }
}

impl ListUpdater {
    pub fn new(pool: Arc<WorkerPool>, lists: ListsDb) -> Self {
        Self { pool, lists }
    }

    /// Updates `module`'s list and returns what it added. Blocks until done, so call it from a
    /// thread outside any tokio runtime. Terminating `terminate` stops it after the callbacks in
    /// flight; what it found is then kept only as FMD2 keeps it (see below).
    ///
    /// The steps are FMD2's (baseunits/uUpdateThread.pas:626-780):
    /// 1. `OnAfterUpdateList`, then `OnBeforeUpdateList` (:672-675).
    /// 2. `OnGetDirectoryPageNumber` for each of the module's `TotalDirectory` directories, with
    ///    `PAGENUMBER` 1 and `WORKPTR` the directory; fewer than 1 page counts as 1 (:192-212).
    /// 3. For each directory, with `MODULE.CurrentDirectoryIndex` set to it, `OnGetNameAndLink`
    ///    for each page index; links lose their host (:224-275). When the list already holds
    ///    titles and the module sets `SortedList`, a page holding an already listed link ends
    ///    the directory: no further page is handed out (:247-251, :852-853).
    /// 4. `OnBeforeUpdateList` again (:705-706).
    /// 5. `OnGetInfo` for each new title, unless `no_manga_info` is set or the module clears
    ///    `InformationAvailable`, in which case titles are stored with their listed name
    ///    (:716-741). A title whose info is not found or has status `-1` is not stored
    ///    (:282-284).
    /// 6. The new titles are merged into the list; listed titles are never removed.
    ///
    /// Workers: `MaxThreadPerTaskLimit` when the module declares one, else `max_threads`, at
    /// most `MaxConnectionLimit` and at least 1 (`GetCurrentLimit`, :826-840).
    pub fn update(
        &self,
        module: &Arc<Module>,
        options: &UpdateOptions,
        terminate: &TerminateToken,
        progress: &mut dyn FnMut(&ListProgress),
    ) -> Result<UpdateOutcome, ListError> {
        let def = module.def();
        let known = self.lists.masterlist().links(&def.id)?;
        let mut run = Run {
            module,
            def,
            terminate,
            progress,
            phase: ListPhase::Preparing,
            limit: 0,
            handed_out: 0,
            known,
            found: Vec::new(),
            found_links: HashSet::new(),
            finished: false,
            stopped_early: false,
            rows: Vec::new(),
        };
        run.report("Preparing...".into());
        let threads = threads(&run.def, options);

        self.after_update_list(&mut run)?;
        self.before_update_list(&mut run)?;
        let pages = self.directory_page_counts(&mut run, threads)?;
        if terminate.is_terminated() {
            self.after_update_list(&mut run)?;
            return Ok(cancelled());
        }

        // `FIsPreListAvailable` (baseunits/uUpdateThread.pas:687-689).
        let pre_list = !run.known.is_empty();
        for (directory, pages) in pages.into_iter().enumerate() {
            let directory = i32::try_from(directory).unwrap_or(i32::MAX);
            self.directory_pages(&mut run, threads, directory, pages, pre_list)?;
            if terminate.is_terminated() {
                break;
            }
        }

        self.before_update_list(&mut run)?;
        let sorted = run.def.sorted_list;
        if terminate.is_terminated() && !(options.no_manga_info && !sorted) {
            return Ok(cancelled());
        }

        run.phase = ListPhase::Info;
        run.report("Indexing new title(s)...".into());
        let jdn = today_jdn();
        if !run.def.information_available || options.no_manga_info {
            run.rows = run
                .found
                .iter()
                .map(|(link, name)| MangaListing {
                    link: link.clone(),
                    title: name.clone(),
                    added_jdn: jdn,
                    ..MangaListing::default()
                })
                .collect();
        } else {
            self.info(&mut run, threads, jdn)?;
        }

        // FMD2 saves unless a sorted list was terminated, which would leave titles between
        // the stored ones that a later early stop never reaches (:743-748).
        let cancelled = terminate.is_terminated();
        if cancelled && sorted {
            return Ok(UpdateOutcome {
                cancelled,
                ..UpdateOutcome::default()
            });
        }
        run.phase = ListPhase::Saving;
        run.report("Saving data...".into());
        let added = self.lists.masterlist().insert_new(&run.def.id, &run.rows)?;
        Ok(UpdateOutcome {
            added,
            stopped_early: run.stopped_early,
            cancelled,
        })
    }

    /// `OnBeforeUpdateList`, when declared; its result is not looked at
    /// (baseunits/uUpdateThread.pas:674-675, :705-706).
    fn before_update_list(&self, run: &mut Run<'_>) -> Result<(), ListError> {
        if run.def.on_before_update_list.is_none() {
            return Ok(());
        }
        let pending = self.caller(run).before_update_list(run.list());
        if let Some(reply) = callback_result(&run.def.id, pending.wait())? {
            run.read_back(reply.value.list);
        }
        Ok(())
    }

    /// `OnAfterUpdateList`, when declared; its result is not looked at
    /// (baseunits/uUpdateThread.pas:672-673, :683-684).
    fn after_update_list(&self, run: &mut Run<'_>) -> Result<(), ListError> {
        if run.def.on_after_update_list.is_none() {
            return Ok(());
        }
        let pending = self.caller(run).after_update_list(run.list());
        if let Some(reply) = callback_result(&run.def.id, pending.wait())? {
            run.read_back(reply.value.list);
        }
        Ok(())
    }

    fn caller<'a>(&'a self, run: &Run<'_>) -> fmd_lua::Caller<'a> {
        self.pool
            .on(run.module)
            .with_terminate(run.terminate.clone())
    }

    /// The page count of each directory (`TotalDirectoryPage`, `CS_DIRECTORY_COUNT`,
    /// baseunits/uUpdateThread.pas:183-213).
    fn directory_page_counts(
        &self,
        run: &mut Run<'_>,
        threads: usize,
    ) -> Result<Vec<i32>, ListError> {
        let directories = usize::try_from(run.def.total_directory).unwrap_or(0);
        let mut pages = vec![1; directories];
        if run.def.on_get_directory_page_number.is_none() {
            return Ok(pages);
        }
        run.phase = ListPhase::DirectoryCount;
        run.limit = run.def.total_directory;
        run.handed_out = 0;
        run.finished = false;
        check_out(
            run,
            threads,
            |run, work_ptr| {
                run.report("Getting directory...".into());
                let list = run.list();
                self.caller(run)
                    .get_directory_page_number(list, 1, work_ptr)
            },
            |run, work_ptr, result| {
                if let Some(reply) = callback_result(&run.def.id, result)? {
                    let count = reply.value.page.max(1);
                    if let Some(slot) = usize::try_from(work_ptr)
                        .ok()
                        .and_then(|i| pages.get_mut(i))
                    {
                        *slot = count;
                    }
                    run.read_back(reply.value.list);
                }
                Ok(())
            },
        )?;
        Ok(pages)
    }

    /// The names and links of `directory`'s `pages` pages (`CS_DIRECTORY_PAGE`,
    /// baseunits/uUpdateThread.pas:215-276, :690-702).
    fn directory_pages(
        &self,
        run: &mut Run<'_>,
        threads: usize,
        directory: i32,
        pages: i32,
        pre_list: bool,
    ) -> Result<(), ListError> {
        if run.def.on_get_name_and_link.is_none() {
            return Ok(());
        }
        run.module.set_current_directory_index(directory);
        run.phase = ListPhase::DirectoryPages;
        run.limit = pages;
        run.handed_out = 0;
        run.finished = false;
        let total_directory = run.def.total_directory;
        check_out(
            run,
            threads,
            |run, page| {
                let text = format!(
                    "Looking for new title(s) {}/{total_directory}...",
                    directory + 1
                );
                run.report(text);
                let list = run.list();
                self.caller(run).get_name_and_link(list, page)
            },
            |run, _, result| {
                let Some(reply) = callback_result(&run.def.id, result)? else {
                    return Ok(());
                };
                let reply = reply.value;
                if reply.status != INFORMATION_NOT_FOUND {
                    run.add_links(&reply.names, &reply.links, pre_list);
                }
                run.read_back(reply.list);
                Ok(())
            },
        )
    }

    /// The info of each new title (`CS_INFO`, baseunits/uUpdateThread.pas:278-307).
    fn info(&self, run: &mut Run<'_>, threads: usize, jdn: i64) -> Result<(), ListError> {
        run.limit = i32::try_from(run.found.len()).unwrap_or(i32::MAX);
        run.handed_out = 0;
        run.finished = false;
        check_out(
            run,
            threads,
            |run, i| {
                let (link, name) = run.found_title(i);
                let pending = self.caller(run).get_info(&link);
                run.report(format!("Getting info \"{name}\""));
                pending
            },
            |run, i, result| {
                let Some(reply) = callback_result(&run.def.id, result)? else {
                    return Ok(());
                };
                let reply = reply.value;
                if reply.status != INFORMATION_NOT_FOUND
                    && !run.terminate.is_terminated()
                    && reply.info.status != "-1"
                {
                    let (link, name) = run.found_title(i);
                    run.rows.push(listing(&reply.info, &name, &link, jdn));
                }
                Ok(())
            },
        )
    }
}

impl Run<'_> {
    /// The `(link, name)` of new title `i`.
    fn found_title(&self, i: i32) -> (String, String) {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.found.get(i))
            .cloned()
            .unwrap_or_default()
    }

    /// Takes the names and links one page produced (baseunits/uUpdateThread.pas:236-264).
    fn add_links(&mut self, names: &[String], links: &[String], pre_list: bool) {
        // `RemoveHostFromURLsPair` only strips hosts, and drops pairs whose link becomes
        // empty, when both lists are the same length (baseunits/uBaseUnit.pas:983-1000).
        let paired = names.len() == links.len();
        for (i, link) in links.iter().enumerate() {
            let link = if paired {
                remove_host(link)
            } else {
                link.clone()
            };
            if link.is_empty() {
                continue;
            }
            let name = names.get(i).cloned().unwrap_or_default();
            if pre_list && self.known.contains(&link) {
                if self.def.sorted_list {
                    self.finished = true;
                    self.stopped_early = true;
                }
                continue;
            }
            // `tempDataProcess.AddData` ignores a link it already holds.
            if self.found_links.insert(link.clone()) {
                self.found.push((link, name));
            }
        }
    }
}

fn cancelled() -> UpdateOutcome {
    UpdateOutcome {
        cancelled: true,
        ..UpdateOutcome::default()
    }
}

/// `GetCurrentLimit` without the per-module `UpdateListNumberOfThread` override
/// (baseunits/uUpdateThread.pas:826-840).
fn threads(def: &ModuleDef, options: &UpdateOptions) -> usize {
    let mut threads = if def.max_thread_per_task_limit > 0 {
        def.max_thread_per_task_limit
    } else {
        i32::try_from(options.max_threads).unwrap_or(i32::MAX)
    };
    if def.max_connection_limit > 0 && threads > def.max_connection_limit {
        threads = def.max_connection_limit;
    }
    usize::try_from(threads.max(1)).unwrap_or(1)
}

/// One `CheckOut` (baseunits/uUpdateThread.pas:441-449): hands out work items `0..run.limit`
/// to at most `threads` callbacks at a time, as `GetNext` does (:842-903), and handles each
/// result in the order handed out. Nothing more is handed out once the run is terminated or
/// `run.finished` is set; callbacks in flight still finish.
fn check_out<'r, T: 'static>(
    run: &mut Run<'r>,
    threads: usize,
    mut submit: impl FnMut(&mut Run<'r>, i32) -> Pending<Reply<T>>,
    mut handle: impl FnMut(&mut Run<'r>, i32, Result<Reply<T>, JobError>) -> Result<(), ListError>,
) -> Result<(), ListError> {
    let mut in_flight: VecDeque<(i32, Pending<Reply<T>>)> = VecDeque::new();
    loop {
        while in_flight.len() < threads
            && run.handed_out < run.limit
            && !run.finished
            && !run.terminate.is_terminated()
        {
            let work_ptr = run.handed_out;
            run.handed_out += 1;
            let pending = submit(run, work_ptr);
            in_flight.push_back((work_ptr, pending));
        }
        let Some((work_ptr, pending)) = in_flight.pop_front() else {
            return Ok(());
        };
        handle(run, work_ptr, pending.wait())?;
    }
}
