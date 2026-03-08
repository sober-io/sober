//! Binary Context Format (BCF) — compact binary container for memory chunks.
//!
//! BCF is used for export/backup and offline snapshots. Live memory goes
//! through Qdrant + PostgreSQL.

mod reader;
mod types;
mod writer;

pub use reader::{BcfChunk, BcfReader};
pub use types::{BcfHeader, ChunkType};
pub use writer::BcfWriter;
