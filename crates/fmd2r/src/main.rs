//! `fmd2r`: the FMD2r server binary plus developer CLI subcommands.

use anyhow::bail;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the REST API and the web UI.
    Serve,
    /// Exercise website modules from the command line.
    #[command(subcommand)]
    Module(ModuleCommand),
    /// Evaluate XPath expressions with the FMD2 XPath engine.
    #[command(subcommand)]
    Xpath(XpathCommand),
}

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

fn main() -> anyhow::Result<()> {
    let ticket = match Cli::parse().command {
        Command::Serve => "T21",
        Command::Module(ModuleCommand::Init | ModuleCommand::Info | ModuleCommand::Pages) => "T15",
        Command::Module(ModuleCommand::Download) => "T20",
        Command::Xpath(XpathCommand::Eval) => "T08",
    };
    bail!("not implemented yet ({ticket})")
}
