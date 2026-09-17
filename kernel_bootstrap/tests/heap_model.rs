//! Model-based checking of the kernel's free-list heap
//! (`kernel_bootstrap_lib::free_list_heap::FreeListHeap`).
//!
//! Random allocate/free sequences (fixed-seed xorshift) and systematic
//! free-order permutations are run against a host arena, checking after
//! every operation:
//!
//! - H1 alignment: every returned pointer honours the requested alignment;
//! - H2 containment: every live block lies inside the arena;
//! - H3 exclusivity: live blocks never overlap;
//! - H4 integrity: every live block still holds the byte pattern written
//!   into it (no metadata write ever lands inside a live block);
//! - H5 stats: `used + free == total`, `largest_free <= free`, counters
//!   match the operations that succeeded, and `free_blocks == 0` exactly
//!   when `free == 0`;
//! - H6 coalescing: once everything is freed the heap is one free block of
//!   the full size, regardless of the order of frees;
//! - H7 availability: a request that fits in the largest free block with
//!   room for the allocator's own overhead never fails.

use core::alloc::{GlobalAlloc, Layout};
use kernel_bootstrap_lib::free_list_heap::{FreeListHeap, GRANULE};

const ARENA_BYTES: usize = 8192;

struct Live {
    ptr: usize,
    size: usize,
    align: usize,
    fill: u8,
}

struct Arena {
    _backing: Vec<u8>,
    start: usize,
    end: usize,
    heap: FreeListHeap,
    live: Vec<Live>,
    allocations: u64,
    frees: u64,
    trace: Vec<String>,
}

impl Arena {
    fn new() -> Self {
        // Over-allocate so the arena can be granule-aligned inside it.
        let backing = vec![0u8; ARENA_BYTES + 64];
        let raw = backing.as_ptr() as usize;
        let start = (raw + GRANULE - 1) & !(GRANULE - 1);
        let heap = FreeListHeap::empty();
        // SAFETY: the backing vector outlives the heap and nothing else uses it.
        unsafe { heap.init(start, ARENA_BYTES) };
        let total = heap.stats().total;
        Self {
            _backing: backing,
            start,
            end: start + total,
            heap,
            live: Vec::new(),
            allocations: 0,
            frees: 0,
            trace: Vec::new(),
        }
    }

    fn total(&self) -> usize {
        self.end - self.start
    }

    fn alloc(&mut self, size: usize, align: usize, fill: u8) -> bool {
        self.trace.push(format!("alloc({size}, {align})"));
        let largest_before = self.heap.stats().largest_free;
        let layout = Layout::from_size_align(size, align).unwrap();
        let ptr = unsafe { self.heap.alloc(layout) };
        if ptr.is_null() {
            // H7: with room for a header, padding, and a remainder block,
            // first fit must have succeeded.
            let needed = size + align + 3 * GRANULE;
            assert!(
                largest_before < needed,
                "H7 null with largest_free={largest_before} for {size}/{align}\ntrace: {:?}",
                self.trace
            );
            return false;
        }
        let addr = ptr as usize;
        assert_eq!(addr % align, 0, "H1 alignment\ntrace: {:?}", self.trace);
        assert!(
            addr >= self.start && addr + size <= self.end,
            "H2 containment\ntrace: {:?}",
            self.trace
        );
        for other in &self.live {
            let disjoint = addr + size <= other.ptr || other.ptr + other.size <= addr;
            assert!(
                disjoint,
                "H3 overlap with live block\ntrace: {:?}",
                self.trace
            );
        }
        unsafe { core::ptr::write_bytes(ptr, fill, size) };
        self.live.push(Live {
            ptr: addr,
            size,
            align,
            fill,
        });
        self.allocations += 1;
        self.check();
        true
    }

    fn free(&mut self, index: usize) {
        let block = self.live.swap_remove(index);
        self.trace
            .push(format!("free({}, {})", block.size, block.align));
        // The block must still hold its pattern right before release.
        self.verify(&block);
        let layout = Layout::from_size_align(block.size, block.align).unwrap();
        unsafe { self.heap.dealloc(block.ptr as *mut u8, layout) };
        self.frees += 1;
        self.check();
    }

