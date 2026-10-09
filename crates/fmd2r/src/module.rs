//! `fmd2r module …`: exercise website modules from the command line.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
use fmd_http::{
    HttpClient, RecordingTransport, ReplayOptions, ReplayTransport, ReqwestTransport, Transport,
};
use fmd_lua::{
    Callback, InfoReply, JobError, LoadReport, MangaInfo, Module, ModuleDef, ModuleOption,
    ModuleRegistry, OptionKind, PoolConfig, Task, WorkerPool, XPathCorpusWriter,
};
use serde_json::{Value, json};

#[derive(Subcommand)]
pub enum ModuleCommand {
    /// Load the Lua website modules and report them.
    Init(InitArgs),
    /// Run `OnGetInfo` for a manga URL and print the result.
    Info(RunArgs),
    /// Run the page callbacks for a chapter URL and print the page links.
    Pages(RunArgs),
    /// Download a chapter.
    Download(DownloadArgs),
}

/// Where the modules come from.
#[derive(Args)]
pub struct LoadArgs {
    /// The directory holding FMD2's `lua/` tree (`modules/`, `utils/`, ...).
    #[arg(long, default_value = "lua")]
    lua_dir: PathBuf,
    /// Only the module with this ID.
    #[arg(long, value_name = "ID", conflicts_with = "file")]
    module: Option<String>,
    /// Load only this module file instead of every file in `<lua-dir>/modules`.
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,
}

impl LoadArgs {
    /// Loads the modules: every file in `<lua-dir>/modules`, or just `--file`.
    fn load(&self) -> LoadReport {
        match &self.file {
            Some(file) => ModuleRegistry::load_file(&self.lua_dir, file),
            None => ModuleRegistry::load_dir(&self.lua_dir),
        }
    }

    /// The loaded modules `--module` selects: the one with that ID, or all of them.
    fn selected(&self, report: &LoadReport) -> anyhow::Result<Vec<Arc<Module>>> {
        let modules = report.registry.modules();
        match &self.module {
            Some(id) => match report.registry.get(id) {
                Some(module) => Ok(vec![module.clone()]),
                None => bail!("no module with ID {id}"),
            },
            None => Ok(modules.to_vec()),
        }
    }
}

#[derive(Args)]
pub struct InitArgs {
    #[command(flatten)]
    load: LoadArgs,
    /// Print JSON instead of a table.
    #[arg(long)]
    json: bool,
}

/// What `info` and `pages` run a module's callbacks on.
#[derive(Args)]
pub struct RunArgs {
    /// The manga's URL for `info`, the chapter's for `pages`.
    url: String,
    #[command(flatten)]
    load: LoadArgs,
    #[command(flatten)]
    http: HttpArgs,
    /// Record every XPath evaluation into this differential corpus directory (see
    /// fixtures/xpath-corpus), adding to what is there.
    #[arg(long, value_name = "DIR")]
    xpath_corpus: Option<PathBuf>,
}

#[derive(Args)]
pub struct DownloadArgs {
    /// The chapter's URL.
    url: String,
    /// The directory to save the chapter in.
    #[arg(long, value_name = "DIR")]
    out: PathBuf,
}

/// Where HTTP requests go: the network, the network with recording, or recorded fixtures.
#[derive(Args)]
pub struct HttpArgs {
    /// Write every HTTP exchange into this fixture directory (see docs/fixtures.md).
    #[arg(long, value_name = "DIR", conflicts_with = "replay")]
    record: Option<PathBuf>,
    /// Serve HTTP from the fixtures recorded in this directory instead of the network; a request
    /// with no recorded exchange fails the command.
    #[arg(long, value_name = "DIR")]
    replay: Option<PathBuf>,
    /// With --replay, a request header whose value must match the recorded one too (method, URL
    /// and body always do). Repeatable.
    #[arg(long, value_name = "NAME", requires = "replay")]
    match_header: Vec<String>,
}

/// The HTTP client of a command, and the replay transport behind it, if any.
struct CommandHttp {
    client: HttpClient,
    replay: Option<Arc<ReplayTransport>>,
}

