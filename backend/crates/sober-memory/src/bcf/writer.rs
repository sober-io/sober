//! BCF writer — builds a complete BCF byte sequence from chunks.

use super::types::{BCF_MAGIC, BCF_VERSION_V1, CHUNK_TABLE_ENTRY_SIZE, ChunkType, HEADER_SIZE};
use crate::error::MemoryError;

/// Incrementally builds a BCF container.
///
/// Chunks are collected in memory, then [`finish`](BcfWriter::finish) serializes
/// the header, chunk table, and data in one pass.
pub struct BcfWriter {
    scope_id: uuid::Uuid,
    chunks: Vec<(ChunkType, Vec<u8>)>,
}

impl BcfWriter {
    /// Creates a new writer for the given scope.
    #[must_use]
    pub fn new(scope_id: uuid::Uuid) -> Self {
        Self {
            scope_id,
            chunks: Vec::new(),
        }
    }

    /// Appends a chunk of the given type with raw data.
    pub fn add_chunk(&mut self, chunk_type: ChunkType, data: Vec<u8>) {
        self.chunks.push((chunk_type, data));
    }

    /// Serializes all collected chunks into a complete BCF byte sequence.
    ///
    /// Consumes the writer. The output contains the header (28 bytes),
    /// chunk table (13 bytes per entry), and concatenated chunk data.
    pub fn finish(self) -> Result<Vec<u8>, MemoryError> {
        let chunk_count = self.chunks.len();
        let table_size = chunk_count * CHUNK_TABLE_ENTRY_SIZE;
        let total_data_len: usize = self.chunks.iter().map(|(_, d)| d.len()).sum();
        let total_size = HEADER_SIZE + table_size + total_data_len;

        let mut buf = Vec::with_capacity(total_size);

        // Header (28 bytes)
        buf.extend_from_slice(&BCF_MAGIC);
        buf.extend_from_slice(&BCF_VERSION_V1.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // flags: none set in v1
        buf.extend_from_slice(self.scope_id.as_bytes()); // 16 bytes LE
        buf.extend_from_slice(&(chunk_count as u32).to_le_bytes());

        // Chunk table
        let mut data_offset: u64 = 0;
        for (chunk_type, data) in &self.chunks {
            buf.extend_from_slice(&data_offset.to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.push(u8::from(*chunk_type));
            data_offset += data.len() as u64;
        }

        // Chunk data
        for (_, data) in &self.chunks {
            buf.extend_from_slice(data);
        }

        debug_assert_eq!(buf.len(), total_size);
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_writer_produces_valid_bcf() {
        let scope_id = uuid::Uuid::nil();
        let writer = BcfWriter::new(scope_id);
        let bytes = writer.finish().unwrap();

        assert_eq!(bytes.len(), HEADER_SIZE);
        assert_eq!(&bytes[0..4], &BCF_MAGIC);
        // chunk count = 0
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 0);
    }

    #[test]
    fn single_chunk_output_size() {
        let mut writer = BcfWriter::new(uuid::Uuid::nil());
        writer.add_chunk(ChunkType::Fact, b"hello".to_vec());
        let bytes = writer.finish().unwrap();

        assert_eq!(bytes.len(), HEADER_SIZE + CHUNK_TABLE_ENTRY_SIZE + 5);
    }
}
