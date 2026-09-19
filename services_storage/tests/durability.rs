//! What the filesystem must still be after it is mounted again.
//!
//! Every test here mounts, writes, drops the filesystem, and mounts the very
//! same bytes a second time. That is the only way to see the class of defect
//! that matters most for a persistent disk, and nothing in the crate's unit
//! tests does it.

use hal::{BlockDevice, BlockError, BLOCK_SIZE};
use services_storage::{BlockStorage, ObjectId, ObjectKind, PersistentFilesystem};
use std::cell::RefCell;
use std::rc::Rc;

/// Well-known root, as the kernel uses, so a second mount can find it.
const ROOT: ObjectId = ObjectId::from_bytes(*b"PANDAGEN-ROOT-01");

/// A RAM disk whose bytes outlive the device handle, so the same storage can
/// be mounted twice.
#[derive(Clone)]
struct SharedDisk {
    bytes: Rc<RefCell<Vec<u8>>>,
    blocks: u64,
}

impl SharedDisk {
    fn new(blocks: u64) -> Self {
        Self {
            bytes: Rc::new(RefCell::new(vec![0u8; blocks as usize * BLOCK_SIZE])),
            blocks,
        }
    }

    /// Overwrite raw bytes, the way a corrupted or hostile disk would look.
    fn poke(&self, block: u64, offset: usize, value: &[u8]) {
        let start = block as usize * BLOCK_SIZE + offset;
        self.bytes.borrow_mut()[start..start + value.len()].copy_from_slice(value);
    }

    fn read_raw(&self, block: u64) -> Vec<u8> {
        let start = block as usize * BLOCK_SIZE;
        self.bytes.borrow()[start..start + BLOCK_SIZE].to_vec()
    }
}

impl BlockDevice for SharedDisk {
    fn block_count(&self) -> u64 {
        self.blocks
    }

    fn read_block(&mut self, block_idx: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        if block_idx >= self.blocks {
            return Err(BlockError::OutOfBounds);
        }
        if buffer.len() < BLOCK_SIZE {
            return Err(BlockError::InvalidSize);
        }
        let start = block_idx as usize * BLOCK_SIZE;
        buffer[..BLOCK_SIZE].copy_from_slice(&self.bytes.borrow()[start..start + BLOCK_SIZE]);
        Ok(())
    }

    fn write_block(&mut self, block_idx: u64, buffer: &[u8]) -> Result<(), BlockError> {
        if block_idx >= self.blocks {
            return Err(BlockError::OutOfBounds);
        }
        if buffer.len() < BLOCK_SIZE {
            return Err(BlockError::InvalidSize);
        }
        let start = block_idx as usize * BLOCK_SIZE;
        self.bytes.borrow_mut()[start..start + BLOCK_SIZE].copy_from_slice(&buffer[..BLOCK_SIZE]);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), BlockError> {
        Ok(())
    }
}

#[test]
fn an_empty_file_is_ordinary_input() {
    // `blocks[0]` on a zero-length write used to index an empty vector.
    // The kernel aborts on panic, so creating an empty file killed the machine.
    let disk = SharedDisk::new(64);
    let mut fs = PersistentFilesystem::format_with_root(disk, "test", ROOT).unwrap();
    let id = fs
        .write_file(b"")
        .expect("writing an empty file must succeed");
    assert_eq!(fs.read_file(id).unwrap(), Vec::<u8>::new());

    // And it must still be empty, not missing, after everything else moves on.
    let other = fs.write_file(b"payload").unwrap();
    assert_eq!(fs.read_file(other).unwrap(), b"payload");
    assert_eq!(fs.read_file(id).unwrap(), Vec::<u8>::new());
}

#[test]
fn every_committed_file_survives_a_remount_past_the_commit_log_wrap() {
    // The commit log is a ring. Recovery used to scan it in *block* order and
    // accept a record only when its sequence exceeded the highest seen so
    // far, so once the ring wrapped it kept the few newest records and threw
    // away every older one, even though they were intact and checksummed.
    //
    // 512 blocks gives a 25-block log, so 80 files wrap it more than twice.
    let disk = SharedDisk::new(512);
    const FILES: usize = 80;

    let mut ids = Vec::new();
    {
        let mut fs = PersistentFilesystem::format_with_root(disk.clone(), "test", ROOT).unwrap();
        for index in 0..FILES {
            let body = format!("contents of file {index}");
            ids.push(fs.write_file(body.as_bytes()).unwrap());
        }
    }

    let mut fs = PersistentFilesystem::open(disk, ROOT).expect("the disk must remount");
    let mut missing = Vec::new();
    let mut wrong = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let want = format!("contents of file {index}");
        match fs.read_file(*id) {
            Err(_) => missing.push(index),
            Ok(got) if got != want.as_bytes() => wrong.push(index),
            Ok(_) => {}
        }
    }
    assert!(
        missing.is_empty() && wrong.is_empty(),
        "after a remount {} of {FILES} committed files are unreadable and {} hold the wrong bytes \
         (first missing: {:?})",
        missing.len(),
        wrong.len(),
        missing.first()
    );
}

