//! `ctrim` — strip noise from terminal output before it reaches an LLM.
//!
//! The crate is a streaming, line-oriented pipeline:
//!
//! ```text
//! input -> detector -> processor chain -> formatter -> output
//! ```
//!
//! Each processor is a state machine over lines ([`processors::LineProcessor`]),
//! so memory stays bounded regardless of input size. [`process`] is the
//! whole-string convenience API for library users.

pub mod detector;
pub mod formatters;
pub mod pipeline;
pub mod processors;
pub mod token;
pub mod utils;

pub use detector::FormatType;
pub use processors::{LogProcessor, ProcessedOutput};
pub use utils::stats::ReductionStats;

/// Output layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Fenced code block with a short header.
    #[default]
    Markdown,
    /// XML tags that models parse reliably (`<context>`, `<error_summary>`).
    Xml,
    /// Processed lines, nothing added.
    Raw,
}

/// Which processor chain to run. `Auto` defers to [`detector`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preset {
    #[default]
    Auto,
    Pytest,
    Cargo,
    Diff,
    Docker,
    Json,
    Generic,
}

impl Preset {
    /// The format a non-`Auto` preset pins the pipeline to.
    pub fn as_format(self) -> Option<FormatType> {
        match self {
            Preset::Auto => None,
            Preset::Pytest => Some(FormatType::Pytest),
            Preset::Cargo => Some(FormatType::CargoCheck),
            Preset::Diff => Some(FormatType::GitDiff),
            Preset::Docker => Some(FormatType::DockerLog),
            Preset::Json => Some(FormatType::JsonLines),
            Preset::Generic => Some(FormatType::GenericLog),
        }
    }
}

/// Everything the pipeline needs to build its processor chain.
#[derive(Debug, Clone)]
pub struct Config {
    pub preset: Preset,
    pub format: OutputFormat,
    /// Vendor frames preserved per folded stack-trace run.
    pub keep_frames: usize,
    /// Keep ANSI escape sequences instead of stripping them.
    pub preserve_ansi: bool,
    /// Run only the repeated-line compressor.
    pub dedupe_only: bool,
    /// Unified-diff context lines kept either side of a change run.
    ///
    /// One line is enough for a model to locate a change, and `git diff`
    /// already ships three, so the default removes two thirds of the context a
    /// normal diff carries.
    pub context_lines: usize,
    /// How many recent lines the deduplicator remembers.
    pub dedupe_window: usize,
    /// Fold lines that differ only in their numbers and ids.
    pub fuzzy_dedupe: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            preset: Preset::Auto,
            format: OutputFormat::Markdown,
            keep_frames: 3,
            preserve_ansi: false,
            dedupe_only: false,
            context_lines: 1,
            dedupe_window: 64,
            fuzzy_dedupe: true,
        }
    }
}

/// Compress `input` in one shot. Convenience wrapper over [`pipeline`].
///
/// Prefer [`pipeline::run`] for large inputs — it never holds the whole log in
/// memory, which this function does by definition.
pub fn process(input: &str, config: &Config) -> ProcessedOutput {
    pipeline::run_str(input, config)
}
