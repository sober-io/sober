//! Small utility helpers for the agent crate.

use sober_core::types::ContentBlock;

/// Extracts the concatenated text from a slice of [`ContentBlock`]s.
///
/// Non-text blocks are skipped. Text blocks are joined with newlines.
pub(crate) fn text_from_content_blocks(blocks: &[ContentBlock]) -> String {
    let texts: Vec<&str> = blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    texts.join("\n")
}