#[test]
fn a_directory_listing_never_outlives_the_files_it_names() {
    // The root directory is rewritten on every link, so it tends to survive
    // even when the blobs it points at do not. A listing full of names whose
    // contents are gone is worse than an empty one.
    let disk = SharedDisk::new(512);
    const FILES: usize = 40;
    {
        let mut fs = PersistentFilesystem::format_with_root(disk.clone(), "test", ROOT).unwrap();
        for index in 0..FILES {
            let id = fs.write_file(format!("body {index}").as_bytes()).unwrap();
            fs.link(format!("file{index}"), ROOT, id, ObjectKind::Blob, 0)
                .unwrap();
        }
    }

    let mut fs = PersistentFilesystem::open(disk, ROOT).unwrap();
    let names = fs.list(ROOT).expect("root must be listable");
    let mut dangling = Vec::new();
    for (name, entry) in &names {
        if fs.read_file(entry.object_id).is_err() {
            dangling.push(name.clone());
        }
    }
    assert!(
        dangling.is_empty(),
        "{} of {} directory entries point at unreadable objects: {:?}",
        dangling.len(),
        names.len(),
        &dangling[..dangling.len().min(5)]
    );
}

#[test]
fn a_hostile_superblock_cannot_hang_or_exhaust_the_mount() {
    // `total_blocks` came straight out of block 0 and was used to build the
    // free set, with no check against the real device. A byte edit turned a
    // mount into an unbounded allocation, which on the kernel is an abort.
    let disk = SharedDisk::new(64);
    {
        PersistentFilesystem::format_with_root(disk.clone(), "test", ROOT).unwrap();
    }

    let block = disk.read_raw(0);
    let text = String::from_utf8_lossy(&block[..block.iter().position(|&b| b == 0).unwrap()]);
    let patched = text.replace("\"total_blocks\":64", "\"total_blocks\":200000000000");
    assert_ne!(patched, text, "the superblock must name total_blocks");
    disk.poke(0, 0, &vec![0u8; BLOCK_SIZE]);
    disk.poke(0, 0, patched.as_bytes());

    // Must refuse, promptly, rather than trying to represent 200 billion
    // free blocks.
    let started = std::time::Instant::now();
    let result = PersistentFilesystem::open(disk, ROOT);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "mounting a hostile superblock took {:?}",
        started.elapsed()
    );
    assert!(
        result.is_err(),
        "a superblock claiming more blocks than the device has must be refused"
    );
}

#[test]
fn the_identity_serial_never_repeats_across_a_remount() {
    // This is the mechanism the test below depends on, and the only part of
    // the defect that a host test can actually see: on the host `ObjectId::new`
    // was always a random v4 UUID, so collisions never showed up here. Only
    // the kernel build minted ids from a counter that restarted at one on
    // every boot. The fix is to stop depending on boot-local entropy at all
    // and hand out serials from a high-water mark kept in the superblock, so
    // assert exactly that: no serial is ever handed out twice.
    let disk = SharedDisk::new(512);
    const PER_BOOT: u64 = 2_500; // past the 1024-serial reservation, twice over

    let mut first = Vec::new();
    {
        let mut storage = BlockStorage::format(disk.clone()).unwrap();
        for _ in 0..PER_BOOT {
            first.push(storage.next_serial().unwrap());
        }
    }

    let mut storage = BlockStorage::open(disk).expect("the disk must remount");
    let mut second = Vec::new();
    for _ in 0..PER_BOOT {
        second.push(storage.next_serial().unwrap());
    }

    let highest = *first.iter().max().unwrap();
    let reused: Vec<_> = second.iter().copied().filter(|s| *s <= highest).collect();
    assert!(
        reused.is_empty(),
        "{} serials minted after the remount were at or below the {highest} already \
         handed out before it (first: {:?})",
        reused.len(),
        reused.first()
    );

    // A crash between reservations may skip serials; that is the price and it
    // is fine. Reusing even one is not.
    let mut all = first;
    all.extend(&second);
    let unique: std::collections::BTreeSet<_> = all.iter().copied().collect();
    assert_eq!(unique.len(), all.len(), "a serial was handed out twice");
}

#[test]
fn identities_from_two_boots_of_one_disk_never_collide() {
    // The end-to-end shape of the same defect. On the host this passes either
    // way; it is here so the property is stated where it belongs, and it is
    // real coverage for the kernel, which shares this code. The proof that the
    // kernel is fixed is the disk-image scan, not this test.
    let disk = SharedDisk::new(512);
    const PER_BOOT: usize = 12;

    let mut first = Vec::new();
    {
        let mut fs = PersistentFilesystem::format_with_root(disk.clone(), "test", ROOT).unwrap();
        for index in 0..PER_BOOT {
            let id = fs
                .write_file(format!("first boot {index}").as_bytes())
                .unwrap();
            fs.link(format!("a{index}"), ROOT, id, ObjectKind::Blob, 0)
                .unwrap();
            first.push(id);
        }
    }

    let mut fs = PersistentFilesystem::open(disk, ROOT).expect("the disk must remount");
    let mut second = Vec::new();
    for index in 0..PER_BOOT {
        let id = fs
            .write_file(format!("second boot {index}").as_bytes())
            .unwrap();
        fs.link(format!("b{index}"), ROOT, id, ObjectKind::Blob, 0)
            .unwrap();
        second.push(id);
    }

    let clashes: Vec<_> = second.iter().filter(|id| first.contains(id)).collect();
    assert!(
        clashes.is_empty(),
        "{} of {PER_BOOT} identities minted after the remount were already in use \
         before it (first clash: {:?})",
        clashes.len(),
        clashes.first()
    );

    // And the older files must still read back as themselves, not as their
    // namesakes from the second boot.
    for (index, id) in first.iter().enumerate() {
        let want = format!("first boot {index}");
        assert_eq!(
            fs.read_file(*id).unwrap(),
            want.as_bytes(),
            "file {index} from the first boot no longer holds its own contents"
        );
    }
}
