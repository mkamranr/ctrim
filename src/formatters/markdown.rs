//! Fenced Markdown block plus an error summary when one was found.

use std::io::{self, Write};

use super::{Formatter, RenderContext};

pub struct MarkdownFormatter;

impl Formatter for MarkdownFormatter {
    fn begin(&mut self, w: &mut dyn Write, ctx: &RenderContext) -> io::Result<()> {
        writeln!(w, "```{}", ctx.format.fence_language())
    }

    fn line(&mut self, w: &mut dyn Write, line: &str) -> io::Result<()> {
        writeln!(w, "{line}")
    }

    fn end(&mut self, w: &mut dyn Write, ctx: &RenderContext) -> io::Result<()> {
        writeln!(w, "```")?;
        if !ctx.error_summary.is_empty() {
            writeln!(w, "\n**Errors**")?;
            for line in &ctx.error_summary {
                writeln!(w, "- {line}")?;
            }
        }
        Ok(())
    }
}
