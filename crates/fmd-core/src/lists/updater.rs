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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOptions {
    /// Workers when the module declares no `MaxThreadPerTaskLimit`
    /// (`OptionMaxUpdateListThreads`).
    pub max_threads: u32,
    /// Skip `OnGetInfo` for new titles (`OptionUpdateListNoMangaInfo`).
    pub no_manga_info: bool,
}

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

/// FMD2's update-list status bar (baseunits/uUpdateThread.pas:607-624, :862-897).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListProgress {
    pub phase: ListPhase,
    /// Handed out so far.
    pub done: u64,
    /// Grows when a module raises `UPDATELIST.CurrentDirectoryPageNumber`.
    pub total: u64,
    /// FMD2's status text, or the module's own from `UPDATELIST.UpdateStatusText`.
    pub status_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateOutcome {
    pub added: u64,
    /// Whether a sorted list stopped at a page holding an already listed title.
    pub stopped_early: bool,
    pub cancelled: bool,
}

#[derive(Debug, Error)]
pub enum ListError {
    #[error("lists.db: {0}")]
    Store(#[from] StoreError),
    #[error("worker pool: {0}")]
    Pool(JobError),
}

pub struct ListUpdater {
    pool: Arc<WorkerPool>,
    lists: ListsDb,
}

struct Run<'a> {
    module: &'a Arc<Module>,
    def: ModuleDef,
    terminate: &'a TerminateToken,
    progress: &'a mut dyn FnMut(&ListProgress),
    phase: ListPhase,
    /// `FCurrentGetInfoLimit`.
    limit: i32,
    /// `workPtr`.
    handed_out: i32,
    /// The listed links (`mainDataProcess`).
    known: HashSet<String>,
    /// `(link, name)` in the order found (`tempDataProcess`).
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

    /// A raised `CurrentDirectoryPageNumber` only ever grows the phase
    /// (`SetCurrentDirectoryPageNumber`, baseunits/uUpdateThread.pas:451-460).
    fn read_back(&mut self, list: UpdateList) {
        if list.current_directory_page_number > self.limit {
            self.limit = list.current_directory_page_number;
        }
        if let Some(text) = list.status_text {
            self.report(text);
        }
    }

    /// `CurrentDirectoryPageNumber` reads the phase's limit (baseunits/uUpdateThread.pas:118).
    fn list(&self) -> UpdateList {
        UpdateList {
            current_directory_page_number: self.limit,
            status_text: None,
        }
    }
}

/// A failed callback is logged and skipped (e.g. baseunits/uUpdateThread.pas:207-210); a pool
/// that is gone ends the update.
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

    /// Blocking. Terminating stops it after the callbacks in flight.
    ///
    /// FMD2's steps (baseunits/uUpdateThread.pas:626-780):
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
    /// Workers as `GetCurrentLimit` (:826-840).
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

        self.update_list_callback(&mut run, false)?;
        self.update_list_callback(&mut run, true)?;
        let pages = self.directory_page_counts(&mut run, threads)?;
        if terminate.is_terminated() {
            self.update_list_callback(&mut run, false)?;
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

        self.update_list_callback(&mut run, true)?;
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

        // Not saved when a sorted list was terminated: a later early stop would never reach
        // the gap (:743-748).
        let cancelled = terminate.is_terminated();
        if cancelled && sorted {
            return Ok(self::cancelled());
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

    /// `OnBeforeUpdateList` (baseunits/uUpdateThread.pas:674-675, :705-706) or
    /// `OnAfterUpdateList` (:672-673, :683-684); the result is ignored.
    fn update_list_callback(&self, run: &mut Run<'_>, before: bool) -> Result<(), ListError> {
        let declared = if before {
            &run.def.on_before_update_list
        } else {
            &run.def.on_after_update_list
        };
        if declared.is_none() {
            return Ok(());
        }
        let caller = self.caller(run);
        let pending = if before {
            caller.before_update_list(run.list())
        } else {
            caller.after_update_list(run.list())
        };
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

    /// `TotalDirectoryPage`, `CS_DIRECTORY_COUNT` (baseunits/uUpdateThread.pas:183-213).
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
        run.start_phase(ListPhase::DirectoryCount, run.def.total_directory);
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

    /// `CS_DIRECTORY_PAGE` (baseunits/uUpdateThread.pas:215-276, :690-702).
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
        run.start_phase(ListPhase::DirectoryPages, pages);
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

    /// `CS_INFO` (baseunits/uUpdateThread.pas:278-307).
    fn info(&self, run: &mut Run<'_>, threads: usize, jdn: i64) -> Result<(), ListError> {
        run.start_phase(
            ListPhase::Info,
            i32::try_from(run.found.len()).unwrap_or(i32::MAX),
        );
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
    /// baseunits/uUpdateThread.pas:441-449.
    fn start_phase(&mut self, phase: ListPhase, limit: i32) {
        self.phase = phase;
        self.limit = limit;
        self.handed_out = 0;
        self.finished = false;
    }

    fn found_title(&self, i: i32) -> (String, String) {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.found.get(i))
            .cloned()
            .unwrap_or_default()
    }

    /// baseunits/uUpdateThread.pas:236-264.
    fn add_links(&mut self, names: &[String], links: &[String], pre_list: bool) {
        // `RemoveHostFromURLsPair` only strips hosts, and drops pairs whose link becomes
        // empty, when both lists are the same length (baseunits/uBaseUnit.pas:983-1000).
        let paired = names.len() == links.len();
        // `mainDataProcess.AddData` holds a page's new links until the `Rollback` after it, so
        // a link repeated within one page counts as listed the second time (:244-252).
        let mut on_page: HashSet<String> = HashSet::new();
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
            if pre_list && (self.known.contains(&link) || !on_page.insert(link.clone())) {
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

/// One `CheckOut` (baseunits/uUpdateThread.pas:441-449; `GetNext`, :842-903), handling results
/// in the order handed out. Stops handing out once terminated or `run.finished`. Handing out may
/// lag FMD2's behind a slow page, but what is fetched and stored is the same.
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
