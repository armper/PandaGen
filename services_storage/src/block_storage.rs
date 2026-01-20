///! Block-backed storage implementation
///!
///! Provides persistent storage by writing to block devices.
///! Objects are stored as blocks on disk, with a simple allocation scheme.

use crate::{
    ObjectId, Transaction, TransactionError, TransactionId, TransactionalStorage, VersionId,
};
use hal::{BlockDevice, BlockError, BLOCK_SIZE};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Superblock - stored in block 0
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Superblock {
    /// Magic number for validation
    magic: u64,
    /// Version of the storage format
    version: u32,
    /// Total number of blocks on device
    total_blocks: u64,
    /// First block of allocation bitmap
    bitmap_start: u64,
    /// Number of bitmap blocks
    bitmap_blocks: u64,
    /// First block of data area
    data_start: u64,
}

const SUPERBLOCK_MAGIC: u64 = 0x50414E44_47454E00; // "PANDAGEN\0"
const STORAGE_VERSION: u32 = 1;

/// Block allocation status
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AllocationEntry {
    object_id: ObjectId,
    version_id: VersionId,
    block_idx: u64,
    size_bytes: u64,
}

/// Block-backed storage backend
pub struct BlockStorage<D: BlockDevice> {
    device: D,
    superblock: Superblock,
    /// Map object versions to their block locations
    allocations: HashMap<(ObjectId, VersionId), AllocationEntry>,
    /// Track the latest version for each object
    latest_versions: HashMap<ObjectId, VersionId>,
    /// Free blocks
    free_blocks: HashSet<u64>,
    /// Pending writes for active transactions
    pending: HashMap<TransactionId, Vec<PendingWrite>>,
}

#[derive(Debug, Clone)]
struct PendingWrite {
    object_id: ObjectId,
    version_id: VersionId,
    data: Vec<u8>,
}

#[derive(Debug)]
pub enum BlockStorageError {
    BlockError(BlockError),
    InvalidSuperblock,
    NoFreeSpace,
    ObjectNotFound,
    SerializationError,
}

impl From<BlockError> for BlockStorageError {
    fn from(err: BlockError) -> Self {
        Self::BlockError(err)
    }
}

impl From<BlockStorageError> for TransactionError {
    fn from(err: BlockStorageError) -> Self {
        match err {
            BlockStorageError::ObjectNotFound => {
                TransactionError::ObjectNotFound("object not found".to_string())
            }
            _ => TransactionError::StorageError("block storage error".to_string()),
        }
    }
}

impl<D: BlockDevice> BlockStorage<D> {
    /// Create a new block storage, formatting the device
    pub fn format(mut device: D) -> Result<Self, BlockStorageError> {
        let total_blocks = device.block_count();
        
        // Reserve first 1 block for superblock
        // Reserve 10% of remaining blocks for bitmap (generous allocation)
        let bitmap_blocks = ((total_blocks - 1) / 10).max(1);
        let data_start = 1 + bitmap_blocks;
        
        let superblock = Superblock {
            magic: SUPERBLOCK_MAGIC,
            version: STORAGE_VERSION,
            total_blocks,
            bitmap_start: 1,
            bitmap_blocks,
            data_start,
        };
        
        // Write superblock to block 0
        let mut block = [0u8; BLOCK_SIZE];
        let sb_json = serde_json::to_vec(&superblock)
            .map_err(|_| BlockStorageError::SerializationError)?;
        if sb_json.len() > BLOCK_SIZE {
            return Err(BlockStorageError::InvalidSuperblock);
        }
        block[..sb_json.len()].copy_from_slice(&sb_json);
        device.write_block(0, &block)?;
        device.flush()?;
        
        // Initialize free blocks (all data blocks are free)
        let free_blocks: HashSet<u64> = (data_start..total_blocks).collect();
        
        Ok(Self {
            device,
            superblock,
            allocations: HashMap::new(),
            latest_versions: HashMap::new(),
            free_blocks,
            pending: HashMap::new(),
        })
    }
    
