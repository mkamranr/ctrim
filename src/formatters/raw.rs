//! Processed lines, nothing added.

use std::io::{self, Write};

use super::Formatter;

pub struct RawFormatter;

impl Formatter for RawFormatter {
    fn line(&mut self, w: &mut dyn Write, line: &str) -> io::Result<()> {
        writeln!(w, "{line}")
    }
}
