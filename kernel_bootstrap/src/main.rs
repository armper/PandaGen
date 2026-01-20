#![no_std]
#![no_main]

use core::arch::{asm, global_asm};
use core::fmt::Write;
use core::panic::PanicInfo;
use core::str;
use limine_protocol::structures::memory_map_entry::EntryType;
use limine_protocol::{HHDMRequest, KernelAddressRequest, MemoryMapRequest, Request};

// Provide a small, deterministic stack and jump into Rust.
global_asm!(
    r#"
.section .text.entry, "ax"
.global _start
.extern rust_main
_start:
    lea rsp, [rip + stack_top]
    and rsp, -16
    call rust_main
1:
    hlt
    jmp 1b

.section .bss.stack, "aw", @nobits
.align 16
stack_bottom:
    .skip 65536
stack_top:
"#
);

#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    let mut serial = serial::SerialPort::new(serial::COM1);
    serial.init();
    let _ = writeln!(serial, "PandaGen: kernel_bootstrap online");
    let boot = boot_info(&mut serial);
    let (allocator, heap) = init_memory(&mut serial, &boot);
    let mut kernel = Kernel::new(boot, allocator, heap);
    let _ = writeln!(serial, "Type 'help' for commands.");
    let _ = write!(serial, "> ");

    console_loop(&mut serial, &mut kernel)
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // In this minimal bootstrap kernel we cannot rely on any output device
    // (such as a serial port or VGA text buffer) being initialized yet, so
    // we intentionally ignore the panic information and simply halt.
    // Future work may attempt to log `_info` once basic I/O is available.
    halt_loop()
}

