//! First-fit free-list heap with coalescing.
//!
//! The kernel's original global allocator was a bump allocator: fast, but it
//! never returned memory, and the graphical desktop rebuilds its model every
//! frame (about 58 KiB and a thousand allocations per pointer move). This
//! heap frees. It is intentionally simple: an address-ordered singly linked
//! free list, first fit, split on allocate, coalesce with both neighbours on
//! free, 16-byte granularity. Single-CPU only (no SMP yet), like the rest of
//! `kernel_bootstrap`.
//!
//! Layout of an allocated block: `[Header][pad?][payload]`, where the header
//! sits immediately before the payload and records the whole block's size
//! and how far the block start is behind the header, so `dealloc` can
//! recover the block from the payload pointer alone.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr;

/// Allocation granularity; every block boundary is a multiple of this.
pub const GRANULE: usize = 16;
const HEADER: usize = core::mem::size_of::<Header>();
const FREE_NODE: usize = core::mem::size_of::<Free>();
/// Smallest block worth keeping on the free list.
const MIN_BLOCK: usize = 2 * GRANULE;

#[repr(C)]
struct Header {
    /// Total block size from block start, including pad and header.
    size: usize,
    /// Bytes from block start to this header.
    pad: usize,
}

#[repr(C)]
struct Free {
    size: usize,
    next: *mut Free,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeapStats {
    pub used: usize,
    pub free: usize,
    pub total: usize,
    pub allocations: u64,
    pub frees: u64,
    pub largest_free: usize,
    pub free_blocks: usize,
}

struct Inner {
    start: usize,
    end: usize,
    head: *mut Free,
    used: usize,
    allocations: u64,
    frees: u64,
}

/// Global-allocator-capable free-list heap.
pub struct FreeListHeap {
    inner: UnsafeCell<Inner>,
}

// SAFETY: kernel_bootstrap runs on a single CPU with no preemption inside
// allocator calls (interrupt handlers do not allocate).
unsafe impl Sync for FreeListHeap {}

const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

impl FreeListHeap {
    /// An uninitialised heap; every allocation fails until `init`.
    pub const fn empty() -> Self {
        Self {
            inner: UnsafeCell::new(Inner {
                start: 0,
                end: 0,
                head: ptr::null_mut(),
                used: 0,
                allocations: 0,
                frees: 0,
            }),
        }
    }

    /// Adopt `[start, start + size)` as the arena. The region must be
    /// writable, unused, and outlive the heap.
    ///
    /// # Safety
    /// Caller guarantees exclusive ownership of the memory range.
    pub unsafe fn init(&self, start: usize, size: usize) {
        let inner = &mut *self.inner.get();
        let start = align_up(start, GRANULE);
        let end = (start + size) & !(GRANULE - 1);
        inner.start = start;
        inner.end = end;
        inner.used = 0;
        inner.allocations = 0;
        inner.frees = 0;
        if end > start + MIN_BLOCK {
            let block = start as *mut Free;
            (*block).size = end - start;
            (*block).next = ptr::null_mut();
            inner.head = block;
        } else {
            inner.head = ptr::null_mut();
        }
    }

    pub fn stats(&self) -> HeapStats {
        // SAFETY: single-CPU read of the free list.
        unsafe {
            let inner = &*self.inner.get();
            let mut free = 0;
            let mut largest = 0;
            let mut blocks = 0;
            let mut cur = inner.head;
            while !cur.is_null() {
                let size = (*cur).size;
                free += size;
                largest = largest.max(size);
                blocks += 1;
                cur = (*cur).next;
            }
            HeapStats {
                used: inner.used,
                free,
                total: inner.end.saturating_sub(inner.start),
                allocations: inner.allocations,
                frees: inner.frees,
                largest_free: largest,
                free_blocks: blocks,
            }
        }
    }