impl HttpArgs {
    fn client(&self) -> anyhow::Result<CommandHttp> {
        let mut replay = None;
        let transport: Arc<dyn Transport> = match (&self.record, &self.replay) {
            (Some(dir), _) => Arc::new(
                RecordingTransport::new(dir, Arc::new(ReqwestTransport::new()))
                    .context("starting the recording")?,
            ),
            (None, Some(dir)) => {
                let options = ReplayOptions {
                    match_headers: self.match_header.clone(),
                };
                let transport =
                    Arc::new(ReplayTransport::open(dir, options).context("loading the fixtures")?);
                replay = Some(transport.clone());
                transport
            }
            (None, None) => Arc::new(ReqwestTransport::new()),
        };
        let client = HttpClient::with_transport(transport).context("starting the HTTP client")?;
        Ok(CommandHttp { client, replay })
    }
}

impl CommandHttp {
    /// Fails, naming each request, when a replay met requests it had no exchange for.
    fn check_replay(&self) -> anyhow::Result<()> {
        let Some(replay) = &self.replay else {
            return Ok(());
        };
        let misses = replay.misses();
        if misses.is_empty() {
            return Ok(());
        }
        for miss in &misses {
            eprintln!("replay: no recorded exchange for {miss}");
        }
        bail!("replay: {} unrecorded request(s)", misses.len())
    }
}

pub fn run(command: ModuleCommand) -> anyhow::Result<()> {
    match command {
        ModuleCommand::Init(args) => init(args),
        ModuleCommand::Info(args) => info(args),
        ModuleCommand::Pages(args) => pages(args),
        ModuleCommand::Download(_) => bail!("not implemented yet (T20)"),
    }
}

