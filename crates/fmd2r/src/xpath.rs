//! `fmd2r xpath …`: evaluate XPath on either backend, and compare the backends on the
//! differential corpus (docs/tickets/T35-xpath-differential-corpus.md).

use std::path::PathBuf;

use anyhow::{Context, anyhow, bail};
use clap::{Args, Subcommand, ValueEnum};
use fmd_xpath::corpus::Corpus;
use fmd_xpath::{Backend, Normalized, XPathEngine, diff};

#[derive(Subcommand)]
pub enum XpathCommand {
    /// Evaluate an expression against an HTML document and print the result as the differential
    /// runner compares it.
    Eval(EvalArgs),
    /// Evaluate every entry of the differential corpus on both backends and report where they
    /// differ. Exits with an error on any mismatch.
    Diff(DiffArgs),
}

#[derive(Args)]
pub struct EvalArgs {
    /// The backend that evaluates the expression.
    #[arg(long, value_enum, default_value_t = BackendArg::Native)]
    backend: BackendArg,
    /// The expression is a CSS selector.
    #[arg(long)]
    css: bool,
    /// The HTML document.
    file: PathBuf,
    /// The XPath expression (or CSS selector, with --css).
    expr: String,
}

#[derive(Args)]
pub struct DiffArgs {
    /// The corpus directory.
    #[arg(long, default_value = "fixtures/xpath-corpus")]
    corpus: PathBuf,
    /// Write the Markdown report to this file instead of stdout.
    #[arg(long)]
    out: Option<PathBuf>,
}

/// The `xpath.backend` setting's values.
#[derive(Clone, Copy, ValueEnum)]
enum BackendArg {
    Fpc,
    Native,
}

impl BackendArg {
    fn engine(self) -> anyhow::Result<std::rc::Rc<dyn XPathEngine>> {
        let backend = match self {
            BackendArg::Fpc => Backend::Fpc,
            BackendArg::Native => Backend::Native,
        };
        backend.engine().ok_or_else(|| {
            anyhow!("the {backend:?} XPath backend is not built in (build fmd2r with --features xpath-fpc)")
        })
    }
}

pub fn run(command: XpathCommand) -> anyhow::Result<()> {
    match command {
        XpathCommand::Eval(args) => eval(args),
        XpathCommand::Diff(args) => diff_corpus(args),
    }
}

fn eval(args: EvalArgs) -> anyhow::Result<()> {
    let engine = args.backend.engine()?;
    let html =
        std::fs::read(&args.file).with_context(|| format!("reading {}", args.file.display()))?;
    let doc = engine.parse(&html)?;
    let value = doc.eval(&args.expr, None, args.css);
    print!("{}", Normalized::of(value.as_ref()));
    Ok(())
}

fn diff_corpus(args: DiffArgs) -> anyhow::Result<()> {
    let reference = BackendArg::Fpc.engine()?;
    let candidate = BackendArg::Native.engine()?;
    let corpus = Corpus::load(&args.corpus)
        .with_context(|| format!("loading the corpus in {}", args.corpus.display()))?;
    let report = diff(&corpus, reference.as_ref(), candidate.as_ref());
    let rendered = report.render();
    match &args.out {
        Some(path) => std::fs::write(path, &rendered)
            .with_context(|| format!("writing {}", path.display()))?,
        None => print!("{rendered}"),
    }
    if !report.mismatches.is_empty() {
        bail!(
            "{} of {} corpus entries differ between the fpc and native backends",
            report.mismatches.len(),
            report.entries
        );
    }
    eprintln!("{} corpus entries, no mismatches", report.entries);
    Ok(())
}
