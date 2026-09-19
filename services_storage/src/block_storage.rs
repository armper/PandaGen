//! Block-backed storage implementation with crash-safe commits
//!
//! Provides persistent storage by writing to block devices.
//! Objects are stored as blocks on disk, with a crash-safe commit protocol.
//!
//! ## Crash Safety Model
//! This implementation uses an append-only commit log with checksums:
//! - Each transaction writes data blocks first
//! - Then writes a commit record with checksum (atomic point of truth)
//! - On recovery, scans for valid commit records
//! - Incomplete transactions (no commit record or bad checksum) are discarded
use crate::{
    ObjectId, Transaction, TransactionError, TransactionId, TransactionalStorage, VersionId,
};
use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use hal::{BlockDevice, BlockError, BLOCK_SIZE};

/// How many identity serials are reserved on disk at a time. A crash loses
/// at most this many; reusing even one would alias two objects.
const SERIAL_RESERVATION: u64 = 1024;

/// Upper bound on the commit ring, matching what `format` will ever choose.
/// A mounted superblock claiming more is refused rather than scanned.
const MAX_COMMIT_LOG_BLOCKS: u64 = 256;
use serde::{Deserialize, Serialize};

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
    /// First block of commit log
    commit_log_start: u64,
    /// Number of commit log blocks
    commit_log_blocks: u64,
    /// Commit sequence number (monotonically increasing)
    commit_sequence: u64,
    /// Next identity serial that has not been handed out. Reserved in
    /// batches, so a crash may skip serials but can never reuse one.
    #[serde(default)]
    next_serial: u64,
    /// Sequence covered by the checkpoint in the reserved region. Zero means
    /// there is none; defaulted so a disk written before checkpoints existed
    /// still mounts.
    #[serde(default)]
    checkpoint_sequence: u64,
}

/// The allocation map, written whole into the reserved region so that a
/// mount does not depend on commit records that the ring has overwritten.
#[derive(Debug, Serialize, Deserialize)]
struct Checkpoint {
    sequence: u64,
    allocations: Vec<AllocationEntry>,
    latest: Vec<(ObjectId, VersionId)>,
}

/// Header block of a checkpoint: how long the payload is and whether it is
/// intact. A checkpoint that fails this is ignored and the log is used alone.
#[derive(Debug, Serialize, Deserialize)]
struct CheckpointHeader {
    bytes: u64,
    sequence: u64,
    checksum: u32,
}

const SUPERBLOCK_MAGIC: u64 = 0x50414E44_47454E00; // "PANDAGEN\0"
const STORAGE_VERSION: u32 = 2; // Bumped for crash-safe storage

/// Commit record for crash-safe transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommitRecord {
    /// Transaction ID
    transaction_id: TransactionId,
    /// Sequence number (for ordering)
    sequence: u64,
    /// List of allocations made in this transaction
    allocations: Vec<AllocationEntry>,
    /// CRC32 checksum of the commit record (excluding this field)
    checksum: u32,
}

impl CommitRecord {
    /// Create a new commit record with computed checksum
    fn new(
        transaction_id: TransactionId,
        sequence: u64,
        allocations: Vec<AllocationEntry>,
    ) -> Self {
        let mut record = Self {
            transaction_id,
            sequence,
            allocations,
            checksum: 0,
        };
        record.checksum = record.compute_checksum();
        record
    }

    /// Compute CRC32 checksum of record (excluding checksum field)
    fn compute_checksum(&self) -> u32 {
        let mut temp = self.clone();
        temp.checksum = 0;
        let data = serde_json::to_vec(&temp).unwrap_or_default();
        crc32fast::hash(&data)
    }

    /// Validate checksum
    fn is_valid(&self) -> bool {
        let computed = self.compute_checksum();
        computed == self.checksum
    }
}

/// Storage recovery report
#[derive(Debug, Clone)]
pub struct StorageRecoveryReport {
    /// Number of valid commits recovered
    pub recovered_commits: usize,
    /// Number of invalid/incomplete transactions discarded
    pub discarded_transactions: usize,
    /// Last valid commit sequence number
    pub last_sequence: u64,
    /// Whether recovery was successful
    pub success: bool,
    /// Recovery error message if any
    pub error: Option<String>,
}

/// Block allocation status
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AllocationEntry {
    object_id: ObjectId,
    version_id: VersionId,
    block_idx: u64,
    size_bytes: u64,
    /// Every block this version occupies, as `(start, count)` runs in order.
    /// `allocate_blocks` takes the lowest free blocks one at a time and does
    /// not promise they adjoin, so `block_idx` alone described the allocation
    /// only when the free list happened to have no holes.
    ///
    /// Runs rather than a flat list because a commit record must fit in one
    /// block: the ordinary contiguous allocation is a single pair however
    /// large the object, and a fragmented one costs a pair per run. Absent on
    /// disks written before this field existed, where the contiguous range is
    /// the best guess available.
    #[serde(default)]
    extents: Vec<(u64, u64)>,
}

impl AllocationEntry {
    /// Collapse an ordered block list into runs.
    fn extents_of(blocks: &[u64]) -> Vec<(u64, u64)> {
        let mut runs: Vec<(u64, u64)> = Vec::new();
        for &block in blocks {
            match runs.last_mut() {
                Some((start, count)) if *start + *count == block => *count += 1,
                _ => runs.push((block, 1)),
            }
        }
        runs
    }