fn init(args: InitArgs) -> anyhow::Result<()> {
    let report = args.load.load();
    let selected = match args.load.selected(&report) {
        Ok(selected) => selected,
        Err(e) => {
            print_failures(&report);
            return Err(e);
        }
    };
    let defs: Vec<ModuleDef> = selected.iter().map(|m| m.def()).collect();
    if args.json {
        let out = json!({
            "modules": defs.iter().map(module_json).collect::<Vec<_>>(),
            "failures": report.failures.iter().map(|f| json!({
                "file": f.file.display().to_string(),
                "error": f.error,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        print!("{}", table(&defs));
    }
    fail_on_load_errors(&report)
}

/// One module run: the module and link a URL resolves to, and the worker running it.
struct ModuleRun {
    target: Target,
    http: CommandHttp,
    pool: WorkerPool,
    xpath_corpus: Option<XPathCorpusWriter>,
}

impl ModuleRun {
    fn start(args: &RunArgs) -> anyhow::Result<ModuleRun> {
        let target = Target::resolve(&args.load, &args.url)?;
        let http = args.http.client()?;
        let xpath_corpus = args
            .xpath_corpus
            .as_ref()
            .map(|dir| {
                XPathCorpusWriter::open(dir)
                    .with_context(|| format!("opening the XPath corpus in {}", dir.display()))
            })
            .transpose()?;
        let pool = pool(&args.load, &http, xpath_corpus.clone())?;
        Ok(ModuleRun {
            target,
            http,
            pool,
            xpath_corpus,
        })
    }

    /// The callbacks' `result`, after failing on any request a replay had no exchange for, which
    /// is what usually made a callback fail.
    fn finish<T>(&self, result: Result<T, JobError>) -> anyhow::Result<T> {
        if let Some(corpus) = &self.xpath_corpus {
            corpus.finish().context("recording the XPath corpus")?;
        }
        self.http.check_replay()?;
        Ok(result?)
    }
}

/// `GetInfoFromURL` (baseunits/uData.pas:85-109) up to `OnGetInfo`: the info as the callback
/// left it, without FMD2's cleanup of the fields afterwards (:111-206).
fn info(args: RunArgs) -> anyhow::Result<()> {
    let run = ModuleRun::start(&args)?;
    let module = &run.target.module;
    let reply = if module.def().on_get_info.is_some() {
        let result = run.pool.on(module).get_info(&run.target.link).wait();
        run.finish(result)?.value
    } else {
        InfoReply {
            status: INFORMATION_NOT_FOUND,
            info: MangaInfo::default(),
        }
    };
    let info = reply.info;
    let out = json!({
        "module": module.def().id,
        "status": reply.status,
        "info": {
            "url": info.url,
            "title": info.title,
            "alt_titles": info.alt_titles,
            "link": info.link,
            "cover_link": info.cover_link,
            "authors": info.authors,
            "artists": info.artists,
            "genres": info.genres,
            "status": info.status,
            "summary": info.summary,
            "chapter_names": info.chapter_names,
            "chapter_links": info.chapter_links,
        },
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

/// `INFORMATION_NOT_FOUND` (baseunits/uBaseUnit.pas), the `information_not_found` of Lua.
const INFORMATION_NOT_FOUND: u8 = 2;

/// What an unresolved page link holds (baseunits/uDownloadsManager.pas:845-849, :1208-1211).
const UNRESOLVED_PAGE: &str = "W";

/// Prepares one chapter as FMD2's task thread does before downloading (`TTaskThread.Execute`,
/// baseunits/uDownloadsManager.pas:975-1250, the part at :1167-1246): `OnTaskStart`; then, when
/// it left no page links, `DoGetPageNumber` (:829-881); then, unless the module sets
/// `DynamicPageLink`, `OnGetImageURL` for every page still unresolved (`DoPageLink`, :421-433,
/// with `GetLinkPageFromURL`, :327-333). All run on one worker, in order, each with a fresh
/// `HTTP` session.
fn pages(args: RunArgs) -> anyhow::Result<()> {
    let run = ModuleRun::start(&args)?;
    let result = prepare_chapter(&run);
    let task = run.finish(result)?;
    let out = json!({
        "module": run.target.module.def().id,
        "page_number": task.page_number,
        "page_links": task.page_links,
        "page_container_links": task.page_container_links,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

/// The callbacks of [`pages`], returning the task as they left it.
fn prepare_chapter(run: &ModuleRun) -> Result<Task, JobError> {
    let def = run.target.module.def();
    let link = &run.target.link;
    let affinity = run.pool.affinity();
    let on_module = || run.pool.on(&run.target.module).with_affinity(affinity);
    let mut task = Task {
        chapter_links: vec![link.clone()],
        chapter_names: vec![String::new()],
        ..Task::default()
    };
    if def.on_task_start.is_some() {
        task = on_module().task_start(task).wait()?.value.task;
    }
    if task.page_links.is_empty() {
        task.page_number = 0;
        if def.on_get_page_number.is_some() {
            task = on_module().get_page_number(task, link).wait()?.value.task;
        }
        // `TrimStrings` (baseunits/uBaseUnit.pas:1396-1410): FPC's `Trim`, dropping empty items.
        task.page_links = task
            .page_links
            .iter()
            .map(|link| fpc_trim(link).to_owned())
            .filter(|link| !link.is_empty())
            .collect();
        let wanted = usize::try_from(task.page_number).unwrap_or(0);
        while task.page_links.len() < wanted {
            task.page_links.push(UNRESOLVED_PAGE.to_owned());
        }
    }
    if task.page_links.is_empty() {
        task.page_links.push(UNRESOLVED_PAGE.to_owned());
    }
    task.page_number = i32::try_from(task.page_links.len()).unwrap_or(i32::MAX);
    // `CheckForPrepare` (baseunits/uDownloadsManager.pas:980-1001): a page is unresolved or empty.
    let to_prepare = task
        .page_links
        .iter()
        .any(|l| l == UNRESOLVED_PAGE || l.is_empty());
    if !def.dynamic_page_link && to_prepare {
        if def.on_get_image_url.is_some() {
            // Like `InternalGetPageLinkWorkId` (baseunits/uDownloadsManager.pas:927-945), against
            // the page links as the previous callback left them.
            let mut work_id = 0;
            while let Some(page) = task.page_links.get(work_id) {
                if page == UNRESOLVED_PAGE {
                    let id = i32::try_from(work_id).unwrap_or(i32::MAX);
                    task = on_module().get_image_url(task, id, link).wait()?.value.task;
                }
                work_id += 1;
            }
        }
        // A page link left blank is unresolved again (baseunits/uDownloadsManager.pas:1236-1246).
        for page in &mut task.page_links {
            if fpc_trim(page).is_empty() {
                *page = UNRESOLVED_PAGE.to_owned();
            }
        }
    }
    Ok(task)
}

/// FPC's `Trim`: strips characters up to `' '` at both ends.
fn fpc_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c <= ' ')
}

/// The module a URL is run on, and the link (the URL's path) its callback gets.
struct Target {
    module: Arc<Module>,
    link: String,
}

impl Target {
    /// Like FMD2's add-by-URL box (`edURLButtonClick`, mangadownloader/forms/frmMain.pas:
    /// 6579-6607): `SplitURL` the URL into host and link, both required, then the module given
    /// by `--module` or else the one `LocateModuleByHost` finds for the host.
    fn resolve(load: &LoadArgs, url: &str) -> anyhow::Result<Target> {
        let report = load.load();
        print_failures(&report);
        let (host, link) = fmd_http::split_url_bytes(url.as_bytes());
        let host = String::from_utf8_lossy(&host);
        let link = String::from_utf8_lossy(&link).into_owned();
        if host.is_empty() || link.is_empty() {
            bail!("URL not supported: {url}");
        }
        let module = match &load.module {
            Some(_) => load.selected(&report)?.into_iter().next(),
            None => report.registry.locate_by_host(&host).cloned(),
        };
        match module {
            Some(module) => Ok(Target { module, link }),
            None => bail!("no module for host {host}"),
        }
    }
}

/// A one-thread worker pool over `load`'s `lua/` dir, sending HTTP through `http` and recording
/// XPath into `xpath_corpus`, if given.
fn pool(
    load: &LoadArgs,
    http: &CommandHttp,
    xpath_corpus: Option<XPathCorpusWriter>,
) -> anyhow::Result<WorkerPool> {
    let mut config = PoolConfig::new(http.client.clone());
    config.threads = 1;
    config.lua_dir = load.lua_dir.clone();
    config.xpath_corpus = xpath_corpus;
    WorkerPool::new(config).context("starting the Lua worker")
}

/// Lists every module file that failed to load on stderr.
fn print_failures(report: &LoadReport) {
    for failure in &report.failures {
        eprintln!("{}: {}", failure.file.display(), failure.error);
    }
}

/// Fails, listing every failure on stderr, when a module file did not load.
fn fail_on_load_errors(report: &LoadReport) -> anyhow::Result<()> {
    if report.failures.is_empty() {
        return Ok(());
    }
    print_failures(report);
    bail!(
        "{} of {} module files failed to load",
        report.failures.len(),
        report.files
    )
}

/// The callbacks `def` declares, in declaration order.
fn callbacks(def: &ModuleDef) -> Vec<String> {
    Callback::ALL
        .iter()
        .filter(|c| c.function(def).is_some())
        .map(ToString::to_string)
        .collect()
}

fn module_json(def: &ModuleDef) -> Value {
    json!({
        "id": def.id,
        "name": def.name,
        "root_url": def.root_url,
        "callbacks": callbacks(def),
        "options": def.options.iter().map(option_json).collect::<Vec<_>>(),
    })
}

fn option_json(option: &ModuleOption) -> Value {
    let (kind, default) = match &option.kind {
        OptionKind::CheckBox { default } => ("checkbox", json!(default)),
        OptionKind::Edit { default } => ("edit", json!(default)),
        OptionKind::SpinEdit { default } => ("spinedit", json!(default)),
        OptionKind::ComboBox { items, default } => {
            return json!({
                "name": option.name,
                "caption": option.caption,
                "kind": "combobox",
                "items": items,
                "default": default,
            });
        }
    };
    json!({
        "name": option.name,
        "caption": option.caption,
        "kind": kind,
        "default": default,
    })
}

/// The modules as a text table, one row per module.
fn table(defs: &[ModuleDef]) -> String {
    let mut rows = vec![[
        "ID".to_owned(),
        "NAME".to_owned(),
        "ROOT URL".to_owned(),
        "CALLBACKS".to_owned(),
        "OPTIONS".to_owned(),
    ]];
    for def in defs {
        let options: Vec<&str> = def.options.iter().map(|o| o.name.as_str()).collect();
        rows.push([
            def.id.clone(),
            def.name.clone(),
            def.root_url.clone(),
            callbacks(def).join(","),
            options.join(","),
        ]);
    }
    let mut widths = [0; 5];
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for row in &rows {
        let cells: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:width$}"))
            .collect();
        out.push_str(cells.join("  ").trim_end());
        out.push('\n');
    }
    out
}