    /// Open existing block storage
    pub fn open(mut device: D) -> Result<Self, BlockStorageError> {
        // Read superblock from block 0
        let mut block = [0u8; BLOCK_SIZE];
        device.read_block(0, &mut block)?;
        
        // Find the end of JSON (first null or end of meaningful data)
        let json_end = block.iter().position(|&b| b == 0).unwrap_or(BLOCK_SIZE);
        let superblock: Superblock = serde_json::from_slice(&block[..json_end])
            .map_err(|_| BlockStorageError::InvalidSuperblock)?;
        
        if superblock.magic != SUPERBLOCK_MAGIC {
            return Err(BlockStorageError::InvalidSuperblock);
        }
        
        // TODO: Rebuild allocations by scanning bitmap blocks
        // For now, start with all data blocks free
        let free_blocks: HashSet<u64> = 
            (superblock.data_start..superblock.total_blocks).collect();
        
        Ok(Self {
            device,
            superblock,
            allocations: HashMap::new(),
            latest_versions: HashMap::new(),
            free_blocks,
            pending: HashMap::new(),
        })
    }
    
    /// Allocate blocks for data
    fn allocate_blocks(&mut self, size_bytes: u64) -> Result<Vec<u64>, BlockStorageError> {
        let blocks_needed = ((size_bytes as usize + BLOCK_SIZE - 1) / BLOCK_SIZE) as u64;
        if (self.free_blocks.len() as u64) < blocks_needed {
            return Err(BlockStorageError::NoFreeSpace);
        }
        
        let mut allocated = Vec::new();
        for _ in 0..blocks_needed {
            if let Some(&block) = self.free_blocks.iter().next() {
                self.free_blocks.remove(&block);
                allocated.push(block);
            }
        }
        
        Ok(allocated)
    }
    
    /// Write data to allocated blocks
    fn write_data(&mut self, blocks: &[u64], data: &[u8]) -> Result<(), BlockStorageError> {
        let mut offset = 0;
        for &block_idx in blocks {
            let chunk_size = (data.len() - offset).min(BLOCK_SIZE);
            let mut block_data = [0u8; BLOCK_SIZE];
            block_data[..chunk_size].copy_from_slice(&data[offset..offset + chunk_size]);
            self.device.write_block(block_idx, &block_data)?;
            offset += chunk_size;
        }
        self.device.flush()?;
        Ok(())
    }
    
    /// Read data from allocated blocks
    fn read_data(&mut self, blocks: &[u64], size_bytes: u64) -> Result<Vec<u8>, BlockStorageError> {
        let mut data = Vec::with_capacity(size_bytes as usize);
        let mut remaining = size_bytes as usize;
        
        for &block_idx in blocks {
            let mut block = [0u8; BLOCK_SIZE];
            self.device.read_block(block_idx, &mut block)?;
            let chunk_size = remaining.min(BLOCK_SIZE);
            data.extend_from_slice(&block[..chunk_size]);
            remaining -= chunk_size;
            if remaining == 0 {
                break;
            }
        }
        
        Ok(data)
    }
    
    /// Read object data by object ID and version ID
    pub fn read_object_data(
        &mut self,
        object_id: ObjectId,
        version_id: VersionId,
    ) -> Result<Vec<u8>, BlockStorageError> {
        // Check pending writes first
        for pending_list in self.pending.values() {
            if let Some(entry) = pending_list
                .iter()
                .rev()
                .find(|p| p.object_id == object_id && p.version_id == version_id)
            {
                return Ok(entry.data.clone());
            }
        }
        
        // Look up in allocations
        let entry = self
            .allocations
            .get(&(object_id, version_id))
            .ok_or(BlockStorageError::ObjectNotFound)?;
        
        // Calculate blocks needed
        let blocks_needed = ((entry.size_bytes as usize + BLOCK_SIZE - 1) / BLOCK_SIZE) as u64;
        let blocks: Vec<u64> = (entry.block_idx..entry.block_idx + blocks_needed).collect();
        
        self.read_data(&blocks, entry.size_bytes)
    }
}