    fn verify(&self, block: &Live) {
        let bytes = unsafe { core::slice::from_raw_parts(block.ptr as *const u8, block.size) };
        assert!(
            bytes.iter().all(|&b| b == block.fill),
            "H4 block at {:#x} ({} bytes) corrupted\ntrace: {:?}",
            block.ptr,
            block.size,
            self.trace
        );
    }

    fn check(&self) {
        for block in &self.live {
            self.verify(block);
        }
        let stats = self.heap.stats();
        assert_eq!(
            stats.total,
            self.total(),
            "H5 total\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            stats.used + stats.free,
            stats.total,
            "H5 used + free\ntrace: {:?}",
            self.trace
        );
        assert!(
            stats.largest_free <= stats.free,
            "H5 largest_free\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            stats.free_blocks == 0,
            stats.free == 0,
            "H5 free_blocks vs free\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            stats.allocations, self.allocations,
            "H5 allocations\ntrace: {:?}",
            self.trace
        );
        assert_eq!(stats.frees, self.frees, "H5 frees\ntrace: {:?}", self.trace);
        let live_bytes: usize = self.live.iter().map(|b| b.size).sum();
        assert!(
            stats.used >= live_bytes,
            "H5 used {} below live payload {live_bytes}\ntrace: {:?}",
            stats.used,
            self.trace
        );
        if self.live.is_empty() {
            assert_eq!(
                stats.used, 0,
                "H6 used after freeing all\ntrace: {:?}",
                self.trace
            );
            assert_eq!(
                stats.free_blocks, 1,
                "H6 coalesced to one block\ntrace: {:?}",
                self.trace
            );
            assert_eq!(
                stats.largest_free, stats.total,
                "H6 largest_free\ntrace: {:?}",
                self.trace
            );
        }
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[test]
fn random_alloc_free_sequences() {
    const ALIGNS: [usize; 5] = [1, 4, 8, 16, 64];
    for seed in 1..=400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut arena = Arena::new();
        for step in 0..200u64 {
            let bias = rng.below(10);
            if arena.live.is_empty() || (bias < 6 && arena.live.len() < 64) {
                let size = 1 + rng.below(300) as usize;
                let align = ALIGNS[rng.below(ALIGNS.len() as u64) as usize];
                arena.alloc(size, align, (step % 251) as u8 + 1);
            } else {
                let index = rng.below(arena.live.len() as u64) as usize;
                arena.free(index);
            }
        }
        while !arena.live.is_empty() {
            let last = arena.live.len() - 1;
            arena.free(last);
        }
    }
}

#[test]
fn every_free_order_coalesces_fully() {
    // Four blocks of different sizes and alignments, freed in all 24 orders.
    let sizes = [24usize, 100, 7, 300];
    let aligns = [16usize, 8, 64, 1];
    let mut order = [0usize, 1, 2, 3];
    fn permutations(k: usize, order: &mut [usize; 4], out: &mut Vec<[usize; 4]>) {
        if k == 1 {
            out.push(*order);
            return;
        }
        permutations(k - 1, order, out);
        for i in 0..k - 1 {
            if k % 2 == 0 {
                order.swap(i, k - 1);
            } else {
                order.swap(0, k - 1);
            }
            permutations(k - 1, order, out);
        }
    }
    let mut orders = Vec::new();
    permutations(4, &mut order, &mut orders);
    assert_eq!(orders.len(), 24);
    for order in orders {
        let mut arena = Arena::new();
        for i in 0..4 {
            assert!(arena.alloc(sizes[i], aligns[i], 0x40 + i as u8));
        }
        // Free by original index: locate each block by its fill byte.
        for &i in &order {
            let fill = 0x40 + i as u8;
            let index = arena.live.iter().position(|b| b.fill == fill).unwrap();
            arena.free(index);
        }
    }
}

#[test]
fn exhaustion_then_release_restores_the_whole_arena() {
    let mut arena = Arena::new();
    let mut n = 0u8;
    while arena.alloc(200, 16, n.wrapping_add(1)) {
        n = n.wrapping_add(1);
        assert!(n < 200, "arena never exhausted");
    }
    assert!(arena.heap.stats().largest_free < 200 + 16 + 3 * GRANULE);
    while !arena.live.is_empty() {
        let mid = arena.live.len() / 2;
        arena.free(mid);
    }
}
