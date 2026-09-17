//! XML tags, the layout most models parse most reliably.

use std::io::{self, Write};

use super::{Formatter, RenderContext};

pub struct XmlFormatter;

impl Formatter for XmlFormatter {
    fn begin(&mut self, w: &mut dyn Write, ctx: &RenderContext) -> io::Result<()> {
        writeln!(w, "<context type=\"{}\">", ctx.format.label())
    }

    fn line(&mut self, w: &mut dyn Write, line: &str) -> io::Result<()> {
        writeln!(w, "{}", neutralize(line))
    }

    fn end(&mut self, w: &mut dyn Write, ctx: &RenderContext) -> io::Result<()> {
        writeln!(w, "</context>")?;
        if !ctx.error_summary.is_empty() {
            writeln!(w, "<error_summary>")?;
            for line in &ctx.error_summary {
                writeln!(w, "{}", neutralize(line))?;
            }
            writeln!(w, "</error_summary>")?;
        }
        Ok(())
    }
}

/// Content is not XML-escaped — escaping costs tokens and models read raw text
/// fine — but a literal closing tag would end the block early, so only those are
/// defused.
fn neutralize(line: &str) -> String {
    if line.contains("</context>") || line.contains("</error_summary>") {
        line.replace("</context>", "< /context>")
            .replace("</error_summary>", "< /error_summary>")
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_tags_in_content_are_defused() {
        assert_eq!(
            neutralize("log line </context> more"),
            "log line < /context> more"
        );
    }

    #[test]
    fn ordinary_content_is_not_escaped() {
        let line = "if (a < b && c > d) { }";
        assert_eq!(neutralize(line), line);
    }
}
