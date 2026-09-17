//! `ctrim` command line entry point.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{ArgAction, Parser, ValueEnum};

use ctrim::pipeline::{self, Outcome};
use ctrim::utils::clipboard;
use ctrim::{Config, OutputFormat, Preset};

/// Strip noise from terminal output before it reaches an LLM.
#[derive(Debug, Parser)]
#[command(
    name = "ctrim",
    version,
    about,
    long_about = None,
    after_help = "EXAMPLES:\n  git diff | ctrim --clip\n  pytest 2>&1 | ctrim --format xml\n  docker logs app | ctrim --dedupe-only\n  ctrim build.log --out reduced.md"
)]
struct Cli {
    /// Input file. Reads STDIN when omitted.
    file: Option<PathBuf>,

    /// Output layout.
    #[arg(short, long, value_enum, default_value_t = FormatArg::Markdown)]
    format: FormatArg,

    /// Parsing preset; `auto` detects the format from the input.
    #[arg(short, long, value_enum, default_value_t = PresetArg::Auto)]
    preset: PresetArg,

    /// Copy the result to the system clipboard.
    #[arg(short, long, action = ArgAction::SetTrue)]
    clip: bool,

    /// Write the result to a file instead of STDOUT.
    #[arg(short, long)]
    out: Option<PathBuf>,

    /// Vendor stack frames kept per folded run.
    #[arg(short, long, default_value_t = 3)]
    keep_frames: usize,

    /// Unified-diff context lines kept either side of a change.
    #[arg(long, default_value_t = 1)]
    context_lines: usize,

    /// Lines the repeated-line detector remembers.
    #[arg(long, default_value_t = 64)]
    dedupe_window: usize,

    /// Fold only byte-identical lines, never lines that differ by a number.
    #[arg(long, action = ArgAction::SetTrue)]
    exact_dupes: bool,

    /// Run only the repeated-line compressor.
    #[arg(long, action = ArgAction::SetTrue)]
    dedupe_only: bool,

    /// Keep ANSI colour escape sequences.
    #[arg(long, action = ArgAction::SetTrue)]
    preserve_ansi: bool,

    /// Suppress the reduction summary on STDERR.
    #[arg(short, long, action = ArgAction::SetTrue)]
    quiet: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FormatArg {
    Markdown,
    Xml,
    Raw,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PresetArg {
    Auto,
    Pytest,
    Cargo,
    Diff,
    Docker,
    Json,
    Generic,
}

impl From<FormatArg> for OutputFormat {
    fn from(value: FormatArg) -> Self {
        match value {
            FormatArg::Markdown => OutputFormat::Markdown,
            FormatArg::Xml => OutputFormat::Xml,
            FormatArg::Raw => OutputFormat::Raw,
        }
    }
}

impl From<PresetArg> for Preset {
    fn from(value: PresetArg) -> Self {
        match value {
            PresetArg::Auto => Preset::Auto,
            PresetArg::Pytest => Preset::Pytest,
            PresetArg::Cargo => Preset::Cargo,
            PresetArg::Diff => Preset::Diff,
            PresetArg::Docker => Preset::Docker,
            PresetArg::Json => Preset::Json,
            PresetArg::Generic => Preset::Generic,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("ctrim: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let config = Config {
        preset: cli.preset.into(),
        format: cli.format.into(),
        keep_frames: cli.keep_frames,
        preserve_ansi: cli.preserve_ansi,
        dedupe_only: cli.dedupe_only,
        context_lines: cli.context_lines,
        dedupe_window: cli.dedupe_window,
        fuzzy_dedupe: !cli.exact_dupes,
    };

    if cli.file.is_none() && io::stdin().is_terminal() {
        anyhow::bail!("no input: pipe a command into ctrim or pass a file (see --help)");
    }

    let input: Box<dyn Read> = match &cli.file {
        Some(path) => {
            Box::new(File::open(path).with_context(|| format!("cannot read {}", path.display()))?)
        }
        None => Box::new(io::stdin()),
    };
    let input = BufReader::with_capacity(64 * 1024, input);

    // The clipboard needs the whole result, so buffer only in that case.
    let needs_buffer = cli.clip || cli.out.is_some();
    let (outcome, buffered) = if needs_buffer {
        let mut buffer: Vec<u8> = Vec::new();
        let outcome = pipeline::run(input, &mut buffer, &config)?;
        (outcome, Some(buffer))
    } else {
        let stdout = io::stdout().lock();
        let outcome = pipeline::run(input, BufWriter::with_capacity(64 * 1024, stdout), &config)?;
        (outcome, None)
    };

    let mut copied = false;
    if let Some(buffer) = &buffered {
        let text = String::from_utf8_lossy(buffer);
        if let Some(path) = &cli.out {
            std::fs::write(path, buffer)
                .with_context(|| format!("cannot write {}", path.display()))?;
        }
        if cli.clip {
            match clipboard::copy(&text) {
                Ok(()) => copied = true,
                Err(err) => {
                    eprintln!("ctrim: clipboard unavailable ({err:#}); wrote STDOUT instead")
                }
            }
        }
        if cli.out.is_none() && !copied {
            let mut stdout = io::stdout().lock();
            stdout.write_all(buffer)?;
            stdout.flush()?;
        }
    }

    if !cli.quiet {
        report(&outcome, copied);
    }
    Ok(())
}

fn report(outcome: &Outcome, copied: bool) {
    eprintln!("{}", outcome.stats.report(outcome.detected_type, copied));
}