    unsafe fn alloc_inner(&self, layout: Layout) -> *mut u8 {
        let inner = &mut *self.inner.get();
        let align = layout.align().max(GRANULE);
        let size = align_up(layout.size().max(1), GRANULE);

        let mut prev: *mut Free = ptr::null_mut();
        let mut cur = inner.head;
        while !cur.is_null() {
            let block_start = cur as usize;
            let block_size = (*cur).size;
            let block_end = block_start + block_size;
            let payload = align_up(block_start + HEADER, align);
            let needed_end = payload + size;
            if needed_end <= block_end {
                let next = (*cur).next;
                // Front fragment: keep it free if it is a usable block.
                let front = payload - HEADER - block_start;
                let alloc_start = if front >= MIN_BLOCK {
                    (*cur).size = front;
                    prev = cur;
                    block_start + front
                } else {
                    // Absorb the fragment into the allocation; unlink block.
                    if prev.is_null() {
                        inner.head = next;
                    } else {
                        (*prev).next = next;
                    }
                    block_start
                };
                // Tail fragment: split off if usable.
                let mut alloc_end = block_end;
                let tail = block_end - needed_end;
                if tail >= MIN_BLOCK {
                    alloc_end = needed_end;
                    let tail_block = needed_end as *mut Free;
                    (*tail_block).size = tail;
                    (*tail_block).next = next;
                    if prev.is_null() {
                        inner.head = tail_block;
                    } else {
                        (*prev).next = tail_block;
                    }
                } else if front >= MIN_BLOCK {
                    // Front kept, no tail: front block links to next.
                    (*prev).next = next;
                }
                let header = (payload - HEADER) as *mut Header;
                (*header).size = alloc_end - alloc_start;
                (*header).pad = payload - HEADER - alloc_start;
                inner.used += alloc_end - alloc_start;
                inner.allocations += 1;
                return payload as *mut u8;
            }
            prev = cur;
            cur = (*cur).next;
        }
        ptr::null_mut()
    }

    unsafe fn dealloc_inner(&self, payload: *mut u8) {
        let inner = &mut *self.inner.get();
        let header = (payload as usize - HEADER) as *mut Header;
        let block_start = payload as usize - HEADER - (*header).pad;
        let block_size = (*header).size;
        inner.used -= block_size;
        inner.frees += 1;

        // Insert in address order and coalesce.
        let block = block_start as *mut Free;
        (*block).size = block_size;
        let mut prev: *mut Free = ptr::null_mut();
        let mut cur = inner.head;
        while !cur.is_null() && (cur as usize) < block_start {
            prev = cur;
            cur = (*cur).next;
        }
        (*block).next = cur;
        if prev.is_null() {
            inner.head = block;
        } else {
            (*prev).next = block;
        }
        // Merge with following block.
        if !cur.is_null() && block_start + (*block).size == cur as usize {
            (*block).size += (*cur).size;
            (*block).next = (*cur).next;
        }
        // Merge with preceding block.
        if !prev.is_null() && prev as usize + (*prev).size == block_start {
            (*prev).size += (*block).size;
            (*prev).next = (*block).next;
        }
    }
}

unsafe impl GlobalAlloc for FreeListHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.alloc_inner(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if !ptr.is_null() {
            self.dealloc_inner(ptr);
        }
    }
}

