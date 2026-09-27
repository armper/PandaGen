//! Programs (PROC-002): code that runs in ring 3, in an address space of
//! its own, and reaches the kernel only through system calls checked
//! against the capabilities it was given.
//!
//! Until now everything on the machine was the kernel: every card, every
//! command, compiled in and running with the kernel's authority, so one
//! mistake anywhere could take down everything. A program here cannot
//! touch the kernel's memory (its pages are not marked USER), cannot
//! mask interrupts or touch a port (ring 3, no I/O permission), and
//! cannot name anything it was not handed. When it breaks a rule the CPU
//! stops it, the kernel ends it, and the machine carries on.
//!
//! The programs themselves are, for now, a few built in -- short machine
//! code, loaded like any program would be: copied into fresh pages at
//! `CODE_AT`, with a stack below `STACK_TOP`. A program format and a
//! loader for programs from disk come next.

extern crate alloc;

use hal_x86_64::mmio_map::PhysMemory;
use hal_x86_64::user_space::{Access, Frames, UserSpace};

use crate::syscall_abi::{Capability, Handles};

/// Where a program's code is loaded.
pub const CODE_AT: u64 = 0x40_0000;
/// The top of a program's stack (it grows down from here).
pub const STACK_TOP: u64 = 0x80_0000;
/// A program's stack, in pages.
pub const STACK_PAGES: u64 = 4;

/// Physical memory through the direct map, for reading a program's
/// pages from a system call: allocates nothing, frees nothing.
pub struct Direct {
    hhdm: u64,
}

impl Direct {
    pub fn new(hhdm: u64) -> Self {
        Self { hhdm }
    }
}

impl PhysMemory for Direct {
    fn read_entry(&self, table: u64, index: usize) -> u64 {
        // SAFETY: page tables lie in memory the direct map covers.
        unsafe { core::ptr::read_volatile(((self.hhdm + table) as *const u64).add(index)) }
    }
    fn write_entry(&mut self, table: u64, index: usize, value: u64) {
        // SAFETY: as above.
        unsafe { core::ptr::write_volatile(((self.hhdm + table) as *mut u64).add(index), value) }
    }
    fn alloc_table(&mut self) -> Option<u64> {
        None
    }
}

impl Frames for Direct {
    fn free_frame(&mut self, _frame: u64) {}
    fn read_bytes(&self, frame: u64, offset: usize, out: &mut [u8]) {
        // SAFETY: a program's frame, covered by the direct map.
        unsafe {
            core::ptr::copy_nonoverlapping(
                (self.hhdm + frame + offset as u64) as *const u8,
                out.as_mut_ptr(),
                out.len(),
            )
        }
    }
    fn write_bytes(&mut self, frame: u64, offset: usize, data: &[u8]) {
        // SAFETY: as above.
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                (self.hhdm + frame + offset as u64) as *mut u8,
                data.len(),
            )
        }
    }
}

/// Physical memory through the direct map with the frame allocator
/// behind it: for making and freeing address spaces.
pub struct Owned<'a> {
    direct: Direct,
    frames: &'a mut crate::FrameAllocator,
}

impl<'a> Owned<'a> {
    pub fn new(hhdm: u64, frames: &'a mut crate::FrameAllocator) -> Self {
        Self {
            direct: Direct::new(hhdm),
            frames,
        }
    }
}

impl PhysMemory for Owned<'_> {
    fn read_entry(&self, table: u64, index: usize) -> u64 {
        self.direct.read_entry(table, index)
    }
    fn write_entry(&mut self, table: u64, index: usize, value: u64) {
        self.direct.write_entry(table, index, value)
    }
    fn alloc_table(&mut self) -> Option<u64> {
        let frame = self.frames.allocate_frame()?;
        // SAFETY: a fresh frame, covered by the direct map.
        unsafe { core::ptr::write_bytes((self.direct.hhdm + frame) as *mut u8, 0, 4096) };
        Some(frame)
    }
}

impl Frames for Owned<'_> {
    fn free_frame(&mut self, frame: u64) {
        self.frames.free_frame(frame);
    }
    fn read_bytes(&self, frame: u64, offset: usize, out: &mut [u8]) {
        self.direct.read_bytes(frame, offset, out)
    }
    fn write_bytes(&mut self, frame: u64, offset: usize, data: &[u8]) {
        self.direct.write_bytes(frame, offset, data)
    }
}

/// The built-in programs, by name, with what each shows.
pub const CATALOG: &[(&str, &str)] = &[
    (
        "hello",
        "says hello three times; tries a handle and a pointer it was not given",
    ),
    ("crash", "writes to the kernel's memory"),
    ("rogue", "tries to turn interrupts off"),
    ("hog", "loops forever; `stop` ends it"),
];

#[cfg(target_os = "none")]
extern "C" {
    static user_hello_start: u8;
    static user_hello_end: u8;
    static user_crash_start: u8;
    static user_crash_end: u8;
    static user_rogue_start: u8;
    static user_rogue_end: u8;
    static user_hog_start: u8;
    static user_hog_end: u8;
}

/// A built-in program's name and bytes.
#[cfg(target_os = "none")]
fn code_of(name: &str) -> Option<(&'static str, &'static [u8])> {
    // SAFETY: each pair of symbols brackets one program's bytes in the
    // kernel's read-only data.
    unsafe {
        let span = |start: *const u8, end: *const u8| {
            core::slice::from_raw_parts(start, end as usize - start as usize)
        };
        use core::ptr::addr_of;
        Some(match name {
            "hello" => (
                "hello",
                span(addr_of!(user_hello_start), addr_of!(user_hello_end)),
            ),
            "crash" => (
                "crash",
                span(addr_of!(user_crash_start), addr_of!(user_crash_end)),
            ),
            "rogue" => (
                "rogue",
                span(addr_of!(user_rogue_start), addr_of!(user_rogue_end)),
            ),
            "hog" => (
                "hog",
                span(addr_of!(user_hog_start), addr_of!(user_hog_end)),
            ),
            _ => return None,
        })
    }
}

/// Programs are machine code for the machine; a host build has none.
#[cfg(not(target_os = "none"))]
fn code_of(_name: &str) -> Option<(&'static str, &'static [u8])> {
    None
}

/// Start the program `name`, holding one capability: lines to the
/// Terminal, as handle 0. Its thread id, or why not.
pub fn run(
    name: &str,
    hhdm: u64,
    frames: &mut crate::FrameAllocator,
) -> Result<u32, alloc::string::String> {
    let Some((name, code)) = code_of(name) else {
        return Err(alloc::format!("run: no program {name:?} (try `programs`)"));
    };
    let mut memory = Owned::new(hhdm, frames);
    let mut space = UserSpace::new(
        &mut memory,
        crate::threads::kernel_cr3(),
        crate::threads::nx(),
    )
    .map_err(|_| alloc::string::String::from("run: out of memory"))?;
    let loaded = space
        .load(&mut memory, CODE_AT, code, Access::CODE)
        .and_then(|()| {
            (1..=STACK_PAGES).try_for_each(|page| {
                space
                    .map_new(&mut memory, STACK_TOP - page * 4096, Access::DATA)
                    .map(|_| ())
            })
        });
    if loaded.is_err() {
        space.free(&mut memory);
        return Err(alloc::string::String::from("run: out of memory"));
    }
    let mut handles = Handles::new();
    handles.grant(Capability::Console);
    match crate::threads::spawn_program(name, space, handles, CODE_AT, STACK_TOP) {
        Ok(id) => Ok(id),
        Err((why, space)) => {
            space.free(&mut memory);
            Err(alloc::string::String::from(why))
        }
    }
}
