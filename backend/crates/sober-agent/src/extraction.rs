//! Parses and strips `<memory_extractions>` blocks from assistant responses.
//!
//! The LLM is instructed to append a structured extraction block at the end of
//! its response. This module extracts those, stores them in memory, and returns
//! the cleaned response text.

use sober_memory::bcf::ChunkType;

/// A single memory extraction parsed from the LLM response.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MemoryExtraction {
    /// Concise content to store.
    pub content: String,
    /// Chunk type as a string: "fact", "preference", "skill", "code".
    #[serde(rename = "type")]
    pub chunk_type: String,
}

/// Result of stripping extractions from a response.
#[derive(Debug)]
pub struct ExtractionResult {
    /// The response text with the extraction block removed.
    pub cleaned_text: String,
    /// Parsed extractions (may be empty).
    pub extractions: Vec<MemoryExtraction>,
}

/// Parses a chunk type string into a [`ChunkType`], returning `None` for
/// unknown types.
pub fn parse_extraction_type(s: &str) -> Option<ChunkType> {
    match s {
        "fact" => Some(ChunkType::Fact),
        "preference" => Some(ChunkType::Preference),
        "skill" => Some(ChunkType::Skill),
        "code" => Some(ChunkType::Code),
        "conversation" => Some(ChunkType::Conversation),
        "soul" => Some(ChunkType::Soul),
        _ => None,
    }
}

/// Returns the default importance for an extraction by type.
pub fn extraction_importance(ct: ChunkType) -> f64 {
    match ct {
        ChunkType::Soul => 0.9,
        ChunkType::Preference => 0.8,
        ChunkType::Fact | ChunkType::Skill => 0.7,
        ChunkType::Code => 0.6,
        ChunkType::Conversation => 0.5,
        ChunkType::Embedding => 0.5,
    }
}

/// Strips `<memory_extractions>...</memory_extractions>` from the response
/// and parses the JSON content.
///
/// Returns the cleaned text and any successfully parsed extractions.
/// Malformed JSON or missing blocks are silently ignored.
pub fn strip_extractions(response: &str) -> ExtractionResult {
    let Some(start) = response.find("<memory_extractions>") else {
        return ExtractionResult {
            cleaned_text: response.to_owned(),
            extractions: Vec::new(),
        };
    };

    let Some(end) = response.find("</memory_extractions>") else {
        return ExtractionResult {
            cleaned_text: response.to_owned(),
            extractions: Vec::new(),
        };
    };

    let tag_len = "<memory_extractions>".len();
    let json_str = response[start + tag_len..end].trim();

    let extractions: Vec<MemoryExtraction> = serde_json::from_str(json_str).unwrap_or_default();

    // Remove the block and any trailing whitespace
    let mut cleaned = response[..start].trim_end().to_owned();
    let after = response[end + "</memory_extractions>".len()..].trim_start();
    if !after.is_empty() {
        cleaned.push_str("\n\n");
        cleaned.push_str(after);
    }

    ExtractionResult {
        cleaned_text: cleaned,
        extractions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_extraction_block() {
        let result = strip_extractions("Hello, how can I help?");
        assert_eq!(result.cleaned_text, "Hello, how can I help?");
        assert!(result.extractions.is_empty());
    }

    #[test]
    fn valid_extraction_block() {
        let response = r#"Here is my response.

<memory_extractions>
[{"content": "User prefers dark mode", "type": "preference"}, {"content": "User works at Acme Corp", "type": "fact"}]
</memory_extractions>"#;

        let result = strip_extractions(response);
        assert_eq!(result.cleaned_text, "Here is my response.");
        assert_eq!(result.extractions.len(), 2);
        assert_eq!(result.extractions[0].content, "User prefers dark mode");
        assert_eq!(result.extractions[0].chunk_type, "preference");
        assert_eq!(result.extractions[1].content, "User works at Acme Corp");
        assert_eq!(result.extractions[1].chunk_type, "fact");
    }

    #[test]
    fn malformed_json_ignored() {
        let response =
            "Response text.\n\n<memory_extractions>\nnot valid json\n</memory_extractions>";
        let result = strip_extractions(response);
        assert_eq!(result.cleaned_text, "Response text.");
        assert!(result.extractions.is_empty());
    }

    #[test]
    fn empty_extraction_array() {
        let response = "Response.\n\n<memory_extractions>\n[]\n</memory_extractions>";
        let result = strip_extractions(response);
        assert_eq!(result.cleaned_text, "Response.");
        assert!(result.extractions.is_empty());
    }

    #[test]
    fn content_after_block_preserved() {
        let response = "Before.\n\n<memory_extractions>\n[{\"content\": \"fact\", \"type\": \"fact\"}]\n</memory_extractions>\n\nAfter.";
        let result = strip_extractions(response);
        assert_eq!(result.cleaned_text, "Before.\n\nAfter.");
        assert_eq!(result.extractions.len(), 1);
    }

    #[test]
    fn parse_extraction_types() {
        assert_eq!(parse_extraction_type("fact"), Some(ChunkType::Fact));
        assert_eq!(
            parse_extraction_type("preference"),
            Some(ChunkType::Preference)
        );
        assert_eq!(parse_extraction_type("skill"), Some(ChunkType::Skill));
        assert_eq!(parse_extraction_type("code"), Some(ChunkType::Code));
        assert_eq!(parse_extraction_type("unknown"), None);
    }

    #[test]
    fn importance_values() {
        assert!((extraction_importance(ChunkType::Preference) - 0.8).abs() < f64::EPSILON);
        assert!((extraction_importance(ChunkType::Fact) - 0.7).abs() < f64::EPSILON);
    }
}