#[inline(always)]
fn halt_loop() -> ! {
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

fn console_loop(serial: &mut serial::SerialPort, kernel: &mut Kernel) -> ! {
    let mut buffer = [0u8; 128];
    let mut len = 0usize;

    loop {
        if let Some(byte) = serial.try_read_byte() {
            match byte {
                b'\r' | b'\n' => {
                    let _ = serial.write_str("\r\n");
                    kernel.enqueue_command(serial, &buffer[..len]);
                    kernel.poll(serial);
                    len = 0;
                    let _ = serial.write_str("> ");
                }
                0x08 | 0x7f => {
                    if len > 0 {
                        len -= 1;
                        let _ = serial.write_str("\x08 \x08");
                    }
                }
                byte => {
                    if len < buffer.len() {
                        buffer[len] = byte;
                        len += 1;
                        let _ = serial.write_byte(byte);
                    }
                }
            }
        } else {
            unsafe {
                asm!("pause", options(nomem, nostack, preserves_flags));
            }
        }
    }
}

fn boot_info(serial: &mut serial::SerialPort) -> BootInfo {
    let mut info = BootInfo::empty();
    unsafe {
        match HHDM_REQUEST.get_response() {
            Some(resp) => {
                info.hhdm_offset = Some(resp.offset as u64);
            }
            None => {
                info.hhdm_offset = None;
            }
        }

        match KERNEL_ADDRESS_REQUEST.get_response() {
            Some(resp) => {
                info.kernel_phys = Some(resp.physical_base);
                info.kernel_virt = Some(resp.virtual_base);
            }
            None => {
                info.kernel_phys = None;
                info.kernel_virt = None;
            }
        }

        if let Some(map) = MEMORY_MAP_REQUEST.get_response().and_then(|resp| resp.get_memory_map())
        {
            let mut usable = 0u64;
            let mut total = 0u64;
            for entry in map {
                total = total.saturating_add(entry.length);
                if entry.kind == EntryType::Usable {
                    usable = usable.saturating_add(entry.length);
                }
            }
            info.mem_entries = map.len();
            info.mem_total_kib = total / 1024;
            info.mem_usable_kib = usable / 1024;
        }
    }

    print_boot_info(serial, &info);
    info
}

fn print_boot_info(serial: &mut serial::SerialPort, info: &BootInfo) {
    match info.hhdm_offset {
        Some(offset) => {
            let _ = writeln!(serial, "hhdm: offset=0x{:x}", offset);
        }
        None => {
            let _ = writeln!(serial, "hhdm: unavailable");
        }
    }

    match (info.kernel_phys, info.kernel_virt) {
        (Some(phys), Some(virt)) => {
            let _ = writeln!(serial, "kernel: phys=0x{:x} virt=0x{:x}", phys, virt);
        }
        _ => {
            let _ = writeln!(serial, "kernel: address unavailable");
        }
    }

    if info.mem_entries > 0 {
        let _ = writeln!(
            serial,
            "memory: entries={} total={} KiB usable={} KiB",
            info.mem_entries,
            info.mem_total_kib,
            info.mem_usable_kib
        );
    } else {
        let _ = writeln!(serial, "memory: map unavailable");
    }
}

fn init_memory(
    serial: &mut serial::SerialPort,
    boot: &BootInfo,
) -> (Option<FrameAllocator>, Option<BumpHeap>) {
    let Some(map) = (unsafe {
        MEMORY_MAP_REQUEST
            .get_response()
            .and_then(|resp| resp.get_memory_map())
    }) else {
        let _ = writeln!(serial, "allocator: unavailable (no memory map)");
        return (None, None);
    };

    let mut allocator = FrameAllocator::new();
    for entry in map {
        if entry.kind == EntryType::Usable {
            allocator.add_range(entry.base, entry.length);
        }
    }
    allocator.reset_cursor();

    let _ = writeln!(
        serial,
        "allocator: ranges={} frames={}",
        allocator.range_count(),
        allocator.total_frames()
    );

    let heap = match boot.hhdm_offset {
        Some(offset) => init_heap(serial, &mut allocator, offset),
        None => {
            let _ = writeln!(serial, "heap: skipped (no hhdm)");
            None
        }
    };

    (Some(allocator), heap)
}

fn init_heap(
    serial: &mut serial::SerialPort,
    allocator: &mut FrameAllocator,
    hhdm_offset: u64,
) -> Option<BumpHeap> {
    const HEAP_PAGES: u64 = 64;
    let Some(phys_base) = allocator.allocate_contiguous(HEAP_PAGES) else {
        let _ = writeln!(serial, "heap: allocation failed");
        return None;
    };
    let size = (HEAP_PAGES * PAGE_SIZE) as usize;
    let virt_base = (hhdm_offset + phys_base) as usize;
    let heap = BumpHeap::new(virt_base, size);
    let _ = writeln!(
        serial,
        "heap: virt=0x{:x} size={} KiB",
        virt_base,
        size / 1024
    );
    Some(heap)
}

#[used]
#[link_section = ".limine_requests"]
static HHDM_REQUEST: Request<HHDMRequest> = HHDMRequest::new().into();

#[used]
#[link_section = ".limine_requests"]
static MEMORY_MAP_REQUEST: Request<MemoryMapRequest> = MemoryMapRequest::new().into();

#[used]
#[link_section = ".limine_requests"]
static KERNEL_ADDRESS_REQUEST: Request<KernelAddressRequest> =
    KernelAddressRequest::new().into();

const PAGE_SIZE: u64 = 4096;
const CHANNEL_CAPACITY: usize = 8;
const COMMAND_MAX: usize = 64;

#[derive(Copy, Clone)]
struct BootInfo {
    hhdm_offset: Option<u64>,
    kernel_phys: Option<u64>,
    kernel_virt: Option<u64>,
    mem_entries: usize,
    mem_total_kib: u64,
    mem_usable_kib: u64,
}

impl BootInfo {
    const fn empty() -> Self {
        Self {
            hhdm_offset: None,
            kernel_phys: None,
            kernel_virt: None,
            mem_entries: 0,
            mem_total_kib: 0,
            mem_usable_kib: 0,
        }
    }
}

struct Kernel {
    boot: BootInfo,
    allocator: Option<FrameAllocator>,
    heap: Option<BumpHeap>,
    channel: Channel,
}

impl Kernel {
    fn new(boot: BootInfo, allocator: Option<FrameAllocator>, heap: Option<BumpHeap>) -> Self {
        Self {
            boot,
            allocator,
            heap,
            channel: Channel::new(),
        }
    }

    fn enqueue_command(&mut self, serial: &mut serial::SerialPort, line: &[u8]) {
        let Some(msg) = Message::from_bytes(line) else {
            let _ = writeln!(serial, "error: command too long");
            return;
        };
        if let Err(ChannelError::Full) = self.channel.send(msg) {
            let _ = writeln!(serial, "error: command queue full");
        }
    }

    fn poll(&mut self, serial: &mut serial::SerialPort) {
        while let Some(msg) = self.channel.recv() {
            self.handle_message(serial, msg);
        }
    }

    fn handle_message(&mut self, serial: &mut serial::SerialPort, msg: Message) {
        if msg.kind != MessageKind::Command {
            return;
        }

        let Some(command) = msg.as_str() else {
            let _ = writeln!(serial, "error: invalid utf-8");
            return;
        };
        let command = command.trim();
        if command.is_empty() {
            return;
        }

        match command {
            "help" => {
                let _ = writeln!(
                    serial,
                    "commands: help, halt, boot, mem, alloc, heap, heap-alloc"
                );
            }
            "halt" => {
                let _ = writeln!(serial, "halting...");
                halt_loop();
            }
            "boot" => {
                print_boot_info(serial, &self.boot);
            }
            "mem" => {
                let _ = writeln!(
                    serial,
                    "memory: entries={} total={} KiB usable={} KiB",
                    self.boot.mem_entries,
                    self.boot.mem_total_kib,
                    self.boot.mem_usable_kib
                );
                if let Some(allocator) = self.allocator.as_ref() {
                    let _ = writeln!(
                        serial,
                        "allocator: ranges={} frames={} next=0x{:x}",
                        allocator.range_count(),
                        allocator.total_frames(),
                        allocator.next_frame()
                    );
                } else {
                    let _ = writeln!(serial, "allocator: unavailable");
                }
            }
            "alloc" => {
                if let Some(allocator) = self.allocator.as_mut() {
                    if let Some(frame) = allocator.allocate_frame() {
                        if let Some(offset) = self.boot.hhdm_offset {
                            let virt = offset + frame;
                            let _ = writeln!(
                                serial,
                                "frame: phys=0x{:x} virt=0x{:x}",
                                frame,
                                virt
                            );
                        } else {
                            let _ = writeln!(serial, "frame: phys=0x{:x}", frame);
                        }
                    } else {
                        let _ = writeln!(serial, "frame: out of memory");
                    }
                } else {
                    let _ = writeln!(serial, "frame: allocator unavailable");
                }
            }
            "heap" => {
                if let Some(heap) = self.heap.as_ref() {
                    let _ = writeln!(
                        serial,
                        "heap: used={} bytes free={} bytes",
                        heap.used(),
                        heap.free()
                    );
                } else {
                    let _ = writeln!(serial, "heap: unavailable");
                }
            }
            "heap-alloc" => {
                if let Some(heap) = self.heap.as_mut() {
                    match heap.alloc(64, 16) {
                        Some(ptr) => {
                            let _ = writeln!(
                                serial,
                                "heap: allocated 64 bytes at 0x{:x}",
                                ptr
                            );
                        }
                        None => {
                            let _ = writeln!(serial, "heap: out of memory");
                        }
                    }
                } else {
                    let _ = writeln!(serial, "heap: unavailable");
                }
            }
            _ => {
                let _ = writeln!(serial, "unknown command: {}", command);
            }
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum MessageKind {
    Command,
}

#[derive(Copy, Clone)]
struct Message {
    kind: MessageKind,
    len: usize,
    data: [u8; COMMAND_MAX],
}

impl Message {
    const fn empty() -> Self {
        Self {
            kind: MessageKind::Command,
            len: 0,
            data: [0; COMMAND_MAX],
        }
    }

    fn from_bytes(line: &[u8]) -> Option<Self> {
        if line.len() > COMMAND_MAX {
            return None;
        }
        let mut msg = Self::empty();
        msg.len = line.len();
        let mut i = 0;
        while i < line.len() {
            msg.data[i] = line[i];
            i += 1;
        }
        Some(msg)
    }

    fn as_str(&self) -> Option<&str> {
        str::from_utf8(&self.data[..self.len]).ok()
    }
}

#[derive(Copy, Clone)]
struct Channel {
    queue: [Message; CHANNEL_CAPACITY],
    head: usize,
    tail: usize,
    full: bool,
}

impl Channel {
    const fn new() -> Self {
        Self {
            queue: [Message::empty(); CHANNEL_CAPACITY],
            head: 0,
            tail: 0,
            full: false,
        }
    }

    fn send(&mut self, msg: Message) -> Result<(), ChannelError> {
        if self.full {
            return Err(ChannelError::Full);
        }
        self.queue[self.tail] = msg;
        self.tail = (self.tail + 1) % CHANNEL_CAPACITY;
        if self.tail == self.head {
            self.full = true;
        }
        Ok(())
    }

    fn recv(&mut self) -> Option<Message> {
        if self.is_empty() {
            return None;
        }
        let msg = self.queue[self.head];
        self.head = (self.head + 1) % CHANNEL_CAPACITY;
        self.full = false;
        Some(msg)
    }

    fn is_empty(&self) -> bool {
        !self.full && self.head == self.tail
    }
}

enum ChannelError {
    Full,
}

#[derive(Copy, Clone)]
struct Range {
    start: u64,
    end: u64,
}

struct FrameAllocator {
    ranges: [Range; 32],
    len: usize,
    current: usize,
    next: u64,
}

impl FrameAllocator {
    const fn new() -> Self {
        Self {
            ranges: [Range { start: 0, end: 0 }; 32],
            len: 0,
            current: 0,
            next: 0,
        }
    }

    fn add_range(&mut self, base: u64, length: u64) {
        let start = align_up(base, PAGE_SIZE);
        let end = align_down(base.saturating_add(length), PAGE_SIZE);
        if end <= start || self.len >= self.ranges.len() {
            return;
        }
        self.ranges[self.len] = Range { start, end };
        self.len += 1;
    }

    fn reset_cursor(&mut self) {
        self.current = 0;
        self.next = if self.len > 0 {
            self.ranges[0].start
        } else {
            0
        };
    }

    fn range_count(&self) -> usize {
        self.len
    }

    fn total_frames(&self) -> u64 {
        let mut total = 0u64;
        let mut i = 0usize;
        while i < self.len {
            let range = self.ranges[i];
            total = total.saturating_add((range.end - range.start) / PAGE_SIZE);
            i += 1;
        }
        total
    }

    fn next_frame(&self) -> u64 {
        self.next
    }

    fn allocate_frame(&mut self) -> Option<u64> {
        self.allocate_contiguous(1)
    }

    fn allocate_contiguous(&mut self, pages: u64) -> Option<u64> {
        if pages == 0 {
            return None;
        }
        let bytes = pages.saturating_mul(PAGE_SIZE);
        while self.current < self.len {
            let range = self.ranges[self.current];
            let start = if self.next < range.start {
                range.start
            } else {
                self.next
            };
            let end = start.saturating_add(bytes);
            if end <= range.end {
                self.next = end;
                return Some(start);
            }
            self.current += 1;
            if self.current < self.len {
                self.next = self.ranges[self.current].start;
            }
        }
        None
    }
}

struct BumpHeap {
    start: usize,
    end: usize,
    next: usize,
}

impl BumpHeap {
    fn new(start: usize, size: usize) -> Self {
        Self {
            start,
            end: start.saturating_add(size),
            next: start,
        }
    }

    fn alloc(&mut self, size: usize, align: usize) -> Option<usize> {
        let aligned = align_up_usize(self.next, align);
        let end = aligned.saturating_add(size);
        if end > self.end {
            return None;
        }
        self.next = end;
        Some(aligned)
    }

    fn used(&self) -> usize {
        self.next.saturating_sub(self.start)
    }

    fn free(&self) -> usize {
        self.end.saturating_sub(self.next)
    }
}

const fn align_up(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    (value + align - 1) / align * align
}

const fn align_down(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    value / align * align
}

fn align_up_usize(value: usize, align: usize) -> usize {
    if align == 0 {
        return value;
    }
    (value + align - 1) / align * align
}

mod serial {
    use core::arch::asm;
    use core::fmt;

    pub const COM1: u16 = 0x3F8;

    pub struct SerialPort {
        base: u16,
    }

    impl SerialPort {
        pub const fn new(base: u16) -> Self {
            Self { base }
        }

        pub fn init(&mut self) {
            unsafe {
                self.outb(1, 0x00);
                self.outb(3, 0x80);
                self.outb(0, 0x01);
                self.outb(1, 0x00);
                self.outb(3, 0x03);
                self.outb(2, 0xC7);
                self.outb(4, 0x0B);
            }
        }

        pub fn write_byte(&mut self, byte: u8) -> fmt::Result {
            while !self.transmit_ready() {
                unsafe {
                    asm!("pause", options(nomem, nostack, preserves_flags));
                }
            }
            unsafe {
                self.outb(0, byte);
            }
            Ok(())
        }

        pub fn try_read_byte(&mut self) -> Option<u8> {
            if self.data_ready() {
                unsafe { Some(self.inb(0)) }
            } else {
                None
            }
        }

        fn data_ready(&mut self) -> bool {
            unsafe { self.inb(5) & 0x01 != 0 }
        }

        fn transmit_ready(&mut self) -> bool {
            unsafe { self.inb(5) & 0x20 != 0 }
        }

        unsafe fn inb(&mut self, offset: u16) -> u8 {
            let port = self.base + offset;
            let value: u8;
            asm!(
                "in al, dx",
                in("dx") port,
                out("al") value,
                options(nomem, nostack, preserves_flags)
            );
            value
        }

        unsafe fn outb(&mut self, offset: u16, value: u8) {
            let port = self.base + offset;
            asm!(
                "out dx, al",
                in("dx") port,
                in("al") value,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    impl fmt::Write for SerialPort {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            for byte in s.bytes() {
                if byte == b'\n' {
                    self.write_byte(b'\r')?;
                }
                self.write_byte(byte)?;
            }
            Ok(())
        }
    }
}