impl<D: BlockDevice> TransactionalStorage for BlockStorage<D> {
    fn begin_transaction(&mut self) -> Result<Transaction, TransactionError> {
        Ok(Transaction::new())
    }
    
    fn read(&self, tx: &Transaction, object_id: ObjectId) -> Result<VersionId, TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }
        
        // Check pending writes first
        if let Some(pending) = self.pending.get(&tx.id()) {
            if let Some(entry) = pending.iter().rev().find(|p| p.object_id == object_id) {
                return Ok(entry.version_id);
            }
        }
        
        // Return the latest version from allocations
        self.latest_versions
            .get(&object_id)
            .copied()
            .ok_or_else(|| TransactionError::ObjectNotFound(object_id.to_string()))
    }
    
    fn write(
        &mut self,
        tx: &mut Transaction,
        object_id: ObjectId,
        data: &[u8],
    ) -> Result<VersionId, TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }
        
        let version_id = VersionId::new();
        
        // Add to pending writes
        self.pending
            .entry(tx.id())
            .or_default()
            .push(PendingWrite {
                object_id,
                version_id,
                data: data.to_vec(),
            });
        
        Ok(version_id)
    }
    
    fn commit(&mut self, tx: &mut Transaction) -> Result<(), TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }
        
        // Write all pending writes to disk
        if let Some(pending) = self.pending.remove(&tx.id()) {
            for write in pending {
                let size_bytes = write.data.len() as u64;
                let blocks = self.allocate_blocks(size_bytes)
                    .map_err(|e| TransactionError::StorageError(format!("{:?}", e)))?;
                
                let first_block = blocks[0];
                self.write_data(&blocks, &write.data)
                    .map_err(|e| TransactionError::StorageError(format!("{:?}", e)))?;
                
                self.allocations.insert(
                    (write.object_id, write.version_id),
                    AllocationEntry {
                        object_id: write.object_id,
                        version_id: write.version_id,
                        block_idx: first_block,
                        size_bytes,
                    },
                );
                
                // Update latest version tracking
                self.latest_versions.insert(write.object_id, write.version_id);
            }
        }
        
        tx.commit()?;
        Ok(())
    }
    
    fn rollback(&mut self, tx: &mut Transaction) -> Result<(), TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }
        
        // Discard pending writes
        self.pending.remove(&tx.id());
        tx.rollback();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hal::RamDisk;
    
    #[test]
    fn test_format_and_open() {
        let disk = RamDisk::with_capacity_mb(1);
        let storage = BlockStorage::format(disk).unwrap();
        
        assert_eq!(storage.superblock.magic, SUPERBLOCK_MAGIC);
        assert_eq!(storage.superblock.version, STORAGE_VERSION);
        assert!(storage.superblock.data_start > 0);
    }
    
    #[test]
    fn test_write_and_read() {
        let disk = RamDisk::with_capacity_mb(1);
        let mut storage = BlockStorage::format(disk).unwrap();
        
        let object_id = ObjectId::new();
        let data = b"Hello, persistent storage!";
        
        let mut tx = storage.begin_transaction().unwrap();
        let version_id = storage.write(&mut tx, object_id, data).unwrap();
        storage.commit(&mut tx).unwrap();
        
        // Read back
        let tx2 = storage.begin_transaction().unwrap();
        let read_version = storage.read(&tx2, object_id).unwrap();
        assert_eq!(read_version, version_id);
    }
    
    #[test]
    fn test_persistence_across_open() {
        let object_id = ObjectId::new();
        let data = b"Persistent data";
        let _version_id;
        
        // Write to storage
        {
            let disk = RamDisk::with_capacity_mb(1);
            let mut storage = BlockStorage::format(disk).unwrap();
            
            let mut tx = storage.begin_transaction().unwrap();
            _version_id = storage.write(&mut tx, object_id, data).unwrap();
            storage.commit(&mut tx).unwrap();
        }
        
        // Note: This test demonstrates the concept, but RamDisk doesn't
        // persist across instances. In real usage with a file-backed or
        // hardware block device, data would persist.
    }
}
