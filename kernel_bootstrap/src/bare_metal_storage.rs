//! Bare-metal storage integration
//!
//! Provides filesystem access for the kernel_bootstrap environment.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use hal::{BlockDevice, RamDisk};
#[cfg(all(not(test), target_os = "none"))]
use hal_x86_64::virtio::VIRTQ_MAX_SIZE;
#[cfg(all(not(test), target_os = "none"))]
use hal_x86_64::{VirtioBlkDevice, VirtqAvail, VirtqDesc, VirtqUsed};
use services_storage::{ObjectId, PersistentFilesystem, TransactionError};

const RAM_DISK_BLOCKS: usize = 32;

#[cfg(all(not(test), target_os = "none"))]
const VIRTIO_MMIO_REGIONS: [u64; 2] = [0x0A00_0000, 0xFEB0_0000];
#[cfg(all(not(test), target_os = "none"))]
const VIRTIO_MMIO_SLOT_STRIDE: u64 = 0x200;
#[cfg(all(not(test), target_os = "none"))]
const VIRTIO_MMIO_SLOTS_PER_REGION: usize = 8;

#[cfg(all(not(test), target_os = "none"))]
static mut VIRTQ_DESC: [VirtqDesc; VIRTQ_MAX_SIZE] = [VirtqDesc::new(); VIRTQ_MAX_SIZE];
#[cfg(all(not(test), target_os = "none"))]
static mut VIRTQ_AVAIL: VirtqAvail = VirtqAvail::new();
#[cfg(all(not(test), target_os = "none"))]
static mut VIRTQ_USED: VirtqUsed = VirtqUsed::new();

/// Boot storage backend choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackendKind {
    RamDisk,
    #[cfg(all(not(test), target_os = "none"))]
    VirtioBlkMmio,
}

pub(crate) enum StorageBackend {
    RamDisk(RamDisk),
    #[cfg(all(not(test), target_os = "none"))]
    VirtioBlk(VirtioBlkDevice),
}

impl StorageBackend {
    fn kind(&self) -> StorageBackendKind {
        match self {
            Self::RamDisk(_) => StorageBackendKind::RamDisk,
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlk(_) => StorageBackendKind::VirtioBlkMmio,
        }
    }
}

impl BlockDevice for StorageBackend {
    fn block_count(&self) -> u64 {
        match self {
            Self::RamDisk(device) => device.block_count(),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlk(device) => device.block_count(),
        }
    }

    fn read_block(&mut self, block_idx: u64, buffer: &mut [u8]) -> Result<(), hal::BlockError> {
        match self {
            Self::RamDisk(device) => device.read_block(block_idx, buffer),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlk(device) => device.read_block(block_idx, buffer),
        }
    }

    fn write_block(&mut self, block_idx: u64, buffer: &[u8]) -> Result<(), hal::BlockError> {
        match self {
            Self::RamDisk(device) => device.write_block(block_idx, buffer),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlk(device) => device.write_block(block_idx, buffer),
        }
    }

    fn flush(&mut self) -> Result<(), hal::BlockError> {
        match self {
            Self::RamDisk(device) => device.flush(),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlk(device) => device.flush(),
        }
    }
}

/// Bare-metal filesystem wrapper
pub struct BareMetalFilesystem {
    pub(crate) fs: PersistentFilesystem<StorageBackend>,
    root_id: ObjectId,
    backend_kind: StorageBackendKind,
}

impl BareMetalFilesystem {
    /// Create a new filesystem with the best available boot storage backend.
    pub fn new() -> Result<Self, TransactionError> {
        Self::new_with_hhdm(None)
    }

    /// Create a filesystem with optional HHDM info for MMIO backend discovery.
    ///
    /// On bare-metal (`target_os = "none"`), this attempts virtio-blk MMIO first and
    /// falls back to RamDisk if no supported device is found.
    pub fn new_with_hhdm(hhdm_offset: Option<u64>) -> Result<Self, TransactionError> {
        let disk = create_storage_backend(hhdm_offset);
        let backend_kind = disk.kind();
        let fs = PersistentFilesystem::format(disk, "system")?;
        let root_id = fs.root_dir_id();

        Ok(Self {
            fs,
            root_id,
            backend_kind,
        })
    }

    /// Get the root directory ID
    pub fn root_id(&self) -> ObjectId {
        self.root_id
    }

    /// Get the active storage backend kind.
    pub fn backend_kind(&self) -> StorageBackendKind {
        self.backend_kind
    }

