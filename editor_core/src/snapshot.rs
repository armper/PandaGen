//! Editor snapshot for deterministic parity testing

use crate::{EditorMode, Position};
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Complete editor state snapshot for parity testing
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorSnapshot {
    pub mode: EditorMode,
    pub cursor: Position,
    pub buffer_lines: Vec<String>,
    pub dirty: bool,
    pub command_buffer: String,
    pub search_query: String,
    pub undo_depth: usize,
    pub redo_depth: usize,
}

impl EditorSnapshot {
    /// Compute a deterministic hash of the snapshot state
    /// This is used for fast comparison in parity tests
    #[cfg(test)]
    pub fn hash(&self) -> u64 {
        use sha2::{Digest, Sha256};
        
        let mut hasher = Sha256::new();
        
        // Hash mode
        hasher.update([self.mode as u8]);
        
        // Hash cursor
        hasher.update(&self.cursor.row.to_le_bytes());
        hasher.update(&self.cursor.col.to_le_bytes());
        
        // Hash buffer
        for line in &self.buffer_lines {
            hasher.update(line.as_bytes());
            hasher.update(b"\n");
        }
        
        // Hash dirty flag
        hasher.update([self.dirty as u8]);
        
        // Hash command buffer
        hasher.update(self.command_buffer.as_bytes());
        
        // Hash search query
        hasher.update(self.search_query.as_bytes());
        
        // Hash undo/redo depth
        hasher.update(&self.undo_depth.to_le_bytes());
        hasher.update(&self.redo_depth.to_le_bytes());
        
        let result = hasher.finalize();
        let bytes: [u8; 8] = result[..8].try_into().unwrap();
        u64::from_le_bytes(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_snapshot_hash_deterministic() {
        let snapshot = EditorSnapshot {
            mode: EditorMode::Normal,
            cursor: Position::new(0, 0),
            buffer_lines: vec!["hello".into(), "world".into()],
            dirty: false,
            command_buffer: String::new(),
            search_query: String::new(),
            undo_depth: 0,
            redo_depth: 0,
        };

        let hash1 = snapshot.hash();
        let hash2 = snapshot.hash();
        assert_eq!(hash1, hash2, "Hash should be deterministic");
    }

    #[test]
    fn test_snapshot_hash_different_for_different_state() {
        let snapshot1 = EditorSnapshot {
            mode: EditorMode::Normal,
            cursor: Position::new(0, 0),
            buffer_lines: vec!["hello".into()],
            dirty: false,
            command_buffer: String::new(),
            search_query: String::new(),
            undo_depth: 0,
            redo_depth: 0,
        };

        let snapshot2 = EditorSnapshot {
            mode: EditorMode::Normal,
            cursor: Position::new(0, 1),
            buffer_lines: vec!["hello".into()],
            dirty: false,
            command_buffer: String::new(),
            search_query: String::new(),
            undo_depth: 0,
            redo_depth: 0,
        };

        assert_ne!(snapshot1.hash(), snapshot2.hash(), "Different states should have different hashes");
    }
}
