//! BCF reader — parses a BCF byte sequence into header and chunks.

use super::types::{
    BCF_MAGIC, BCF_VERSION_V1, BcfHeader, CHUNK_TABLE_ENTRY_SIZE, ChunkTableEntry, ChunkType,
    HEADER_SIZE,
};
use crate::error::MemoryError;

/// A parsed chunk referencing data in the original byte slice.
#[derive(Debug)]
pub struct BcfChunk<'a> {
    /// The type of this chunk.
    pub chunk_type: ChunkType,
    /// Raw chunk data (zero-copy slice into the original buffer).
    pub data: &'a [u8],
}

/// Zero-copy BCF parser.
///
/// Validates the header and chunk table on construction, then provides
/// an iterator over chunks that slices directly into the source data.
pub struct BcfReader<'a> {
    data: &'a [u8],
    header: BcfHeader,
    entries: Vec<ChunkTableEntry>,
}

impl<'a> BcfReader<'a> {
    /// Parses a BCF byte slice, validating the header and chunk table.
    pub fn parse(data: &'a [u8]) -> Result<Self, MemoryError> {
        if data.len() < HEADER_SIZE {
            return Err(MemoryError::BcfFormat(format!(
                "data too short for header: {} bytes (need {})",
                data.len(),
                HEADER_SIZE
            )));
        }

        // Validate magic
        if data[0..4] != BCF_MAGIC {
            return Err(MemoryError::BcfFormat(format!(
                "invalid magic bytes: {:02x?}",
                &data[0..4]
            )));
        }

        let version = u16::from_le_bytes(
            data[4..6]
                .try_into()
                .expect("slice is exactly 2 bytes after length check"),
        );
        if version != BCF_VERSION_V1 {
            return Err(MemoryError::BcfFormat(format!(
                "unsupported version: {version} (expected {BCF_VERSION_V1})"
            )));
        }

        let flags = u16::from_le_bytes(
            data[6..8]
                .try_into()
                .expect("slice is exactly 2 bytes after length check"),
        );
        let scope_id = uuid::Uuid::from_bytes(
            data[8..24]
                .try_into()
                .expect("slice is exactly 16 bytes after length check"),
        );
        let chunk_count = u32::from_le_bytes(
            data[24..28]
                .try_into()
                .expect("slice is exactly 4 bytes after length check"),
        ) as usize;

        let table_size = chunk_count * CHUNK_TABLE_ENTRY_SIZE;
        let min_size = HEADER_SIZE + table_size;
        if data.len() < min_size {
            return Err(MemoryError::BcfFormat(format!(
                "data too short for chunk table: {} bytes (need {})",
                data.len(),
                min_size
            )));
        }

        // Parse chunk table entries
        let data_section_start = (HEADER_SIZE + table_size) as u64;
        let mut entries = Vec::with_capacity(chunk_count);
        for i in 0..chunk_count {
            let base = HEADER_SIZE + i * CHUNK_TABLE_ENTRY_SIZE;
            let offset = u64::from_le_bytes(
                data[base..base + 8]
                    .try_into()
                    .expect("slice is exactly 8 bytes within bounds-checked region"),
            );
            let length = u32::from_le_bytes(
                data[base + 8..base + 12]
                    .try_into()
                    .expect("slice is exactly 4 bytes within bounds-checked region"),
            );
            let chunk_type = data[base + 12];

            // Bounds-check: the chunk data must fit within the buffer
            let abs_start = data_section_start + offset;
            let abs_end = abs_start + length as u64;
            if abs_end > data.len() as u64 {
                return Err(MemoryError::BcfFormat(format!(
                    "chunk {i} extends past end of data: offset={offset}, length={length}, \
                     data_len={}",
                    data.len()
                )));
            }

            entries.push(ChunkTableEntry {
                offset,
                length,
                chunk_type,
            });
        }

        let header = BcfHeader {
            version,
            flags,
            scope_id,
            chunk_count: chunk_count as u32,
        };

        Ok(Self {
            data,
            header,
            entries,
        })
    }

    /// Returns the parsed header.
    #[must_use]
    pub fn header(&self) -> &BcfHeader {
        &self.header
    }

