//! BCF (Binary Context Format) type definitions.

use crate::error::MemoryError;

/// Magic bytes identifying a BCF file: `SÕBE` (0x53 0xD5 0x42 0x45).
pub const BCF_MAGIC: [u8; 4] = [0x53, 0xD5, 0x42, 0x45];

/// Current BCF format version.
pub const BCF_VERSION_V1: u16 = 1;

/// BCF header size in bytes.
pub const HEADER_SIZE: usize = 28;

/// Size of a single chunk table entry in bytes.
pub const CHUNK_TABLE_ENTRY_SIZE: usize = 13;

/// Memory chunk type discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ChunkType {
    /// Extracted knowledge fact.
    Fact = 0,
    /// User preference or personal setting.
    Preference = 1,
    /// Decision or choice made, with rationale.
    Decision = 2,
    /// Soul layer data (internal, used by sober-mind).
    Soul = 3,
}

impl std::fmt::Display for ChunkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Fact => "fact",
            Self::Preference => "preference",
            Self::Decision => "decision",
            Self::Soul => "soul",
        };
        f.write_str(s)
    }
}

impl TryFrom<u8> for ChunkType {
    type Error = MemoryError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Fact),
            1 => Ok(Self::Preference),
            2 => Ok(Self::Decision),
            3 => Ok(Self::Soul),
            other => Err(MemoryError::InvalidChunkType(other)),
        }
    }
}

impl From<ChunkType> for u8 {
    fn from(ct: ChunkType) -> Self {
        ct as u8
    }
}

/// Parsed BCF file header (28 bytes on disk).
#[derive(Debug, Clone)]
pub struct BcfHeader {
    /// Format version number.
    pub version: u16,
    /// Feature flags (bit 0: encrypted, bit 1: compressed).
    pub flags: u16,
    /// Scope identifier for all chunks in this container.
    pub scope_id: uuid::Uuid,
    /// Number of chunks in the chunk table.
    pub chunk_count: u32,
}

/// A single entry in the chunk table (13 bytes on disk).
#[derive(Debug, Clone, Copy)]
pub struct ChunkTableEntry {
    /// Byte offset into the data section.
    pub offset: u64,
    /// Byte length of the chunk data.
    pub length: u32,
    /// Chunk type discriminant.
    pub chunk_type: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_type_known_values() {
        assert_eq!(ChunkType::try_from(0).unwrap(), ChunkType::Fact);
        assert_eq!(ChunkType::try_from(1).unwrap(), ChunkType::Preference);
        assert_eq!(ChunkType::try_from(2).unwrap(), ChunkType::Decision);
        assert_eq!(ChunkType::try_from(3).unwrap(), ChunkType::Soul);
    }

    #[test]
    fn chunk_type_unknown_value_errors() {
        assert!(ChunkType::try_from(4).is_err());
        assert!(ChunkType::try_from(255).is_err());
    }

    #[test]
    fn chunk_type_roundtrip() {
        for ct in [
            ChunkType::Fact,
            ChunkType::Preference,
            ChunkType::Decision,
            ChunkType::Soul,
        ] {
            let byte: u8 = ct.into();
            assert_eq!(ChunkType::try_from(byte).unwrap(), ct);
        }
    }
}
