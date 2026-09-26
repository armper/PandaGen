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

/// Bare-metal filesystem wrapper.
///
/// # Single-CPU confinement
///
/// The virtio descriptor ring, the available and used rings, and the DMA
/// staging area are `static mut` with no lock. That is **correct today and
/// only by confinement**: this filesystem is moved into the workspace, the
/// workspace runs only in `workspace_loop` on the boot CPU, and no kernel
/// task holds a filesystem handle. There is exactly one owner.
///
/// It is one line from being wrong. Kernel tasks run on application
/// processors, so the moment anything on an AP gains filesystem access -- a
/// background checkpointer, an HTTP route that serves a file, a remote
/// `cat` -- two CPUs share one virtqueue, one available ring and one DMA
/// buffer with no synchronisation. That is device-level corruption, not a
/// lost update.
///
/// If you are about to give a task a handle to this: wrap it in a
/// `SpinLock` first. The debug assertion below is a tripwire, not a
/// guarantee -- it only fires in a debug build, and only once the damage
/// would already be possible.
/// How many earlier contents a name keeps (GFX-057).
pub const MAX_VERSIONS: usize = 5;

pub struct BareMetalFilesystem {
    pub(crate) fs: PersistentFilesystem<StorageBackend>,
    /// Who may do what here, and who the filesystem is acting for
    /// (FS-003): every public call below asks it first.
    pub guard: crate::guard::Guard,
    /// The filesystem's clock -- seconds since the epoch, from the RTC --
    /// as last given: what expiries and history windows are measured in.
    clock: u64,
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
            guard: crate::guard::Guard::new(),
            clock: 0,
            root_id,
            backend_kind,
            backend_flushes,
            freshly_formatted,
        })
    }

    /// Set the clock decisions are made at (seconds since the epoch).
    pub fn set_clock(&mut self, now: u64) {
        if now > 0 {
            self.clock = now;
        }
    }

    pub fn clock(&self) -> u64 {
        self.clock
    }

    fn denied(reason: String) -> TransactionError {
        TransactionError::Denied(reason)
    }

    fn not_found() -> TransactionError {
        TransactionError::ObjectNotFound("File not found".into())
    }

    /// The entry called `name`, if there is one.
    fn entry(
        &mut self,
        name: &str,
    ) -> Result<Option<services_storage::persistent_fs::DirectoryEntry>, TransactionError> {
        Ok(self
            .fs
            .read_directory(self.root_id)?
            .get_entry(name)
            .cloned())
    }

    /// The entry called `name`, which the actor may see; one they may not
    /// see is not there, as far as they can tell.
    fn visible_entry(
        &mut self,
        name: &str,
    ) -> Result<services_storage::persistent_fs::DirectoryEntry, TransactionError> {
        let entry = self.entry(name)?.ok_or_else(Self::not_found)?;
        if !self.guard.visible(&entry, self.clock) {
            return Err(Self::not_found());
        }
        Ok(entry)
    }

    /// The next document id: one past the highest given.
    fn next_doc_id(&mut self) -> Result<u64, TransactionError> {
        let dir = self.fs.read_directory(self.root_id)?;
        Ok(dir.entries.values().map(|e| e.doc_id).max().unwrap_or(0) + 1)
    }

    /// Give every entry nobody owns to `owner`, with a document id (FS-003):
    /// what the system wrote before anyone existed, or a disk from before
    /// owners. `.authority` stays the system's. The system only.
    pub fn adopt_unowned(
        &mut self,
        owner: authority::PrincipalId,
    ) -> Result<usize, TransactionError> {
        if self.guard.principal() != Some(authority::SYSTEM) {
            return Err(Self::denied("only the system adopts".into()));
        }
        let dir = self.fs.read_directory(self.root_id)?;
        let names: Vec<String> = dir
            .entries
            .values()
            .filter(|e| e.owner == services_storage::persistent_fs::NOBODY || e.doc_id == 0)
            .map(|e| e.name.clone())
            .collect();
        let mut next = self.next_doc_id()?;
        let now = self.clock;
        for name in &names {
            let system = name == crate::guard::AUTHORITY_FILE;
            let id = next;
            self.fs.update_entry(self.root_id, name, now, |e| {
                if e.doc_id == 0 {
                    e.doc_id = id;
                }
                if system {
                    e.owner = authority::SYSTEM.0;
                    e.label = authority::Label::Secret as u8;
                } else if e.owner == services_storage::persistent_fs::NOBODY {
                    e.owner = owner.0;
                }
            })?;
            next += 1;
        }
        Ok(names.len())
    }

    /// Relabel `name` (FS-003): its owner, within their clearance.
    pub fn relabel(&mut self, name: &str, label: authority::Label) -> Result<(), TransactionError> {
        let entry = self.visible_entry(name)?;
        let who = self
            .guard
            .principal()
            .ok_or_else(|| Self::denied("no session".into()))?;
        self.guard
            .authority
            .may_relabel(who, &crate::guard::doc_ref(&entry), label)
            .map_err(|e| Self::denied(alloc::format!("{e}")))?;
        let now = self.clock;
        self.fs
            .update_entry(self.root_id, name, now, |e| e.label = label as u8)?;
        self.guard.mark_dirty();
        Ok(())
    }

    /// Share `name` with the person called `to` (FS-003): a grant derived
    /// from what the actor holds, on the terms given.
    pub fn share(
        &mut self,
        name: &str,
        to: &str,
        terms: authority::Terms,
    ) -> Result<authority::GrantId, TransactionError> {
        let entry = self.visible_entry(name)?;
        let who = self
            .guard
            .principal()
            .ok_or_else(|| Self::denied("no session".into()))?;
        let holder = self
            .guard
            .authority
            .principal_named(to)
            .map(|p| p.id)
            .ok_or_else(|| Self::denied(alloc::format!("no one called {to}")))?;
        let now = self.clock;
        let id = self
            .guard
            .authority
            .share(who, &crate::guard::doc_ref(&entry), holder, terms, now)
            .map_err(|e| Self::denied(alloc::format!("{e}")))?;
        self.guard.mark_dirty();
        Ok(id)
    }

    /// Revoke grant `id` on `name`, and what was derived from it.
    pub fn revoke(
        &mut self,
        name: &str,
        id: authority::GrantId,
    ) -> Result<usize, TransactionError> {
        let entry = self.visible_entry(name)?;
        let who = self
            .guard
            .principal()
            .ok_or_else(|| Self::denied("no session".into()))?;
        let n = self
            .guard
            .authority
            .revoke(who, &crate::guard::doc_ref(&entry), id)
            .map_err(|e| Self::denied(alloc::format!("{e}")))?;
        self.guard.mark_dirty();
        Ok(n)
    }

    /// The grants on `name`, for its owner and anyone who may share it.
    pub fn grants_on(&mut self, name: &str) -> Result<Vec<authority::Grant>, TransactionError> {
        let entry = self.visible_entry(name)?;
        let held = self.guard.held(&entry, self.clock);
        if !held.contains(authority::Rights::SHARE) {
            return Err(Self::denied(alloc::format!(
                "share on {name}: holds only {held}"
            )));
        }
        Ok(self
            .guard
            .authority
            .grants_on(authority::DocId(entry.doc_id))
            .cloned()
            .collect())
    }

    /// Keep the guard's state in `.authority`, as the system, whoever is
    /// acting (FS-003).
    pub fn save_guard(&mut self) -> Result<(), TransactionError> {
        let json = serde_json::to_vec(&self.guard.state())
            .map_err(|e| TransactionError::StorageError(alloc::format!("{e}")))?;
        let actor = self.guard.actor();
        self.guard.act_as(crate::guard::Actor::System);
        let now = self.clock;
        let result = self.write_named(
            crate::guard::AUTHORITY_FILE,
            &json,
            now,
            Some("settings/authority"),
        );
        self.guard.act_as(actor);
        result?;
        self.guard.take_dirty();
        Ok(())
    }

    /// Take up what `.authority` holds, if the disk has one. `Ok(false)`
    /// when it has none (a new disk).
    pub fn load_guard(&mut self) -> Result<bool, TransactionError> {
        let Some(entry) = self.entry(crate::guard::AUTHORITY_FILE)? else {
            return Ok(false);
        };
        let bytes = self.fs.read_file(entry.object_id)?;
        let state: crate::guard::GuardState = serde_json::from_slice(&bytes)
            .map_err(|e| TransactionError::StorageError(alloc::format!("{e}")))?;
        self.guard.restore(state);
        Ok(true)
    }

    /// The entry called `name` as the authority sees it, for sharing and
    /// for the Access views: `None` when the actor may not see it.
    pub fn document(
        &mut self,
        name: &str,
    ) -> Result<Option<services_storage::persistent_fs::DirectoryEntry>, TransactionError> {
        match self.visible_entry(name) {
            Ok(e) => Ok(Some(e)),
            Err(TransactionError::ObjectNotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
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
    /// What the mount recovered, and what it threw away.
    pub fn recovery_report(&self) -> Option<&services_storage::StorageRecoveryReport> {
        self.fs.recovery_report()
    }

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
    /// Trip if this is ever touched from anywhere but the boot CPU.
    ///
    /// The virtqueue and DMA area behind this are `static mut` with no lock;
    /// see the type's documentation. Debug builds only, and it fires after
    /// the fact rather than preventing anything -- it is a tripwire for the
    /// change that would make the confinement false, not a guarantee.
    #[inline]
    fn assert_boot_cpu(&self) {
        #[cfg(all(debug_assertions, not(test), target_os = "none"))]
        debug_assert!(
            crate::current_cpu_index() == Some(0),
            "the filesystem was used from an application processor; its \
             virtqueue and DMA area are unsynchronised"
        );
    }

    pub fn create_file(
        &mut self,
        name: &str,
        content: &[u8],
    ) -> Result<ObjectId, TransactionError> {
        self.create_file_at(name, content, 0)
    }

    /// `create_file`, stamping the entry with `now` (seconds since the
    /// epoch, from the RTC) so Files can say when it was last written.
    pub fn create_file_at(
        &mut self,
        name: &str,
        content: &[u8],
        now: u64,
    ) -> Result<ObjectId, TransactionError> {
        self.write_named(name, content, now, None)
    }

    /// Write `content` under `name` (GFX-057). The entry keeps its tags and
    /// its schema unless a new one is given; the content it replaces is
    /// kept as a version, newest first, up to [`MAX_VERSIONS`], and the
    /// version that falls off the end is released. Writing to a name in
    /// the bin takes it out.
    pub fn write_named(
        &mut self,
        name: &str,
        content: &[u8],
        now: u64,
        schema: Option<&str>,
    ) -> Result<ObjectId, TransactionError> {
        self.assert_boot_cpu();
        self.set_clock(now);
        let previous = self
            .fs
            .read_directory(self.root_id)?
            .get_entry(name)
            .cloned();
        // A closed session writes nothing (FS-003).
        if self.guard.principal().is_none() {
            return Err(Self::denied("no session".into()));
        }
        // Writing over a document takes WRITE on it; one the actor cannot
        // see is someone else's, and its name is taken.
        if let Some(old) = &previous {
            if !self.guard.visible(old, now)
                && !self.guard.held(old, now).contains(authority::Rights::WRITE)
            {
                return Err(Self::denied(alloc::format!(
                    "{name} belongs to someone else"
                )));
            }
            self.guard
                .decide(old, authority::Rights::WRITE, now)
                .map_err(Self::denied)?;
        }
        let new_doc = match &previous {
            Some(_) => None,
            None => Some(self.next_doc_id()?),
        };
        let file_id = self.fs.write_file(content)?;
        let mut entry = services_storage::persistent_fs::DirectoryEntry::new(
            name.into(),
            file_id,
            services_storage::ObjectKind::Blob,
        );
        entry.modified_at = now;
        entry.size = content.len() as u64;
        entry.schema = schema.map(String::from);
        // A new document is the actor's; `.authority` is always the
        // system's, and secret.
        if let Some(id) = new_doc {
            entry.doc_id = id;
            entry.owner = self.guard.owner_of_new();
        }
        if name == crate::guard::AUTHORITY_FILE {
            entry.owner = authority::SYSTEM.0;
            entry.label = authority::Label::Secret as u8;
        }
        if let Some(old) = previous {
            entry.doc_id = old.doc_id;
            entry.owner = old.owner;
            entry.label = old.label;
            if entry.schema.is_none() {
                entry.schema = old.schema.clone();
            }
            entry.tags = old.tags.clone();
            if old.object_id != file_id {
                entry
                    .versions
                    .push(services_storage::persistent_fs::VersionRecord {
                        object_id: old.object_id,
                        modified_at: old.modified_at,
                        size: old.size,
                    });
            }
            entry.versions.extend(old.versions.iter().cloned());
            // Bounded: nothing in this tree grows without a ceiling (V11's
            // lesson). What falls off the end is released.
            for dropped in entry
                .versions
                .drain(MAX_VERSIONS.min(entry.versions.len())..)
            {
                let _ = self.fs.release_object(dropped.object_id);
            }
        }
        self.fs.link_entry(self.root_id, entry, now)?;
        Ok(file_id)
    }

    /// Change what is known about `name` -- tags, bin state -- leaving the
    /// content alone. `Ok(false)` when there is no such name.
    pub fn update_entry(
        &mut self,
        name: &str,
        now: u64,
        change: impl FnOnce(&mut services_storage::persistent_fs::DirectoryEntry),
    ) -> Result<bool, TransactionError> {
        self.assert_boot_cpu();
        self.set_clock(now);
        let Some(entry) = self.entry(name)? else {
            return Ok(false);
        };
        if !self.guard.visible(&entry, now) {
            return Ok(false);
        }
        // Try the change on a copy to see what it touches: tags and the
        // like take TAG, the bin takes DELETE (FS-003).
        let mut after = entry.clone();
        change(&mut after);
        let right = if after.trashed != entry.trashed {
            authority::Rights::DELETE
        } else {
            authority::Rights::TAG
        };
        self.guard
            .decide(&entry, right, now)
            .map_err(Self::denied)?;
        let (doc_id, owner, label) = (entry.doc_id, entry.owner, entry.label);
        self.fs.update_entry(self.root_id, name, now, |e| {
            *e = after;
            // Who owns it and how it is labelled are not a tag.
            e.doc_id = doc_id;
            e.owner = owner;
            e.label = label;
        })
    }

    /// The kept versions of `name`, newest first: when each was current
    /// and how large it was.
    pub fn list_versions(
        &mut self,
        name: &str,
    ) -> Result<Vec<services_storage::persistent_fs::VersionRecord>, TransactionError> {
        self.assert_boot_cpu();
        let entry = self.visible_entry(name)?;
        let now = self.clock;
        self.guard
            .decide(&entry, authority::Rights::HISTORY, now)
            .map_err(Self::denied)?;
        Ok(self.versions_within_reach(&entry))
    }

    /// The versions of `entry` the actor's grants reach back to (FS-003).
    fn versions_within_reach(
        &self,
        entry: &services_storage::persistent_fs::DirectoryEntry,
    ) -> Vec<services_storage::persistent_fs::VersionRecord> {
        let Some(who) = self.guard.principal() else {
            return Vec::new();
        };
        let doc = crate::guard::doc_ref(entry);
        entry
            .versions
            .iter()
            .filter(|v| {
                self.guard
                    .authority
                    .check_version(who, &doc, v.modified_at, self.clock)
                    .allowed
            })
            .cloned()
            .collect()
    }

    /// An earlier content of `name`: 0 is the newest kept version.
    ///
    /// Takes `HISTORY` on the document, reaching back to when the version
    /// was saved (FS-003); `index` counts the versions the actor can
    /// reach, not the ones kept.
    pub fn read_version(&mut self, name: &str, index: usize) -> Result<Vec<u8>, TransactionError> {
        self.assert_boot_cpu();
        let entry = self.visible_entry(name)?;
        let now = self.clock;
        self.guard
            .decide(&entry, authority::Rights::HISTORY, now)
            .map_err(Self::denied)?;
        let version = self
            .versions_within_reach(&entry)
            .get(index)
            .cloned()
            .ok_or_else(|| TransactionError::StorageError("No such version".into()))?;
        self.guard
            .decide_version(&entry, version.modified_at, now)
            .map_err(Self::denied)?;
        self.fs.read_file(version.object_id)
    }

    /// Read a file by name
    ///
    /// Takes `READ` (FS-003).
    pub fn read_file_by_name(&mut self, name: &str) -> Result<Vec<u8>, TransactionError> {
        self.assert_boot_cpu();
        let entry = self.visible_entry(name)?;
        let now = self.clock;
        self.guard
            .decide(&entry, authority::Rights::READ, now)
            .map_err(Self::denied)?;
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
        self.assert_boot_cpu();
        let now = self.clock;
        let entries = self.fs.list(self.root_id)?;
        Ok(entries
            .into_iter()
            .filter(|(_, e)| self.guard.visible(e, now))
            .map(|(name, _)| name)
            .collect())
    }

    /// The root directory's entries with what is known about each (GFX-056):
    /// size and last-written time from the entry, reading the object only
    /// for entries written before sizes were recorded.
    pub fn list_entries(&mut self) -> Result<Vec<crate::desk::FileEntry>, TransactionError> {
        self.assert_boot_cpu();
        let now = self.clock;
        let entries = self.fs.list(self.root_id)?;
        let mut out = Vec::with_capacity(entries.len());
        // Only what the actor may see (FS-003), each with its owner, label
        // and what the actor holds on it.
        for (name, entry) in entries
            .into_iter()
            .filter(|(_, e)| self.guard.visible(e, now))
        {
            let held = self.guard.held(&entry, now);
            let size = if entry.size > 0 {
                entry.size
            } else {
                self.fs
                    .read_file(entry.object_id)
                    .map(|b| b.len() as u64)
                    .unwrap_or(0)
            };
            out.push(crate::desk::FileEntry {
                name,
                size,
                kind: match entry.kind {
                    services_storage::ObjectKind::Blob => "file",
                    services_storage::ObjectKind::Map => "folder",
                    _ => "object",
                },
                modified_at: entry.modified_at,
                schema: entry.schema.clone(),
                tags: entry.tags.clone(),
                trashed: entry.trashed,
                versions: entry.versions.len(),
                owner: self.guard.name_of(entry.owner),
                label: crate::guard::label_of(&entry).name(),
                held: held.0,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Delete a file
    pub fn delete_file(&mut self, name: &str) -> Result<(), TransactionError> {
        self.delete_file_at(name, 0)
    }

    /// `delete_file`, stamping the directory with `now`.
    ///
    /// Takes `DELETE` (FS-003); the document's grants go with it.
    pub fn delete_file_at(&mut self, name: &str, now: u64) -> Result<(), TransactionError> {
        self.assert_boot_cpu();
        self.set_clock(now);
        let Some(current) = self.entry(name)? else {
            return Ok(());
        };
        if !self.guard.visible(&current, now) {
            return Ok(());
        }
        self.guard
            .decide(&current, authority::Rights::DELETE, now)
            .map_err(Self::denied)?;
        if current.doc_id != 0 {
            self.guard
                .authority
                .forget(authority::DocId(current.doc_id));
        }
        if let Some(entry) = self.fs.unlink(name, self.root_id, now)? {
            // Deleting used to remove the name and keep the blocks. The
            // kept versions go with it.
            let _ = self.fs.release_object(entry.object_id);
            for version in &entry.versions {
                let _ = self.fs.release_object(version.object_id);
            }
        }
        Ok(())
    }

    /// Read an object by its storage id, unchecked -- for tests only
    /// (FS-003): a document is reached by name, through the guard; an id is
    /// how the storage below finds it, not a way in.
    #[cfg(test)]
    pub(crate) fn read_file(&mut self, object_id: ObjectId) -> Result<Vec<u8>, TransactionError> {
        self.assert_boot_cpu();
        self.fs.read_file(object_id)
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
