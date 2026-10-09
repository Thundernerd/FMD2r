//! `fmd-smoke`: record the smoke list's fixtures, run it live or replayed, and write the nightly
//! report. Run it from the repository root after `cargo build -p fmd2r`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use fmd_smoke::{Results, Smoke, SmokeError, render_report};

#[derive(Parser)]
#[command(about)]
struct Cli {
    #[command(flatten)]
    paths: Paths,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Paths {
    /// The `fmd2r` binary.
    #[arg(long, global = true, default_value = "target/debug/fmd2r")]
    fmd2r: PathBuf,
    /// The `lua/` tree the modules load from.
    #[arg(long, global = true, default_value = "fixtures/lua")]
    lua_dir: PathBuf,
    /// The smoke list's directory (`list.toml` and the recorded entries).
    #[arg(long, global = true, default_value = "fixtures/smoke")]
    dir: PathBuf,
    /// Append every XPath evaluation to this differential corpus directory
    /// (fixtures/xpath-corpus).
    #[arg(long, global = true, value_name = "DIR")]
    xpath_corpus: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Record entries from the live sites, replacing their fixtures and snapshots.
    Record {
        /// The entries' names in `list.toml`.
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Run every entry (or the named ones) and write the results as JSON.
    Run {
        /// Run against the live sites instead of the recorded fixtures.
        #[arg(long)]
        live: bool,
        /// Only these entries.
        names: Vec<String>,
        /// The results file; stdout when not given.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Classify live and replay results as Markdown: a failed replay is an FMD2r regression, a
    /// live-only failure a site or module change.
    Report {
        #[arg(long)]
        live: PathBuf,
        #[arg(long)]
        replay: PathBuf,
        /// The report file; stdout when not given.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let smoke = Smoke {
        fmd2r: cli.paths.fmd2r,
        lua_dir: cli.paths.lua_dir,
        dir: cli.paths.dir,
        xpath_corpus: cli.paths.xpath_corpus,
    };
    match run(&smoke, cli.command) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(smoke: &Smoke, command: Command) -> Result<ExitCode, SmokeError> {
    match command {
        Command::Record { names } => {
            let mut failed = false;
            for name in names {
                let entry = smoke.entry(&name)?;
                eprintln!("recording {name}");
                if let Err(e) = smoke.record(&entry) {
                    eprintln!("error: {e}");
                    failed = true;
                }
            }
            Ok(if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::Run { live, names, out } => {
            let entries = smoke
                .list()?
                .entries
                .into_iter()
                .filter(|e| names.is_empty() || names.contains(&e.name));
            let mut results = Results::default();
            for entry in entries {
                eprintln!(
                    "{} {}",
                    if live { "running" } else { "replaying" },
                    entry.name
                );
                results.entries.push(if live {
                    smoke.live(&entry)
                } else {
                    smoke.replay(&entry)
                });
            }
            let json =
                serde_json::to_string_pretty(&results).map_err(|source| SmokeError::JsonWrite {
                    what: "the results",
                    source,
                })? + "\n";
            write(out.as_deref(), &json)?;
            // Failures are results, not errors: the report classifies them.
            Ok(ExitCode::SUCCESS)
        }
        Command::Report { live, replay, out } => {
            let report = render_report(&read_results(&live)?, &read_results(&replay)?);
            write(out.as_deref(), &report)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn read_results(path: &Path) -> Result<Results, SmokeError> {
    let text = std::fs::read_to_string(path).map_err(|source| SmokeError::Io {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| SmokeError::JsonParse {
        path: path.to_owned(),
        source,
    })
}

fn write(path: Option<&Path>, text: &str) -> Result<(), SmokeError> {
    match path {
        Some(path) => std::fs::write(path, text).map_err(|source| SmokeError::Io {
            path: path.to_owned(),
            source,
        }),
        None => {
            print!("{text}");
            Ok(())
        }
    }
}