    /// The blocks this version actually occupies.
    fn block_list(&self) -> Vec<u64> {
        if self.extents.is_empty() {
            let needed = (self.size_bytes as usize).div_ceil(BLOCK_SIZE) as u64;
            return (self.block_idx..self.block_idx + needed).collect();
        }
        let mut blocks = Vec::new();
        for &(start, count) in &self.extents {
            for offset in 0..count {
                blocks.push(start + offset);
            }
        }
        blocks
    }
}

/// Block-backed storage backend with crash-safe commits
pub struct BlockStorage<D: BlockDevice> {
    device: D,
    superblock: Superblock,
    /// Next identity serial to hand out, and the end of the reservation
    /// currently recorded on disk.
    serial_next: u64,
    serial_limit: u64,
    /// Map object versions to their block locations
    allocations: BTreeMap<(ObjectId, VersionId), AllocationEntry>,
    /// Track the latest version for each object
    latest_versions: BTreeMap<ObjectId, VersionId>,
    /// Free blocks
    free_blocks: BTreeSet<u64>,
    /// Pending writes for active transactions
    pending: BTreeMap<TransactionId, Vec<PendingWrite>>,
    /// Recovery report (if opened from existing storage)
    recovery_report: Option<StorageRecoveryReport>,
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

        // Reserve blocks:
        // - Block 0: superblock
        // - Blocks 1-N: commit log (5% of disk or 1 block min, 256 blocks max)
        // - Blocks N+1-M: allocation bitmap (10% of remaining or 1 block min)
        // - Blocks M+1...: data area
        let commit_log_blocks = ((total_blocks * 5) / 100).clamp(1, 256);
        let bitmap_start = 1 + commit_log_blocks;

        // Ensure we don't overflow
        if bitmap_start >= total_blocks {
            return Err(BlockStorageError::InvalidSuperblock);
        }

        let remaining_after_log = total_blocks - bitmap_start;
        let bitmap_blocks = (remaining_after_log / 10).max(1);
        let data_start = bitmap_start + bitmap_blocks;

        if data_start >= total_blocks {
            return Err(BlockStorageError::InvalidSuperblock);
        }

        let superblock = Superblock {
            magic: SUPERBLOCK_MAGIC,
            version: STORAGE_VERSION,
            total_blocks,
            bitmap_start,
            bitmap_blocks,
            data_start,
            commit_log_start: 1,
            commit_log_blocks,
            commit_sequence: 0,
            checkpoint_sequence: 0,
            next_serial: 0,
        };

        // Write superblock to block 0
        let mut block = [0u8; BLOCK_SIZE];
        let sb_json =
            serde_json::to_vec(&superblock).map_err(|_| BlockStorageError::SerializationError)?;
        if sb_json.len() > BLOCK_SIZE {
            return Err(BlockStorageError::InvalidSuperblock);
        }
        block[..sb_json.len()].copy_from_slice(&sb_json);
        device.write_block(0, &block)?;
        device.flush()?;

        // Initialize free blocks (all data blocks are free)
        let free_blocks: BTreeSet<u64> = (data_start..total_blocks).collect();

