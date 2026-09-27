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
//! Programs come from images (PROC-004): `program_image`'s format,
//! built from Rust against `pandagen_app` and shipped with the machine as
//! boot modules. An image says where each piece of memory goes, whether
//! it may be written or run (never both), and what the program asks
//! for; the loader checks all of it before anything is mapped, lays the
//! pieces out in a fresh address space with a stack below `STACK_TOP`,
//! and grants what was asked, as handles in the order asked.
//!
//! A few programs are built in as well -- short machine code that breaks
//! the rules on purpose, to show what happens when a program does.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use hal_x86_64::mmio_map::PhysMemory;
use hal_x86_64::user_space::{Access, Frames, UserSpace};
use program_image::{Ask, Image};

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

/// A program that has started.
pub struct Started {
    pub id: u32,
    pub name: &'static str,
    /// It asked for a card: the desk opens one for it.
    pub wants_card: bool,
}

/// Images the machine booted with, by name. Registered once, at boot.
static IMAGES: hal_x86_64::SpinLock<Vec<(&'static str, &'static [u8])>> =
    hal_x86_64::SpinLock::new(Vec::new());

/// A program image the machine booted with, named `name`.
pub fn add_image(name: &str, bytes: &'static [u8]) {
    if name.is_empty() {
        return;
    }
    // Thread names are `&'static str`; an image's name lives as long as
    // the machine does, like the image.
    let name: &'static str = alloc::boxed::Box::leak(String::from(name).into_boxed_str());
    IMAGES.lock().push((name, bytes));
}

pub fn image_names() -> Vec<&'static str> {
    IMAGES.lock().iter().map(|(name, _)| *name).collect()
}

fn image_named(name: &str) -> Option<(&'static str, &'static [u8])> {
    IMAGES.lock().iter().find(|(n, _)| *n == name).copied()
}

/// What `programs` shows: the images, what each asks for and how much
/// memory it lays out, then the built-in programs.
pub fn catalog() -> Vec<String> {
    let mut out = Vec::new();
    for (name, bytes) in IMAGES.lock().iter() {
        out.push(match Image::parse(bytes) {
            Ok(image) => {
                let asks: Vec<String> = image.asks.iter().map(|a| a.describe()).collect();
                alloc::format!(
                    "  {name:<8} an image: {} KiB, asks for {}",
                    image.memory() / 1024,
                    if asks.is_empty() {
                        String::from("nothing")
                    } else {
                        asks.join(", ")
                    }
                )
            }
            Err(why) => alloc::format!("  {name:<8} a damaged image ({why:?})"),
        });
    }
    for (name, what) in CATALOG {
        out.push(alloc::format!("  {name:<8} built in: {what}"));
    }
    out
}

/// The built-in programs, by name, with what each shows.
pub const CATALOG: &[(&str, &str)] = &[
    (
        "probe",
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
            "probe" => (
                "probe",
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

/// The images the machine booted with, to be installed on its disk
/// (PROC-010).
pub fn boot_images() -> Vec<(&'static str, &'static [u8])> {
    IMAGES.lock().clone()
}

/// Names that last as long as the machine: a thread's name is
/// `&'static str`, and a program run again reuses its name.
static NAMES: hal_x86_64::SpinLock<Vec<&'static str>> = hal_x86_64::SpinLock::new(Vec::new());

fn intern(name: &str) -> &'static str {
    let mut names = NAMES.lock();
    if let Some(known) = names.iter().find(|n| **n == name) {
        return known;
    }
    let kept: &'static str = alloc::boxed::Box::leak(String::from(name).into_boxed_str());
    names.push(kept);
    kept
}

/// Start the program `name` (PROC-010): from its image on disk
/// (`installed`, read by the caller, who has the filesystem), else from
/// the boot image, else a built-in program. Its thread id, or why not.
pub fn run(
    name: &str,
    installed: Option<&[u8]>,
    hhdm: u64,
    frames: &mut crate::FrameAllocator,
) -> Result<Started, alloc::string::String> {
    // An image, else a built-in program: the image's pieces and asks, or
    // the built-in's code and the console.
    let image_bytes = installed.or_else(|| image_named(name).map(|(_, bytes)| bytes));
    let (name, image) = match image_bytes {
        Some(bytes) => {
            let image = Image::parse(bytes)
                .map_err(|why| alloc::format!("run: {name} is damaged ({why:?})"))?;
            (intern(name), image)
        }
        None => {
            let Some((name, code)) = code_of(name) else {
                return Err(alloc::format!("run: no program {name:?} (try `programs`)"));
            };
            let image = Image {
                name: String::from(name),
                entry: CODE_AT,
                asks: alloc::vec![Ask::Console],
                pieces: alloc::vec![program_image::Piece {
                    at: CODE_AT,
                    size: (code.len() as u64).max(1),
                    access: program_image::EXECUTE,
                    bytes: code.to_vec(),
                }],
            };
            (name, image)
        }
    };
    // Its limits (PROC-007): it must fit before anything is mapped.
    let limits = crate::supervision::PROGRAM_LIMITS;
    limits.check_memory(name, image.memory(), STACK_PAGES * 4096)?;
    let mut memory = Owned::new(hhdm, frames);
    let mut space = UserSpace::new(
        &mut memory,
        crate::threads::kernel_cr3(),
        crate::threads::nx(),
    )
    .map_err(|_| alloc::string::String::from("run: out of memory"))?;
    let loaded = image
        .pieces
        .iter()
        .try_for_each(|piece| {
            let access = Access {
                write: piece.access & program_image::WRITE != 0,
                execute: piece.access & program_image::EXECUTE != 0,
            };
            space.lay_out(&mut memory, piece.at, piece.size, &piece.bytes, access)
        })
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
    // What it asked for, in the order asked: all of it, for now -- the
    // console, and a card on the desk (which the desk opens for it).
    let mut handles = Handles::new();
    for ask in &image.asks {
        match ask {
            Ask::Console => handles.grant(Capability::Console),
            Ask::Card => handles.grant(Capability::Card),
            Ask::Notices => handles.grant(Capability::Notices),
            Ask::Documents(pattern, rights) => {
                handles.grant(Capability::Documents(*pattern, *rights))
            }
        };
    }
    let wants_card = image.asks.contains(&Ask::Card);
    match crate::threads::spawn_program(name, space, handles, image.entry, STACK_TOP, limits.cpu) {
        Ok(id) => Ok(Started {
            id,
            name,
            wants_card,
        }),
        Err((why, space)) => {
            space.free(&mut memory);
            Err(alloc::string::String::from(why))
        }
    }
}
