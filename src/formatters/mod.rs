//! Output rendering.
//!
//! Formatters are streaming: [`Formatter::begin`] runs once the input format is
//! known, [`Formatter::line`] per emitted line, [`Formatter::end`] after the
//! chain flushes, which is when the error summary is available.

pub mod markdown;
pub mod raw;
pub mod xml;

use std::io::{self, Write};

use crate::detector::FormatType;
use crate::OutputFormat;

/// What a formatter knows about the run.
pub struct RenderContext {
    pub format: FormatType,
    pub error_summary: Vec<String>,
}

pub trait Formatter {
    fn begin(&mut self, w: &mut dyn Write, ctx: &RenderContext) -> io::Result<()> {
        let _ = (w, ctx);
        Ok(())
    }

    fn line(&mut self, w: &mut dyn Write, line: &str) -> io::Result<()>;

    fn end(&mut self, w: &mut dyn Write, ctx: &RenderContext) -> io::Result<()> {
        let _ = (w, ctx);
        Ok(())
    }
}

/// Build the formatter for an [`OutputFormat`].
pub fn build(format: OutputFormat) -> Box<dyn Formatter> {
    match format {
        OutputFormat::Markdown => Box::new(markdown::MarkdownFormatter),
        OutputFormat::Xml => Box::new(xml::XmlFormatter),
        OutputFormat::Raw => Box::new(raw::RawFormatter),
    }
}
