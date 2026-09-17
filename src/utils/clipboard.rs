//! Cross-platform clipboard writing.

use anyhow::{Context, Result};

/// Copy `text` to the system clipboard.
///
/// Fails rather than panics where no clipboard exists (headless Linux, CI, a
/// container) so the caller can degrade to STDOUT.
pub fn copy(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("no system clipboard available")?;
    clipboard
        .set_text(text.to_owned())
        .context("failed to write to the system clipboard")
}