    /// Get the active storage backend display name.
    pub fn backend_name(&self) -> &'static str {
        match self.backend_kind {
            StorageBackendKind::RamDisk => "ramdisk",
            #[cfg(all(not(test), target_os = "none"))]
            StorageBackendKind::VirtioBlkMmio => "virtio-blk-mmio",
        }
    }

    /// Create a file with content
    pub fn create_file(
        &mut self,
        name: &str,
        content: &[u8],
    ) -> Result<ObjectId, TransactionError> {
        let file_id = self.fs.write_file(content)?;
        self.fs.link(
            name,
            self.root_id,
            file_id,
            services_storage::ObjectKind::Blob,
            0,
        )?;
        Ok(file_id)
    }

    /// Read a file by name
    pub fn read_file_by_name(&mut self, name: &str) -> Result<Vec<u8>, TransactionError> {
        let dir = self.fs.read_directory(self.root_id)?;
        let entry = dir
            .get_entry(name)
            .ok_or_else(|| TransactionError::StorageError("File not found".into()))?;
        self.fs.read_file(entry.object_id)
    }

    /// Write content to a file (update existing or create new)
    pub fn write_file_by_name(
        &mut self,
        name: &str,
        content: &[u8],
    ) -> Result<ObjectId, TransactionError> {
        // Try to unlink existing file first
        let _ = self.fs.unlink(name, self.root_id, 0);

        // Create new file
        self.create_file(name, content)
    }

    /// List files in root directory
    pub fn list_files(&mut self) -> Result<Vec<String>, TransactionError> {
        let entries = self.fs.list(self.root_id)?;
        Ok(entries.into_iter().map(|(name, _)| name).collect())
    }

    /// Delete a file
    pub fn delete_file(&mut self, name: &str) -> Result<(), TransactionError> {
        self.fs.unlink(name, self.root_id, 0)?;
        Ok(())
    }

    /// Read file by object ID
    pub fn read_file(&mut self, object_id: ObjectId) -> Result<Vec<u8>, TransactionError> {
        self.fs.read_file(object_id)
    }

    /// Write file by object ID (creates new version)
    pub fn write_file(
        &mut self,
        _object_id: ObjectId,
        content: &[u8],
    ) -> Result<ObjectId, TransactionError> {
        // For now, we need to replace the file entirely
        // In a full implementation, we'd update the version
        let file_id = self.fs.write_file(content)?;
        Ok(file_id)
    }
}

impl Default for BareMetalFilesystem {
    fn default() -> Self {
        Self::new().expect("Failed to create filesystem")
    }
}

#[cfg(any(test, not(target_os = "none")))]
fn create_storage_backend(_hhdm_offset: Option<u64>) -> StorageBackend {
    StorageBackend::RamDisk(RamDisk::new(RAM_DISK_BLOCKS))
}

#[cfg(all(not(test), target_os = "none"))]
fn create_storage_backend(hhdm_offset: Option<u64>) -> StorageBackend {
    unsafe { try_create_virtio_backend(hhdm_offset) }
        .map(StorageBackend::VirtioBlk)
        .unwrap_or_else(|| StorageBackend::RamDisk(RamDisk::new(RAM_DISK_BLOCKS)))
}

#[cfg(all(not(test), target_os = "none"))]
unsafe fn try_create_virtio_backend(hhdm_offset: Option<u64>) -> Option<VirtioBlkDevice> {
    let hhdm_offset = hhdm_offset?;

    for region in VIRTIO_MMIO_REGIONS {
        for slot in 0..VIRTIO_MMIO_SLOTS_PER_REGION {
            reset_virtqueue_memory();

            let phys_base = region + (slot as u64 * VIRTIO_MMIO_SLOT_STRIDE);
            let virt_base = hhdm_offset.wrapping_add(phys_base) as usize;

            let desc_ptr = core::ptr::addr_of_mut!(VIRTQ_DESC).cast::<VirtqDesc>();
            let avail_ptr = core::ptr::addr_of_mut!(VIRTQ_AVAIL);
            let used_ptr = core::ptr::addr_of_mut!(VIRTQ_USED);

            if let Ok(device) = VirtioBlkDevice::new(virt_base, desc_ptr, avail_ptr, used_ptr) {
                if device.block_count() > 0 {
                    return Some(device);
                }
            }
        }
    }

    None
}

#[cfg(all(not(test), target_os = "none"))]
unsafe fn reset_virtqueue_memory() {
    VIRTQ_DESC = [VirtqDesc::new(); VIRTQ_MAX_SIZE];
    VIRTQ_AVAIL = VirtqAvail::new();
    VIRTQ_USED = VirtqUsed::new();
}
