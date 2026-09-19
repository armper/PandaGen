//! Bare-metal storage integration
//!
//! Provides filesystem access for the kernel_bootstrap environment.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use hal::{BlockDevice, RamDisk};
#[cfg(all(not(test), target_os = "none"))]
use hal_x86_64::virtio::{QueuePlacement, VIRTQ_MAX_SIZE};
#[cfg(all(not(test), target_os = "none"))]
use hal_x86_64::{
    DmaBuffers, LegacyQueueLayout, QueueMemory, RealPortIo, VirtioBlkDevice, VirtioMmioDevice,
    VirtioPciLegacy, VirtqAvail, VirtqDesc, VirtqUsed,
};
use services_storage::{BlockStorage, ObjectId, PersistentFilesystem, TransactionError};

/// Well-known root directory id for the bare-metal filesystem, so a disk
/// formatted on one boot can be mounted on the next.
const ROOT_DIR_ID: ObjectId = ObjectId::from_bytes(*b"PANDAGEN-ROOT-01");

/// Addresses the storage probe needs from the bootloader.
#[derive(Debug, Clone, Copy, Default)]
pub struct StorageBootInfo {
    pub hhdm_offset: Option<u64>,
    /// Physical and virtual base of the kernel image, for translating the
    /// addresses of static DMA buffers.
    pub kernel_phys: Option<u64>,
    pub kernel_virt: Option<u64>,
}

impl StorageBootInfo {
    /// Physical address of a kernel-image virtual address.
    #[allow(dead_code)]
    pub(crate) fn image_phys(&self, virt: usize) -> Option<u64> {
        Some(
            (virt as u64)
                .wrapping_sub(self.kernel_virt?)
                .wrapping_add(self.kernel_phys?),
        )
    }
}

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

/// One page-aligned region holding a legacy-layout virtqueue (desc, avail,
/// then used on the next page) for up to 256 entries.
#[cfg(all(not(test), target_os = "none"))]
#[repr(C, align(4096))]
struct LegacyQueueArea([u8; 12288]);
#[cfg(all(not(test), target_os = "none"))]
static mut LEGACY_QUEUE: LegacyQueueArea = LegacyQueueArea([0; 12288]);

/// DMA scratch: one block of data, request header, status byte.
#[cfg(all(not(test), target_os = "none"))]
#[repr(C, align(4096))]
struct DmaArea {
    data: [u8; hal::BLOCK_SIZE],
    header: [u8; 16],
    status: [u8; 1],
}
#[cfg(all(not(test), target_os = "none"))]
static mut DMA_AREA: DmaArea = DmaArea {
    data: [0; hal::BLOCK_SIZE],
    header: [0; 16],
    status: [0; 1],
};

/// Boot storage backend choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackendKind {
    RamDisk,
    #[cfg(all(not(test), target_os = "none"))]
    VirtioBlkMmio,
    #[cfg(all(not(test), target_os = "none"))]
    VirtioBlkPci,
}

pub(crate) enum StorageBackend {
    RamDisk(RamDisk),
    #[cfg(all(not(test), target_os = "none"))]
    VirtioBlk(VirtioBlkDevice<VirtioMmioDevice>),
    #[cfg(all(not(test), target_os = "none"))]
    VirtioBlkPci(VirtioBlkDevice<VirtioPciLegacy<RealPortIo>>),
}

impl StorageBackend {
    fn kind(&self) -> StorageBackendKind {
        match self {
            Self::RamDisk(_) => StorageBackendKind::RamDisk,
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlk(_) => StorageBackendKind::VirtioBlkMmio,
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlkPci(_) => StorageBackendKind::VirtioBlkPci,
        }
    }

    /// Whether this backend has a writeback cache that FLUSH must reach.
    /// Printed at boot: a driver that silently skips FLUSH looks exactly
    /// like one that has nothing to flush, and the difference is whether a
    /// commit survives the host losing power.
    fn flush_supported(&self) -> bool {
        match self {
            Self::RamDisk(_) => false,
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlk(dev) => dev.flush_supported(),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlkPci(dev) => dev.flush_supported(),
        }
    }
}

impl BlockDevice for StorageBackend {
    fn block_count(&self) -> u64 {
        match self {
            Self::RamDisk(device) => device.block_count(),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlk(device) => device.block_count(),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlkPci(device) => device.block_count(),
        }
    }

    fn read_block(&mut self, block_idx: u64, buffer: &mut [u8]) -> Result<(), hal::BlockError> {
        match self {
            Self::RamDisk(device) => device.read_block(block_idx, buffer),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlk(device) => device.read_block(block_idx, buffer),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlkPci(device) => device.read_block(block_idx, buffer),
        }
    }

    fn write_block(&mut self, block_idx: u64, buffer: &[u8]) -> Result<(), hal::BlockError> {
        match self {
            Self::RamDisk(device) => device.write_block(block_idx, buffer),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlk(device) => device.write_block(block_idx, buffer),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlkPci(device) => device.write_block(block_idx, buffer),
        }
    }