        Ok(Self {
            device,
            serial_next: superblock.next_serial,
            serial_limit: superblock.next_serial,
            superblock,
            allocations: BTreeMap::new(),
            latest_versions: BTreeMap::new(),
            free_blocks,
            pending: BTreeMap::new(),
            recovery_report: None,
        })
    }

    /// Open existing block storage with crash recovery
    /// Whether block 0 holds a superblock with our magic, without taking
    /// ownership of the device. Lets a caller choose `open` over `format`.
    pub fn has_valid_superblock(device: &mut D) -> bool {
        let mut block = [0u8; BLOCK_SIZE];
        if device.read_block(0, &mut block).is_err() {
            return false;
        }
        let json_end = block.iter().position(|&b| b == 0).unwrap_or(BLOCK_SIZE);
        serde_json::from_slice::<Superblock>(&block[..json_end])
            .map(|sb| sb.magic == SUPERBLOCK_MAGIC)
            .unwrap_or(false)
    }

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

        // Everything below this point sizes allocations from superblock
        // fields, and block 0 is whatever happens to be on the disk. A
        // `total_blocks` of 200 billion used to turn a mount into an
        // unbounded allocation, which on the kernel is an abort: a single
        // edited byte made the machine unbootable. Check the geometry
        // against the device the disk is actually on.
        let device_blocks = device.block_count();
        let geometry_sound = superblock.total_blocks <= device_blocks
            && superblock.data_start < superblock.total_blocks
            && superblock.commit_log_blocks > 0
            && superblock.commit_log_blocks <= MAX_COMMIT_LOG_BLOCKS
            && superblock
                .commit_log_start
                .checked_add(superblock.commit_log_blocks)
                .is_some_and(|end| end <= superblock.total_blocks)
            && superblock.bitmap_start <= superblock.total_blocks;
        if !geometry_sound {
            return Err(BlockStorageError::InvalidSuperblock);
        }

        // Create storage instance
        let free_blocks: BTreeSet<u64> = (superblock.data_start..superblock.total_blocks).collect();

        let mut storage = Self {
            device,
            serial_next: superblock.next_serial,
            serial_limit: superblock.next_serial,
            superblock,
            allocations: BTreeMap::new(),
            latest_versions: BTreeMap::new(),
            free_blocks,
            pending: BTreeMap::new(),
            recovery_report: None,
        };

        // Perform crash recovery
        let recovery_report = storage.perform_recovery()?;
        storage.recovery_report = Some(recovery_report);

        Ok(storage)
    }

    /// A serial that has never been handed out on this disk.
    ///
    /// Identities used to come from a counter that restarted at one on every
    /// boot, so the Nth object of one boot had the same id as the Nth object
    /// of the next: two unrelated files aliased to a single object, and
    /// reading one returned the other's contents.
    pub fn next_serial(&mut self) -> Result<u64, BlockStorageError> {
        if self.serial_next >= self.serial_limit {
            self.serial_limit = self.serial_next.saturating_add(SERIAL_RESERVATION);
            self.superblock.next_serial = self.serial_limit;
            self.write_superblock()?;
        }
        let serial = self.serial_next;
        self.serial_next += 1;
        Ok(serial)
    }

    /// Write the whole allocation map into the reserved region.
    ///
    /// The commit log is a fixed ring, so a record is eventually overwritten
    /// by a later one. Without this the map could only ever be rebuilt from
    /// the last `commit_log_blocks` commits, and everything older became
    /// unreachable even though its data blocks were intact.
    fn write_checkpoint(&mut self) -> Result<(), BlockStorageError> {
        if self.superblock.bitmap_blocks < 2 {
            return Ok(());
        }
        let checkpoint = Checkpoint {
            sequence: self.superblock.commit_sequence,
            allocations: self.allocations.values().cloned().collect(),
            latest: self
                .latest_versions
                .iter()
                .map(|(object, version)| (*object, *version))
                .collect(),
        };
        let payload =
            serde_json::to_vec(&checkpoint).map_err(|_| BlockStorageError::SerializationError)?;
        let payload_blocks = payload.len().div_ceil(BLOCK_SIZE) as u64;
        if payload_blocks + 1 > self.superblock.bitmap_blocks {
            // Too big to record. The log still covers recent commits, so
            // leave the previous checkpoint in place rather than tearing it.
            return Ok(());
        }

        let header = CheckpointHeader {
            bytes: payload.len() as u64,
            sequence: checkpoint.sequence,
            checksum: crc32fast::hash(&payload),
        };
        let header_json =
            serde_json::to_vec(&header).map_err(|_| BlockStorageError::SerializationError)?;
        if header_json.len() > BLOCK_SIZE {
            return Err(BlockStorageError::SerializationError);
        }

        // Payload first, then the header that vouches for it: a crash in
        // between leaves the old header pointing at the old payload, which
        // fails its checksum and is ignored, rather than a header that
        // vouches for a half-written payload.
        for index in 0..payload_blocks {
            let mut block = [0u8; BLOCK_SIZE];
            let start = index as usize * BLOCK_SIZE;
            let end = (start + BLOCK_SIZE).min(payload.len());
            block[..end - start].copy_from_slice(&payload[start..end]);
            self.device
                .write_block(self.superblock.bitmap_start + 1 + index, &block)?;
        }
        self.device.flush()?;

        let mut block = [0u8; BLOCK_SIZE];
        block[..header_json.len()].copy_from_slice(&header_json);
        self.device
            .write_block(self.superblock.bitmap_start, &block)?;
        self.device.flush()?;

        self.superblock.checkpoint_sequence = checkpoint.sequence;
        self.write_superblock()?;
        Ok(())
    }

    /// Load the checkpoint, if there is an intact one. Returns the sequence
    /// it covers.
    fn load_checkpoint(&mut self) -> u64 {
        if self.superblock.bitmap_blocks < 2 || self.superblock.checkpoint_sequence == 0 {
            return 0;
        }
        let mut block = [0u8; BLOCK_SIZE];
        if self
            .device
            .read_block(self.superblock.bitmap_start, &mut block)
            .is_err()
        {
            return 0;
        }
        let json_end = block.iter().position(|&b| b == 0).unwrap_or(BLOCK_SIZE);
        let Ok(header) = serde_json::from_slice::<CheckpointHeader>(&block[..json_end]) else {
            return 0;
        };
        let payload_blocks = (header.bytes as usize).div_ceil(BLOCK_SIZE) as u64;
        if payload_blocks + 1 > self.superblock.bitmap_blocks {
            return 0;
        }
        let mut payload = Vec::with_capacity(header.bytes as usize);
        for index in 0..payload_blocks {
            let mut chunk = [0u8; BLOCK_SIZE];
            if self
                .device
                .read_block(self.superblock.bitmap_start + 1 + index, &mut chunk)
                .is_err()
            {
                return 0;
            }
            let take = (header.bytes as usize - payload.len()).min(BLOCK_SIZE);
            payload.extend_from_slice(&chunk[..take]);
        }
        if crc32fast::hash(&payload) != header.checksum {
            return 0;
        }
        let Ok(checkpoint) = serde_json::from_slice::<Checkpoint>(&payload) else {
            return 0;
        };

        for alloc in &checkpoint.allocations {
            let blocks = alloc.block_list();
            if blocks.iter().any(|b| *b >= self.superblock.total_blocks) {
                continue;
            }
            self.allocations
                .insert((alloc.object_id, alloc.version_id), alloc.clone());
            for block in blocks {
                self.free_blocks.remove(&block);
            }
        }
        for (object, version) in checkpoint.latest {
            self.latest_versions.insert(object, version);
        }
        checkpoint.sequence
    }

    /// Perform crash recovery by scanning commit log
    fn perform_recovery(&mut self) -> Result<StorageRecoveryReport, BlockStorageError> {
        let mut discarded_transactions = 0;
        // Everything up to here is already in the map; the ring only has to
        // cover what happened since.
        let checkpoint_sequence = self.load_checkpoint();

        // The commit log is a ring, so block order and sequence order
        // diverge the moment it wraps. Collect every intact record first and
        // apply them in sequence order; scanning in block order and keeping
        // only records newer than the highest seen so far discarded almost
        // the whole filesystem after the first wrap.
        let mut records: Vec<CommitRecord> = Vec::new();
        for i in 0..self.superblock.commit_log_blocks {
            let block_idx = self.superblock.commit_log_start + i;
            let mut block = [0u8; BLOCK_SIZE];

            match self.device.read_block(block_idx, &mut block) {
                Ok(_) => {
                    let json_end = block.iter().position(|&b| b == 0).unwrap_or(BLOCK_SIZE);
                    if json_end == 0 {
                        continue;
                    }
                    match serde_json::from_slice::<CommitRecord>(&block[..json_end]) {
                        Ok(record)
                            if record.is_valid() && record.sequence > checkpoint_sequence =>
                        {
                            records.push(record)
                        }
                        Ok(record) if record.is_valid() => {
                            // Already covered by the checkpoint.
                            let _ = record;
                        }
                        Ok(_) => discarded_transactions += 1,
                        Err(_) => {}
                    }
                }
                Err(_) => discarded_transactions += 1,
            }
        }

        records.sort_by_key(|record| record.sequence);
        let recovered_commits = records.len();
        let last_sequence = records
            .last()
            .map(|record| record.sequence)
            .unwrap_or(checkpoint_sequence)
            .max(checkpoint_sequence);

        for record in &records {
            for alloc in &record.allocations {
                // A record is only as trustworthy as the disk it came from;
                // its checksum is unkeyed. Refuse allocations that do not fit
                // the device rather than looping over a forged size.
                let blocks = alloc.block_list();
                if blocks.iter().any(|b| *b >= self.superblock.total_blocks) {
                    discarded_transactions += 1;
                    continue;
                }
                self.allocations
                    .insert((alloc.object_id, alloc.version_id), alloc.clone());
                self.latest_versions
                    .insert(alloc.object_id, alloc.version_id);
                for block in blocks {
                    self.free_blocks.remove(&block);
                }
            }
        }

        // The superblock update that records a commit is a separate write
        // from the commit record itself, so a crash between them leaves the
        // sequence stale. Reusing it would overwrite a live record in the
        // ring and silently destroy an already-committed object.
        self.superblock.commit_sequence = self.superblock.commit_sequence.max(last_sequence);

        Ok(StorageRecoveryReport {
            recovered_commits,
            discarded_transactions,
            last_sequence,
            success: true,
            error: None,
        })
    }

    pub fn recovery_report(&self) -> Option<&StorageRecoveryReport> {
        self.recovery_report.as_ref()
    }

    /// Write commit record to commit log
    fn write_commit_record(
        &mut self,
        transaction_id: TransactionId,
        allocations: Vec<AllocationEntry>,
    ) -> Result<(), BlockStorageError> {
        // Increment commit sequence
        self.superblock.commit_sequence += 1;

        let sequence = self.superblock.commit_sequence;

        // Create commit record with checksum
        let record = CommitRecord::new(transaction_id, sequence, allocations);

        // Serialize commit record
        let record_json =
            serde_json::to_vec(&record).map_err(|_| BlockStorageError::SerializationError)?;

        if record_json.len() > BLOCK_SIZE {
            return Err(BlockStorageError::SerializationError);
        }

        // Find commit log slot (round-robin)
        let log_slot = sequence % self.superblock.commit_log_blocks;
        let commit_block_idx = self.superblock.commit_log_start + log_slot;

        // Write commit record
        let mut block = [0u8; BLOCK_SIZE];
        block[..record_json.len()].copy_from_slice(&record_json);
        self.device.write_block(commit_block_idx, &block)?;
        self.device.flush()?;

        // Update superblock with new commit sequence
        self.write_superblock()?;

        Ok(())
    }

    /// Whether the ring is close enough to wrapping that the map should be
    /// folded into the reserved region.
    fn checkpoint_due(&self) -> bool {
        let interval = (self.superblock.commit_log_blocks / 2).max(1);
        self.superblock.commit_sequence % interval == 0
    }

    /// Write superblock to disk
    fn write_superblock(&mut self) -> Result<(), BlockStorageError> {
        let mut block = [0u8; BLOCK_SIZE];
        let sb_json = serde_json::to_vec(&self.superblock)
            .map_err(|_| BlockStorageError::SerializationError)?;
        if sb_json.len() > BLOCK_SIZE {
            return Err(BlockStorageError::InvalidSuperblock);
        }
        block[..sb_json.len()].copy_from_slice(&sb_json);
        self.device.write_block(0, &block)?;
        self.device.flush()?;
        Ok(())
    }

    /// Allocate blocks for data
    fn allocate_blocks(&mut self, size_bytes: u64) -> Result<Vec<u64>, BlockStorageError> {
        let blocks_needed = (size_bytes as usize).div_ceil(BLOCK_SIZE) as u64;
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

        let blocks = entry.block_list();

        self.read_data(&blocks, entry.size_bytes)
    }
    /// The body of a commit. `taken` collects every block this attempt
    /// removed from the free set, so the caller can give them back; `durable`
    /// is set once the commit record has landed and the attempt can no longer
    /// be undone or repeated.
    fn write_pending(
        &mut self,
        id: TransactionId,
        pending: &[PendingWrite],
        taken: &mut Vec<u64>,
        durable: &mut bool,
    ) -> Result<(), TransactionError> {
        let mut allocations_to_commit = Vec::new();

        // Step 1: Write all data blocks
        for write in pending {
            let size_bytes = write.data.len() as u64;
            let blocks = self
                .allocate_blocks(size_bytes)
                .map_err(|e| TransactionError::StorageError(format!("{:?}", e)))?;
            taken.extend_from_slice(&blocks);

            // A zero-length object allocates no blocks. Indexing the
            // empty list here aborted the kernel on an empty file.
            let first_block = blocks.first().copied().unwrap_or(0);
            self.write_data(&blocks, &write.data)
                .map_err(|e| TransactionError::StorageError(format!("{:?}", e)))?;

            allocations_to_commit.push(AllocationEntry {
                object_id: write.object_id,
                version_id: write.version_id,
                block_idx: first_block,
                size_bytes,
                extents: AllocationEntry::extents_of(&blocks),
            });
        }

        // Step 2: Write commit record (atomic point of truth)
        self.write_commit_record(id, allocations_to_commit.clone())
            .map_err(|e| TransactionError::StorageError(format!("{:?}", e)))?;

        // Step 3: Update in-memory state (only after commit record is written)
        for alloc in allocations_to_commit {
            self.allocations
                .insert((alloc.object_id, alloc.version_id), alloc.clone());
            self.latest_versions
                .insert(alloc.object_id, alloc.version_id);
        }
        *durable = true;

        // Step 4: fold the map into the reserved region often enough that
        // the commit ring never wraps past what the checkpoint covers.
        // This must follow step 3, or the checkpoint omits the very
        // commit that triggered it.
        if self.checkpoint_due() {
            self.write_checkpoint()
                .map_err(|e| TransactionError::StorageError(format!("{:?}", e)))?;
        }

        Ok(())
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

        let version_id = VersionId::from_serial(
            self.next_serial()
                .map_err(|e| TransactionError::StorageError(format!("{:?}", e)))?,
        );

        // Add to pending writes
        self.pending.entry(tx.id()).or_default().push(PendingWrite {
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
            let mut taken = Vec::new();
            let mut durable = false;
            match self.write_pending(tx.id(), &pending, &mut taken, &mut durable) {
                Ok(()) => {}
                Err(err) if durable => {
                    // The commit record landed and the map has been updated,
                    // so the data is on the disk whatever went wrong after.
                    // Report the failure, but do not offer the writes again:
                    // a retry would commit them a second time.
                    return Err(err);
                }
                Err(err) => {
                    // Nothing was committed. The transaction is still Active,
                    // so the caller may retry -- and a retry used to find the
                    // pending list already emptied, skip every step and
                    // return Ok, telling the caller its data was safe when
                    // not a byte of it had been recorded. Give the writes
                    // back, and the blocks with them, or each failed attempt
                    // would eat a little more of the disk and leave a hole in
                    // the free list behind it.
                    for block in taken {
                        self.free_blocks.insert(block);
                    }
                    self.pending.insert(tx.id(), pending);
                    return Err(err);
                }
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
        let _ = tx.rollback();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::vec;
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

    // === Crash-Safety Tests ===

    use crate::failing_device::{FailingBlockDevice, FailurePolicy};

    #[test]
    fn test_crash_safe_commit_succeeds() {
        let disk = RamDisk::with_capacity_mb(1);
        let mut storage = BlockStorage::format(disk).unwrap();

        let object_id = ObjectId::new();
        let data = b"Crash-safe data";

        let mut tx = storage.begin_transaction().unwrap();
        storage.write(&mut tx, object_id, data).unwrap();
        storage.commit(&mut tx).unwrap();

        // Verify data is readable
        let tx2 = storage.begin_transaction().unwrap();
        let version = storage.read(&tx2, object_id).unwrap();
        let read_data = storage.read_object_data(object_id, version).unwrap();
        assert_eq!(read_data, data);
    }

    #[test]
    fn test_crash_during_commit_recovery() {
        // This test verifies that recovery works correctly when a failure occurs during commit
        let disk = RamDisk::with_capacity_mb(1);
        let failing_disk = FailingBlockDevice::new(disk, FailurePolicy::Never);

        // First, create a storage and write some data successfully
        let mut storage = BlockStorage::format(failing_disk).unwrap();

        let obj1 = ObjectId::new();
        let mut tx1 = storage.begin_transaction().unwrap();
        storage.write(&mut tx1, obj1, b"successful data").unwrap();
        storage.commit(&mut tx1).unwrap();

        // Now make the device fail on writes
        storage.device.set_policy(FailurePolicy::AfterWrites(0));

        // Try to write new data (will fail)
        let obj2 = ObjectId::new();
        let mut tx2 = storage.begin_transaction().unwrap();
        storage.write(&mut tx2, obj2, b"failing data").unwrap();
        let result = storage.commit(&mut tx2);
        assert!(result.is_err()); // Commit should fail

        // Recover the device
        storage.device.set_policy(FailurePolicy::Never);
        let device = storage.device;

        // Re-open storage (triggers recovery)
        let mut recovered = BlockStorage::open(device).unwrap();

        // Check recovery report
        let report = recovered.recovery_report().unwrap();
        assert!(report.success);

        // First object should still be readable
        let tx3 = recovered.begin_transaction().unwrap();
        assert!(recovered.read(&tx3, obj1).is_ok());

        // Second object (failed commit) should not exist
        assert!(recovered.read(&tx3, obj2).is_err());
    }

    #[test]
    fn test_multiple_commits_recovery() {
        let disk = RamDisk::with_capacity_mb(1);
        let mut storage = BlockStorage::format(disk).unwrap();

        // Write multiple objects
        let obj1 = ObjectId::new();
        let obj2 = ObjectId::new();
        let obj3 = ObjectId::new();

        let mut tx1 = storage.begin_transaction().unwrap();
        storage.write(&mut tx1, obj1, b"data1").unwrap();
        storage.commit(&mut tx1).unwrap();

        let mut tx2 = storage.begin_transaction().unwrap();
        storage.write(&mut tx2, obj2, b"data2").unwrap();
        storage.commit(&mut tx2).unwrap();

        let mut tx3 = storage.begin_transaction().unwrap();
        storage.write(&mut tx3, obj3, b"data3").unwrap();
        storage.commit(&mut tx3).unwrap();

        // Extract and re-open device
        let device = storage.device;
        let mut recovered = BlockStorage::open(device).unwrap();

        // Check recovery report
        let report = recovered.recovery_report().unwrap();
        assert_eq!(report.recovered_commits, 3);
        assert!(report.success);

        // All objects should be readable
        let tx = recovered.begin_transaction().unwrap();
        assert!(recovered.read(&tx, obj1).is_ok());
        assert!(recovered.read(&tx, obj2).is_ok());
        assert!(recovered.read(&tx, obj3).is_ok());
    }

    #[test]
    fn test_crash_after_data_write_before_commit_record() {
        let disk = RamDisk::with_capacity_mb(1);
        let failing_disk = FailingBlockDevice::new(disk, FailurePolicy::Never);

        let mut storage = BlockStorage::format(failing_disk).unwrap();

        // First commit succeeds
        let obj1 = ObjectId::new();
        let mut tx1 = storage.begin_transaction().unwrap();
        storage.write(&mut tx1, obj1, b"stable data").unwrap();
        storage.commit(&mut tx1).unwrap();

        // Fail on commit record block to simulate crash before commit marker
        let next_seq = storage.superblock.commit_sequence + 1;
        let log_slot = (next_seq % storage.superblock.commit_log_blocks) as u64;
        let commit_block_idx = storage.superblock.commit_log_start + log_slot;
        storage
            .device
            .set_policy(FailurePolicy::OnBlocks(vec![commit_block_idx]));

        let obj2 = ObjectId::new();
        let mut tx2 = storage.begin_transaction().unwrap();
        storage.write(&mut tx2, obj2, b"should not commit").unwrap();
        let result = storage.commit(&mut tx2);
        assert!(result.is_err());

        // Recover
        storage.device.set_policy(FailurePolicy::Never);
        let device = storage.device;
        let mut recovered = BlockStorage::open(device).unwrap();

        let tx3 = recovered.begin_transaction().unwrap();
        assert!(recovered.read(&tx3, obj1).is_ok());
        assert!(recovered.read(&tx3, obj2).is_err());
    }

    #[test]
    fn an_object_whose_blocks_are_not_contiguous_still_reads_back_as_itself() {
        // `allocate_blocks` hands out the lowest free blocks one at a time and
        // makes no promise that they adjoin. The allocation record kept only
        // the *first* block, and the reader rebuilt the list as
        // `first..first + n`. As long as the free list had no holes the two
        // agreed by accident. A hole is easy to make: a commit that fails
        // after its data is written leaks those blocks, and the next mount
        // rebuilds the free list from the records that survived, so the
        // leaked blocks come back free with live blocks above them.
        //
        // The object then reads blocks it does not own -- here, the previous
        // object's -- and nothing reports an error.
        let disk = RamDisk::with_capacity_mb(1);
        let failing = FailingBlockDevice::new(disk, FailurePolicy::Never);
        let mut storage = BlockStorage::format(failing).unwrap();

        let first = ObjectId::new();
        let mut tx = storage.begin_transaction().unwrap();
        storage
            .write(&mut tx, first, &[b'1'; BLOCK_SIZE * 2])
            .unwrap();
        storage.commit(&mut tx).unwrap();

        // A commit that dies on its record: the data blocks are already
        // written, and nothing will ever say they are in use.
        let next_seq = storage.superblock.commit_sequence + 1;
        let log_slot = (next_seq % storage.superblock.commit_log_blocks) as u64;
        let record_block = storage.superblock.commit_log_start + log_slot;
        storage
            .device
            .set_policy(FailurePolicy::OnBlocks(vec![record_block]));
        let leaked = ObjectId::new();
        let mut tx = storage.begin_transaction().unwrap();
        storage
            .write(&mut tx, leaked, &[b'x'; BLOCK_SIZE * 2])
            .unwrap();
        assert!(storage.commit(&mut tx).is_err(), "the commit must fail");
        storage.device.set_policy(FailurePolicy::Never);

        // A third object lands above the hole and is recorded.
        let third = ObjectId::new();
        let mut tx = storage.begin_transaction().unwrap();
        storage
            .write(&mut tx, third, &[b'3'; BLOCK_SIZE * 2])
            .unwrap();
        storage.commit(&mut tx).unwrap();

        // Remount: the free list is rebuilt from what survived, so the two
        // leaked blocks are free again with `third`'s blocks above them.
        let device = storage.device;
        let mut storage = BlockStorage::open(device).unwrap();

        // Three blocks now cannot be contiguous: two come from the hole and
        // one from above `third`.
        let fourth = ObjectId::new();
        let body = [b'4'; BLOCK_SIZE * 3];
        let mut tx = storage.begin_transaction().unwrap();
        storage.write(&mut tx, fourth, &body).unwrap();
        storage.commit(&mut tx).unwrap();

        let tx = storage.begin_transaction().unwrap();
        let version = storage.read(&tx, fourth).unwrap();
        let got = storage.read_object_data(fourth, version).unwrap();
        assert_eq!(
            got.len(),
            body.len(),
            "the object came back the wrong length"
        );
        let differs = got.iter().zip(body.iter()).filter(|(a, b)| a != b).count();
        assert_eq!(
            differs,
            0,
            "{differs} of {} bytes read back as something else; the first \
             wrong byte is at {:?} and reads {:?}",
            body.len(),
            got.iter().zip(body.iter()).position(|(a, b)| a != b),
            got.iter()
                .zip(body.iter())
                .find(|(a, b)| a != b)
                .map(|(a, _)| *a as char)
        );

        // And the object whose blocks were read must be untouched.
        let version = storage.read(&tx, third).unwrap();
        let got = storage.read_object_data(third, version).unwrap();
        assert!(
            got.iter().all(|&b| b == b'3'),
            "the third object's blocks were handed to a later write"
        );
    }

    #[test]
    fn extents_collapse_runs_and_survive_a_round_trip() {
        assert_eq!(AllocationEntry::extents_of(&[]), Vec::new());
        assert_eq!(AllocationEntry::extents_of(&[7]), vec![(7, 1)]);
        assert_eq!(AllocationEntry::extents_of(&[7, 8, 9]), vec![(7, 3)]);
        assert_eq!(
            AllocationEntry::extents_of(&[7, 8, 11, 12, 40]),
            vec![(7, 2), (11, 2), (40, 1)]
        );

        for blocks in [vec![], vec![3], vec![3, 4, 5], vec![3, 4, 9, 20, 21]] {
            let entry = AllocationEntry {
                object_id: ObjectId::new(),
                version_id: VersionId::new(),
                block_idx: blocks.first().copied().unwrap_or(0),
                size_bytes: (blocks.len() * BLOCK_SIZE) as u64,
                extents: AllocationEntry::extents_of(&blocks),
            };
            assert_eq!(entry.block_list(), blocks, "round trip lost blocks");
        }
    }

    #[test]
    fn a_large_contiguous_object_still_fits_in_one_commit_record() {
        // A commit record must fit in a single block. Recording the block
        // list as runs keeps the ordinary case to one pair no matter how big
        // the object; a flat list of block numbers would have put a ceiling
        // on object size that did not exist before.
        let disk = RamDisk::with_capacity_mb(8);
        let mut storage = BlockStorage::format(disk).unwrap();
        let object = ObjectId::new();
        let body = vec![b'z'; BLOCK_SIZE * 512];
        let mut tx = storage.begin_transaction().unwrap();
        storage.write(&mut tx, object, &body).unwrap();
        storage
            .commit(&mut tx)
            .expect("a 2 MiB contiguous object must still commit");

        let tx = storage.begin_transaction().unwrap();
        let version = storage.read(&tx, object).unwrap();
        assert_eq!(storage.read_object_data(object, version).unwrap(), body);
    }

    #[test]
    fn a_commit_that_failed_is_not_reported_as_done_when_it_is_retried() {
        // `commit` took the pending writes out of the map before the first
        // step that can fail. The transaction stays Active on the error path,
        // so a caller is entitled to retry -- and the retry found nothing
        // pending, skipped straight to the end and returned Ok. The caller
        // was told its data was committed. Nothing had been written.
        let disk = RamDisk::with_capacity_mb(1);
        let failing = FailingBlockDevice::new(disk, FailurePolicy::Never);
        let mut storage = BlockStorage::format(failing).unwrap();

        let next_seq = storage.superblock.commit_sequence + 1;
        let log_slot = (next_seq % storage.superblock.commit_log_blocks) as u64;
        let record_block = storage.superblock.commit_log_start + log_slot;
        storage
            .device
            .set_policy(FailurePolicy::OnBlocks(vec![record_block]));

        let object = ObjectId::new();
        let mut tx = storage.begin_transaction().unwrap();
        storage
            .write(&mut tx, object, b"the data the caller wants")
            .unwrap();
        assert!(storage.commit(&mut tx).is_err(), "the commit must fail");

        // The disk is healthy again and the caller retries, as it may.
        storage.device.set_policy(FailurePolicy::Never);
        let retry = storage.commit(&mut tx);

        if retry.is_ok() {
            let tx = storage.begin_transaction().unwrap();
            let version = storage
                .read(&tx, object)
                .expect("commit returned Ok, so the object must be there");
            assert_eq!(
                storage.read_object_data(object, version).unwrap(),
                b"the data the caller wants",
                "commit returned Ok but the object holds something else"
            );
        }
    }

    #[test]
    fn a_failed_commit_gives_its_blocks_back() {
        // Step 1 takes blocks out of the free set. Every later step can fail,
        // and none of them put the blocks back, so each failed attempt ate
        // the disk a little. It also punched a hole in the free list, which
        // is how a later object came to read its neighbour's blocks.
        let disk = RamDisk::with_capacity_mb(1);
        let failing = FailingBlockDevice::new(disk, FailurePolicy::Never);
        let mut storage = BlockStorage::format(failing).unwrap();
        let before = storage.free_blocks.len();

        for attempt in 0..3 {
            let next_seq = storage.superblock.commit_sequence + 1;
            let log_slot = (next_seq % storage.superblock.commit_log_blocks) as u64;
            let record_block = storage.superblock.commit_log_start + log_slot;
            storage
                .device
                .set_policy(FailurePolicy::OnBlocks(vec![record_block]));

            let mut tx = storage.begin_transaction().unwrap();
            storage
                .write(&mut tx, ObjectId::new(), &[b'q'; BLOCK_SIZE * 2])
                .unwrap();
            assert!(storage.commit(&mut tx).is_err(), "attempt {attempt}");
            storage.device.set_policy(FailurePolicy::Never);

            assert_eq!(
                storage.free_blocks.len(),
                before,
                "after {} failed commits the free set has lost {} blocks",
                attempt + 1,
                before - storage.free_blocks.len()
            );
        }
    }

    #[test]
    fn test_crash_after_commit_record_before_superblock_update() {
        let disk = RamDisk::with_capacity_mb(1);
        let failing_disk = FailingBlockDevice::new(disk, FailurePolicy::Never);

        let mut storage = BlockStorage::format(failing_disk).unwrap();

        // Seed a baseline commit
        let obj1 = ObjectId::new();
        let mut tx1 = storage.begin_transaction().unwrap();
        storage.write(&mut tx1, obj1, b"baseline").unwrap();
        storage.commit(&mut tx1).unwrap();

        // Fail on superblock write (block 0) to simulate crash mid-metadata update
        storage.device.set_policy(FailurePolicy::OnBlocks(vec![0]));

        let obj2 = ObjectId::new();
        let mut tx2 = storage.begin_transaction().unwrap();
        storage.write(&mut tx2, obj2, b"new data").unwrap();
        let result = storage.commit(&mut tx2);
        assert!(result.is_err());

        // Recover
        storage.device.set_policy(FailurePolicy::Never);
        let device = storage.device;
        let mut recovered = BlockStorage::open(device).unwrap();

        let tx3 = recovered.begin_transaction().unwrap();
        assert!(recovered.read(&tx3, obj1).is_ok());

        // Commit record may have landed before the superblock update; either outcome is valid
        let obj2_visible = recovered.read(&tx3, obj2).is_ok();
        if obj2_visible {
            let version = recovered.read(&tx3, obj2).unwrap();
            let data = recovered.read_object_data(obj2, version).unwrap();
            assert_eq!(data, b"new data");
        }
    }

    #[test]
    fn test_checksum_validation() {
        // Create a commit record with invalid checksum
        let alloc = AllocationEntry {
            object_id: ObjectId::new(),
            version_id: VersionId::new(),
            block_idx: 100,
            size_bytes: 128,
            extents: vec![(100, 1)],
        };

        let mut record = CommitRecord::new(TransactionId::new(), 1, vec![alloc]);

        // Record should be valid initially
        assert!(record.is_valid());

        // Corrupt the checksum
        record.checksum = 0xDEADBEEF;
        assert!(!record.is_valid());
    }

    #[test]
    fn test_commit_log_wrap_around() {
        let disk = RamDisk::with_capacity_mb(1);
        let mut storage = BlockStorage::format(disk).unwrap();

        let log_size = storage.superblock.commit_log_blocks as usize;

        // Write more commits than log size to test wrap-around
        for i in 0..(log_size + 5) {
            let obj = ObjectId::new();
            let data = format!("data_{}", i);

            let mut tx = storage.begin_transaction().unwrap();
            storage.write(&mut tx, obj, data.as_bytes()).unwrap();
            storage.commit(&mut tx).unwrap();
        }

        // Should still work correctly
        assert_eq!(storage.superblock.commit_sequence, (log_size + 5) as u64);
    }

    #[test]
    fn test_recovery_with_no_commits() {
        let disk = RamDisk::with_capacity_mb(1);
        let storage = BlockStorage::format(disk).unwrap();

        // Re-open immediately (no commits)
        let device = storage.device;
        let recovered = BlockStorage::open(device).unwrap();

        let report = recovered.recovery_report().unwrap();
        assert_eq!(report.recovered_commits, 0);
        assert_eq!(report.last_sequence, 0);
        assert!(report.success);
    }
}