    /// Returns an iterator over the chunks, yielding zero-copy slices.
    pub fn chunks(&self) -> impl Iterator<Item = Result<BcfChunk<'a>, MemoryError>> + '_ {
        let data_section_start = HEADER_SIZE + self.entries.len() * CHUNK_TABLE_ENTRY_SIZE;

        self.entries.iter().map(move |entry| {
            let chunk_type = ChunkType::try_from(entry.chunk_type)?;
            let start = data_section_start + entry.offset as usize;
            let end = start + entry.length as usize;
            Ok(BcfChunk {
                chunk_type,
                data: &self.data[start..end],
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bcf::writer::BcfWriter;

    #[test]
    fn roundtrip_multiple_chunks() {
        let scope_id = uuid::Uuid::now_v7();
        let mut writer = BcfWriter::new(scope_id);
        writer.add_chunk(ChunkType::Fact, b"fact data here".to_vec());
        writer.add_chunk(ChunkType::Decision, b"decision data".to_vec());
        writer.add_chunk(ChunkType::Soul, b"soul layer".to_vec());

        let bytes = writer.finish().unwrap();
        let reader = BcfReader::parse(&bytes).unwrap();

        assert_eq!(reader.header().version, BCF_VERSION_V1);
        assert_eq!(reader.header().scope_id, scope_id);
        assert_eq!(reader.header().chunk_count, 3);

        let chunks: Vec<_> = reader.chunks().collect::<Result<_, _>>().unwrap();
        assert_eq!(chunks[0].chunk_type, ChunkType::Fact);
        assert_eq!(chunks[0].data, b"fact data here");
        assert_eq!(chunks[1].chunk_type, ChunkType::Decision);
        assert_eq!(chunks[1].data, b"decision data");
        assert_eq!(chunks[2].chunk_type, ChunkType::Soul);
        assert_eq!(chunks[2].data, b"soul layer");
    }

    #[test]
    fn empty_bcf_roundtrip() {
        let writer = BcfWriter::new(uuid::Uuid::nil());
        let bytes = writer.finish().unwrap();
        let reader = BcfReader::parse(&bytes).unwrap();

        assert_eq!(reader.header().chunk_count, 0);
        assert_eq!(reader.chunks().count(), 0);
    }

    #[test]
    fn invalid_magic_rejected() {
        let mut bytes = vec![0u8; HEADER_SIZE];
        bytes[0..4].copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        let result = BcfReader::parse(&bytes);
        assert!(matches!(result, Err(MemoryError::BcfFormat(msg)) if msg.contains("magic")));
    }

    #[test]
    fn unsupported_version_rejected() {
        let mut bytes = vec![0u8; HEADER_SIZE];
        bytes[0..4].copy_from_slice(&BCF_MAGIC);
        bytes[4..6].copy_from_slice(&99u16.to_le_bytes());
        let result = BcfReader::parse(&bytes);
        assert!(matches!(result, Err(MemoryError::BcfFormat(msg)) if msg.contains("version")));
    }

    #[test]
    fn truncated_data_rejected() {
        let result = BcfReader::parse(&[0x53, 0xD5]);
        assert!(matches!(result, Err(MemoryError::BcfFormat(msg)) if msg.contains("too short")));
    }

    #[test]
    fn embedding_chunk_roundtrip() {
        let scope_id = uuid::Uuid::now_v7();
        let mut writer = BcfWriter::new(scope_id);

        // Store raw f32 vector as bytes
        let vector: Vec<f32> = vec![1.0, 2.0, 3.0, 0.5];
        let vector_bytes: Vec<u8> = vector.iter().flat_map(|f| f.to_le_bytes()).collect();
        writer.add_chunk(ChunkType::Fact, vector_bytes.clone());

        let bytes = writer.finish().unwrap();
        let reader = BcfReader::parse(&bytes).unwrap();
        let chunks: Vec<_> = reader.chunks().collect::<Result<_, _>>().unwrap();

        assert_eq!(chunks[0].chunk_type, ChunkType::Fact);
        assert_eq!(chunks[0].data, vector_bytes.as_slice());

        // Verify we can reconstruct the f32 vector
        let reconstructed: Vec<f32> = chunks[0]
            .data
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(reconstructed, vector);
    }
}
