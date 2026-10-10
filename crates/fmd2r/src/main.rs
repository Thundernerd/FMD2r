//! `fmd2r`: the FMD2r server binary plus developer CLI subcommands.

mod module;
mod xpath;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use fmd_import::{ImportOptions, PathMap, TimeZone};
use fmd_server::{EventBus, LogBuffer, ServeConfig};
use fmd_store::{ACCOUNTS_KEY_FILE, AppDb, KeyFileCipher};
use module::ModuleCommand;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use xpath::XpathCommand;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the REST API and the web UI.
    Serve(ServeArgs),
    /// Print the REST API's OpenAPI document (the web client is generated from it).
    Openapi {
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Import FMD2's userdata (queue, favorites, downloaded chapters, module settings, accounts,
    /// settings). Run it while the server is stopped.
    Import(ImportArgs),
    /// Exercise website modules from the command line.
    #[command(subcommand)]
    Module(ModuleCommand),
    /// Evaluate XPath expressions, and compare the XPath backends on the differential corpus.
    #[command(subcommand)]
    Xpath(XpathCommand),
}

#[derive(Args)]
struct ServeArgs {
    /// Address to listen on; overrides the `server.bind` setting (127.0.0.1:8080 by default).
    #[arg(long, env = "FMD2R_BIND")]
    bind: Option<SocketAddr>,
    /// Directory holding the databases.
    #[arg(long, env = "FMD2R_DATA_DIR", default_value = "data")]
    data_dir: PathBuf,
    /// Password/token required for the API; overrides `server.auth_token`. Without either, the
    /// API is open.
    #[arg(long, env = "FMD2R_PASSWORD", hide_env_values = true)]
    password: Option<String>,
    /// FlareSolverr URL (e.g. http://flaresolverr:8191) for Cloudflare-protected sites; overrides
    /// `connections.flaresolverr_url`. Empty turns FlareSolverr off.
    #[arg(long, env = "FMD2R_FLARESOLVERR_URL")]
    flaresolverr_url: Option<String>,
    /// Don't sync Lua modules with upstream; those already in `<data dir>/lua` still load.
    #[arg(long, env = "FMD2R_NO_MODULE_UPDATES")]
    no_module_updates: bool,
}

#[derive(Args)]
struct ImportArgs {
    /// FMD2's `userdata` directory.
    #[arg(long)]
    from: PathBuf,
    /// Directory holding the databases.
    #[arg(long, default_value = "data")]
    data_dir: PathBuf,
    /// Report what would be imported without writing anything.
    #[arg(long)]
    dry_run: bool,
    /// Rewrite save-to paths under FROM to TO, e.g. 'C:\Manga=/data/manga'. Repeatable; longest
    /// FROM wins.
    #[arg(long, value_name = "FROM=TO")]
    map_path: Vec<PathMap>,
    /// Queue tasks FMD2 was running as waiting, so they resume, instead of stopped.
    #[arg(long)]
    resume: bool,
    /// IANA time zone FMD2 ran in (it stores zone-less local times), e.g. 'Europe/Amsterdam'.
    /// Defaults to this machine's.
    #[arg(long, value_name = "ZONE")]
    timezone: Option<TimeZone>,
}

/// Log lines kept for `GET /api/logs`.
const LOG_LINES: usize = 2000;

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Serve(args) => serve(args),
        Command::Openapi { out } => openapi(out),
        Command::Import(args) => import(args),
        Command::Module(command) => module::run(command),
        Command::Xpath(command) => xpath::run(command),
    }
}

fn serve(args: ServeArgs) -> anyhow::Result<()> {
    let logs = LogBuffer::new(LOG_LINES, EventBus::new());
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(logs.clone())
        .try_init()
        .context("installing the log subscriber")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    runtime.block_on(fmd_server::serve(ServeConfig {
        bind: args.bind,
        data_dir: args.data_dir,
        auth: args.password.filter(|p| !p.is_empty()),
        flaresolverr_url: args.flaresolverr_url,
        logs,
        module_updates: !args.no_module_updates,
    }))?;
    Ok(())
}

fn openapi(out: Option<PathBuf>) -> anyhow::Result<()> {
    let json = fmd_server::openapi_json().context("serializing the OpenAPI document")? + "\n";
    match out {
        Some(path) => {
            std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?
        }
        None => print!("{json}"),
    }
    Ok(())
}

fn import(args: ImportArgs) -> anyhow::Result<()> {
    std::fs::create_dir_all(&args.data_dir)
        .with_context(|| format!("creating {}", args.data_dir.display()))?;
    let db = AppDb::open(args.data_dir.join("app.db")).context("opening app.db")?;
    let cipher = KeyFileCipher::open_or_create(args.data_dir.join(ACCOUNTS_KEY_FILE))
        .context("opening the accounts key")?;
    let opts = ImportOptions {
        dry_run: args.dry_run,
        resume_in_progress: args.resume,
        path_maps: args.map_path,
        timezone: args.timezone.unwrap_or_default(),
    };
    let report = fmd_import::import(&args.from, &db, &cipher, &opts)
        .with_context(|| format!("importing {}", args.from.display()))?;
    print!("{report}");
    Ok(())
}