    fn flush(&mut self) -> Result<(), hal::BlockError> {
        match self {
            Self::RamDisk(device) => device.flush(),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlk(device) => device.flush(),
            #[cfg(all(not(test), target_os = "none"))]
            Self::VirtioBlkPci(device) => device.flush(),
        }
    }
}

/// Bare-metal filesystem wrapper
pub struct BareMetalFilesystem {
    pub(crate) fs: PersistentFilesystem<StorageBackend>,
    root_id: ObjectId,
    backend_kind: StorageBackendKind,
    /// Whether that backend has a writeback cache FLUSH must reach.
    backend_flushes: bool,
    /// True when this boot formatted the disk (no valid superblock found).
    freshly_formatted: bool,
}

impl BareMetalFilesystem {
    /// Create a filesystem with the best available boot storage backend.
    pub fn new() -> Result<Self, TransactionError> {
        Self::new_with_boot(StorageBootInfo::default())
    }

    /// Create a filesystem with optional HHDM info for MMIO backend discovery.
    pub fn new_with_hhdm(hhdm_offset: Option<u64>) -> Result<Self, TransactionError> {
        Self::new_with_boot(StorageBootInfo {
            hhdm_offset,
            ..StorageBootInfo::default()
        })
    }

    /// Probe storage (virtio-blk over PCI, then MMIO, then a RAM disk) and
    /// mount the existing filesystem if the disk carries one; otherwise
    /// format it with the well-known root id so later boots can mount it.
    pub fn new_with_boot(boot: StorageBootInfo) -> Result<Self, TransactionError> {
        let mut disk = create_storage_backend(boot);
        let backend_kind = disk.kind();
        let backend_flushes = disk.flush_supported();
        let formatted = BlockStorage::has_valid_superblock(&mut disk);
        let (fs, freshly_formatted) = if formatted {
            (PersistentFilesystem::open(disk, ROOT_DIR_ID)?, false)
        } else {
            (
                PersistentFilesystem::format_with_root(disk, "system", ROOT_DIR_ID)?,
                true,
            )
        };
        let root_id = fs.root_dir_id();

        Ok(Self {
            fs,
            root_id,
            backend_kind,
            backend_flushes,
            freshly_formatted,
        })
    }

    pub fn was_freshly_formatted(&self) -> bool {
        self.freshly_formatted
    }

    pub fn root_id(&self) -> ObjectId {
        self.root_id
    }

    /// Get the active storage backend kind.
    pub fn backend_kind(&self) -> StorageBackendKind {
        self.backend_kind
    }

    /// Get the active storage backend display name.
    /// Whether the backing device has a cache that `flush` must reach.
    pub fn backend_flushes(&self) -> bool {
        self.backend_flushes
    }

