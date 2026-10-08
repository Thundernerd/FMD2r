//! `fmd2r`: the FMD2r server binary plus developer CLI subcommands.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use fmd_import::{ImportOptions, PathMap};
use fmd_server::{EventBus, LogBuffer, ServeConfig};
use fmd_store::{AppDb, KeyFileCipher};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

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
    /// Print the OpenAPI document of the REST API (the web client is generated from it).
    Openapi {
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Import an FMD2 installation's userdata (queue, favorites, downloaded chapters, module
    /// settings and accounts, settings). Run it while the server is stopped.
    Import(ImportArgs),
    /// Exercise website modules from the command line.
    #[command(subcommand)]
    Module(ModuleCommand),
    /// Evaluate XPath expressions with the FMD2 XPath engine.
    #[command(subcommand)]
    Xpath(XpathCommand),
}

#[derive(Args)]
struct ServeArgs {
    /// Address to listen on.
    #[arg(long, default_value = "127.0.0.1:8080")]
    bind: SocketAddr,
    /// Directory holding the databases.
    #[arg(long, default_value = "data")]
    data_dir: PathBuf,
    /// Password/token required for the API; leave unset for no auth.
    #[arg(long, env = "FMD2R_PASSWORD", hide_env_values = true)]
    password: Option<String>,
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
    /// Rewrite save-to paths under FROM to TO, e.g. 'C:\Manga=/data/manga'. Repeatable; the
    /// longest matching FROM wins.
    #[arg(long, value_name = "FROM=TO")]
    map_path: Vec<PathMap>,
    /// Queue tasks FMD2 was running as waiting, so they resume, instead of stopped.
    #[arg(long)]
    resume: bool,
}

/// The key file account credentials are encrypted with, in the data directory.
const ACCOUNTS_KEY_FILE: &str = "accounts.key";

#[derive(Subcommand)]
enum ModuleCommand {
    /// Load the Lua website modules and report them.
    Init,
    /// Run `OnGetInfo` for a manga URL and print the result.
    Info,
    /// Run the page callbacks for a chapter URL and print the page links.
    Pages,
    /// Download a chapter.
    Download,
}

#[derive(Subcommand)]
enum XpathCommand {
    /// Evaluate an expression against an HTML document.
    Eval,
}

/// Log lines kept for `GET /api/logs`.
const LOG_LINES: usize = 2000;

fn main() -> anyhow::Result<()> {
    let ticket = match Cli::parse().command {
        Command::Serve(args) => return serve(args),
        Command::Openapi { out } => return openapi(out),
        Command::Import(args) => return import(args),
        Command::Module(ModuleCommand::Init | ModuleCommand::Info | ModuleCommand::Pages) => "T15",
        Command::Module(ModuleCommand::Download) => "T20",
        Command::Xpath(XpathCommand::Eval) => "T35",
    };
    bail!("not implemented yet ({ticket})")
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
        logs,
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
    };
    let report = fmd_import::import(&args.from, &db, &cipher, &opts)
        .with_context(|| format!("importing {}", args.from.display()))?;
    print!("{report}");
    Ok(())
}
