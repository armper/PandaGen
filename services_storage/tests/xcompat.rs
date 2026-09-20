//! A disk written by an older build must still be readable.
//!
//! Phases 286, 288 and 291 all added fields to the on-disk records. The
//! commit record's checksum is computed over the serialised struct, so a
//! record written before a field existed re-serialises differently under the
//! new build and fails its own checksum -- and recovery throws it away as
//! corrupt. Everything committed since the last checkpoint is lost, silently,
//! the first time a user upgrades.
//!
//! `tests/fixtures/pre288.sparse` is a real image written by the Phase 287
//! sources -- `git show 5b295aa:services_storage/src/block_storage.rs` into a
//! scratch tree, write six files, keep the bytes. It is stored sparsely (a
//! block count, then the non-zero blocks) because the disk is mostly zeroes.
//!
//! It is checked in deliberately. The first version of this test read an
//! image from /tmp and *returned quietly* when it was absent -- and nothing
//! in the tree created it, so on every machine but the one that wrote it,
//! the only end-to-end guard against silent upgrade loss was a no-op that
//! printed a line and passed.

use hal::{BlockDevice, BlockError, BLOCK_SIZE};
use services_storage::{ObjectId, PersistentFilesystem};
use std::cell::RefCell;
use std::rc::Rc;

const ROOT: ObjectId = ObjectId::from_bytes(*b"PANDAGEN-ROOT-01");
const FIXTURE: &[u8] = include_bytes!("fixtures/pre288.sparse");

/// Rebuild the full image from the sparse fixture.
fn pre288_image() -> Vec<u8> {
    let word = |at: usize| u32::from_le_bytes(FIXTURE[at..at + 4].try_into().unwrap()) as usize;
    let blocks = word(0);
    let present = word(4);
    let mut image = vec![0u8; blocks * BLOCK_SIZE];
    let mut at = 8;
    for _ in 0..present {
        let index = word(at);
        at += 4;
        image[index * BLOCK_SIZE..(index + 1) * BLOCK_SIZE]
            .copy_from_slice(&FIXTURE[at..at + BLOCK_SIZE]);
        at += BLOCK_SIZE;
    }
    image
}

#[derive(Clone)]
struct Disk {
    bytes: Rc<RefCell<Vec<u8>>>,
    blocks: u64,
}

impl BlockDevice for Disk {
    fn block_count(&self) -> u64 {
        self.blocks
    }
    fn read_block(&mut self, i: u64, b: &mut [u8]) -> Result<(), BlockError> {
        if i >= self.blocks {
            return Err(BlockError::OutOfBounds);
        }
        let s = i as usize * BLOCK_SIZE;
        b[..BLOCK_SIZE].copy_from_slice(&self.bytes.borrow()[s..s + BLOCK_SIZE]);
        Ok(())
    }
    fn write_block(&mut self, i: u64, b: &[u8]) -> Result<(), BlockError> {
        if i >= self.blocks {
            return Err(BlockError::OutOfBounds);
        }
        let s = i as usize * BLOCK_SIZE;
        self.bytes.borrow_mut()[s..s + BLOCK_SIZE].copy_from_slice(&b[..BLOCK_SIZE]);
        Ok(())
    }
    fn flush(&mut self) -> Result<(), BlockError> {
        Ok(())
    }
}

#[test]
fn a_disk_written_by_an_older_build_keeps_its_files() {
    let bytes = pre288_image();
    let blocks = (bytes.len() / BLOCK_SIZE) as u64;
    let disk = Disk {
        bytes: Rc::new(RefCell::new(bytes)),
        blocks,
    };

    let mut fs = PersistentFilesystem::open(disk, ROOT).expect("the old disk must mount");
    let names: Vec<String> = fs.list(ROOT).unwrap().into_iter().map(|(n, _)| n).collect();

    let mut missing = Vec::new();
    for index in 0..6 {
        let want = format!("f{index}.txt");
        if !names.contains(&want) {
            missing.push(want);
        }
    }
    assert!(
        missing.is_empty(),
        "{} of 6 files written by the older build are gone after the upgrade: {:?} \
         (the directory holds {:?})",
        missing.len(),
        missing,
        names
    );

    for (name, entry) in fs.list(ROOT).unwrap() {
        let index: usize = name
            .trim_start_matches('f')
            .trim_end_matches(".txt")
            .parse()
            .unwrap();
        assert_eq!(
            fs.read_file(entry.object_id).unwrap(),
            format!("contents {index}").into_bytes(),
            "{name} came back holding something else"
        );
    }
}