    pub fn backend_name(&self) -> &'static str {
        match self.backend_kind {
            StorageBackendKind::RamDisk => "ramdisk",
            #[cfg(all(not(test), target_os = "none"))]
            StorageBackendKind::VirtioBlkMmio => "virtio-blk-mmio",
            #[cfg(all(not(test), target_os = "none"))]
            StorageBackendKind::VirtioBlkPci => "virtio-blk-pci",
        }
    }

    /// Create a file with content
    pub fn create_file(
        &mut self,
        name: &str,
        content: &[u8],
    ) -> Result<ObjectId, TransactionError> {
        let file_id = self.fs.write_file(content)?;
        let displaced = self.fs.link(
            name,
            self.root_id,
            file_id,
            services_storage::ObjectKind::Blob,
            0,
        )?;
        // The name now points at the new object, so whatever it displaced is
        // unreachable: nothing in this tree ever binds one object to two
        // names. Without this, every save kept its predecessor's blocks
        // forever and the disk filled in proportion to how often a file was
        // saved rather than how large it was.
        if let Some(old) = displaced {
            if old.object_id != file_id {
                let _ = self.fs.release_object(old.object_id);
            }
        }
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

    /// Write content to a file (update existing or create new).
    ///
    /// This used to unlink the name first, which left a window where the
    /// file did not exist: if the write then failed -- a full disk is
    /// enough -- the old content was unreachable and the new content had
    /// never been written, so saving destroyed the file being saved.
    /// `create_file` writes the content and only then rebinds the name, and
    /// `link` replaces an existing entry in a single directory update, so
    /// the name points at the old object or the new one and never at
    /// nothing.
    pub fn write_file_by_name(
        &mut self,
        name: &str,
        content: &[u8],
    ) -> Result<ObjectId, TransactionError> {
        self.create_file(name, content)
    }

    /// List files in root directory
    pub fn list_files(&mut self) -> Result<Vec<String>, TransactionError> {
        let entries = self.fs.list(self.root_id)?;
        Ok(entries.into_iter().map(|(name, _)| name).collect())
    }

    /// Delete a file
    pub fn delete_file(&mut self, name: &str) -> Result<(), TransactionError> {
        if let Some(entry) = self.fs.unlink(name, self.root_id, 0)? {
            // Deleting used to remove the name and keep the blocks.
            let _ = self.fs.release_object(entry.object_id);
        }
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
fn create_storage_backend(_boot: StorageBootInfo) -> StorageBackend {
    StorageBackend::RamDisk(RamDisk::new(RAM_DISK_BLOCKS))
}

#[cfg(all(not(test), target_os = "none"))]
fn create_storage_backend(boot: StorageBootInfo) -> StorageBackend {
    if let Some(device) = unsafe { try_create_virtio_pci_backend(boot) } {
        return StorageBackend::VirtioBlkPci(device);
    }
    unsafe { try_create_virtio_mmio_backend(boot) }
        .map(StorageBackend::VirtioBlk)
        .unwrap_or_else(|| StorageBackend::RamDisk(RamDisk::new(RAM_DISK_BLOCKS)))
}

/// DMA scratch buffers with physical addresses derived from the kernel image.
#[cfg(all(not(test), target_os = "none"))]
unsafe fn dma_buffers(boot: StorageBootInfo) -> Option<DmaBuffers> {
    let data = core::ptr::addr_of_mut!(DMA_AREA.data) as *mut u8;
    let header = core::ptr::addr_of_mut!(DMA_AREA.header);
    let status = core::ptr::addr_of_mut!(DMA_AREA.status) as *mut u8;
    Some(DmaBuffers {
        header,
        header_phys: boot.image_phys(header as usize)?,
        status,
        status_phys: boot.image_phys(status as usize)?,
        data,
        data_phys: boot.image_phys(data as usize)?,
    })
}

/// virtio-blk over PCI (what `cargo xtask qemu` attaches).
#[cfg(all(not(test), target_os = "none"))]
unsafe fn try_create_virtio_pci_backend(
    boot: StorageBootInfo,
) -> Option<VirtioBlkDevice<VirtioPciLegacy<RealPortIo>>> {
    let mut io = RealPortIo::new();
    let info = hal_x86_64::pci::find_virtio_blk(&mut io)?;
    let base = info.io_base()?;
    hal_x86_64::pci::enable_io_and_bus_master(&mut io, info.address);

    // Legacy layout inside one page-aligned static area.
    let area = core::ptr::addr_of_mut!(LEGACY_QUEUE.0) as *mut u8;
    core::ptr::write_bytes(area, 0, 12288);
    let layout = LegacyQueueLayout::for_size(VIRTQ_MAX_SIZE as u16);
    let desc = area.add(layout.desc_offset) as *mut VirtqDesc;
    let avail = area.add(layout.avail_offset) as *mut VirtqAvail;
    let used = area.add(layout.used_offset) as *mut VirtqUsed;
    let area_phys = boot.image_phys(area as usize)?;
    let placement = QueuePlacement {
        desc_phys: area_phys + layout.desc_offset as u64,
        avail_phys: area_phys + layout.avail_offset as u64,
        used_phys: area_phys + layout.used_offset as u64,
    };
    let queue = QueueMemory {
        desc,
        avail,
        used,
        placement,
    };
    let dma = dma_buffers(boot)?;
    let transport = VirtioPciLegacy::new(RealPortIo::new(), base);
    let device = VirtioBlkDevice::new(transport, queue, dma).ok()?;
    if device.block_count() == 0 {
        return None;
    }
    Some(device)
}

/// virtio-blk over MMIO windows (microvm-style machines).
#[cfg(all(not(test), target_os = "none"))]
unsafe fn try_create_virtio_mmio_backend(
    boot: StorageBootInfo,
) -> Option<VirtioBlkDevice<VirtioMmioDevice>> {
    let hhdm_offset = boot.hhdm_offset?;
    let dma = dma_buffers(boot)?;

    for region in VIRTIO_MMIO_REGIONS {
        for slot in 0..VIRTIO_MMIO_SLOTS_PER_REGION {
            reset_virtqueue_memory();

            let phys_base = region + (slot as u64 * VIRTIO_MMIO_SLOT_STRIDE);
            let virt_base = hhdm_offset.wrapping_add(phys_base) as usize;
            let Some(transport) = VirtioMmioDevice::new(virt_base) else {
                continue;
            };
            if transport.device_id() != hal_x86_64::virtio::VIRTIO_DEVICE_ID_BLOCK {
                continue;
            }

            let desc_ptr = core::ptr::addr_of_mut!(VIRTQ_DESC).cast::<VirtqDesc>();
            let avail_ptr = core::ptr::addr_of_mut!(VIRTQ_AVAIL);
            let used_ptr = core::ptr::addr_of_mut!(VIRTQ_USED);
            let placement = QueuePlacement {
                desc_phys: boot.image_phys(desc_ptr as usize)?,
                avail_phys: boot.image_phys(avail_ptr as usize)?,
                used_phys: boot.image_phys(used_ptr as usize)?,
            };
            let queue = QueueMemory {
                desc: desc_ptr,
                avail: avail_ptr,
                used: used_ptr,
                placement,
            };
            if let Ok(device) = VirtioBlkDevice::new(transport, queue, dma) {
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