const _: () = assert!(HEADER == GRANULE && FREE_NODE == GRANULE);

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    struct Arena {
        _storage: Vec<u128>,
        heap: FreeListHeap,
        size: usize,
    }

    fn arena(size: usize) -> Arena {
        let mut storage = vec![0u128; size / 16 + 2];
        let heap = FreeListHeap::empty();
        let start = storage.as_mut_ptr() as usize;
        unsafe { heap.init(start, size) };
        Arena {
            _storage: storage,
            heap,
            size,
        }
    }

    unsafe fn alloc(heap: &FreeListHeap, size: usize, align: usize) -> *mut u8 {
        heap.alloc(Layout::from_size_align(size, align).unwrap())
    }

    unsafe fn free(heap: &FreeListHeap, p: *mut u8, size: usize, align: usize) {
        heap.dealloc(p, Layout::from_size_align(size, align).unwrap())
    }

    #[test]
    fn test_alloc_free_and_full_coalescing() {
        let a = arena(4096);
        let stats = a.heap.stats();
        assert_eq!(stats.total, stats.free);
        assert_eq!(stats.free_blocks, 1);
        unsafe {
            let p1 = alloc(&a.heap, 100, 8);
            let p2 = alloc(&a.heap, 200, 8);
            let p3 = alloc(&a.heap, 300, 8);
            assert!(!p1.is_null() && !p2.is_null() && !p3.is_null());
            assert!(p1 < p2 && p2 < p3);
            // Writing the whole payload must not corrupt neighbours.
            ptr::write_bytes(p1, 0xAA, 100);
            ptr::write_bytes(p2, 0xBB, 200);
            ptr::write_bytes(p3, 0xCC, 300);
            assert_eq!(*p2, 0xBB);
            assert_eq!(*p2.add(199), 0xBB);
            let used_before = a.heap.stats().used;
            assert!(used_before >= 600);

            free(&a.heap, p2, 200, 8);
            let s = a.heap.stats();
            assert_eq!(s.free_blocks, 2, "middle hole plus tail");
            free(&a.heap, p1, 100, 8);
            assert_eq!(a.heap.stats().free_blocks, 2, "p1 merged into the hole");
            free(&a.heap, p3, 300, 8);
            let s = a.heap.stats();
            assert_eq!(s.free_blocks, 1, "everything coalesced back");
            assert_eq!(s.free, s.total);
            assert_eq!(s.used, 0);
            assert_eq!(s.allocations, 3);
            assert_eq!(s.frees, 3);
            assert_eq!(s.largest_free, s.total);
        }
        let _ = a.size;
    }

    #[test]
    fn test_reuse_after_free_keeps_heap_bounded() {
        let a = arena(64 * 1024);
        unsafe {
            let baseline = a.heap.stats().used;
            for _ in 0..10_000 {
                let p = alloc(&a.heap, 1000, 8);
                assert!(!p.is_null());
                let q = alloc(&a.heap, 50, 8);
                assert!(!q.is_null());
                free(&a.heap, p, 1000, 8);
                free(&a.heap, q, 50, 8);
            }
            assert_eq!(a.heap.stats().used, baseline);
            assert_eq!(a.heap.stats().free_blocks, 1);
        }
    }

    #[test]
    fn test_alignment_is_honoured_and_blocks_are_recoverable() {
        let a = arena(64 * 1024);
        unsafe {
            let small = alloc(&a.heap, 24, 8);
            let big_align = alloc(&a.heap, 100, 4096);
            assert_eq!(big_align as usize % 4096, 0);
            let another = alloc(&a.heap, 10, 64);
            assert_eq!(another as usize % 64, 0);
            ptr::write_bytes(big_align, 0x11, 100);
            free(&a.heap, big_align, 100, 4096);
            free(&a.heap, small, 24, 8);
            free(&a.heap, another, 10, 64);
            let s = a.heap.stats();
            assert_eq!(s.used, 0);
            assert_eq!(s.free_blocks, 1);
            assert_eq!(s.free, s.total);
        }
    }

    #[test]
    fn test_exhaustion_returns_null_and_recovers() {
        let a = arena(4096);
        unsafe {
            let mut blocks = Vec::new();
            loop {
                let p = alloc(&a.heap, 256, 16);
                if p.is_null() {
                    break;
                }
                blocks.push(p);
            }
            assert!(blocks.len() >= 10 && blocks.len() <= 16, "{}", blocks.len());
            assert!(alloc(&a.heap, 1, 1).is_null() || a.heap.stats().largest_free >= 32);
            for p in blocks {
                free(&a.heap, p, 256, 16);
            }
            assert_eq!(a.heap.stats().free_blocks, 1);
            assert!(!alloc(&a.heap, 3000, 16).is_null());
        }
    }

    #[test]
    fn test_empty_heap_allocates_nothing() {
        let heap = FreeListHeap::empty();
        unsafe {
            assert!(alloc(&heap, 16, 16).is_null());
        }
        assert_eq!(heap.stats(), HeapStats::default());
    }
}
