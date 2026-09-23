#![cfg_attr(all(not(test), target_os = "none"), no_std)]
#![cfg_attr(all(not(test), target_os = "none"), no_main)]
#![cfg_attr(all(not(test), target_os = "none"), feature(alloc_error_handler))]
// Allow unused code - this is infrastructure for future phases
#![allow(dead_code)]
// Allow manual div_ceil - explicit for readability in no_std
#![allow(clippy::manual_div_ceil)]
// Allow manual is_multiple_of - explicit for readability
#![allow(clippy::manual_is_multiple_of)]
// Allow large enum variants - this is a boot kernel
#![allow(clippy::large_enum_variant)]

#[cfg(test)]
extern crate std;

#[cfg(not(test))]
extern crate alloc;

mod bare_metal_editor_io;
#[cfg(all(not(test), target_os = "none"))]
mod bare_metal_net;
mod bare_metal_storage;
mod desk;
mod desktop_frame;
mod display_mode;
mod display_sink;
mod framebuffer;
mod free_list_heap;
mod minimal_editor;
mod notepad;
mod optimized_render;
mod output;
mod palette_overlay;
mod present_policy;
mod render_stats;
mod rtc;
mod vga;
mod workspace;

#[cfg(not(test))]
#[cfg(debug_assertions)]
use crate::minimal_editor::EditorMode;
#[cfg(not(test))]
use core::arch::asm;
#[cfg(all(not(test), target_os = "none"))]
use core::arch::global_asm;
use core::fmt::Write;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
#[cfg(all(not(test), target_os = "none"))]
use core::panic::PanicInfo;
use core::str;
#[cfg(not(test))]
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};
#[cfg(all(not(test), target_os = "none"))]
use limine::memory_map::EntryType;
#[cfg(all(not(test), target_os = "none"))]
use limine::request::{
    ExecutableAddressRequest, ExecutableCmdlineRequest, FramebufferRequest, HhdmRequest,
    MemoryMapRequest, MpRequest,
};
#[cfg(all(not(test), target_os = "none"))]
use limine::BaseRevision;

#[cfg(all(not(test), target_os = "none"))]
// Provide a small, deterministic stack and jump into Rust.
//
// This is only needed for bare-metal execution, not for tests.
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
    # 256 KiB: the network stack (TCP buffers) and desktop state are built
    # as stack temporaries in rust_main before moving into statics.
    .skip 262144
stack_top:
"#
);

// IDT structure and interrupt handlers
#[cfg(not(test))]
#[repr(C, packed)]
#[derive(Copy, Clone)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    flags: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

#[cfg(not(test))]
impl IdtEntry {
    const fn new() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            flags: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    fn set_handler(&mut self, handler: unsafe extern "C" fn(), selector: u16) {
        self.set_handler_ist(handler, selector, 0);
    }

    /// Like `set_handler`, switching to interrupt stack `ist` (1-7) on entry.
    fn set_handler_ist(&mut self, handler: unsafe extern "C" fn(), selector: u16, ist: u8) {
        let addr = handler as usize;
        self.offset_low = (addr & 0xFFFF) as u16;
        self.offset_mid = ((addr >> 16) & 0xFFFF) as u16;
        self.offset_high = ((addr >> 32) & 0xFFFFFFFF) as u32;
        self.selector = selector;
        self.ist = ist & 0x7;
        self.flags = IDT_PRESENT_INTERRUPT_GATE;
        self.reserved = 0;
    }
}

/// CPUs that get their own GDT, TSS, and interrupt stack.
const MAX_TABLE_CPUS: usize = 8;
/// Bytes per double-fault interrupt stack.
const IST_STACK_BYTES: usize = 16 * 1024;

#[cfg(all(not(test), target_os = "none"))]
static mut CPU_GDTS: [hal_x86_64::Gdt; MAX_TABLE_CPUS] = [hal_x86_64::Gdt::new(); MAX_TABLE_CPUS];
#[cfg(all(not(test), target_os = "none"))]
static mut CPU_TSSS: [hal_x86_64::Tss; MAX_TABLE_CPUS] = [hal_x86_64::Tss::new(); MAX_TABLE_CPUS];
#[cfg(all(not(test), target_os = "none"))]
#[repr(C, align(16))]
struct IstStacks([[u8; IST_STACK_BYTES]; MAX_TABLE_CPUS]);
#[cfg(all(not(test), target_os = "none"))]
static mut IST_STACKS: IstStacks = IstStacks([[0; IST_STACK_BYTES]; MAX_TABLE_CPUS]);

/// Install this CPU's GDT and TSS (with IST1 for double faults). CPUs past
/// `MAX_TABLE_CPUS` keep the bootloader's tables. Returns whether loaded.
#[cfg(all(not(test), target_os = "none"))]
fn init_cpu_tables(cpu_index: usize) -> bool {
    if cpu_index >= MAX_TABLE_CPUS {
        return false;
    }
    // SAFETY: each CPU touches only its own slot, once, before using it.
    unsafe {
        let tss = core::ptr::addr_of_mut!(CPU_TSSS[cpu_index]);
        let stack_top =
            core::ptr::addr_of!(IST_STACKS.0[cpu_index]) as u64 + IST_STACK_BYTES as u64;
        (*tss).ist[0] = stack_top;
        let gdt = core::ptr::addr_of_mut!(CPU_GDTS[cpu_index]);
        (*gdt).set_tss(tss);
        hal_x86_64::gdt::load(&*gdt);
    }
    true
}

/// Registers as pushed by the exception stubs, followed by the vector,
/// error code, and the CPU's interrupt frame.
#[repr(C)]
struct ExceptionFrame {
    regs: [u64; 15],
    vector: u64,
    error_code: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

fn exception_name(vector: u64) -> &'static str {
    match vector {
        0 => "#DE divide error",
        6 => "#UD invalid opcode",
        8 => "#DF double fault",
        13 => "#GP general protection",
        14 => "#PF page fault",
        _ => "exception",
    }
}

/// Print a fatal exception and stop this CPU. Nothing is recoverable yet.
#[cfg(not(test))]
#[no_mangle]
extern "C" fn exception_handler(frame: *const ExceptionFrame) -> ! {
    // SAFETY: the stub passes a pointer to the frame it just built.
    let frame = unsafe { &*frame };
    let cr2: u64;
    unsafe { asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags)) };
    let mut serial = serial::UnlockedSerial(serial::SerialPort::new(serial::COM1));
    let cpu = LAPIC.get().map(|apic| apic.id());
    let _ = writeln!(
        serial,
        "KERNEL EXCEPTION {} vector={} err=0x{:x} rip=0x{:x} cs=0x{:x} rflags=0x{:x} rsp=0x{:x} cr2=0x{:x} cpu_lapic={:?}",
        exception_name(frame.vector),
        frame.vector,
        frame.error_code,
        frame.rip,
        frame.cs,
        frame.rflags,
        frame.rsp,
        cr2,
        cpu
    );
    let _ = writeln!(
        serial,
        "  rax=0x{:x} rbx=0x{:x} rcx=0x{:x} rdx=0x{:x} rsi=0x{:x} rdi=0x{:x} rbp=0x{:x}",
        frame.regs[14],
        frame.regs[11],
        frame.regs[13],
        frame.regs[12],
        frame.regs[9],
        frame.regs[8],
        frame.regs[10]
    );
    loop {
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

#[cfg(not(test))]
#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

#[cfg(not(test))]
static mut IDT: [IdtEntry; 256] = [IdtEntry::new(); 256];

#[cfg(not(test))]
static KERNEL_TICK_COUNTER: AtomicU64 = AtomicU64::new(0);

// Keyboard event queue (lock-free ring buffer)
#[cfg(not(test))]
static KEYBOARD_EVENT_QUEUE: KeyboardEventQueue = KeyboardEventQueue::new();

// PS/2 mouse byte queue (same IRQ-to-loop ring buffer, fed from IRQ 12).
#[cfg(not(test))]
static MOUSE_EVENT_QUEUE: KeyboardEventQueue = KeyboardEventQueue::new();

const KBD_DEBUG_LOG: bool = false;
const FB_SHADOW_ENABLED: bool = true;
/// Caret blink period in PIT ticks (100 Hz): on for 0.5 s, off for 0.5 s.
const CARET_BLINK_PERIOD_TICKS: u64 = 100;
/// How long a shell notice stays on screen (8 s).
const NOTICE_TTL_TICKS: u64 = 800;
/// Editor viewport rows used by the text console renderer.
const TEXT_EDITOR_VIEWPORT_ROWS: usize = 23;
/// Share of the kernel heap the desktop may reserve for pixel buffers, in
/// quarters (3 = 75%), leaving the rest for everything else.
const GRAPHICS_HEAP_SHARE_QUARTERS: usize = 3;
/// Consecutive heap samples before a pressure level is adopted or left
/// (critical is immediate).
const PRESSURE_HYSTERESIS_SAMPLES: u32 = 3;

#[cfg(not(test))]
const IDT_PRESENT_INTERRUPT_GATE: u8 = 0x8E; // Present, DPL=0, interrupt gate

#[cfg(all(not(test), target_os = "none"))]
global_asm!(
    r#"
.section .text
.global irq_timer_entry
irq_timer_entry:
    # Save all general-purpose registers
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    
    call timer_irq_handler
    
    # Restore all registers in reverse order
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rbp
    pop rbx
    pop rdx
    pop rcx
    pop rax
    iretq

.global irq_keyboard_entry
irq_keyboard_entry:
    # Save all general-purpose registers
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    
    call keyboard_irq_handler
    
    # Restore all registers in reverse order
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rbp
    pop rbx
    pop rdx
    pop rcx
    pop rax
    iretq

.global irq_mouse_entry
irq_mouse_entry:
    # Save all general-purpose registers
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    
    call mouse_irq_handler
    
    # Restore all registers in reverse order
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rbp
    pop rbx
    pop rdx
    pop rcx
    pop rax
    iretq
"#
);

#[cfg(all(not(test), target_os = "none"))]
global_asm!(
    r#"
.section .text
.global irq_ipi_entry
irq_ipi_entry:
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    call ipi_handler
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rbp
    pop rbx
    pop rdx
    pop rcx
    pop rax
    iretq

.global irq_spurious_entry
irq_spurious_entry:
    iretq

.macro EXC_NOERR name, vector
.global \name
\name:
    push 0
    push \vector
    jmp exception_common
.endm

.macro EXC_ERR name, vector
.global \name
\name:
    push \vector
    jmp exception_common
.endm

EXC_NOERR exc_divide_entry, 0
EXC_NOERR exc_invalid_opcode_entry, 6
EXC_ERR exc_double_fault_entry, 8
EXC_ERR exc_general_protection_entry, 13
EXC_ERR exc_page_fault_entry, 14

exception_common:
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    mov rdi, rsp
    and rsp, -16
    call exception_handler
1:
    hlt
    jmp 1b

.global irq_lapic_timer_entry
irq_lapic_timer_entry:
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    call lapic_timer_handler
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rbp
    pop rbx
    pop rdx
    pop rcx
    pop rax
    iretq
"#
);

#[cfg(not(test))]
extern "C" {
    fn irq_timer_entry();
    fn irq_keyboard_entry();
    fn irq_mouse_entry();
    fn irq_ipi_entry();
    fn irq_spurious_entry();
    fn irq_lapic_timer_entry();
    fn exc_divide_entry();
    fn exc_invalid_opcode_entry();
    fn exc_double_fault_entry();
    fn exc_general_protection_entry();
    fn exc_page_fault_entry();
}

/// Vector used to wake application processors.
#[cfg(not(test))]
const IPI_WAKE_VECTOR: u8 = 0xF0;
/// LAPIC spurious vector (no EOI required).
#[cfg(not(test))]
const SPURIOUS_VECTOR: u8 = 0xFF;
/// Per-CPU LAPIC timer vector.
#[cfg(not(test))]
const LAPIC_TIMER_VECTOR: u8 = 0xF1;
/// LAPIC timer rate on application processors.
const LAPIC_TIMER_HZ: u64 = 100;

/// Ticks delivered by each CPU's own LAPIC timer, by registry index.
static CPU_TICKS: [core::sync::atomic::AtomicU64; hal_x86_64::MAX_CPUS] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const ZERO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    [ZERO; hal_x86_64::MAX_CPUS]
};

/// LAPIC timer initial count for one `LAPIC_TIMER_HZ` period at divide 16
/// (0 until calibrated against the PIT on the boot CPU).
static LAPIC_TIMER_INITIAL: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Registry index of the CPU running this code, via its LAPIC id.
#[cfg(all(not(test), target_os = "none"))]
fn current_cpu_index() -> Option<usize> {
    let id = LAPIC.get()?.id();
    (0..CPUS.online()).find(|&i| CPUS.lapic_id(i) == Some(id))
}

/// Per-CPU timer tick: count it and acknowledge.
#[cfg(not(test))]
#[no_mangle]
extern "C" fn lapic_timer_handler() {
    #[cfg(target_os = "none")]
    if let Some(index) = current_cpu_index() {
        CPU_TICKS[index].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
    if let Some(mut apic) = LAPIC.get() {
        apic.end_of_interrupt();
    }
}

/// Measure LAPIC timer counts per PIT tick on the boot CPU (interrupts must
/// be enabled) and publish the periodic initial count for the APs.
#[cfg(all(not(test), target_os = "none"))]
fn calibrate_lapic_timer(serial: &mut serial::SerialPort) {
    let Some(mut apic) = LAPIC.get() else {
        return;
    };
    const SAMPLE_TICKS: u64 = 10;
    // How far the LAPIC counter may fall before we conclude the PIT is never
    // going to tick. The LAPIC is the only clock here that does not depend on
    // the thing being waited for, and a quarter of its range is tens of
    // seconds at any plausible bus frequency -- long enough that a slow
    // machine cannot trip it, finite enough that a machine without a working
    // 8254 still boots.
    const GIVE_UP_COUNTS: u32 = u32::MAX / 4;
    apic.set_timer_divide(16);
    apic.start_timer_masked(u32::MAX);

    // Align to a tick edge, then count over SAMPLE_TICKS ticks. Both waits
    // used to spin forever: `get_tick_count` is written only by the PIT's IRQ
    // 0 handler, so on a machine where that interrupt never arrives -- no
    // 8254, no 8259, or IRQ 0 routed somewhere we never unmask -- the boot
    // CPU stopped here for good, after "PIT configured for 100 Hz" and
    // before anything else. No prompt, no message, no recovery.
    let mut timed_out = false;
    let start_tick = get_tick_count();
    let guard = apic.timer_current();
    while get_tick_count() == start_tick {
        if guard.wrapping_sub(apic.timer_current()) > GIVE_UP_COUNTS {
            timed_out = true;
            break;
        }
        core::hint::spin_loop();
    }
    let begin = apic.timer_current();
    let edge = get_tick_count();
    while !timed_out && get_tick_count() < edge + SAMPLE_TICKS {
        if begin.wrapping_sub(apic.timer_current()) > GIVE_UP_COUNTS {
            timed_out = true;
        }
        core::hint::spin_loop();
    }
    let end = apic.timer_current();
    apic.stop_timer();
    if timed_out {
        // Leave LAPIC_TIMER_INITIAL at zero: the APs already treat that as
        // "do not start a timer", so they idle rather than misfire.
        let _ = writeln!(
            serial,
            "LAPIC timer: calibration gave up, no PIT tick arrived; \
             per-CPU timers stay off"
        );
        apic.send_ipi_all_excluding_self(IPI_WAKE_VECTOR);
        return;
    }
    let counts = begin.wrapping_sub(end) as u64;
    // PIT ticks are 100 Hz, so one LAPIC period at LAPIC_TIMER_HZ is:
    let per_period = counts * 100 / SAMPLE_TICKS / LAPIC_TIMER_HZ;
    let initial = per_period.clamp(1, u32::MAX as u64) as u32;
    LAPIC_TIMER_INITIAL.store(initial, core::sync::atomic::Ordering::Release);
    let _ = writeln!(
        serial,
        "LAPIC timer: {} counts per {} ticks, initial={} for {} Hz",
        counts, SAMPLE_TICKS, initial, LAPIC_TIMER_HZ
    );
    // Wake the APs so they pick the value up and start their timers.
    apic.send_ipi_all_excluding_self(IPI_WAKE_VECTOR);
}

/// Wake-up IPI: nothing to do beyond acknowledging; the idle loop on the
/// target CPU re-checks the work queue after `hlt` returns.
#[cfg(not(test))]
#[no_mangle]
extern "C" fn ipi_handler() {
    hal_x86_64::lapic::IPI_COUNT.fetch_add(1, Ordering::Relaxed);
    if let Some(mut apic) = LAPIC.get() {
        apic.end_of_interrupt();
    }
}

#[cfg(not(test))]
#[no_mangle]
extern "C" fn timer_irq_handler() {
    KERNEL_TICK_COUNTER.fetch_add(1, Ordering::Relaxed);

    // Send EOI to PIC
    unsafe {
        outb(0x20, 0x20);
    }
}

#[cfg(not(test))]
#[no_mangle]
extern "C" fn keyboard_irq_handler() {
    // `UnlockedSerial`, as `exception_handler` uses. `writeln!` on a
    // `SerialPort` takes SERIAL_LOCK, and this handler can preempt a CPU
    // that already holds it -- `SpinLock` does not mask interrupts -- so
    // flipping KBD_DEBUG_LOG to true would have been an instant
    // self-deadlock. It is compiled out today, which is the only reason
    // nobody has hit it.
    if KBD_DEBUG_LOG {
        let mut serial = serial::UnlockedSerial(serial::SerialPort::new(serial::COM1));
        let _ = writeln!(serial, "kbd irq fired");
    }

    // Read scancode from PS/2 data port
    unsafe {
        let status = inb(0x64);
        // Bit 0: output buffer full. Bit 5: the byte is auxiliary (mouse)
        // data. The mouse handler checks both; this checked only the first,
        // so any condition that raised IRQ 1 with an aux byte latched pushed
        // a mouse byte into the keystroke stream. The two handlers read the
        // same register and should agree about what it means.
        if (status & 0x21) == 0x01 {
            let scancode = inb(0x60);
            let dropped = KEYBOARD_EVENT_QUEUE.push(scancode);
            if KBD_DEBUG_LOG {
                let mut serial = serial::UnlockedSerial(serial::SerialPort::new(serial::COM1));
                if dropped {
                    let _ = writeln!(serial, "kbd queue overflow (dropped oldest)");
                }
                let _ = writeln!(serial, "kbd scancode={:#x}", scancode);
            }
        } else if KBD_DEBUG_LOG {
            let mut serial = serial::UnlockedSerial(serial::SerialPort::new(serial::COM1));
            let _ = writeln!(serial, "kbd irq no-data status={:#x}", status);
        }

        // Send EOI to PIC
        outb(0x20, 0x20);
    }
}

/// IRQ 12: one PS/2 mouse byte is waiting in the controller output buffer.
///
/// Only the raw byte is queued here; packet framing happens in the main loop
/// so the interrupt path stays a few port reads long. Both PICs need an EOI
/// because IRQ 12 arrives through the slave.
#[cfg(not(test))]
#[no_mangle]
extern "C" fn mouse_irq_handler() {
    unsafe {
        let status = inb(0x64);
        // Bit 0: output buffer full. Bit 5: the byte is auxiliary (mouse) data.
        if (status & 0x21) == 0x21 {
            let byte = inb(0x60);
            MOUSE_EVENT_QUEUE.push(byte);
        }
        outb(0xA0, 0x20);
        outb(0x20, 0x20);
    }
}

#[cfg(not(test))]
unsafe fn outb(port: u16, value: u8) {
    asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags)
    );
}

#[cfg(not(test))]
fn install_idt() {
    unsafe {
        let code_segment = current_code_segment();
        // Set up timer interrupt (IRQ 0 = vector 32)
        IDT[32].set_handler(irq_timer_entry, code_segment);

        // Set up keyboard interrupt (IRQ 1 = vector 33)
        IDT[33].set_handler(irq_keyboard_entry, code_segment);

        // Set up PS/2 mouse interrupt (IRQ 12 = vector 44, via slave PIC)
        IDT[44].set_handler(irq_mouse_entry, code_segment);

        // Inter-processor wake-up and LAPIC spurious vectors
        IDT[IPI_WAKE_VECTOR as usize].set_handler(irq_ipi_entry, code_segment);
        IDT[SPURIOUS_VECTOR as usize].set_handler(irq_spurious_entry, code_segment);
        IDT[LAPIC_TIMER_VECTOR as usize].set_handler(irq_lapic_timer_entry, code_segment);

        // CPU exceptions: diagnostics instead of a silent triple fault. The
        // double fault runs on IST1 so a blown stack still reports.
        IDT[0].set_handler(exc_divide_entry, code_segment);
        IDT[6].set_handler(exc_invalid_opcode_entry, code_segment);
        IDT[8].set_handler_ist(exc_double_fault_entry, code_segment, 1);
        IDT[13].set_handler(exc_general_protection_entry, code_segment);
        IDT[14].set_handler(exc_page_fault_entry, code_segment);

        load_idt();
    }
}

/// Point this CPU at the shared IDT (used by the BSP and every AP).
#[cfg(not(test))]
fn load_idt() {
    let idtr = IdtPointer {
        limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: core::ptr::addr_of!(IDT) as *const _ as u64,
    };
    unsafe {
        asm!(
            "lidt [{}]",
            in(reg) &idtr,
            options(readonly, nostack, preserves_flags)
        );
    }
}

#[cfg(not(test))]
fn current_code_segment() -> u16 {
    let cs: u16;
    unsafe {
        asm!(
            "mov {0:x}, cs",
            out(reg) cs,
            options(nomem, nostack, preserves_flags)
        );
    }
    cs
}

#[cfg(not(test))]
fn init_pic() {
    unsafe {
        // Start initialization sequence
        outb(0x20, 0x11);
        outb(0xA0, 0x11);

        // Remap IRQs to 32-47
        outb(0x21, 32);
        outb(0xA1, 40);

        // Configure cascade
        outb(0x21, 0x04);
        outb(0xA1, 0x02);

        // 8086 mode
        outb(0x21, 0x01);
        outb(0xA1, 0x01);

        // Mask all IRQs initially
        outb(0x21, 0xFF);
        outb(0xA1, 0xFF);
    }
}

#[cfg(not(test))]
fn init_pit() {
    unsafe {
        // Configure PIT channel 0 for 100 Hz
        // Frequency = 1193182 / divisor
        // For 100 Hz: divisor = 11932
        let divisor: u16 = 11932;

        // Command: channel 0, lo/hi byte, rate generator, binary
        outb(0x43, 0x36);

        // Send divisor
        outb(0x40, (divisor & 0xFF) as u8);
        outb(0x40, ((divisor >> 8) & 0xFF) as u8);
    }
}

#[cfg(not(test))]
fn unmask_timer_irq() {
    unsafe {
        let mask = inb(0x21);
        outb(0x21, mask & !0x01); // Unmask IRQ 0
    }
}

#[cfg(not(test))]
fn unmask_keyboard_irq() {
    unsafe {
        let mask = inb(0x21);
        outb(0x21, mask & !0x02); // Unmask IRQ 1
    }
}

#[cfg(not(test))]
fn unmask_mouse_irq() {
    unsafe {
        // IRQ 12 lives on the slave PIC, which reaches the CPU through the
        // master's IRQ 2 cascade line; both must be unmasked.
        let master = inb(0x21);
        outb(0x21, master & !0x04);
        let slave = inb(0xA1);
        outb(0xA1, slave & !0x10);
    }
}

#[cfg(not(test))]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    asm!(
        "in al, dx",
        in("dx") port,
        out("al") value,
        options(nomem, nostack, preserves_flags)
    );
    value
}

#[cfg(not(test))]
fn enable_interrupts() {
    unsafe {
        asm!("sti", options(nomem, nostack, preserves_flags));
    }
}

#[cfg(not(test))]
fn read_rflags() -> u64 {
    let flags: u64;
    unsafe {
        asm!(
            "pushfq",
            "pop {}",
            out(reg) flags,
            options(nomem, nostack, preserves_flags)
        );
    }
    flags
}

#[cfg(not(test))]
fn interrupts_enabled() -> bool {
    (read_rflags() & (1 << 9)) != 0
}

#[cfg(not(test))]
fn read_idtr() -> IdtPointer {
    let mut idtr = IdtPointer { limit: 0, base: 0 };
    unsafe {
        asm!("sidt [{}]", in(reg) &mut idtr, options(nomem, nostack, preserves_flags));
    }
    idtr
}

#[cfg(not(test))]
fn log_interrupt_state(serial: &mut serial::SerialPort) {
    let idtr = read_idtr();
    let base = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(idtr.base)) };
    let limit = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(idtr.limit)) };
    let if_set = interrupts_enabled();
    let _ = writeln!(
        serial,
        "IDT loaded base=0x{:x} limit=0x{:x} IF={}",
        base,
        limit,
        if if_set { "1" } else { "0" }
    );
}

#[cfg(not(test))]
fn log_pic_masks(serial: &mut serial::SerialPort) {
    unsafe {
        let master_mask = inb(0x21);
        let slave_mask = inb(0xA1);
        let _ = writeln!(
            serial,
            "PIC masks: master=0x{:02x} slave=0x{:02x}",
            master_mask, slave_mask
        );
    }
}

#[cfg(not(test))]
fn ps2_wait_input_clear() -> bool {
    const LIMIT: usize = 10000;
    for _ in 0..LIMIT {
        let status = unsafe { inb(0x64) };
        if (status & 0x02) == 0 {
            return true;
        }
        unsafe {
            asm!("pause", options(nomem, nostack, preserves_flags));
        }
    }
    false
}

#[cfg(not(test))]
fn ps2_wait_output_full() -> bool {
    const LIMIT: usize = 10000;
    for _ in 0..LIMIT {
        let status = unsafe { inb(0x64) };
        if (status & 0x01) != 0 {
            return true;
        }
        unsafe {
            asm!("pause", options(nomem, nostack, preserves_flags));
        }
    }
    false
}

#[cfg(not(test))]
fn enable_ps2_keyboard_irq(serial: &mut serial::SerialPort) {
    unsafe {
        if !ps2_wait_input_clear() {
            let _ = writeln!(serial, "ps2: input buffer stuck (pre-read)");
            return;
        }

        // Read controller command byte
        outb(0x64, 0x20);
        if !ps2_wait_output_full() {
            let _ = writeln!(serial, "ps2: no command byte available");
            return;
        }
        let mut cmd = inb(0x60);
        let original = cmd;

        // Enable keyboard IRQ and ensure keyboard clock not disabled
        cmd |= 0x01; // IRQ1 enable
        cmd &= !0x10; // clear keyboard disable bit

        if cmd != original {
            if !ps2_wait_input_clear() {
                let _ = writeln!(serial, "ps2: input buffer stuck (pre-write)");
                return;
            }
            outb(0x64, 0x60); // write command byte
            if !ps2_wait_input_clear() {
                let _ = writeln!(serial, "ps2: input buffer stuck (cmd write)");
                return;
            }
            outb(0x60, cmd);
            let _ = writeln!(
                serial,
                "ps2: command byte updated {:02x} -> {:02x}",
                original, cmd
            );
        } else {
            let _ = writeln!(serial, "ps2: command byte already {:02x}", cmd);
        }
    }
}

#[cfg(not(test))]
fn get_tick_count() -> u64 {
    KERNEL_TICK_COUNTER.load(Ordering::Relaxed)
}

// Syscall stubs
#[cfg(not(test))]
fn sys_yield() {
    // No-op for now
}

// NOTE: Phase 58 Update - This busy-wait implementation is acceptable for single-task bare-metal.
// When multi-tasking is added to bare-metal, this should use scheduler blocking like sim_kernel does:
//   - Task enters Blocked { wake_tick } state
//   - Scheduler skips blocked tasks
//   - Timer IRQ wakes tasks when current_tick >= wake_tick
// For now, idle_pause() provides CPU power savings while waiting.
#[cfg(not(test))]
fn sys_sleep(ticks: u64) {
    let start = get_tick_count();
    while get_tick_count() < start + ticks {
        idle_pause();
    }
}

#[cfg(not(test))]
fn sys_send(
    ctx: &mut KernelContext,
    channel: ChannelId,
    msg: KernelMessage,
) -> Result<(), KernelError> {
    ctx.send(channel, msg)
}

#[cfg(not(test))]
fn sys_recv(ctx: &mut KernelContext, channel: ChannelId) -> Result<KernelMessage, KernelError> {
    ctx.recv(channel)
}

// Logging macros
#[cfg(not(test))]
macro_rules! klog {
    ($serial:expr, $($arg:tt)*) => {
        {
            use core::fmt::Write;
            let _ = write!($serial, $($arg)*);
        }
    };
}

#[cfg(not(test))]
macro_rules! kprintln {
    ($serial:expr, $($arg:tt)*) => {
        {
            use core::fmt::Write;
            let _ = writeln!($serial, $($arg)*);
        }
    };
}

#[cfg(all(not(test), target_os = "none"))]
#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    let mut serial = serial::SerialPort::new(serial::COM1);
    serial.init();
    kprintln!(serial, "PandaGen: kernel_bootstrap online");
    let boot = boot_info(&mut serial);
    let (mut allocator, heap) = init_memory(&mut serial, &boot);
    if heap.is_none() {
        // Without a heap the global allocator stays at start == end == 0 and
        // every `alloc` returns null. Boot used to carry on regardless and
        // die ten steps later inside `alloc_error_handler`, with
        // "ALLOCATION ERROR: size=16" and nothing to connect it to the cause
        // it had already printed. Stop where the reason is still legible.
        kprintln!(
            serial,
            "FATAL: no heap; PandaGen cannot start on this machine. See the \
             heap line above for why."
        );
        halt_loop();
    }

    kprintln!(serial, "Initializing interrupts...");
    if init_cpu_tables(0) {
        kprintln!(
            serial,
            "GDT/TSS installed for cpu0 (IST1 for double faults)"
        );
    }
    install_idt();
    klog!(
        serial,
        "IDT installed at 0x{:x}\r\n",
        core::ptr::addr_of!(IDT) as usize
    );
    if KBD_DEBUG_LOG {
        let idt_flags = unsafe { IDT[33].flags };
        let idt_selector = unsafe { IDT[33].selector };
        let _ = writeln!(
            serial,
            "IDT[33] flags=0x{:02x} selector=0x{:04x}",
            idt_flags, idt_selector
        );
    }

    let _ = CPUS.register(
        MP_REQUEST
            .get_response()
            .map(|r| r.bsp_lapic_id())
            .unwrap_or(0),
    );
    start_application_processors(&mut serial, allocator.as_mut());

    init_pic();
    kprintln!(serial, "PIC remapped to IRQ base 32");
    if KBD_DEBUG_LOG {
        log_pic_masks(&mut serial);
    }

    kprintln!(serial, "PS/2 controller: enabling keyboard IRQ1");
    enable_ps2_keyboard_irq(&mut serial);

    // Bring up the PS/2 mouse while interrupts are still off, so the ACK
    // bytes of the handshake are read here instead of by the IRQ handler.
    let mouse_config = match hal_x86_64::Ps2MouseInit::initialize(&mut hal_x86_64::RealPortIo::new())
    {
        Ok(report) => {
            kprintln!(
                serial,
                "ps2 mouse: ready (id={} wheel={} cmd_byte={:#04x})",
                report.device_id,
                report.wheel,
                report.command_byte
            );
            Some(report)
        }
        Err(err) => {
            kprintln!(serial, "ps2 mouse: unavailable ({:?})", err);
            None
        }
    };

    init_pit();
    kprintln!(serial, "PIT configured for 100 Hz");

    unmask_timer_irq();
    unmask_keyboard_irq();
    if mouse_config.is_some() {
        unmask_mouse_irq();
    }
    enable_interrupts();
    calibrate_lapic_timer(&mut serial);
    if KBD_DEBUG_LOG {
        log_pic_masks(&mut serial);
        log_interrupt_state(&mut serial);
    }
    kprintln!(
        serial,
        "Interrupts enabled, timer at 100 Hz, keyboard IRQ 1"
    );

    // Initialise through a unique reference, then immediately narrow to a
    // shared one: every application processor also holds `&Kernel`, and a
    // live `&mut` alongside those is undefined behaviour even though each
    // field is individually locked.
    let kernel: &Kernel = unsafe {
        Kernel::init_in_place(
            &mut *core::ptr::addr_of_mut!(KERNEL_STORAGE),
            boot,
            allocator,
            heap,
        )
    };

    // Phase 78: Boot with display console for QEMU window UI
    kprintln!(serial, "\r\n=== PandaGen Workspace ===");

    // Prefer framebuffer if available (QEMU typically boots in graphics mode)
    let mut fb_console = unsafe { framebuffer::BareMetalFramebuffer::from_boot_info(&kernel.boot) };
    let mut vga_console = None;

    if fb_console.is_some() {
        kprintln!(serial, "Framebuffer console initialized (primary)");
        kprintln!(serial, "Main UI in QEMU window, serial logs here");
    } else {
        kprintln!(serial, "Framebuffer unavailable - trying VGA text console");
        vga_console = unsafe { vga::init_vga_console(&kernel.boot) };

        if vga_console.is_some() {
            kprintln!(serial, "VGA text console initialized (80x25)");
            kprintln!(serial, "Main UI in QEMU window, serial logs here");
        } else {
            kprintln!(serial, "No display available - serial-only mode");
        }
    }

    kprintln!(serial, "Boot complete. Type 'help' for commands.\r\n");

    // Test that global allocator works
    {
        extern crate alloc;
        use alloc::string::String;
        use alloc::vec::Vec;

        let mut test_vec: Vec<u32> = Vec::new();
        test_vec.push(1);
        test_vec.push(2);
        test_vec.push(3);

        let test_string = String::from("Alloc works!");

        kprintln!(
            serial,
            "Alloc test: vec={:?}, string={}",
            test_vec,
            test_string
        );
    }

    // Initialize filesystem with example files
    kprintln!(serial, "Initializing filesystem...");
    let storage_boot = bare_metal_storage::StorageBootInfo {
        hhdm_offset: kernel.boot.hhdm_offset,
        kernel_phys: kernel.boot.kernel_phys,
        kernel_virt: kernel.boot.kernel_virt,
    };
    let remote_enabled = REMOTE_ENABLED.load(core::sync::atomic::Ordering::Acquire);
    match bare_metal_net::NetStack::probe(storage_boot, remote_enabled) {
        Some(mut net) => {
            net.dhcp(&get_tick_count, &mut serial);
            klog!(
                serial,
                "net: virtio-net-pci mac={} ip={}\r\n",
                net_stack::wire::fmt_mac(net.mac()),
                net_stack::wire::fmt_ipv4(net.ip())
            );
            *NET.lock() = Some(net);
        }
        None => kprintln!(serial, "net: no virtio-net device"),
    }

    let mut filesystem = match bare_metal_storage::BareMetalFilesystem::new_with_boot(storage_boot)
    {
        Ok(fs) => {
            kprintln!(
                serial,
                "Filesystem ready (backend: {}, {}, flush={})",
                {
                    *STORAGE_BACKEND.lock() = fs.backend_name();
                    fs.backend_name()
                },
                if fs.was_freshly_formatted() {
                    "formatted"
                } else {
                    "mounted existing"
                },
                // A driver that silently skips FLUSH looks exactly like one
                // with nothing to flush, and the difference is whether a
                // commit survives the host losing power. Say which it is.
                if fs.backend_flushes() {
                    "negotiated"
                } else {
                    "device has no cache"
                }
            );
            if let Some(report) = fs.recovery_report() {
                if report.discarded_transactions > 0 {
                    // Never silent again. A format change once made every
                    // record an older build had written fail its own
                    // checksum; recovery discarded all of them and the only
                    // trace was a counter nobody read.
                    kprintln!(
                        serial,
                        "storage: WARNING recovery discarded {} transactions ({} recovered, last sequence {})",
                        report.discarded_transactions,
                        report.recovered_commits,
                        report.last_sequence
                    );
                }
            }
            fs
        }
        Err(_) => {
            kprintln!(serial, "Warning: failed to initialize filesystem");
            // Continue without filesystem
            workspace_loop(
                &mut serial,
                kernel,
                vga_console.as_mut(),
                fb_console.as_mut(),
                None,
                mouse_config,
            )
        }
    };

    // Create some example files on a fresh disk only; a mounted disk keeps
    // whatever the user saved on previous boots.
    if filesystem.was_freshly_formatted() {
        let _ = filesystem.create_file(
            "welcome.txt",
            b"Welcome to PandaGen!\nThis is a bare-metal operating system.",
        );
        let _ = filesystem.create_file(
            "readme.md",
            b"# PandaGen\n\nA capability-based OS written in Rust.\n\nTry: open editor readme.md",
        );
        let _ = filesystem.create_file(
            "test.txt",
            b"Hello, World!\nThis is a test file.\nYou can edit this with :w to save.",
        );
        kprintln!(
            serial,
            "Created example files: welcome.txt, readme.md, test.txt"
        );
    }

    workspace_loop(
        &mut serial,
        kernel,
        vga_console.as_mut(),
        fb_console.as_mut(),
        Some(filesystem),
        mouse_config,
    )
}

#[cfg(all(not(test), target_os = "none"))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut serial = serial::SerialPort::new(serial::COM1);
    kprintln!(serial, "\r\n\r\nKERNEL PANIC:");
    if let Some(location) = info.location() {
        kprintln!(
            serial,
            "  at {}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    }
    kprintln!(serial, "  {}", info.message());
    halt_loop()
}

#[inline(always)]
#[cfg(not(test))]
fn halt_loop() -> ! {
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

#[inline(always)]
fn idle_pause() {
    #[cfg(not(test))]
    unsafe {
        asm!("pause", options(nomem, nostack, preserves_flags));
    }
}

#[cfg(not(test))]
fn console_loop(serial: &mut serial::SerialPort, kernel: &Kernel) -> ! {
    let mut last_tick_display = 0u64;
    loop {
        let progressed = kernel.run_once(serial);

        // Display a dot every 100 ticks (every second at 100Hz)
        let current_tick = get_tick_count();
        if current_tick >= last_tick_display + 100 {
            klog!(serial, ".");
            last_tick_display = current_tick;
        }

        if !progressed {
            idle_pause();
        }
    }
}

/// Workspace loop - main interactive session
/// Phase 64: This replaces the demo editor loop with a proper workspace prompt
/// Phase 69: Now supports framebuffer console output
/// Phase 78: VGA text console is primary UI for QEMU window
/// Phase 97: Integrated bare-metal filesystem storage
#[cfg(not(test))]
fn workspace_loop(
    serial: &mut serial::SerialPort,
    kernel: &Kernel,
    mut vga_console: Option<&mut console_vga::VgaConsole>,
    mut fb_console: Option<&mut framebuffer::BareMetalFramebuffer>,
    filesystem: Option<bare_metal_storage::BareMetalFilesystem>,
    mouse_config: Option<hal_x86_64::MouseInitReport>,
) -> ! {
    // Get command and response channels from kernel
    let command_channel = ChannelId(0);
    // Channel 1 belongs to the serial console task. The graphical workspace
    // gets its own below, because a channel with two readers loses messages
    // to whichever CPU polls first.

    let workspace_response = kernel
        .create_channel()
        .expect("workspace reply channel available");
    let mut workspace = workspace::WorkspaceSession::new(command_channel, workspace_response);
    // GFX-047: pixel buffers are budgeted against a fixed share of the heap
    // and refused with a typed error rather than failing inside `alloc`.
    let heap_total = GLOBAL_HEAP.stats().total;
    let mut surface_budget =
        services_gui_host::SurfaceBudget::new(heap_total / 4 * GRAPHICS_HEAP_SHARE_QUARTERS);
    kprintln!(
        serial,
        "gfx budget: {} KiB of {} KiB heap",
        surface_budget.limit() / 1024,
        heap_total / 1024
    );

    // Remote IPC over UDP: calls arrive on the network, run through the
    // command service, and answer on this channel.
    let remote_channel = kernel.create_channel().ok();
    let mut remote_server = RemoteCommandServer::new(remote_channel);

    // GFX-039: pipeline stages reply on their own channel.
    match kernel.create_channel() {
        Ok(channel) => workspace.set_pipeline_channel(channel),
        Err(_) => kprintln!(serial, "pipeline: no reply channel available"),
    }

    // Install filesystem if available
    if let Some(fs) = filesystem {
        workspace.set_filesystem(fs);
    }

    let mut parser_state = Ps2ParserState::new();

    let mut input_dirty = true;
    let mut output_dirty = true;
    let mut output_initialized = false;
    let mut last_output_seq = 0u64;
    let mut last_output_rows = 0usize;
    let mut prompt_initialized = false;
    let mut status_initialized = false;
    let mut last_prompt_row = 0usize;
    let mut last_view_start = 0usize;
    let mut last_prompt_cli_active = false;
    let mut last_status_cli_active = false;
    #[cfg(debug_assertions)]
    let mut last_editor_input: Option<u8> = None;
    let mut last_palette_open = false;
    let mut last_palette_query_hash = 0u64;
    let mut last_palette_result_count = 0usize;
    let mut last_palette_selection = 0usize;

    // Editor render cache for incremental updates (Phase 96 optimization)
    let mut editor_render_cache = optimized_render::EditorRenderCache::new();
    let mut last_view_len = 0usize;
    let mut last_cursor_col = 0usize;
    let mut last_editor_active = false;

    #[cfg(feature = "console_vga")]
    let mut vga_backbuffer = [0u16; console_vga::VGA_WIDTH * console_vga::VGA_HEIGHT];
    #[cfg(feature = "console_vga")]
    let mut vga_shadow =
        unsafe { console_vga::VgaConsole::new(vga_backbuffer.as_mut_ptr() as usize) };

    let mut fb_shadow: Option<framebuffer::BareMetalFramebuffer> = None;
    // GFX-018: presents of the shadow are paced, not tied 1:1 to render passes.
    let mut present_pacer =
        present_policy::FramePacer::new(present_policy::PresentPolicy::DEFAULT_BARE_METAL);

    // GFX-020: text console vs graphics desktop. Graphics needs a framebuffer.
    let graphics_available = fb_console.is_some();
    let mut display_mode = match kernel.boot.display_mode {
        Some(mode) if mode.is_graphics() && !graphics_available => {
            kprintln!(
                serial,
                "display: graphics requested but no framebuffer; using text"
            );
            display_mode::DisplayMode::TextConsole
        }
        Some(mode) => mode,
        None => display_mode::DisplayMode::DEFAULT,
    };
    kprintln!(serial, "display: {} mode", display_mode.label());
    workspace.set_graphics_available(graphics_available);
    workspace.set_display_mode(display_mode);
    let mut desktop_renderer: Option<desktop_frame::DesktopFrameRenderer> = None;
    // The desk (GFX-050) is built when the desk mode first renders, because
    // it needs the surface size, and lives for the session after that so
    // windows keep their places across mode switches.
    let mut desk: Option<desk::Desk> = None;
    let mut rtc_port = hal_x86_64::RealPortIo::new();
    // What the desk asked the kernel for this iteration: file operations,
    // listings, a display switch. Served after input, once, in order.
    let mut desk_requests: alloc::vec::Vec<desk::DeskRequest> = alloc::vec::Vec::new();
    // The desk's look is read from disk once, on the first desk frame.
    let mut look_loaded = false;
    // GFX-048: degrade in a fixed order under memory pressure instead of
    // failing inside an allocation.
    let mut pressure_monitor = services_gui_host::PressureMonitor::new(
        services_gui_host::PressureThresholds::DEFAULT,
        PRESSURE_HYSTERESIS_SAMPLES,
    );
    // GFX-049: display path telemetry, read by `gfx stats`.
    let mut gfx_telemetry = services_gui_host::GfxTelemetry::new();
    if display_mode.is_graphics() {
        if let Some(fb) = fb_console.as_ref() {
            let info = fb.info();
            let bytes = services_gui_host::memory::rgba_surface_bytes(info.width, info.height);
            if let Err(err) = surface_budget.reserve("desktop target", bytes) {
                kprintln!(
                    serial,
                    "gfx budget: desktop target refused at boot: {:?}; using text",
                    err
                );
                display_mode = display_mode::DisplayMode::TextConsole;
                workspace.set_display_mode(display_mode);
            }
        }
    }

    // GFX-022: pointer path. IRQ 12 queues raw bytes; the loop frames packets
    // and translates them into absolute pointer events confined to the display.
    let (pointer_width, pointer_height) = match fb_console.as_ref() {
        Some(fb) => {
            let info = fb.info();
            (info.width as u32, info.height as u32)
        }
        None => (
            console_vga::VGA_WIDTH as u32 * 8,
            console_vga::VGA_HEIGHT as u32 * 16,
        ),
    };
    let mut mouse_parser =
        hal_x86_64::Ps2MousePacketParser::new(mouse_config.is_some_and(|report| report.wheel));
    let mut pointer_translator = hal::PointerTranslator::new(pointer_width, pointer_height);
    // GFX-024: explicit pointer focus / keyboard focus / capture policy.
    let mut input_router = services_gui_host::DesktopInputRouter::new();
    // GFX-030: the caret blinks on a tick timeline; the clock tells the loop
    // when the next redraw is due instead of rendering every tick.
    let mut caret_blink = services_gui_host::Blink::new(CARET_BLINK_PERIOD_TICKS);
    let mut animation_clock = services_gui_host::AnimationClock::new();
    workspace.set_pointer_available(mouse_config.is_some());
    workspace.set_pointer_state(
        pointer_translator.position().x,
        pointer_translator.position().y,
        0,
    );
    if FB_SHADOW_ENABLED {
        if let Some(ref fb) = fb_console {
            let info = fb.info();
            let shadow_bytes = info.buffer_size();
            if let Err(err) = surface_budget.reserve("text shadow", shadow_bytes) {
                kprintln!(serial, "gfx budget: text shadow refused: {:?}", err);
            }
            #[cfg(not(test))]
            if surface_budget.has("text shadow") {
                use alloc::boxed::Box;
                use alloc::vec;

                let buffer = vec![0u8; info.buffer_size()].into_boxed_slice();
                let leaked = Box::leak(buffer);
                fb_shadow = Some(unsafe {
                    framebuffer::BareMetalFramebuffer::from_info_and_buffer(info, leaked)
                });
            }
        }
    }

    // Show initial prompt
    workspace.show_prompt(serial);

    // Seed banner into output so it scrolls like a terminal
    workspace.append_output_text("PandaGen Workspace");
    workspace.append_output_text("Type 'help' for commands");

    KERNEL_READY.store(true, core::sync::atomic::Ordering::Release);

    loop {
        // Run kernel tasks (idle application processors poll them too).
        // Application processors run command tasks; this CPU keeps the
        // console task and the desktop responsive.
        let skip_commands =
            CPUS.online() > 1 && !BSP_RUNS_COMMANDS.load(core::sync::atomic::Ordering::Relaxed);
        let kernel_progressed = kernel.run_once_filtered(serial, skip_commands);

        // Present any shadow content whose pacing interval has elapsed. This
        // is the single hardware present point of the loop.
        if let present_policy::PresentDecision::Present = present_pacer.poll(get_tick_count()) {
            if let Some(fb) = fb_console.as_mut() {
                let started = get_tick_count();
                let started_cycles = hal_x86_64::rdtsc();
                let outcome = if display_mode.is_graphics() {
                    desktop_renderer
                        .as_ref()
                        .map(|renderer| present_desktop_frame(serial, fb, renderer))
                } else {
                    fb_shadow
                        .as_mut()
                        .map(|backbuffer| present_framebuffer_shadow(serial, fb, backbuffer))
                };
                match outcome {
                    Some(true) => {
                        gfx_telemetry.record_present(get_tick_count().saturating_sub(started));
                        gfx_telemetry.record_present_cycles(
                            hal_x86_64::rdtsc().saturating_sub(started_cycles),
                            present_workers() as u32,
                        );
                    }
                    Some(false) => gfx_telemetry.record_present_rejected(),
                    None => {}
                }
            }
        }

        // Process keyboard input
        let mut input_progressed = false;
        while let Some(scancode) = KEYBOARD_EVENT_QUEUE.pop() {
            if let Some(ch) = parser_state.process_scancode(scancode, serial) {
                if KBD_DEBUG_LOG {
                    if ch.is_ascii_graphic() || ch == b' ' {
                        kprintln!(
                            serial,
                            "kbd keyevent scancode={:#x} ch='{}'",
                            scancode,
                            ch as char
                        );
                    } else {
                        kprintln!(serial, "kbd keyevent scancode={:#x} ch={:#x}", scancode, ch);
                    }
                }
                // Build kernel context
                let mut ctx = kernel.context();

                // In desk mode a key belongs to the focused app, or to the
                // desk's own shortcuts, before the workspace sees it.
                let mut desk_took_it = false;
                if display_mode.is_desk() {
                    if let Some(desk) = desk.as_mut() {
                        let (request, changed) = desk.handle_key(ch);
                        desk_took_it = changed || request.is_some();
                        if let Some(request) = request.clone() {
                            desk_requests.push(request);
                        }
                        if let Some(desk::DeskRequest::Terminal(byte)) = request {
                            // The Terminal card is the console: the key goes
                            // where it always went, and the card redraws.
                            let _ = workspace.process_input(byte, &mut ctx, serial);
                            if byte == b'\n' || byte == b'\r' {
                                output_dirty = true;
                            }
                        }
                    }
                }
                input_progressed = if desk_took_it {
                    output_dirty = true;
                    true
                } else {
                    workspace.process_input(ch, &mut ctx, serial)
                };
                #[cfg(debug_assertions)]
                if input_progressed && workspace.is_editor_active() {
                    last_editor_input = Some(ch);
                }

                if KBD_DEBUG_LOG {
                    kprintln!(
                        serial,
                        "kbd runtime consumed={}",
                        if input_progressed { "yes" } else { "no" }
                    );
                }

                // Update display on input change
                if input_progressed {
                    input_dirty = true;
                    if (ch == b'\n' || ch == b'\r') && !workspace.is_editor_active() {
                        output_dirty = true;
                    }
                }

                // Check if palette is now open (for debugging/logging)
                if workspace.is_palette_open() {
                    if KBD_DEBUG_LOG {
                        kprintln!(
                            serial,
                            "palette: OPEN query='{}' results={}",
                            workspace.palette_overlay().query(),
                            workspace.palette_overlay().result_count()
                        );
                    }
                }
            }
        }

        // Check for responses from command service
        let mut ctx = kernel.context();

        // Service the network (ARP, ping, UDP echo, remote calls).
        #[cfg(all(not(test), target_os = "none"))]
        remote_server.poll(
            &mut ctx,
            serial,
            command_channel,
            bare_metal_net::SystemStatus {
                cpus_online: CPUS.online(),
                cpus_total: CPU_TOTAL.load(core::sync::atomic::Ordering::Relaxed),
                uptime_ticks: get_tick_count(),
                heap_used: GLOBAL_HEAP.stats().used,
                heap_total: GLOBAL_HEAP.stats().total,
                frames: gfx_telemetry.frames_rendered,
                presents: gfx_telemetry.presents,
                storage: *STORAGE_BACKEND.lock(),
            },
            get_tick_count(),
        );

        // Try to receive response
        if let Some(message) = ctx.try_recv(workspace_response) {
            if let KernelMessage::CommandResponse(response) = message {
                match response.status {
                    CommandStatus::Ok => {
                        if let Some(output) = response.output_str() {
                            let _ = serial.write_str(output);
                            let _ = serial.write_str("\r\n");
                            workspace.append_output_text(output);
                            output_dirty = true;
                        }
                    }
                    CommandStatus::Error(err) => {
                        let _ = serial.write_str("error: ");
                        if let Some(msg) = err.as_str() {
                            let _ = serial.write_str(msg);
                            workspace.append_output_text(msg);
                        } else {
                            let _ = serial.write_str("invalid error");
                            workspace.append_output_text("invalid error");
                        }
                        let _ = serial.write_str("\r\n");
                        output_dirty = true;
                    }
                }
                workspace.show_prompt(serial);
                input_dirty = true;
            }
        }

        // Drain PS/2 mouse bytes into packets, then into pointer events.
        while let Some(byte) = MOUSE_EVENT_QUEUE.pop() {
            let Some(packet) = mouse_parser.feed(byte) else {
                continue;
            };
            let batch = pointer_translator.translate(packet, input_types::Modifiers::NONE);
            for event in batch.iter() {
                if KBD_DEBUG_LOG {
                    let _ = writeln!(serial, "pointer event: {:?}", event);
                }
                workspace.set_pointer_state(
                    event.position.x,
                    event.position.y,
                    event.buttons.bits(),
                );
                gfx_telemetry.record_pointer_event();
                // Route against the desktop the user currently sees.
                if display_mode.is_desk() {
                    if let Some(renderer) = desktop_renderer.as_ref() {
                        let (w, h) = (renderer.width(), renderer.height());
                        let desk = desk.get_or_insert_with(|| desk::Desk::new(w, h));
                        desk.set_pointer(Some((
                            event.position.x.max(0) as usize,
                            event.position.y.max(0) as usize,
                        )));
                        let terminal = desk
                            .terminal_rows()
                            .map(|rows| terminal_view(&workspace, rows));
                        let windows = desk.windows("", true, terminal.as_ref());
                        let deliveries =
                            input_router.route(renderer.compositor(), &windows, *event);
                        let (requests, changed) = desk.handle_deliveries_with_requests(&deliveries);
                        if changed {
                            output_dirty = true;
                        }
                        for request in requests {
                            desk_requests.push(request);
                        }
                    }
                    input_dirty = true;
                } else if display_mode.is_graphics() {
                    if let Some(renderer) = desktop_renderer.as_ref() {
                        let model = build_desktop_model(
                            &workspace,
                            get_tick_count(),
                            renderer.layout().main_content_rows(),
                        );
                        let windows = renderer.windows(&model);
                        let deliveries =
                            input_router.route(renderer.compositor(), &windows, *event);
                        if KBD_DEBUG_LOG {
                            for delivery in &deliveries {
                                let _ = writeln!(serial, "pointer route: {:?}", delivery);
                            }
                        }
                        // Pointer interaction with shell surfaces (GFX-031/033).
                        let launcher_id = renderer.view_ids().launcher();
                        let palette_id = renderer.view_ids().palette();
                        for delivery in &deliveries {
                            let services_gui_host::Delivery::Pointer {
                                target,
                                event: routed,
                                hit,
                            } = delivery
                            else {
                                continue;
                            };
                            let content_line = hit.and_then(|h| match h.region {
                                services_gui_host::HitRegion::Content { line, .. } => Some(line),
                                _ => None,
                            });
                            let primary_press =
                                routed.is_press(input_types::PointerButton::Primary);

                            if *target == palette_id {
                                let result =
                                    content_line.and_then(desktop_frame::palette_result_at_line);
                                match routed.kind {
                                    input_types::PointerEventKind::Move { .. } => {
                                        if let Some(index) = result {
                                            if workspace.palette_hover_result(index) {
                                                input_dirty = true;
                                            }
                                        }
                                    }
                                    input_types::PointerEventKind::Wheel { dy, .. } => {
                                        if workspace.palette_scroll(dy) {
                                            input_dirty = true;
                                        }
                                    }
                                    _ if primary_press => {
                                        if let Some(index) = result {
                                            let mut ctx = kernel.context();
                                            if workspace
                                                .palette_click_result(index, &mut ctx, serial)
                                            {
                                                input_dirty = true;
                                                output_dirty = true;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                                continue;
                            }

                            if *target == renderer.view_ids().main() && workspace.has_hosted() {
                                let event = match routed.kind {
                                    input_types::PointerEventKind::Move { .. } => content_line
                                        .map(|line| services_gui_host::HostEvent::Hover { line }),
                                    input_types::PointerEventKind::Wheel { dy, .. } => {
                                        Some(services_gui_host::HostEvent::Wheel { notches: dy })
                                    }
                                    _ if primary_press => content_line.map(|line| {
                                        services_gui_host::HostEvent::Activate { line }
                                    }),
                                    _ => None,
                                };
                                if let Some(event) = event {
                                    if workspace.host_event(event, serial) {
                                        input_dirty = true;
                                        output_dirty = true;
                                    }
                                }
                                continue;
                            }

                            if *target == renderer.view_ids().main()
                                && workspace.is_file_picker_open()
                            {
                                let entry =
                                    content_line.and_then(desktop_frame::picker_entry_at_line);
                                match routed.kind {
                                    input_types::PointerEventKind::Move { .. } => {
                                        if let Some(index) = entry {
                                            if workspace.picker_hover(index) {
                                                input_dirty = true;
                                            }
                                        }
                                    }
                                    input_types::PointerEventKind::Wheel { dy, .. } => {
                                        if workspace.picker_scroll(dy) {
                                            input_dirty = true;
                                        }
                                    }
                                    _ if primary_press => {
                                        if let Some(index) = entry {
                                            if workspace.picker_click(index, serial) {
                                                input_dirty = true;
                                                output_dirty = true;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                                continue;
                            }

                            // Wheel over the text-native workspace/CLI surface scrolls history.
                            if *target == renderer.view_ids().main()
                                && !workspace.is_editor_active()
                                && !workspace.is_palette_open()
                            {
                                if let input_types::PointerEventKind::Wheel { dy, .. } = routed.kind
                                {
                                    let visible =
                                        renderer.layout().main_content_rows().saturating_sub(1);
                                    if workspace.scroll_view(dy, visible) {
                                        input_dirty = true;
                                    }
                                    continue;
                                }
                            }

                            if primary_press && workspace.is_palette_open() {
                                // Clicking anywhere else dismisses the palette.
                                if workspace.palette_dismiss() {
                                    input_dirty = true;
                                }
                                continue;
                            }

                            if *target == launcher_id && primary_press {
                                let Some(line) = content_line else {
                                    continue;
                                };
                                let mut ctx = kernel.context();
                                if workspace.activate_launcher_line(line, &mut ctx, serial) {
                                    output_dirty = true;
                                }
                            }
                        }
                        let role_of = |id: Option<view_types::ViewId>| {
                            id.and_then(|id| windows.iter().find(|w| w.frame.view_id == id))
                                .map(|w| w.role.label())
                        };
                        workspace.set_pointer_routing(
                            role_of(input_router.hovered()),
                            role_of(input_router.keyboard_focus()),
                            input_router.capture().is_some(),
                        );
                    }
                    // The cursor is a desktop surface: moving it is a redraw.
                    input_dirty = true;
                }
            }
        }

        // Documents that have sat still save themselves (GFX-060).
        if display_mode.is_desk() {
            if let Some(desk) = desk.as_mut() {
                desk_requests.extend(desk.tick(get_tick_count()));
            }
        }

        // Serve what the desk asked for (GFX-053).
        if !desk_requests.is_empty() {
            let pending: alloc::vec::Vec<desk::DeskRequest> = desk_requests.drain(..).collect();
            let now = get_tick_count();
            // What writes are stamped with (GFX-056): the RTC's date, or 0.
            let now_secs = rtc::read_clock(&mut rtc_port)
                .map(|d| d.unix_seconds())
                .unwrap_or(0);
            for request in pending {
                let Some(desk) = desk.as_mut() else {
                    break;
                };
                match request {
                    desk::DeskRequest::Terminal(_) => {}
                    desk::DeskRequest::ReadVersion { id, name, index } => {
                        let Some(fs) = workspace.take_filesystem() else {
                            continue;
                        };
                        let mut io =
                            bare_metal_editor_io::BareMetalEditorIo::with_clock(fs, now_secs);
                        let versions = io.list_versions(&name).unwrap_or_default();
                        let content = if index < versions.len() {
                            io.read_version(&name, index).ok()
                        } else {
                            None
                        };
                        workspace.set_filesystem(io.into_filesystem());
                        let when = versions.get(index).map(|v| v.0).unwrap_or(0);
                        desk.version_loaded(id, index, versions.len(), content.as_deref(), when);
                        output_dirty = true;
                    }
                    desk::DeskRequest::SaveLook { text } => {
                        if let Some(fs) = workspace.take_filesystem() {
                            let mut io =
                                bare_metal_editor_io::BareMetalEditorIo::with_clock(fs, now_secs);
                            let result = io.write_setting(bare_metal_editor_io::LOOK_FILE, &text);
                            workspace.set_filesystem(io.into_filesystem());
                            match result {
                                Ok(()) => desk.notify(
                                    services_gui_host::NoticeLevel::Success,
                                    alloc::format!("Look kept: {}", text.replace('\n', " ").trim()),
                                    now,
                                ),
                                Err(e) => desk.notify(
                                    services_gui_host::NoticeLevel::Error,
                                    alloc::format!("The look could not be kept: {e:?}"),
                                    now,
                                ),
                            }
                            output_dirty = true;
                        }
                    }
                    desk::DeskRequest::LoadLook => {
                        if let Some(fs) = workspace.take_filesystem() {
                            let mut io =
                                bare_metal_editor_io::BareMetalEditorIo::with_clock(fs, now_secs);
                            let text = io.read_setting(bare_metal_editor_io::LOOK_FILE);
                            let recent = io.read_setting(bare_metal_editor_io::RECENT_FILE);
                            workspace.set_filesystem(io.into_filesystem());
                            desk.apply_look(text.as_deref());
                            desk.apply_recent(recent.as_deref());
                            output_dirty = true;
                        }
                    }
                    desk::DeskRequest::SaveRecent { text } => {
                        if let Some(fs) = workspace.take_filesystem() {
                            let mut io =
                                bare_metal_editor_io::BareMetalEditorIo::with_clock(fs, now_secs);
                            // Quietly: a list of names is not news.
                            let _ = io.write_setting(bare_metal_editor_io::RECENT_FILE, &text);
                            workspace.set_filesystem(io.into_filesystem());
                        }
                    }
                    desk::DeskRequest::TerminalScroll { notches } => {
                        let visible = desk.terminal_rows().unwrap_or(1).saturating_sub(1);
                        if workspace.scroll_view(notches, visible) {
                            output_dirty = true;
                        }
                    }
                    desk::DeskRequest::TextConsole => {
                        workspace.request_display_mode(display_mode::DisplayMode::TextConsole);
                    }
                    desk::DeskRequest::ListFiles { id } => {
                        let entries = match workspace.take_filesystem() {
                            Some(fs) => {
                                let mut io = bare_metal_editor_io::BareMetalEditorIo::new(fs);
                                let entries = io.list_entries().unwrap_or_default();
                                workspace.set_filesystem(io.into_filesystem());
                                entries
                            }
                            None => alloc::vec::Vec::new(),
                        };
                        desk.files_listed(id, entries);
                        output_dirty = true;
                    }
                    op @ (desk::DeskRequest::CreateFile { .. }
                    | desk::DeskRequest::RenameFile { .. }
                    | desk::DeskRequest::TrashFile { .. }
                    | desk::DeskRequest::PurgeFile { .. }
                    | desk::DeskRequest::TagFile { .. }) => {
                        // The file operations share one shape: do it with
                        // the clock, say how it went, list again.
                        let Some(fs) = workspace.take_filesystem() else {
                            continue;
                        };
                        let mut io =
                            bare_metal_editor_io::BareMetalEditorIo::with_clock(fs, now_secs);
                        let (id, name, text, result, opens) = match op {
                            desk::DeskRequest::CreateFile { id, name } => {
                                let result = io.create_empty(&name);
                                (
                                    id,
                                    name.clone(),
                                    alloc::format!("Created {name}"),
                                    result,
                                    true,
                                )
                            }
                            desk::DeskRequest::RenameFile { id, from, to } => {
                                let result = io.rename(&from, &to);
                                (
                                    id,
                                    from.clone(),
                                    alloc::format!("Renamed {from} to {to}"),
                                    result,
                                    false,
                                )
                            }
                            desk::DeskRequest::TrashFile { id, name, trashed } => {
                                let result = io.set_trashed(&name, trashed);
                                let text = if trashed {
                                    alloc::format!("{name} is in the bin")
                                } else {
                                    alloc::format!("{name} is back")
                                };
                                (id, name.clone(), text, result, false)
                            }
                            desk::DeskRequest::PurgeFile { id, name } => {
                                let result = io.delete(&name);
                                (
                                    id,
                                    name.clone(),
                                    alloc::format!("{name} removed for good"),
                                    result,
                                    false,
                                )
                            }
                            desk::DeskRequest::TagFile {
                                id,
                                name,
                                add,
                                remove,
                            } => {
                                let result = io.set_tags(&name, &add, &remove);
                                (
                                    id,
                                    name.clone(),
                                    alloc::format!("Tagged {name}"),
                                    result,
                                    false,
                                )
                            }
                            _ => {
                                workspace.set_filesystem(io.into_filesystem());
                                continue;
                            }
                        };
                        let entries = io.list_entries().unwrap_or_default();
                        workspace.set_filesystem(io.into_filesystem());
                        match result {
                            Ok(()) => {
                                desk.notify(services_gui_host::NoticeLevel::Success, text, now);
                                if opens {
                                    // A new file opens where it will be written.
                                    let notepad_id = desk.launch(desk::DeskApp::Notepad);
                                    desk_requests.push(desk::DeskRequest::Io {
                                        id: notepad_id,
                                        effect: notepad::NotepadEffect::Open { path: name.clone() },
                                    });
                                }
                            }
                            Err(error) => desk.notify(
                                services_gui_host::NoticeLevel::Error,
                                alloc::format!("{text}: failed, {error:?}"),
                                now,
                            ),
                        }
                        desk.files_listed(id, entries);
                        output_dirty = true;
                    }
                    desk::DeskRequest::Io { id, effect } => {
                        let result = match workspace.take_filesystem() {
                            Some(fs) => {
                                let mut io = bare_metal_editor_io::BareMetalEditorIo::with_clock(
                                    fs, now_secs,
                                );
                                let result = match &effect {
                                    notepad::NotepadEffect::Save { path, content } => io
                                        .save_as(path, content)
                                        .map(|_| None)
                                        .map_err(|e| alloc::format!("{e:?}")),
                                    notepad::NotepadEffect::Open { path } => match io.open(path) {
                                        Ok((content, _)) => Ok(Some(content)),
                                        Err(_) => Ok(None),
                                    },
                                    _ => Ok(None),
                                };
                                workspace.set_filesystem(io.into_filesystem());
                                result
                            }
                            None => Err(alloc::string::String::from("no filesystem")),
                        };
                        if let Some(follow_up) = desk.io_done(id, &effect, result, now) {
                            desk_requests.push(follow_up);
                        }
                        output_dirty = true;
                    }
                }
            }
        }

        // Advance any pipeline run through the kernel command service.
        {
            let mut ctx = kernel.context();
            if workspace.pipeline_poll(&mut ctx, serial, get_tick_count()) {
                input_dirty = true;
                output_dirty = true;
            }
        }

        // Publish telemetry for `gfx stats` (GFX-049).
        if workspace.consume_gfx_reset() {
            gfx_telemetry.reset();
        }
        {
            let heap = GLOBAL_HEAP.stats();
            let pacer = present_pacer.stats();
            workspace.set_gfx_snapshot(services_gui_host::GfxSnapshot {
                telemetry: gfx_telemetry,
                pacer_presents: pacer.presents,
                pacer_deferred: pacer.deferred_polls,
                pacer_coalesced: pacer.coalesced_marks,
                pacer_forced: pacer.forced_presents,
                pressure: pressure_monitor.pressure(),
                pressure_transitions: pressure_monitor.transitions(),
                heap_used: heap.used,
                heap_free: heap.free,
                heap_total: heap.total,
                budget_used: surface_budget.used(),
                budget_limit: surface_budget.limit(),
                tick: get_tick_count(),
            });
        }

        // Animation wakes are redraw requests scheduled by the previous frame.
        if display_mode.is_graphics() && animation_clock.poll(get_tick_count()) {
            input_dirty = true;
            gfx_telemetry.record_animation_wake();
        }
        // Typing restarts the blink so the caret is visible right after input.
        if input_progressed {
            caret_blink.restart_at(get_tick_count());
        }

        let editor_active = workspace.is_editor_active();
        let mut clear_terminal = false;

        // Sample memory pressure once per iteration (GFX-048).
        {
            let heap = GLOBAL_HEAP.stats();
            if let Some(level) = pressure_monitor.sample(heap.free, heap.total) {
                kprintln!(
                    serial,
                    "memory pressure: {:?} (free {} KiB of {} KiB)",
                    level,
                    heap.free / 1024,
                    heap.total / 1024
                );
                let degradation = pressure_monitor.degradation();
                if degradation.fall_back_to_text && display_mode.is_graphics() {
                    workspace.push_notice(
                        services_gui_host::NoticeLevel::Error,
                        "Low memory: leaving graphics mode",
                    );
                    display_mode = display_mode::DisplayMode::TextConsole;
                    workspace.set_display_mode(display_mode);
                    workspace.set_editor_viewport_rows(TEXT_EDITOR_VIEWPORT_ROWS);
                    output_dirty = true;
                    input_dirty = true;
                    output_initialized = false;
                    prompt_initialized = false;
                    status_initialized = false;
                    last_output_rows = 0;
                    last_output_seq = 0;
                    clear_terminal = true;
                    editor_render_cache.invalidate();
                    if let Some(shadow) = fb_shadow.as_mut() {
                        shadow.invalidate();
                    }
                } else if display_mode.is_graphics() {
                    input_dirty = true;
                }
            }
        }
        let palette_open = workspace.is_palette_open();
        if last_palette_open && !palette_open {
            // Palette closed: force a full redraw to clear overlay
            output_dirty = true;
            input_dirty = true;
            prompt_initialized = false;
            status_initialized = false;
            clear_terminal = true;
        }
        last_palette_open = palette_open;
        if last_editor_active && !editor_active {
            // Transition from editor to terminal: clear screen + reset caches
            output_dirty = true;
            output_initialized = false;
            prompt_initialized = false;
            status_initialized = false;
            editor_render_cache.invalidate();
            clear_terminal = true;
        }
        last_editor_active = editor_active;

        if workspace.consume_clear_request() {
            output_dirty = true;
            input_dirty = true;
            output_initialized = false;
            prompt_initialized = false;
            status_initialized = false;
            last_output_rows = 0;
            last_output_seq = 0;
            clear_terminal = true;
        }

        if let Some(requested) = workspace.consume_display_mode_request() {
            let mut budget_ok = true;
            if requested.is_graphics() && graphics_available && desktop_renderer.is_none() {
                let bytes = fb_console
                    .as_ref()
                    .map(|fb| {
                        let info = fb.info();
                        services_gui_host::memory::rgba_surface_bytes(info.width, info.height)
                    })
                    .unwrap_or(0);
                if !surface_budget.has("desktop target") {
                    if let Err(err) = surface_budget.reserve("desktop target", bytes) {
                        kprintln!(serial, "gfx budget: desktop target refused: {:?}", err);
                        workspace.push_notice(
                            services_gui_host::NoticeLevel::Error,
                            "Graphics refused: pixel memory budget exceeded",
                        );
                        budget_ok = false;
                    }
                }
            }
            if budget_ok
                && requested != display_mode
                && (graphics_available || !requested.is_graphics())
            {
                display_mode = requested;
                workspace.set_display_mode(requested);
                kprintln!(serial, "display: switched to {} mode", requested.label());
                // Whichever renderer takes over must repaint everything: the
                // hardware buffer holds the other mode's pixels.
                output_dirty = true;
                input_dirty = true;
                output_initialized = false;
                prompt_initialized = false;
                status_initialized = false;
                last_output_rows = 0;
                last_output_seq = 0;
                clear_terminal = true;
                editor_render_cache.invalidate();
                if let Some(shadow) = fb_shadow.as_mut() {
                    shadow.invalidate();
                }
                if !display_mode.is_graphics() {
                    // Back to the text console's editor viewport.
                    workspace.set_editor_viewport_rows(TEXT_EDITOR_VIEWPORT_ROWS);
                }
            }
        }

        // Update display if needed
        if display_mode.is_graphics() && fb_console.is_some() {
            if input_dirty || output_dirty || clear_terminal {
                let (width, height) = {
                    let info = fb_console
                        .as_ref()
                        .expect("framebuffer checked above")
                        .info();
                    (info.width, info.height)
                };
                if desktop_renderer.is_none() {
                    desktop_renderer = desktop_frame::DesktopFrameRenderer::try_new(width, height);
                }
                let Some(renderer) = desktop_renderer.as_mut() else {
                    // Too large a surface for the software backend. Say so
                    // and stay in text, the same way an unaffordable budget
                    // is handled -- rather than aborting the machine.
                    klog!(
                        serial,
                        "gfx: {}x{} is larger than the software backend will take; staying in text mode\r\n",
                        width,
                        height
                    );
                    display_mode = display_mode::DisplayMode::TextConsole;
                    continue;
                };
                let now = get_tick_count();
                // The graphical editor uses the full window height.
                workspace.set_editor_viewport_rows(renderer.layout().main_content_rows());
                animation_clock
                    .wake_after(workspace.stamp_and_expire_notices(now, NOTICE_TTL_TICKS));
                let mut model =
                    build_desktop_model(&workspace, now, renderer.layout().main_content_rows());
                let degradation = pressure_monitor.degradation();
                if degradation.drop_notices {
                    model.notices.clear();
                }
                if degradation.hide_pointer {
                    model.pointer = None;
                }
                model.caret_visible = caret_blink.is_on_at(now);
                animation_clock.wake_after(caret_blink.next_flip_after(now));
                if display_mode.is_desk() {
                    let (w, h) = (renderer.width(), renderer.height());
                    let desk = desk.get_or_insert_with(|| desk::Desk::new(w, h));
                    if !look_loaded {
                        look_loaded = true;
                        desk_requests.push(desk::DeskRequest::LoadLook);
                    }
                    // The desk decides focus; the router mirrors it, so
                    // `apply_focus` below paints the ring where the desk
                    // says. Two records of one thing with one writer.
                    let _ = input_router.set_keyboard_focus(desk.focus());
                    // A clock that is one: the CMOS RTC, with uptime as the
                    // fallback if it never settles.
                    let clock = match rtc::read_time(&mut rtc_port) {
                        Some(t) => alloc::format!("{:02}:{:02}", t.hour, t.minute),
                        None => alloc::format!("up {}:{:02}", now / 6000, (now / 100) % 60),
                    };
                    let terminal = desk
                        .terminal_rows()
                        .map(|rows| terminal_view(&workspace, rows));
                    let shell_notices = workspace.notices();
                    let mut windows = desk.windows_at(
                        &clock,
                        model.caret_visible,
                        terminal.as_ref(),
                        now,
                        &shell_notices,
                    );
                    input_router.apply_focus(&mut windows);
                    renderer.render_windows_with_theme(windows, model.pointer, desk.theme());
                } else {
                    let mut windows = renderer.windows(&model);
                    input_router.apply_focus(&mut windows);
                    renderer.render_windows(windows, model.pointer);
                }
                present_pacer.mark_dirty();
                gfx_telemetry.record_frame();
                // The text renderer's caches no longer describe the screen.
                editor_render_cache.invalidate();
                input_dirty = false;
                output_dirty = false;
            }
        } else if input_dirty || output_dirty || clear_terminal {
            let draw_palette_overlay = workspace.is_palette_open() && input_dirty;
            let mut rendered_editor = false;
            {
                use crate::display_sink::{DisplaySink, VgaDisplaySink};
                let mut vga_sink_storage: Option<VgaDisplaySink> = None;
                let mut sink: Option<&mut dyn DisplaySink> = None;

                // Through the shadow, like every other text surface. The
                // editor used to write straight to VRAM, so the shadow -- a
                // full framebuffer, ~3 MiB, charged against the graphics
                // budget -- sat reserved and unused for as long as the
                // editor was open, and the two buffers were permanently out
                // of step. Nothing presented the stale shadow over the
                // editor today, but only by accident: any caller that marked
                // the pacer dirty while editing would have painted the old
                // workspace over the user's text.
                if let Some(shadow) = fb_shadow.as_mut() {
                    sink = Some(shadow as &mut dyn DisplaySink);
                } else if let Some(ref mut fb) = fb_console {
                    sink = Some(*fb);
                } else if let Some(ref mut vga) = vga_console {
                    vga_sink_storage = Some(VgaDisplaySink::new(*vga));
                    sink = vga_sink_storage.as_mut().map(|s| s as &mut dyn DisplaySink);
                }

                if let Some(sink) = sink {
                    // Not while the palette is up. Opening the palette does
                    // not change the active component, so this branch kept
                    // drawing the editor -- and both palette renderers live
                    // behind `!rendered_editor`, so nothing drew the palette
                    // at all. Input was already being routed to it, so the
                    // machine looked frozen: typed characters stopped
                    // reaching the editor, nothing appeared, and Enter ran
                    // whatever was selected in an invisible search box.
                    if workspace.is_editor_active() && !workspace.palette_is_open() {
                        let normal_attr = console_vga::Style::Normal.to_vga_attr();
                        let bold_attr = console_vga::Style::Bold.to_vga_attr();

                        if let Some(editor) = workspace.editor() {
                            // Phase 96+: Use optimized incremental renderer
                            // force_full only on:
                            // - editor open/close (cache invalidated)
                            // - viewport/layout/font/theme changes (must invalidate cache)
                            // - explicit invalidate request (output_dirty on entry)
                            // Keep full redraws out of normal typing/cursor movement paths.
                            let force_full = !editor_render_cache.valid || output_dirty;
                            let current_tick = get_tick_count();
                            let _stats = optimized_render::render_editor_optimized(
                                sink,
                                editor,
                                &mut editor_render_cache,
                                normal_attr,
                                bold_attr,
                                force_full,
                                current_tick,
                            );

                            #[cfg(debug_assertions)]
                            {
                                if KBD_DEBUG_LOG {
                                    let _ = writeln!(
                                        serial,
                                        "editor render: dirty_lines={} spans={} glyphs={} rects={} pixels={} flush={} full={}",
                                        _stats.dirty_lines_count,
                                        _stats.dirty_spans_count,
                                        _stats.glyph_blits_count,
                                        _stats.rect_fills_count,
                                        _stats.pixels_written,
                                        _stats.flush_calls,
                                        _stats.full_redraws
                                    );
                                }
                                if let Some(last_input) = last_editor_input {
                                    if editor.mode() == EditorMode::Insert
                                        && is_typing_byte(last_input)
                                    {
                                        debug_assert!(
                                            _stats.dirty_lines_count <= 1,
                                            "typing must dirty at most one line"
                                        );
                                        debug_assert!(
                                            _stats.full_redraws == 0,
                                            "typing must not trigger full redraw"
                                        );
                                    }
                                }
                            }
                        }
                        input_dirty = false;
                        output_dirty = false;
                        #[cfg(debug_assertions)]
                        {
                            last_editor_input = None;
                        }
                        rendered_editor = true;
                    }
                }
                if rendered_editor && fb_shadow.is_some() {
                    present_pacer.mark_dirty();
                }
            }

            if !rendered_editor {
                // Invalidate editor cache when not rendering editor
                editor_render_cache.invalidate();

                if let Some(ref mut vga) = vga_console {
                    let normal_attr = console_vga::Style::Normal.to_vga_attr();
                    let bold_attr = console_vga::Style::Bold.to_vga_attr();
                    let rows = console_vga::VGA_HEIGHT;
                    let cols = console_vga::VGA_WIDTH;

                    #[cfg(feature = "console_vga")]
                    let vga_target = &mut vga_shadow;

                    if clear_terminal {
                        for row in 0..rows {
                            clear_vga_line(vga_target, row, normal_attr);
                        }
                        last_output_rows = 0;
                        last_output_seq = 0;
                    }

                    // Check if editor is active
                    if workspace.is_editor_active() {
                        // Phase 96: Use optimized renderer via VGA sink wrapper
                        use crate::display_sink::VgaDisplaySink;
                        let mut vga_sink = VgaDisplaySink::new(vga);

                        if let Some(editor) = workspace.editor() {
                            // force_full only on:
                            // - editor open/close (cache invalidated)
                            // - viewport/layout/font/theme changes (must invalidate cache)
                            // - explicit invalidate request (output_dirty on entry)
                            // Typing, Enter, Esc, cursor moves, and :w must stay incremental.
                            let force_full = !editor_render_cache.is_valid() || output_dirty;
                            let current_tick = get_tick_count();
                            let _stats = optimized_render::render_editor_optimized(
                                &mut vga_sink,
                                editor,
                                &mut editor_render_cache,
                                normal_attr,
                                bold_attr,
                                force_full,
                                current_tick,
                            );

                            #[cfg(debug_assertions)]
                            {
                                if KBD_DEBUG_LOG {
                                    let _ = writeln!(
                                    serial,
                                    "vga editor render: dirty_lines={} spans={} glyphs={} rects={} pixels={} flush={} full={}",
                                    _stats.dirty_lines_count,
                                    _stats.dirty_spans_count,
                                    _stats.glyph_blits_count,
                                    _stats.rect_fills_count,
                                    _stats.pixels_written,
                                    _stats.flush_calls,
                                    _stats.full_redraws
                                );
                                }
                                if let Some(last_input) = last_editor_input {
                                    if editor.mode() == EditorMode::Insert
                                        && is_typing_byte(last_input)
                                    {
                                        debug_assert!(
                                            _stats.dirty_lines_count <= 1,
                                            "typing must dirty at most one line"
                                        );
                                        debug_assert!(
                                            _stats.full_redraws == 0,
                                            "typing must not trigger full redraw"
                                        );
                                    }
                                }
                            }
                        }

                        input_dirty = false;
                        output_dirty = false;
                        #[cfg(debug_assertions)]
                        {
                            last_editor_input = None;
                        }
                        continue; // Skip normal workspace rendering
                    }

                    // Render command palette overlay if open
                    if draw_palette_overlay
                        && !clear_terminal
                        && !output_dirty
                        && output_initialized
                    {
                        let palette = workspace.palette_overlay();
                        let overlay_attr = get_palette_vga_attr(workspace.is_cli_active());
                        let _ = render_palette_overlay_vga(
                            vga_target,
                            palette,
                            rows,
                            cols,
                            overlay_attr,
                            last_palette_open,
                            &mut last_palette_query_hash,
                            &mut last_palette_result_count,
                            &mut last_palette_selection,
                        );
                        input_dirty = false;
                        vga.blit_from_cells(&vga_backbuffer);
                        continue; // Skip normal workspace rendering when palette open
                    }

                    // Normal workspace rendering (when editor not active)

                    let max_output_rows = rows.saturating_sub(2);
                    let total = workspace.output_line_count();
                    let output_rows = total.min(max_output_rows);
                    let start = total.saturating_sub(max_output_rows);
                    let status_row = rows.saturating_sub(2);
                    let prompt_row = rows.saturating_sub(1);
                    let output_seq = workspace.output_sequence();
                    let delta_lines = output_seq.saturating_sub(last_output_seq) as usize;

                    let screen_is_full = output_rows == max_output_rows;
                    let screen_was_full = last_output_rows == max_output_rows;
                    let reasonable_delta = delta_lines > 0 && delta_lines < max_output_rows;

                    let can_scroll =
                        output_initialized && screen_is_full && screen_was_full && reasonable_delta;

                    let can_fill_scroll = output_initialized
                        && screen_is_full
                        && !screen_was_full
                        && last_output_rows > 0
                        && reasonable_delta;

                    let can_append = output_initialized
                        && !screen_is_full
                        && delta_lines > 0
                        && delta_lines <= output_rows;

                    if output_dirty {
                        if can_scroll {
                            vga_target.scroll_up(delta_lines, normal_attr);
                            let first_row = max_output_rows - delta_lines;
                            let first_line = total.saturating_sub(delta_lines);
                            for i in 0..delta_lines {
                                let row = first_row + i;
                                let line_idx = first_line + i;
                                if let Some(line) = workspace.output_line(line_idx) {
                                    let bytes = line.as_bytes();
                                    let len = bytes.len().min(cols);
                                    if let Ok(text) = core::str::from_utf8(&bytes[..len]) {
                                        vga_target.write_line_at(row, text, normal_attr);
                                    } else {
                                        clear_vga_line(vga_target, row, normal_attr);
                                    }
                                } else {
                                    clear_vga_line(vga_target, row, normal_attr);
                                }
                            }
                        } else if can_fill_scroll {
                            let overflow = total.saturating_sub(max_output_rows);
                            if overflow > 0 {
                                vga_target.scroll_up(overflow, normal_attr);
                            }
                            let lines_to_draw = delta_lines.min(max_output_rows);
                            let first_row = max_output_rows - lines_to_draw;
                            let first_line = total.saturating_sub(lines_to_draw);
                            for i in 0..lines_to_draw {
                                let row = first_row + i;
                                let line_idx = first_line + i;
                                if let Some(line) = workspace.output_line(line_idx) {
                                    let bytes = line.as_bytes();
                                    let len = bytes.len().min(cols);
                                    if let Ok(text) = core::str::from_utf8(&bytes[..len]) {
                                        vga_target.write_line_at(row, text, normal_attr);
                                    } else {
                                        clear_vga_line(vga_target, row, normal_attr);
                                    }
                                } else {
                                    clear_vga_line(vga_target, row, normal_attr);
                                }
                            }
                        } else if can_append {
                            let first_row = output_rows.saturating_sub(delta_lines);
                            for i in 0..delta_lines {
                                let row = first_row + i;
                                let line_idx = start + row;
                                if let Some(line) = workspace.output_line(line_idx) {
                                    let bytes = line.as_bytes();
                                    let len = bytes.len().min(cols);
                                    if let Ok(text) = core::str::from_utf8(&bytes[..len]) {
                                        vga_target.write_line_at(row, text, normal_attr);
                                    } else {
                                        clear_vga_line(vga_target, row, normal_attr);
                                    }
                                } else {
                                    clear_vga_line(vga_target, row, normal_attr);
                                }
                            }
                        } else {
                            for row in 0..output_rows {
                                let line_idx = start + row;
                                if let Some(line) = workspace.output_line(line_idx) {
                                    let bytes = line.as_bytes();
                                    let len = bytes.len().min(cols);
                                    if let Ok(text) = core::str::from_utf8(&bytes[..len]) {
                                        vga_target.write_line_at(row, text, normal_attr);
                                    } else {
                                        clear_vga_line(vga_target, row, normal_attr);
                                    }
                                } else {
                                    clear_vga_line(vga_target, row, normal_attr);
                                }
                            }
                        }
                        output_initialized = true;
                        last_output_rows = output_rows;
                        last_output_seq = output_seq;
                    }
                    let status_line = workspace.status_line();
                    let status_display = if status_line.len() > cols {
                        &status_line[..cols]
                    } else {
                        status_line
                    };
                    let status_full = !status_initialized
                        || output_dirty
                        || workspace.is_cli_active() != last_status_cli_active;
                    if status_full {
                        vga_target.write_line_at(status_row, status_display, bold_attr);
                        status_initialized = true;
                        last_status_cli_active = workspace.is_cli_active();
                    }

                    let prompt_prefix = workspace.prompt_prefix();
                    let prompt_prefix_bytes = prompt_prefix.as_bytes();
                    let prefix_len = prompt_prefix_bytes.len();
                    let cmd_bytes = workspace.get_command_text();
                    let (view_start, cmd_slice, cursor_col) =
                        prompt_view(cmd_bytes, cols, prefix_len);
                    let view_len = cmd_slice.len();
                    let prompt_full = !prompt_initialized
                        || output_dirty
                        || prompt_row != last_prompt_row
                        || view_start != last_view_start
                        || workspace.is_cli_active() != last_prompt_cli_active;

                    if prompt_full {
                        vga_target.write_line_at(prompt_row, "", normal_attr);
                        vga_target.write_str_at(0, prompt_row, prompt_prefix, bold_attr);
                        if let Ok(cmd_str) = core::str::from_utf8(cmd_slice) {
                            vga_target.write_str_at(prefix_len, prompt_row, cmd_str, normal_attr);
                        }
                    } else {
                        vga_target.write_str_at(0, prompt_row, prompt_prefix, bold_attr);
                        for (idx, &byte) in cmd_slice.iter().enumerate() {
                            vga_target.write_at(prefix_len + idx, prompt_row, byte, normal_attr);
                        }
                        if last_view_len > view_len {
                            for col in (prefix_len + view_len)..(prefix_len + last_view_len) {
                                vga_target.write_at(col, prompt_row, b' ', normal_attr);
                            }
                        }

                        if last_cursor_col != cursor_col {
                            let ch = if last_cursor_col >= prefix_len {
                                let idx = last_cursor_col.saturating_sub(prefix_len);
                                if idx < view_len {
                                    cmd_slice[idx]
                                } else {
                                    b' '
                                }
                            } else {
                                *prompt_prefix_bytes.get(last_cursor_col).unwrap_or(&b' ')
                            };
                            vga_target.write_at(last_cursor_col, prompt_row, ch, normal_attr);
                        }
                    }

                    vga_target.draw_cursor(cursor_col, prompt_row, normal_attr);
                    prompt_initialized = true;
                    last_prompt_row = prompt_row;
                    last_view_start = view_start;
                    last_view_len = view_len;
                    last_cursor_col = cursor_col;
                    last_prompt_cli_active = workspace.is_cli_active();

                    if draw_palette_overlay {
                        let palette = workspace.palette_overlay();
                        let overlay_attr = get_palette_vga_attr(workspace.is_cli_active());
                        let _ = render_palette_overlay_vga(
                            vga_target,
                            palette,
                            rows,
                            cols,
                            overlay_attr,
                            last_palette_open,
                            &mut last_palette_query_hash,
                            &mut last_palette_result_count,
                            &mut last_palette_selection,
                        );
                        input_dirty = false;
                    }

                    vga.blit_from_cells(&vga_backbuffer);
                } else if let Some(ref mut fb) = fb_console {
                    // Render workspace state to framebuffer
                    let bg = (0x00, 0x20, 0x40);
                    let fg = (0xFF, 0xFF, 0xFF);
                    let accent = (0x80, 0xFF, 0x80);
                    let fb_target = fb_shadow.as_mut().unwrap_or(fb);
                    let rows = fb_target.rows();
                    let cols = fb_target.cols();

                    // Render command palette overlay if open
                    if draw_palette_overlay
                        && !clear_terminal
                        && !output_dirty
                        && output_initialized
                    {
                        let palette = workspace.palette_overlay();
                        let (overlay_bg, overlay_fg) =
                            get_palette_fb_colors(workspace.is_cli_active());
                        let _ = render_palette_overlay_fb(
                            fb_target,
                            palette,
                            rows,
                            cols,
                            overlay_bg,
                            overlay_fg,
                            last_palette_open,
                            &mut last_palette_query_hash,
                            &mut last_palette_result_count,
                            &mut last_palette_selection,
                        );
                        input_dirty = false;
                        if fb_shadow.is_some() {
                            present_pacer.mark_dirty();
                        }
                        continue; // Skip normal workspace rendering when palette open
                    }
                    let max_output_rows = rows.saturating_sub(2);
                    let total = workspace.output_line_count();
                    let output_rows = total.min(max_output_rows);
                    let start = total.saturating_sub(max_output_rows);
                    let status_row = rows.saturating_sub(2);
                    let prompt_row = rows.saturating_sub(1);
                    let output_seq = workspace.output_sequence();
                    let delta_lines = output_seq.saturating_sub(last_output_seq) as usize;

                    // Determine rendering strategy - prefer incremental over full redraw
                    //
                    // Fast paths (only draw changed content):
                    // - scroll: screen was full, is full, new lines added
                    // - append: screen not full, add new lines at bottom
                    // - fill_scroll: screen just became full, scroll + draw new
                    //
                    // Slow path:
                    // - full: first render or complex state change

                    let screen_is_full = output_rows == max_output_rows;
                    let screen_was_full = last_output_rows == max_output_rows;
                    let reasonable_delta = delta_lines > 0 && delta_lines < max_output_rows;

                    let can_scroll =
                        output_initialized && screen_is_full && screen_was_full && reasonable_delta;

                    // Screen just became full - scroll by overflow amount
                    let can_fill_scroll = output_initialized
                        && screen_is_full
                        && !screen_was_full
                        && last_output_rows > 0
                        && reasonable_delta;

                    // Screen not full yet, just append
                    let can_append = output_initialized
                        && !screen_is_full
                        && delta_lines > 0
                        && delta_lines <= output_rows;

                    if output_dirty {
                        if clear_terminal {
                            for row in 0..rows {
                                clear_fb_line(fb_target, row, cols, bg, fg);
                            }
                            last_output_rows = 0;
                            last_output_seq = 0;
                        }
                        if can_scroll {
                            // Scroll up and draw only new bottom lines
                            fb_target.scroll_up_text_lines(delta_lines, bg);
                            let first_row = max_output_rows - delta_lines;
                            let first_line = total.saturating_sub(delta_lines);
                            for i in 0..delta_lines {
                                let row = first_row + i;
                                let line_idx = first_line + i;
                                if let Some(line) = workspace.output_line(line_idx) {
                                    let bytes = line.as_bytes();
                                    let len = bytes.len().min(cols);
                                    if let Ok(text) = core::str::from_utf8(&bytes[..len]) {
                                        fb_target.draw_line(row, text, fg, bg);
                                    }
                                } else {
                                    fb_target.draw_line(row, "", fg, bg);
                                }
                            }
                        } else if can_fill_scroll {
                            // Screen just became full - scroll the overflow and draw new lines
                            let overflow = total.saturating_sub(max_output_rows);
                            if overflow > 0 {
                                fb_target.scroll_up_text_lines(overflow, bg);
                            }
                            // Draw only the new lines at bottom
                            let lines_to_draw = delta_lines.min(max_output_rows);
                            let first_row = max_output_rows - lines_to_draw;
                            let first_line = total.saturating_sub(lines_to_draw);
                            for i in 0..lines_to_draw {
                                let row = first_row + i;
                                let line_idx = first_line + i;
                                if let Some(line) = workspace.output_line(line_idx) {
                                    let bytes = line.as_bytes();
                                    let len = bytes.len().min(cols);
                                    if let Ok(text) = core::str::from_utf8(&bytes[..len]) {
                                        fb_target.draw_line(row, text, fg, bg);
                                    }
                                } else {
                                    fb_target.draw_line(row, "", fg, bg);
                                }
                            }
                        } else if can_append {
                            // Just draw new lines at bottom (no scroll needed)
                            let first_row = output_rows.saturating_sub(delta_lines);
                            for i in 0..delta_lines {
                                let row = first_row + i;
                                if let Some(line) = workspace.output_line(row) {
                                    let bytes = line.as_bytes();
                                    let len = bytes.len().min(cols);
                                    if let Ok(text) = core::str::from_utf8(&bytes[..len]) {
                                        fb_target.draw_line(row, text, fg, bg);
                                    }
                                } else {
                                    fb_target.draw_line(row, "", fg, bg);
                                }
                            }
                        } else {
                            // Full redraw (first render or complex change)
                            for row in 0..output_rows {
                                let line_idx = start + row;
                                if let Some(line) = workspace.output_line(line_idx) {
                                    let bytes = line.as_bytes();
                                    let len = bytes.len().min(cols);
                                    if let Ok(text) = core::str::from_utf8(&bytes[..len]) {
                                        fb_target.draw_line(row, text, fg, bg);
                                    }
                                } else {
                                    fb_target.draw_line(row, "", fg, bg);
                                }
                            }
                        }
                        output_initialized = true;
                        last_output_rows = output_rows;
                        last_output_seq = output_seq;
                    }
                    let status_line = workspace.status_line();
                    let status_display = if status_line.len() > cols {
                        &status_line[..cols]
                    } else {
                        status_line
                    };
                    let status_full = !status_initialized
                        || output_dirty
                        || workspace.is_cli_active() != last_status_cli_active;
                    if status_full {
                        clear_fb_line(fb_target, status_row, cols, bg, fg);
                        fb_target.draw_text_at(0, status_row, status_display, accent, bg);
                        status_initialized = true;
                        last_status_cli_active = workspace.is_cli_active();
                    }

                    let prompt_prefix = workspace.prompt_prefix();
                    let prompt_prefix_bytes = prompt_prefix.as_bytes();
                    let prefix_len = prompt_prefix_bytes.len();
                    let cmd_bytes = workspace.get_command_text();
                    let (view_start, cmd_slice, cursor_col) =
                        prompt_view(cmd_bytes, cols, prefix_len);
                    let view_len = cmd_slice.len();
                    let prompt_full = !prompt_initialized
                        || output_dirty
                        || prompt_row != last_prompt_row
                        || view_start != last_view_start
                        || workspace.is_cli_active() != last_prompt_cli_active;

                    if prompt_full {
                        clear_fb_line(fb_target, prompt_row, cols, bg, fg);
                        fb_target.draw_text_at(0, prompt_row, prompt_prefix, accent, bg);
                        if let Ok(cmd_str) = core::str::from_utf8(cmd_slice) {
                            fb_target.draw_text_at(prefix_len, prompt_row, cmd_str, fg, bg);
                        }
                    } else {
                        fb_target.draw_text_at(0, prompt_row, prompt_prefix, accent, bg);
                        for (idx, &byte) in cmd_slice.iter().enumerate() {
                            fb_target.draw_char_at(prefix_len + idx, prompt_row, byte, fg, bg);
                        }
                        if last_view_len > view_len {
                            for col in (prefix_len + view_len)..(prefix_len + last_view_len) {
                                fb_target.draw_char_at(col, prompt_row, b' ', fg, bg);
                            }
                        }

                        if last_cursor_col != cursor_col {
                            let ch = if last_cursor_col >= prefix_len {
                                let idx = last_cursor_col.saturating_sub(prefix_len);
                                if idx < view_len {
                                    cmd_slice[idx]
                                } else {
                                    b' '
                                }
                            } else {
                                *prompt_prefix_bytes.get(last_cursor_col).unwrap_or(&b' ')
                            };
                            fb_target.draw_char_at(last_cursor_col, prompt_row, ch, fg, bg);
                        }
                    }

                    fb_target.draw_cursor(cursor_col, prompt_row, fg, bg);
                    prompt_initialized = true;
                    last_prompt_row = prompt_row;
                    last_view_start = view_start;
                    last_view_len = view_len;
                    last_cursor_col = cursor_col;
                    last_prompt_cli_active = workspace.is_cli_active();

                    if draw_palette_overlay {
                        let palette = workspace.palette_overlay();
                        let (overlay_bg, overlay_fg) =
                            get_palette_fb_colors(workspace.is_cli_active());
                        let _ = render_palette_overlay_fb(
                            fb_target,
                            palette,
                            rows,
                            cols,
                            overlay_bg,
                            overlay_fg,
                            last_palette_open,
                            &mut last_palette_query_hash,
                            &mut last_palette_result_count,
                            &mut last_palette_selection,
                        );
                        input_dirty = false;
                    }

                    if fb_shadow.is_some() {
                        present_pacer.mark_dirty();
                        gfx_telemetry.record_frame();
                    }
                }
            } // End !rendered_editor

            input_dirty = false;
            output_dirty = false;
        }

        if !kernel_progressed && !input_progressed && !present_pacer.is_pending() {
            idle_pause();
        }
    }
}

/// Simple editor state for keyboard demo
#[cfg(not(test))]
struct EditorState {
    buffer: [u8; 1024],
    len: usize,
    cursor: usize,
    pending_e0: bool,
}

#[cfg(not(test))]
impl EditorState {
    fn new() -> Self {
        Self {
            buffer: [0; 1024],
            len: 0,
            cursor: 0,
            pending_e0: false,
        }
    }

    fn insert_char(&mut self, ch: u8) {
        if self.len < self.buffer.len() {
            // Shift text right if needed
            if self.cursor < self.len {
                for i in (self.cursor..self.len).rev() {
                    self.buffer[i + 1] = self.buffer[i];
                }
            }
            self.buffer[self.cursor] = ch;
            self.len += 1;
            self.cursor += 1;
        }
    }

    fn delete_char(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            for i in self.cursor..self.len - 1 {
                self.buffer[i] = self.buffer[i + 1];
            }
            if self.len > 0 {
                self.len -= 1;
            }
        }
    }

    fn get_text(&self) -> &[u8] {
        &self.buffer[..self.len]
    }
}

/// Main editor loop with keyboard input
#[cfg(not(test))]
fn editor_loop(serial: &mut serial::SerialPort, _kernel: &mut Kernel) -> ! {
    let mut editor = EditorState::new();
    let mut last_render = 0u64;
    let mut parser_state = Ps2ParserState::new();

    loop {
        // Drain keyboard queue and process scancodes
        let mut events_processed = 0;
        while let Some(scancode) = KEYBOARD_EVENT_QUEUE.pop() {
            if let Some(ch) = parser_state.process_scancode(scancode, serial) {
                if ch == 0x08 {
                    // Backspace
                    editor.delete_char();
                } else {
                    editor.insert_char(ch);
                }
                events_processed += 1;
            }
        }

        // Render on change (rate-limited to every 10 ticks = 100ms)
        let current_tick = get_tick_count();
        if events_processed > 0 && current_tick >= last_render + 10 {
            render_editor(serial, &editor);
            last_render = current_tick;
        }

        idle_pause();
    }
}

fn clear_vga_line(vga: &mut console_vga::VgaConsole, row: usize, attr: u8) {
    vga.clear_row(row, attr);
}

fn clear_fb_line(
    fb: &mut framebuffer::BareMetalFramebuffer,
    row: usize,
    _cols: usize,
    bg: (u8, u8, u8),
    _fg: (u8, u8, u8),
) {
    // Optimized: use fast row fill instead of drawing space characters
    fb.clear_text_row(row, bg);
}

#[cfg(feature = "console_vga")]
/// Helper function to get palette overlay colors based on CLI mode
fn get_palette_vga_attr(is_cli_active: bool) -> u8 {
    if is_cli_active {
        console_vga::VgaColor::make_attr(console_vga::VgaColor::White, console_vga::VgaColor::Green)
    } else {
        console_vga::VgaColor::make_attr(console_vga::VgaColor::White, console_vga::VgaColor::Blue)
    }
}

/// Helper function to get palette overlay colors for framebuffer based on CLI mode
fn get_palette_fb_colors(is_cli_active: bool) -> ((u8, u8, u8), (u8, u8, u8)) {
    let overlay_fg = (0xFF, 0xFF, 0xFF);
    let overlay_bg = if is_cli_active {
        (0x10, 0x60, 0x20) // Green background
    } else {
        (0x10, 0x40, 0x80) // Blue background
    };
    (overlay_bg, overlay_fg)
}

#[cfg(feature = "console_vga")]
#[allow(clippy::too_many_arguments)]
fn render_palette_overlay_vga(
    vga: &mut console_vga::VgaConsole,
    palette: &crate::palette_overlay::PaletteOverlayState,
    rows: usize,
    cols: usize,
    overlay_attr: u8,
    last_palette_open: bool,
    last_palette_query_hash: &mut u64,
    last_palette_result_count: &mut usize,
    last_palette_selection: &mut usize,
) -> bool {
    let overlay_width = 60.min(cols);
    let overlay_col = (cols - overlay_width) / 2;
    let results = palette.displayed_results();
    // Every result the palette is willing to display, as far as the screen
    // allows. This drew four, while the selection could reach the tenth: from
    // the fifth onwards there was no marker anywhere on screen and Enter ran
    // a command the user could not see.
    let max_results = crate::palette_overlay::MAX_DISPLAYED_RESULTS.min(rows.saturating_sub(4));
    let overlay_height = 3 + max_results + 1; // context header + query + results + help
    let overlay_start_row = (rows.saturating_sub(overlay_height)) / 2;

    let query_text = palette.query();
    let query_hash = palette_query_hash(query_text);
    let result_count = palette.result_count();
    let selected_idx = palette.selection_index();

    let palette_stable = last_palette_open
        && query_hash == *last_palette_query_hash
        && result_count == *last_palette_result_count;

    if palette_stable && selected_idx == *last_palette_selection {
        return false;
    }

    if palette_stable {
        let prev_idx = *last_palette_selection;
        for &idx in &[prev_idx, selected_idx] {
            if idx < max_results {
                if let Some(result) = results.get(idx) {
                    let row = overlay_start_row + 2 + idx; // +2 for context and query headers
                    let indicator = if idx == selected_idx { "> " } else { "  " };
                    vga.write_str_at(overlay_col, row, indicator, overlay_attr);
                    vga.write_str_at(
                        overlay_col + indicator.len(),
                        row,
                        &result.name,
                        overlay_attr,
                    );
                }
            }
        }
    } else {
        for row in 0..overlay_height {
            let target_row = overlay_start_row + row;
            if target_row < rows {
                clear_vga_line(vga, target_row, overlay_attr);
            }
        }

        // Context header
        let context_header = palette.context_header();
        vga.write_str_at(overlay_col, overlay_start_row, context_header, overlay_attr);

        // Query line
        let header = "Search: ";
        vga.write_str_at(overlay_col, overlay_start_row + 1, header, overlay_attr);
        if !query_text.is_empty() {
            vga.write_str_at(
                overlay_col + header.len(),
                overlay_start_row + 1,
                query_text,
                overlay_attr,
            );
        }

        for idx in 0..max_results {
            let row = overlay_start_row + 2 + idx;
            if let Some(result) = results.get(idx) {
                let indicator = if idx == selected_idx { "> " } else { "  " };
                vga.write_str_at(overlay_col, row, indicator, overlay_attr);
                vga.write_str_at(
                    overlay_col + indicator.len(),
                    row,
                    &result.name,
                    overlay_attr,
                );
            }
        }

        let help_row = overlay_start_row + 2 + max_results;
        let help = "[ESC] Close  [Enter] Execute";
        vga.write_str_at(overlay_col, help_row, help, overlay_attr);
    }

    *last_palette_query_hash = query_hash;
    *last_palette_result_count = result_count;
    *last_palette_selection = selected_idx;
    true
}

#[allow(clippy::too_many_arguments)]
fn render_palette_overlay_fb(
    fb: &mut framebuffer::BareMetalFramebuffer,
    palette: &crate::palette_overlay::PaletteOverlayState,
    rows: usize,
    cols: usize,
    overlay_bg: (u8, u8, u8),
    overlay_fg: (u8, u8, u8),
    last_palette_open: bool,
    last_palette_query_hash: &mut u64,
    last_palette_result_count: &mut usize,
    last_palette_selection: &mut usize,
) -> bool {
    let overlay_width = 60.min(cols);
    let overlay_col = (cols.saturating_sub(overlay_width)) / 2;
    let results = palette.displayed_results();
    let max_results = 4usize.min(rows.saturating_sub(4)); // One more row for context header
    let overlay_height = 3 + max_results + 1; // context header + query + results + help
    let overlay_start_row = (rows.saturating_sub(overlay_height)) / 2;

    let query_text = palette.query();
    let query_hash = palette_query_hash(query_text);
    let result_count = palette.result_count();
    let selected_idx = palette.selection_index();

    let palette_stable = last_palette_open
        && query_hash == *last_palette_query_hash
        && result_count == *last_palette_result_count;

    if palette_stable && selected_idx == *last_palette_selection {
        return false;
    }

    if palette_stable {
        let prev_idx = *last_palette_selection;
        for &idx in &[prev_idx, selected_idx] {
            if idx < max_results {
                if let Some(result) = results.get(idx) {
                    let row = overlay_start_row + 2 + idx; // +2 for context and query headers
                    let indicator = if idx == selected_idx { "> " } else { "  " };
                    fb.draw_text_at(overlay_col, row, indicator, overlay_fg, overlay_bg);
                    fb.draw_text_at(
                        overlay_col + indicator.len(),
                        row,
                        &result.name,
                        overlay_fg,
                        overlay_bg,
                    );
                }
            }
        }
    } else {
        for row in 0..overlay_height {
            let target_row = overlay_start_row + row;
            if target_row < rows {
                clear_fb_line(fb, target_row, cols, overlay_bg, overlay_fg);
            }
        }

        // Context header
        let context_header = palette.context_header();
        fb.draw_text_at(
            overlay_col,
            overlay_start_row,
            context_header,
            overlay_fg,
            overlay_bg,
        );

        // Query line
        let header = "Search: ";
        let max_query_chars = overlay_width.saturating_sub(header.len());
        let query_display = if query_text.len() > max_query_chars {
            &query_text[query_text.len().saturating_sub(max_query_chars)..]
        } else {
            query_text
        };
        fb.draw_text_at(
            overlay_col,
            overlay_start_row + 1,
            header,
            overlay_fg,
            overlay_bg,
        );
        if !query_display.is_empty() {
            fb.draw_text_at(
                overlay_col + header.len(),
                overlay_start_row + 1,
                query_display,
                overlay_fg,
                overlay_bg,
            );
        }

        for idx in 0..max_results {
            let row = overlay_start_row + 2 + idx;
            if let Some(result) = results.get(idx) {
                let indicator = if idx == selected_idx { "> " } else { "  " };
                fb.draw_text_at(overlay_col, row, indicator, overlay_fg, overlay_bg);
                fb.draw_text_at(
                    overlay_col + indicator.len(),
                    row,
                    &result.name,
                    overlay_fg,
                    overlay_bg,
                );
            }
        }

        let help = "[ESC] Close  [Enter] Execute";
        let help_row = overlay_start_row + 2 + max_results;
        fb.draw_text_at(overlay_col, help_row, help, overlay_fg, overlay_bg);
    }

    *last_palette_query_hash = query_hash;
    *last_palette_result_count = result_count;
    *last_palette_selection = selected_idx;
    true
}

/// Present the shadow backbuffer through the desktop presenter (GFX-017).
///
/// This is the only place the workspace loop touches the hardware framebuffer
/// as a whole frame. A rejected present is a contract bug, not a transient
/// condition, so it is logged once per occurrence on serial and counted in
/// render stats rather than silently ignored.
fn present_framebuffer_shadow(
    serial: &mut serial::SerialPort,
    fb: &mut framebuffer::BareMetalFramebuffer,
    backbuffer: &mut framebuffer::BareMetalFramebuffer,
) -> bool {
    // GFX-019: copy only the shadow's damage bounding box, not the full frame.
    match fb.present_shadow_damage(backbuffer) {
        Ok(stats) => {
            render_stats::record_desktop_present(stats.copied_pixels as u64);
            true
        }
        Err(err) => {
            render_stats::record_desktop_present_error();
            let _ = writeln!(serial, "framebuffer present rejected: {:?}", err);
            false
        }
    }
}

/// Commands a remote caller may run.
///
/// `boot` is deliberately absent: it prints the kernel's physical and virtual
/// load addresses and the HHDM offset, which is exactly what turns a memory
/// bug into an exploit.
const REMOTE_ALLOWED_COMMANDS: [&str; 7] = ["help", "mem", "cpus", "heap", "ticks", "net", "spin"];

/// Whether a remote command line is on the read-only allowlist. `net`
/// is allowed only without arguments (status).
fn remote_command_allowed(command: &str) -> bool {
    let mut parts = command.split_whitespace();
    let Some(head) = parts.next() else {
        return false;
    };
    if !REMOTE_ALLOWED_COMMANDS.contains(&head) {
        return false;
    }
    head != "net" || parts.next().is_none()
}

/// One remote call in flight: who asked, and which envelope to answer.
/// Where a remote reply goes.
#[derive(Clone, Copy)]
enum ReplyTarget {
    /// A `remote_ipc` envelope answered by datagram, signed for `caller`.
    Udp {
        src: net_stack::Ipv4,
        src_port: u16,
        envelope_id: ipc::MessageId,
        request_id: ipc::MessageId,
        caller: RemoteToken,
    },
    /// A signed line answered on the same TCP connection.
    ///
    /// The peer is carried with the slot index because the index alone is
    /// not an address: a command may take up to `TIMEOUT_TICKS` to run, and
    /// if the caller's connection goes away inside that window the slot is
    /// reused, so replying by index alone hands a signed command's output to
    /// a stranger on whatever port they happened to connect to.
    Tcp {
        conn: usize,
        peer: net_stack::Ipv4,
        peer_port: u16,
    },
}

struct RemoteInFlight {
    target: ReplyTarget,
    local_id: MessageId,
    started_tick: u64,
}

/// Serves `remote_ipc` calls arriving over UDP by running them through
/// the command service, one at a time.
struct RemoteCommandServer {
    channel: Option<ChannelId>,
    in_flight: Option<RemoteInFlight>,
    served: u64,
    denied: u64,
}

impl RemoteCommandServer {
    const TIMEOUT_TICKS: u64 = 300;

    fn new(channel: Option<ChannelId>) -> Self {
        Self {
            channel,
            in_flight: None,
            served: 0,
            denied: 0,
        }
    }

    #[cfg(all(not(test), target_os = "none"))]
    fn poll(
        &mut self,
        ctx: &mut KernelContext,
        serial: &mut serial::SerialPort,
        command_channel: ChannelId,
        status: bare_metal_net::SystemStatus,
        now: u64,
    ) {
        let Some(channel) = self.channel else {
            return;
        };
        // Deliver a finished command back to its caller.
        if let Some(pending) = self.in_flight.as_ref() {
            let reply = match ctx.try_recv(channel) {
                Some(KernelMessage::CommandResponse(response))
                    if response.correlation_id == pending.local_id =>
                {
                    Some(match response.status {
                        CommandStatus::Ok => Ok(response.output[..response.len].to_vec()),
                        CommandStatus::Error(err) => {
                            Err(alloc::string::String::from(err.as_str().unwrap_or("error")))
                        }
                    })
                }
                Some(_) => None,
                None if now.saturating_sub(pending.started_tick) > Self::TIMEOUT_TICKS => {
                    Some(Err(alloc::string::String::from("timeout")))
                }
                None => None,
            };
            if let Some(result) = reply {
                let pending = self.in_flight.take().unwrap();
                self.respond(pending.target, result);
                self.served += 1;
            }
        }
        // Pick up the next call once idle.
        if self.in_flight.is_some() {
            return;
        }
        let request = {
            let Some(mut guard) = NET.try_lock() else {
                return;
            };
            let Some(net) = guard.as_mut() else {
                return;
            };
            net.service(&get_tick_count, status, serial)
        };
        let Some(request) = request else {
            return;
        };
        let token = *REMOTE_TOKEN.lock();
        let keys = remote_ipc::MasterKey(token.as_bytes());
        let (target, command): (ReplyTarget, alloc::string::String) = match request {
            bare_metal_net::RemoteRequest::Udp(datagram) => {
                let (envelope, caller) =
                    match remote_ipc::envelope_from_bytes(&datagram.bytes, &keys) {
                        Ok(verified) => verified,
                        Err(err) => {
                            self.denied += 1;
                            klog!(serial, "remote: bad envelope ({err})\r\n");
                            return;
                        }
                    };
                if !REMOTE_CALLERS.lock().allows(&caller) {
                    self.denied += 1;
                    klog!(serial, "remote: caller {:?} not allowed\r\n", caller);
                    return;
                }
                // `accept_ordered`, not `accept`: envelope ids are minted
                // from the sender's clock, so a captured datagram goes stale
                // instead of coming back into range when the window rolls.
                // Per caller, so one caller's clock cannot lock out another.
                if !REMOTE_REPLAY.lock().accept_ordered(&caller, envelope.id) {
                    self.denied += 1;
                    klog!(serial, "remote: replayed message dropped\r\n");
                    return;
                }
                let caller = RemoteToken::from_str(&caller);
                let call = match remote_ipc::authorize_call(
                    &envelope,
                    &[remote_ipc::CAP_KERNEL_COMMAND],
                ) {
                    Ok(call) => call,
                    Err(err) => {
                        self.denied += 1;
                        klog!(serial, "remote: denied ({err})\r\n");
                        if let Ok(call) = remote_ipc::decode_call(&envelope) {
                            let target = ReplyTarget::Udp {
                                src: datagram.src,
                                src_port: datagram.src_port,
                                envelope_id: envelope.id,
                                request_id: call.request_id,
                                caller,
                            };
                            self.respond(target, Err(alloc::string::String::from("unauthorized")));
                        }
                        return;
                    }
                };
                let target = ReplyTarget::Udp {
                    src: datagram.src,
                    src_port: datagram.src_port,
                    envelope_id: envelope.id,
                    request_id: call.request_id,
                    caller,
                };
                if call.action != remote_ipc::ACTION_KERNEL_COMMAND_RUN {
                    self.denied += 1;
                    self.respond(
                        target,
                        Err(alloc::string::String::from("command not allowed")),
                    );
                    return;
                }
                let command = core::str::from_utf8(&call.payload).unwrap_or("").trim();
                (target, alloc::string::String::from(command))
            }
            bare_metal_net::RemoteRequest::TcpLine {
                conn,
                peer,
                peer_port,
                line,
            } => {
                let target = ReplyTarget::Tcp {
                    conn,
                    peer,
                    peer_port,
                };
                let text = core::str::from_utf8(&line).unwrap_or("");
                let Some((nonce, caller, command)) = remote_ipc::line::verify(&keys, text) else {
                    self.denied += 1;
                    klog!(serial, "remote: tcp line rejected (bad signature)\r\n");
                    self.respond(target, Err(alloc::string::String::from("unauthorized")));
                    return;
                };
                if !REMOTE_CALLERS.lock().allows(caller) {
                    self.denied += 1;
                    klog!(serial, "remote: caller {:?} not allowed\r\n", caller);
                    self.respond(target, Err(alloc::string::String::from("unauthorized")));
                    return;
                }
                if !REMOTE_REPLAY.lock().accept_key(caller, nonce) {
                    self.denied += 1;
                    klog!(serial, "remote: tcp line rejected (replay)\r\n");
                    self.respond(target, Err(alloc::string::String::from("unauthorized")));
                    return;
                }
                (target, alloc::string::String::from(command.trim()))
            }
        };
        if !remote_command_allowed(&command) {
            self.denied += 1;
            klog!(serial, "remote: refused command {:?}\r\n", command);
            self.respond(
                target,
                Err(alloc::string::String::from("command not allowed")),
            );
            return;
        }
        let local_id = ctx.next_message_id();
        let Some(request) = CommandRequest::from_bytes(command.as_bytes(), local_id, channel)
        else {
            self.respond(target, Err(alloc::string::String::from("command too long")));
            return;
        };
        if ctx
            .send(command_channel, KernelMessage::CommandRequest(request))
            .is_err()
        {
            self.respond(
                target,
                Err(alloc::string::String::from("command queue full")),
            );
            return;
        }
        klog!(serial, "remote: running {:?}\r\n", command);
        self.in_flight = Some(RemoteInFlight {
            target,
            local_id,
            started_tick: now,
        });
    }

    #[cfg(all(not(test), target_os = "none"))]
    fn respond(
        &mut self,
        target: ReplyTarget,
        result: Result<alloc::vec::Vec<u8>, alloc::string::String>,
    ) {
        match target {
            ReplyTarget::Udp {
                src,
                src_port,
                envelope_id,
                request_id,
                caller,
            } => {
                let response = remote_ipc::RemoteResponse { request_id, result };
                let Ok(envelope) = remote_ipc::encode_response(response, envelope_id) else {
                    return;
                };
                let token = *REMOTE_TOKEN.lock();
                let caller_name = core::str::from_utf8(caller.as_bytes()).unwrap_or("");
                let key = remote_ipc::derive_caller_key(token.as_bytes(), caller_name);
                let Ok(bytes) = remote_ipc::envelope_to_bytes(&envelope, caller_name, &key) else {
                    return;
                };
                if let Some(net) = NET.lock().as_mut() {
                    if !net.udp_reply(src, src_port, &bytes) {
                        let mut serial = serial::SerialPort::new(serial::COM1);
                        klog!(
                            serial,
                            "remote: reply of {} bytes not sent\r\n",
                            bytes.len()
                        );
                    }
                }
            }
            ReplyTarget::Tcp {
                conn,
                peer,
                peer_port,
            } => {
                let line = remote_ipc::line::reply(&result);
                if let Some(net) = NET.lock().as_mut() {
                    if !net.tcp_reply_to(conn, peer, peer_port, line.as_bytes()) {
                        let mut serial = serial::SerialPort::new(serial::COM1);
                        klog!(serial, "remote: tcp reply not sent\r\n");
                    }
                }
            }
        }
    }
}

/// How long the boot CPU waits for another CPU to convert a present band
/// before doing it itself.
///
/// This was 100,000,000 polls. Each poll is two atomic loads and a `pause`,
/// so that is seconds of wall clock -- guarding a job that converts one band
/// of a frame, tens of microseconds of work, six orders of magnitude apart.
/// And the wait is paid per band, in order, so one frame could stall on
/// several of them. The steady state makes it easy to hit: by design the APs
/// run workspace commands while the boot CPU runs the display, so a busy AP
/// is normal, not exceptional. The desktop stopped repainting for seconds at
/// a time while keystrokes were still being accepted.
///
/// A present is paced at 100 Hz, so waiting longer than a frame is pointless
/// -- past that the right answer is always to convert it here. Err small:
/// giving up early only costs parallelism, while giving up late freezes what
/// the user is looking at.
const PRESENT_BAND_SPINS: u64 = 20_000;

/// How long `smp run` waits for one job. These sum a few thousand squares --
/// microseconds -- and the wait is paid per job, serially, on the CPU the
/// console is talking to. This was 200,000,000 polls, about five seconds
/// each: the same defect Phase 298 fixed for present bands and left here.
const SMP_JOB_SPINS: u64 = 1_000_000;

/// Workers currently inside `convert_rgba_rows` for the present in flight.
/// The boot CPU waits for this to reach zero before it lets the surfaces
/// those workers are writing to be reused.
static PRESENT_WORKERS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// CPUs a present may be split across right now.
fn present_workers() -> usize {
    if PARALLEL_PRESENT.load(core::sync::atomic::Ordering::Relaxed) {
        CPUS.online().clamp(1, framebuffer::MAX_PRESENT_BANDS)
    } else {
        1
    }
}

/// Run band 0 here and the rest on application processors, falling back
/// to local conversion for any band whose CPU does not answer in time.
fn run_present_bands(bands: &[framebuffer::PresentBand]) {
    if bands.len() <= 1 {
        for band in bands {
            // SAFETY: see `present_rgba8888_bands`.
            unsafe { framebuffer::convert_rgba_rows(band) };
        }
        return;
    }
    {
        let mut slots = PRESENT_BANDS.lock();
        slots[..bands.len()].copy_from_slice(bands);
    }
    let generation = PRESENT_GENERATION
        .fetch_add(1, core::sync::atomic::Ordering::AcqRel)
        .wrapping_add(1);
    let mut ids = [u64::MAX; framebuffer::MAX_PRESENT_BANDS];
    for (i, slot) in ids.iter_mut().enumerate().take(bands.len()).skip(1) {
        *slot = WORK
            .submit(JOB_PRESENT_BAND, (generation << 8) | i as u64)
            .unwrap_or(u64::MAX);
    }
    #[cfg(not(test))]
    if let Some(mut apic) = LAPIC.get() {
        apic.send_ipi_all_excluding_self(IPI_WAKE_VECTOR);
    }
    // SAFETY: see `present_rgba8888_bands`.
    unsafe { framebuffer::convert_rgba_rows(&bands[0]) };
    for (i, id) in ids.iter().enumerate().take(bands.len()).skip(1) {
        if *id == u64::MAX || WORK.wait(*id, PRESENT_BAND_SPINS).is_none() {
            PRESENT_FALLBACKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            // Nobody picked it up in time. Reclaim the ring slot if the job
            // is still queued, then convert the band here.
            if *id != u64::MAX {
                WORK.cancel(*id);
            }
            // SAFETY: as above.
            unsafe { framebuffer::convert_rgba_rows(&bands[i]) };
        }
    }
    // Wait for any worker that already claimed a band to finish with these
    // buffers. Bumping the generation only stops workers that have not yet
    // claimed one; a worker already inside `convert_rgba_rows` is writing to
    // the surfaces this frame published, and the next frame may reallocate
    // them. Bounded, because a hung CPU must not stop the display for ever.
    let mut spins = 0u64;
    while PRESENT_WORKERS.load(core::sync::atomic::Ordering::Acquire) > 0
        && spins < PRESENT_BAND_SPINS
    {
        spins += 1;
        core::hint::spin_loop();
    }
    PRESENT_GENERATION.fetch_add(1, core::sync::atomic::Ordering::Release);
}

fn present_desktop_frame(
    serial: &mut serial::SerialPort,
    fb: &mut framebuffer::BareMetalFramebuffer,
    renderer: &desktop_frame::DesktopFrameRenderer,
) -> bool {
    let surface = framebuffer::DesktopSurface::rgba8888(
        renderer.width(),
        renderer.height(),
        renderer.pixels(),
    );
    match fb.present_desktop_surface_with(surface, present_workers(), run_present_bands) {
        Ok(stats) => {
            render_stats::record_desktop_present(stats.copied_pixels as u64);
            true
        }
        Err(err) => {
            render_stats::record_desktop_present_error();
            let _ = writeln!(serial, "framebuffer present rejected: {:?}", err);
            false
        }
    }
}

/// The console as the desk's Terminal card shows it: the newest lines that
/// fit above the prompt, and the caret on the prompt.
fn terminal_view(workspace: &workspace::WorkspaceSession, rows: usize) -> desk::TerminalView {
    extern crate alloc;
    use alloc::string::String;

    let mut view = desk::TerminalView::default();
    if rows == 0 {
        return view;
    }
    let total = workspace.output_line_count();
    let scrolled = workspace.scrollback_offset().min(total);
    let end = total - scrolled;
    let shown = rows.saturating_sub(1).min(end);
    for index in end - shown..end {
        if let Some(line) = workspace.output_line(index) {
            view.lines
                .push(String::from_utf8_lossy(line.as_bytes()).into_owned());
        }
    }
    let mut prompt = String::from(workspace.prompt_prefix());
    let column = prompt.chars().count() + workspace.get_cursor_col();
    prompt.push_str(&String::from_utf8_lossy(workspace.get_command_text()));
    view.cursor = Some((view.lines.len(), column));
    view.lines.push(prompt);
    view.status = String::from(workspace.status_line());
    view
}

/// Snapshot the workspace into the data model the desktop builder consumes.
fn build_desktop_model(
    workspace: &workspace::WorkspaceSession,
    now_tick: u64,
    content_rows: usize,
) -> desktop_frame::DesktopModel {
    extern crate alloc;
    use alloc::string::String;

    let mut model = desktop_frame::DesktopModel::default();
    for index in 0..workspace.output_line_count() {
        if let Some(line) = workspace.output_line(index) {
            model
                .output_lines
                .push(String::from_utf8_lossy(line.as_bytes()).into_owned());
        }
    }
    let mut prompt = String::from(workspace.prompt_prefix());
    prompt.push_str(&String::from_utf8_lossy(workspace.get_command_text()));
    model.prompt = prompt;
    model.prompt_cursor = workspace.get_cursor_col();
    model.status = String::from(workspace.status_line());
    model.main_title = String::from(if workspace.is_cli_active() {
        "CLI"
    } else {
        "Workspace"
    });

    if workspace.is_editor_active() {
        if let Some(editor) = workspace.editor() {
            model.editor = Some(desktop_frame::EditorModel {
                title: workspace.editor_title(),
                lines: (0..editor.viewport_rows())
                    .map(|row| String::from(editor.get_viewport_line(row).unwrap_or("")))
                    .collect(),
                cursor: editor
                    .get_viewport_cursor()
                    .map(|position| (position.row, position.col)),
                status: String::from(editor.status_line()),
                first_line: editor.scroll_offset(),
                line_count: editor.line_count(),
                dirty: editor.is_dirty(),
            });
        }
    }

    if workspace.is_pointer_available() {
        let (x, y) = workspace.pointer_position();
        model.pointer = Some((x.max(0) as usize, y.max(0) as usize));
    }

    model.launcher = workspace
        .launcher_items()
        .into_iter()
        .map(|(id, name, active)| services_gui_host::LauncherItem {
            label: name,
            command: String::from(id.as_str()),
            active,
        })
        .collect();
    model.notices = workspace.notices();
    // Persistent status card (GFX-034): shown for as long as the condition
    // holds, computed per frame rather than queued and expired.
    if let Some(editor) = workspace.editor().filter(|e| e.is_dirty()) {
        let _ = editor;
        model.notices.insert(
            0,
            services_gui_host::ShellNotice::new(
                services_gui_host::NoticeLevel::Warning,
                alloc::format!("Unsaved changes: {}", workspace.editor_title()),
            )
            .with_title("UNSAVED"),
        );
        model
            .notices
            .truncate(services_gui_host::shell::MAX_NOTICES);
    }
    model.status_right = alloc::format!("{} | t={}", workspace.display_mode().label(), now_tick);

    model.scrollback_offset = workspace.scrollback_offset();
    model.hosted = workspace.hosted_surface(content_rows);
    if let Some(run) = workspace.pipeline_run() {
        model.pipeline = Some(desktop_frame::PipelineModel {
            lines: workspace::pipeline_trace_lines(run),
            running: run.running_index(),
            done: run.done_count(),
            total: run.stages.len(),
            failed: run.failed(),
            finished: run.is_finished(),
        });
    }

    if let Some(picker) = workspace.file_picker() {
        model.picker = Some(desktop_frame::PickerModel {
            breadcrumb: String::new(),
            entries: picker.entries.clone(),
            selection: picker.selection,
        });
    }

    if workspace.is_palette_open() {
        let palette = workspace.palette_overlay();
        model.palette = Some(desktop_frame::PaletteModel {
            header: String::from(palette.context_header()),
            query: String::from(palette.query()),
            results: palette
                .displayed_results()
                .iter()
                .map(|descriptor| descriptor.name.clone())
                .collect(),
            selection: palette.selection_index(),
        });
    }

    model
}

fn prompt_view(cmd: &[u8], cols: usize, prefix_len: usize) -> (usize, &[u8], usize) {
    if cols == 0 {
        return (0, &[], 0);
    }
    if cols <= prefix_len {
        return (0, &[], cols - 1);
    }
    let available = cols.saturating_sub(prefix_len);
    if available == 0 {
        return (0, &[], cols.saturating_sub(1));
    }

    if cmd.len() > available {
        let start = cmd.len() - available;
        let slice = &cmd[start..];
        let mut cursor = prefix_len + available;
        if cursor >= cols {
            cursor = cols - 1;
        }
        (start, slice, cursor)
    } else {
        let slice = cmd;
        let mut cursor = prefix_len + cmd.len();
        if cursor >= cols {
            cursor = cols - 1;
        }
        (0, slice, cursor)
    }
}

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(debug_assertions)]
fn is_typing_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'a'..=b'z'
            | b'A'..=b'Z'
            | b'0'..=b'9'
            | b' '
            | b'.'
            | b','
            | b';'
            | b':'
            | b'\''
            | b'"'
            | b'-'
            | b'_'
            | b'!'
            | b'?'
    )
}

/// Renders editor state to serial using structured view output
///
/// Phase 60: This now uses the unified output model instead of direct printing.
/// The editor state is converted to structured views before rendering.
///
/// # Safety
///
/// This function uses `static mut OUTPUT` which is safe in the current single-task
/// bare-metal kernel_bootstrap context. Only one execution path calls this function
/// sequentially. Future multi-tasking kernel would need either:
/// - Per-task rendering contexts, or
/// - Mutex/spinlock around OUTPUT access, or
/// - Message-passing to a dedicated rendering task
#[cfg(not(test))]
fn render_editor(serial: &mut serial::SerialPort, editor: &EditorState) {
    // Static output handler for revision tracking
    // SAFETY: Single-task bare-metal kernel; no concurrent access possible.
    // This is documented architectural constraint, not an oversight.
    static mut OUTPUT: output::BareMetalOutput = output::BareMetalOutput::new();

    // Convert editor buffer to text lines (simple line splitting)
    // For now, just show as single line for simplicity
    let text = editor.get_text();
    let text_str = core::str::from_utf8(text).unwrap_or("<invalid utf8>");
    let lines: [&str; 1] = [text_str];

    // Cursor position (for now, just show line 0)
    let cursor_line = Some(0);
    let cursor_col = Some(editor.cursor);

    let mut status_buf: [u8; 64] = [0; 64];
    let status = {
        let mut cursor_pos = 0usize;
        // Manually format the status string
        let prefix = b"Cursor: ";
        for &b in prefix {
            if cursor_pos < status_buf.len() {
                status_buf[cursor_pos] = b;
                cursor_pos += 1;
            }
        }
        // Simple number formatting for cursor
        let mut cursor_val = editor.cursor;
        let mut digits = [0u8; 20];
        let mut digit_count = 0;
        if cursor_val == 0 {
            digits[0] = b'0';
            digit_count = 1;
        } else {
            while cursor_val > 0 && digit_count < 20 {
                digits[digit_count] = b'0' + (cursor_val % 10) as u8;
                cursor_val /= 10;
                digit_count += 1;
            }
        }
        // Reverse and copy digits
        for i in 0..digit_count {
            if cursor_pos < status_buf.len() {
                status_buf[cursor_pos] = digits[digit_count - 1 - i];
                cursor_pos += 1;
            }
        }
        let mid = b" | Length: ";
        for &b in mid {
            if cursor_pos < status_buf.len() {
                status_buf[cursor_pos] = b;
                cursor_pos += 1;
            }
        }
        // Format length
        let mut len_val = editor.len;
        let mut len_digits = [0u8; 20];
        let mut len_digit_count = 0;
        if len_val == 0 {
            len_digits[0] = b'0';
            len_digit_count = 1;
        } else {
            while len_val > 0 && len_digit_count < 20 {
                len_digits[len_digit_count] = b'0' + (len_val % 10) as u8;
                len_val /= 10;
                len_digit_count += 1;
            }
        }
        for i in 0..len_digit_count {
            if cursor_pos < status_buf.len() {
                status_buf[cursor_pos] = len_digits[len_digit_count - 1 - i];
                cursor_pos += 1;
            }
        }
        core::str::from_utf8(&status_buf[..cursor_pos]).unwrap_or("status error")
    };

    // Revision counter starts at 0; first render will be revision 1
    static REVISION: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    let revision = REVISION.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;

    // Render using the unified output model
    unsafe {
        OUTPUT.render_to_serial(
            serial,
            &lines,
            cursor_line,
            cursor_col,
            Some(status),
            revision,
        );
    }
}

/// PS/2 scancode parser state for translating to ASCII
struct Ps2ParserState {
    pending_e0: bool,
    shift_pressed: bool,
    ctrl_pressed: bool,
}

impl Ps2ParserState {
    fn new() -> Self {
        Self {
            pending_e0: false,
            shift_pressed: false,
            ctrl_pressed: false,
        }
    }

    /// Process a scancode byte and return ASCII character if available
    fn process_scancode<W: core::fmt::Write>(
        &mut self,
        scancode: u8,
        serial: &mut W,
    ) -> Option<u8> {
        // Log state before processing
        if crate::KBD_DEBUG_LOG {
            let _ = writeln!(
                serial,
                "parser: in={:#x} pending_e0={} shift={}",
                scancode, self.pending_e0, self.shift_pressed
            );
        }

        // E0 prefix handling
        if scancode == 0xE0 {
            self.pending_e0 = true;
            return None;
        }

        let is_break = (scancode & 0x80) != 0;
        let code = scancode & 0x7F;

        // Handle shift state
        if code == 0x2A || code == 0x36 {
            // Left/Right Shift
            self.shift_pressed = !is_break;
            self.pending_e0 = false;
            return None;
        }

        // Handle ctrl state (0x1D = Left Ctrl, E0 0x1D = Right Ctrl)
        if code == 0x1D {
            self.ctrl_pressed = !is_break;
            self.pending_e0 = false;
            return None;
        }

        // E0-prefixed keys: the arrows and Delete (GFX-051). These were
        // dropped outright, which is why every editor on this machine
        // navigated with hjkl. They travel as private bytes above ASCII,
        // which the workspace ignores and the desk's apps understand.
        if self.pending_e0 {
            self.pending_e0 = false;
            if is_break {
                return None;
            }
            // With Shift held the arrows, Home and End grow a selection
            // (GFX-054); they are their own bytes so the app can tell.
            let shift = self.shift_pressed;
            return match (code, shift) {
                (0x48, false) => Some(crate::notepad::KEY_UP),
                (0x50, false) => Some(crate::notepad::KEY_DOWN),
                (0x4B, false) => Some(crate::notepad::KEY_LEFT),
                (0x4D, false) => Some(crate::notepad::KEY_RIGHT),
                (0x48, true) => Some(crate::notepad::KEY_SHIFT_UP),
                (0x50, true) => Some(crate::notepad::KEY_SHIFT_DOWN),
                (0x4B, true) => Some(crate::notepad::KEY_SHIFT_LEFT),
                (0x4D, true) => Some(crate::notepad::KEY_SHIFT_RIGHT),
                (0x47, false) => Some(crate::notepad::KEY_HOME),
                (0x4F, false) => Some(crate::notepad::KEY_END),
                (0x47, true) => Some(crate::notepad::KEY_SHIFT_HOME),
                (0x4F, true) => Some(crate::notepad::KEY_SHIFT_END),
                (0x53, _) => Some(crate::notepad::KEY_DELETE),
                (0x49, _) => Some(crate::notepad::KEY_PAGE_UP),
                (0x51, _) => Some(crate::notepad::KEY_PAGE_DOWN),
                _ => None,
            };
        }

        // Ignore break codes for now
        if is_break {
            if crate::KBD_DEBUG_LOG {
                let _ = writeln!(
                    serial,
                    "parser: drop reason pending_e0={} is_break={}",
                    self.pending_e0, is_break
                );
            }
            return None;
        }

        self.pending_e0 = false;

        // Translate make code to ASCII
        let ascii = match code {
            0x01 => 0x1B, // Escape
            0x02..=0x0B => {
                // 1-9, 0
                let digit = if code == 0x0B { 0 } else { code - 0x01 };
                if self.shift_pressed {
                    match digit {
                        1 => b'!',
                        2 => b'@',
                        3 => b'#',
                        4 => b'$',
                        5 => b'%',
                        6 => b'^',
                        7 => b'&',
                        8 => b'*',
                        9 => b'(',
                        0 => b')',
                        _ => return None,
                    }
                } else {
                    b'0' + digit
                }
            }
            0x10 => {
                if self.shift_pressed {
                    b'Q'
                } else {
                    b'q'
                }
            }
            0x11 => {
                if self.shift_pressed {
                    b'W'
                } else {
                    b'w'
                }
            }
            0x12 => {
                if self.shift_pressed {
                    b'E'
                } else {
                    b'e'
                }
            }
            0x13 => {
                if self.shift_pressed {
                    b'R'
                } else {
                    b'r'
                }
            }
            0x14 => {
                if self.shift_pressed {
                    b'T'
                } else {
                    b't'
                }
            }
            0x15 => {
                if self.shift_pressed {
                    b'Y'
                } else {
                    b'y'
                }
            }
            0x16 => {
                if self.shift_pressed {
                    b'U'
                } else {
                    b'u'
                }
            }
            0x17 => {
                if self.shift_pressed {
                    b'I'
                } else {
                    b'i'
                }
            }
            0x18 => {
                if self.shift_pressed {
                    b'O'
                } else {
                    b'o'
                }
            }
            0x19 => {
                // P key - handle Ctrl+P specially
                if self.ctrl_pressed {
                    return Some(0x10); // Ctrl+P
                } else if self.shift_pressed {
                    b'P'
                } else {
                    b'p'
                }
            }
            0x1E => {
                if self.shift_pressed {
                    b'A'
                } else {
                    b'a'
                }
            }
            0x1F => {
                if self.shift_pressed {
                    b'S'
                } else {
                    b's'
                }
            }
            0x20 => {
                if self.shift_pressed {
                    b'D'
                } else {
                    b'd'
                }
            }
            0x21 => {
                if self.shift_pressed {
                    b'F'
                } else {
                    b'f'
                }
            }
            0x22 => {
                if self.shift_pressed {
                    b'G'
                } else {
                    b'g'
                }
            }
            0x23 => {
                if self.shift_pressed {
                    b'H'
                } else {
                    b'h'
                }
            }
            0x24 => {
                if self.shift_pressed {
                    b'J'
                } else {
                    b'j'
                }
            }
            0x25 => {
                if self.shift_pressed {
                    b'K'
                } else {
                    b'k'
                }
            }
            0x26 => {
                if self.shift_pressed {
                    b'L'
                } else {
                    b'l'
                }
            }
            0x2C => {
                if self.shift_pressed {
                    b'Z'
                } else {
                    b'z'
                }
            }
            0x2D => {
                if self.shift_pressed {
                    b'X'
                } else {
                    b'x'
                }
            }
            0x2E => {
                if self.shift_pressed {
                    b'C'
                } else {
                    b'c'
                }
            }
            0x2F => {
                if self.shift_pressed {
                    b'V'
                } else {
                    b'v'
                }
            }
            0x30 => {
                if self.shift_pressed {
                    b'B'
                } else {
                    b'b'
                }
            }
            0x31 => {
                if self.shift_pressed {
                    b'N'
                } else {
                    b'n'
                }
            }
            0x32 => {
                if self.shift_pressed {
                    b'M'
                } else {
                    b'm'
                }
            }
            0x39 => {
                // Ctrl+Space opens the desk's palette; a private byte, like
                // the arrows.
                if self.ctrl_pressed {
                    return Some(crate::desk::KEY_CTRL_SPACE);
                }
                b' '
            }
            0x1C => b'\n', // Enter
            0x0F => {
                // Tab was not mapped at all. Ctrl+Tab cycles the desk's
                // windows and travels as a private byte like the arrows.
                if self.ctrl_pressed {
                    return Some(crate::desk::KEY_CTRL_TAB);
                }
                b'\t'
            }
            0x0E => {
                // Backspace
                return Some(0x08); // Special marker for backspace
            }
            0x0C => {
                if self.shift_pressed {
                    b'_'
                } else {
                    b'-'
                }
            }
            0x0D => {
                if self.shift_pressed {
                    b'+'
                } else {
                    b'='
                }
            }
            0x1A => {
                if self.shift_pressed {
                    b'{'
                } else {
                    b'['
                }
            }
            0x1B => {
                if self.shift_pressed {
                    b'}'
                } else {
                    b']'
                }
            }
            0x27 => {
                if self.shift_pressed {
                    b':'
                } else {
                    b';'
                }
            }
            0x28 => {
                if self.shift_pressed {
                    b'"'
                } else {
                    b'\''
                }
            }
            0x29 => {
                if self.shift_pressed {
                    b'~'
                } else {
                    b'`'
                }
            }
            0x2B => {
                if self.shift_pressed {
                    b'|'
                } else {
                    b'\\'
                }
            }
            0x33 => {
                if self.shift_pressed {
                    b'<'
                } else {
                    b','
                }
            }
            0x34 => {
                if self.shift_pressed {
                    b'>'
                } else {
                    b'.'
                }
            }
            0x35 => {
                if self.shift_pressed {
                    b'?'
                } else {
                    b'/'
                }
            }
            _ => return None,
        };

        // Ctrl+letter is the matching control byte, the way a terminal
        // would send it. Ctrl+P was the one letter special-cased above and
        // it stays 0x10 either way; this gives Ctrl+S, Ctrl+Z, Ctrl+N,
        // Ctrl+O and Ctrl+W to the desk's apps without a table per app.
        if self.ctrl_pressed && ascii.is_ascii_alphabetic() {
            return Some(ascii.to_ascii_lowercase() & 0x1f);
        }

        Some(ascii)
    }
}

#[cfg(test)]
mod keyboard_scancode_tests {
    use super::Ps2ParserState;
    use core::fmt::{self, Write};

    struct DummyWriter;
    impl Write for DummyWriter {
        fn write_str(&mut self, _s: &str) -> fmt::Result {
            Ok(())
        }
    }

    #[test]
    fn test_scancode_basic_letters() {
        let mut parser = Ps2ParserState::new();
        let mut writer = DummyWriter;
        assert_eq!(parser.process_scancode(0x1E, &mut writer), Some(b'a'));
        assert_eq!(parser.process_scancode(0x30, &mut writer), Some(b'b'));
    }

    #[test]
    fn test_scancode_escape() {
        let mut parser = Ps2ParserState::new();
        let mut writer = DummyWriter;
        // 0x01 is Escape scancode -> 0x1B ASCII
        assert_eq!(parser.process_scancode(0x01, &mut writer), Some(0x1B));
    }

    #[test]
    fn test_scancode_shifted_letter() {
        let mut parser = Ps2ParserState::new();
        let mut writer = DummyWriter;
        // Left shift down
        assert_eq!(parser.process_scancode(0x2A, &mut writer), None);
        assert_eq!(parser.process_scancode(0x1E, &mut writer), Some(b'A'));
        // Left shift up
        assert_eq!(parser.process_scancode(0xAA, &mut writer), None);
        assert_eq!(parser.process_scancode(0x1E, &mut writer), Some(b'a'));
    }

    /// This test used to be `test_scancode_e0_prefix_ignored` and asserted
    /// that an arrow key produced nothing -- pinning the limitation that
    /// made every editor on this machine navigate with hjkl. It now asserts
    /// the contract the desk's apps rely on (GFX-051): the arrows and Delete
    /// arrive as private bytes above ASCII, other E0 keys still produce
    /// nothing, and the prefix never leaks into the next plain key.
    #[test]
    fn test_scancode_e0_arrows_are_delivered_and_other_e0_keys_are_not() {
        let mut parser = Ps2ParserState::new();
        let mut writer = DummyWriter;
        for (code, byte) in [
            (0x48, crate::notepad::KEY_UP),
            (0x50, crate::notepad::KEY_DOWN),
            (0x4B, crate::notepad::KEY_LEFT),
            (0x4D, crate::notepad::KEY_RIGHT),
            (0x53, crate::notepad::KEY_DELETE),
        ] {
            assert_eq!(parser.process_scancode(0xE0, &mut writer), None);
            assert_eq!(parser.process_scancode(code, &mut writer), Some(byte));
            // The break code of the same key is silent.
            assert_eq!(parser.process_scancode(0xE0, &mut writer), None);
            assert_eq!(parser.process_scancode(code | 0x80, &mut writer), None);
        }
        // An E0 key that is not one of those (Insert) is still dropped.
        assert_eq!(parser.process_scancode(0xE0, &mut writer), None);
        assert_eq!(parser.process_scancode(0x52, &mut writer), None);
        // Next normal key should still work
        assert_eq!(parser.process_scancode(0x1E, &mut writer), Some(b'a'));

        // Shift held: the same keys are the selection bytes; Home and End
        // travel too (GFX-054).
        assert_eq!(parser.process_scancode(0x2A, &mut writer), None);
        for (code, byte) in [
            (0x48, crate::notepad::KEY_SHIFT_UP),
            (0x4B, crate::notepad::KEY_SHIFT_LEFT),
            (0x47, crate::notepad::KEY_SHIFT_HOME),
            (0x4F, crate::notepad::KEY_SHIFT_END),
        ] {
            assert_eq!(parser.process_scancode(0xE0, &mut writer), None);
            assert_eq!(parser.process_scancode(code, &mut writer), Some(byte));
        }
        assert_eq!(parser.process_scancode(0x2A | 0x80, &mut writer), None);
        assert_eq!(parser.process_scancode(0xE0, &mut writer), None);
        assert_eq!(
            parser.process_scancode(0x47, &mut writer),
            Some(crate::notepad::KEY_HOME)
        );
        assert_eq!(parser.process_scancode(0xE0, &mut writer), None);
        assert_eq!(
            parser.process_scancode(0x4F, &mut writer),
            Some(crate::notepad::KEY_END)
        );
    }

    /// Ctrl+letter is the matching control byte; Ctrl+P is 0x10 either way.
    #[test]
    fn test_ctrl_letter_is_a_control_byte() {
        let mut parser = Ps2ParserState::new();
        let mut writer = DummyWriter;
        assert_eq!(parser.process_scancode(0x1D, &mut writer), None); // ctrl down
        assert_eq!(
            parser.process_scancode(0x1F, &mut writer),
            Some(crate::notepad::CTRL_S)
        );
        assert_eq!(
            parser.process_scancode(0x31, &mut writer),
            Some(crate::notepad::CTRL_N)
        );
        assert_eq!(
            parser.process_scancode(0x2C, &mut writer),
            Some(crate::notepad::CTRL_Z)
        );
        assert_eq!(parser.process_scancode(0x19, &mut writer), Some(0x10));
        assert_eq!(parser.process_scancode(0x1D | 0x80, &mut writer), None); // ctrl up
        assert_eq!(parser.process_scancode(0x1F, &mut writer), Some(b's'));
    }

    #[test]
    fn test_unexpected_byte_resets_prefix() {
        let mut parser = Ps2ParserState::new();
        let mut writer = DummyWriter;

        // 1. Send E0 (pending_e0 becomes true)
        assert_eq!(parser.process_scancode(0xE0, &mut writer), None);
        assert!(parser.pending_e0);

        // 2. Send unexpected normal byte (e.g. 's' 0x1f) which simulates a dropped E0-sequence byte
        // It should be consumed (dropped) to reset state
        assert_eq!(parser.process_scancode(0x1F, &mut writer), None);
        assert!(!parser.pending_e0); // Prefix should be cleared

        // 3. Send valid byte (e.g. 'a' 0x1E)
        // It should be accepted
        assert_eq!(parser.process_scancode(0x1E, &mut writer), Some(b'a'));
    }
}

#[cfg(all(not(test), target_os = "none"))]
fn boot_info(serial: &mut serial::SerialPort) -> BootInfo {
    let mut info = BootInfo::empty();
    match HHDM_REQUEST.get_response() {
        Some(resp) => {
            info.hhdm_offset = Some(resp.offset());
        }
        None => {
            info.hhdm_offset = None;
        }
    }

    match EXECUTABLE_ADDRESS_REQUEST.get_response() {
        Some(resp) => {
            info.kernel_phys = Some(resp.physical_base());
            info.kernel_virt = Some(resp.virtual_base());
        }
        None => {
            info.kernel_phys = None;
            info.kernel_virt = None;
        }
    }

    if let Some(resp) = MEMORY_MAP_REQUEST.get_response() {
        let map = resp.entries();
        let mut usable = 0u64;
        let mut total = 0u64;
        for entry in map {
            total = total.saturating_add(entry.length);
            if entry.entry_type == EntryType::USABLE {
                usable = usable.saturating_add(entry.length);
            }
        }
        info.mem_entries = map.len();
        info.mem_total_kib = total / 1024;
        info.mem_usable_kib = usable / 1024;
    }

    // Kernel command line: `display=text|graphics` selects the boot display mode.
    if let Some(cmdline) = EXECUTABLE_CMDLINE_REQUEST.get_response() {
        let bytes = cmdline.cmdline().to_bytes();
        info.display_mode = display_mode::DisplayMode::from_cmdline(bytes);
        match RemoteToken::from_cmdline(bytes) {
            Some(token) => {
                *REMOTE_TOKEN.lock() = token;
                REMOTE_ENABLED.store(true, core::sync::atomic::Ordering::Release);
                kprintln!(serial, "remote: control enabled by command-line secret");
            }
            None => kprintln!(
                serial,
                "remote: no remote_token= given; remote control ports stay closed"
            ),
        }
        if let Some(callers) = RemoteCallers::from_cmdline(bytes) {
            kprintln!(serial, "remote: {} caller(s) allowed", callers.len);
            *REMOTE_CALLERS.lock() = callers;
        }
        kprintln!(
            serial,
            "cmdline: {:?} display_mode={:?}",
            core::str::from_utf8(bytes).unwrap_or("<non-utf8>"),
            info.display_mode.map(|mode| mode.label())
        );
    }

    // Request framebuffer from Limine
    match FRAMEBUFFER_REQUEST.get_response() {
        Some(fb_resp) => {
            if let Some(fb) = fb_resp.framebuffers().next() {
                let width = fb.width();
                let height = fb.height();
                let pitch = fb.pitch();
                let bpp = fb.bpp();

                if width == 0 || height == 0 || pitch == 0 || bpp == 0 {
                    kprintln!(
                        serial,
                        "framebuffer: invalid ({}x{} @ 0x{:x}, {} bpp)",
                        width,
                        height,
                        fb.addr() as usize,
                        bpp
                    );
                } else {
                    info.framebuffer_addr = Some(fb.addr());
                    info.framebuffer_width = width;
                    info.framebuffer_height = height;
                    info.framebuffer_pitch = pitch;
                    info.framebuffer_bpp = bpp;
                    kprintln!(
                        serial,
                        "framebuffer: {}x{} @ 0x{:x} ({} bpp)",
                        width,
                        height,
                        fb.addr() as usize,
                        bpp
                    );
                }
            } else {
                kprintln!(serial, "framebuffer: no framebuffer devices available");
            }
        }
        None => {
            kprintln!(serial, "framebuffer: unavailable (no response)");
        }
    }

    print_boot_info(serial, &info);
    info
}

#[cfg(not(test))]
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
            info.mem_entries, info.mem_total_kib, info.mem_usable_kib
        );
    } else {
        let _ = writeln!(serial, "memory: map unavailable");
    }
}

#[cfg(all(not(test), target_os = "none"))]
fn init_memory(
    serial: &mut serial::SerialPort,
    boot: &BootInfo,
) -> (Option<FrameAllocator>, Option<BumpHeap>) {
    let Some(resp) = MEMORY_MAP_REQUEST.get_response() else {
        let _ = writeln!(serial, "allocator: unavailable (no memory map)");
        return (None, None);
    };
    let map = resp.entries();

    let mut allocator = FrameAllocator::new();
    let mut dropped_reserved = 0usize;
    for entry in map {
        match entry.entry_type {
            EntryType::USABLE => allocator.add_range(entry.base, entry.length),
            EntryType::BOOTLOADER_RECLAIMABLE | EntryType::EXECUTABLE_AND_MODULES => {
                if !allocator.add_reserved_range(entry.base, entry.length) {
                    dropped_reserved += 1;
                }
            }
            _ => {}
        }
    }
    allocator.reset_cursor();

    let _ = writeln!(
        serial,
        "allocator: ranges={} frames={} reserved={}",
        allocator.range_count(),
        allocator.total_frames(),
        allocator.reserved_range_count()
    );
    if dropped_reserved > 0 {
        // A memory map with more reserved regions than the table holds. The
        // allocator will hand out memory that belongs to the bootloader or
        // the kernel image, and the corruption surfaces somewhere else
        // entirely -- so say it here, where it is still explicable.
        let _ = writeln!(
            serial,
            "allocator: WARNING {dropped_reserved} reserved regions did not fit; \
             memory that must not be allocated may be handed out"
        );
    }

    let heap = match boot.hhdm_offset {
        Some(offset) => init_heap(serial, &mut allocator, offset),
        None => {
            let _ = writeln!(serial, "heap: skipped (no hhdm)");
            None
        }
    };

    (Some(allocator), heap)
}

#[cfg(all(not(test), target_os = "none"))]
fn init_heap(
    serial: &mut serial::SerialPort,
    allocator: &mut FrameAllocator,
    hhdm_offset: u64,
) -> Option<BumpHeap> {
    // 32 MiB: the text shadow and the desktop RGBA target are ~4 MiB each at
    // 1280x800, and the bump allocator never returns per-frame allocations.
    const HEAP_PAGES: u64 = 8192;
    // Below this there is no point continuing; the machine cannot build a
    // console, let alone a desktop.
    const MIN_HEAP_PAGES: u64 = 512; // 2 MiB
                                     // This asked for 32 MiB of *contiguous* memory and gave up if it could
                                     // not have it -- then boot carried on for ten more steps past a
                                     // condition it had already printed as fatal, and died in the global
                                     // allocator with "ALLOCATION ERROR: size=16". A smaller machine can run
                                     // a smaller heap; take what is there.
    let mut pages = HEAP_PAGES;
    let phys_base = loop {
        if let Some(base) = allocator.allocate_contiguous(pages) {
            break base;
        }
        if pages <= MIN_HEAP_PAGES {
            let _ = writeln!(
                serial,
                "heap: no contiguous run of {} KiB is available; this machine \
                 has too little memory to run PandaGen",
                MIN_HEAP_PAGES * PAGE_SIZE / 1024
            );
            return None;
        }
        pages /= 2;
    };
    if pages < HEAP_PAGES {
        let _ = writeln!(
            serial,
            "heap: only {} KiB available, less than the {} KiB wanted; \
             graphics modes will be limited",
            pages * PAGE_SIZE / 1024,
            HEAP_PAGES * PAGE_SIZE / 1024
        );
    }
    let virt_base = (hhdm_offset + phys_base) as usize;
    let size = (pages * PAGE_SIZE) as usize;

    // Initialize the global allocator (bare-metal only): a freeing
    // free-list heap, so per-frame desktop allocations are returned.
    #[cfg(all(not(test), target_os = "none"))]
    unsafe {
        GLOBAL_HEAP.init(virt_base, size);
    }

    let heap = BumpHeap::new(virt_base, size);
    let _ = writeln!(serial, "heap: base=0x{:x} size={} bytes", virt_base, size);
    Some(heap)
}

#[cfg(all(not(test), target_os = "none"))]
#[used]
#[link_section = ".limine_requests"]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[cfg(all(not(test), target_os = "none"))]
#[used]
#[link_section = ".limine_requests"]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[cfg(all(not(test), target_os = "none"))]
#[used]
#[link_section = ".limine_requests"]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[cfg(all(not(test), target_os = "none"))]
#[used]
#[link_section = ".limine_requests"]
static EXECUTABLE_ADDRESS_REQUEST: ExecutableAddressRequest = ExecutableAddressRequest::new();

#[cfg(all(not(test), target_os = "none"))]
#[used]
#[link_section = ".limine_requests"]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[cfg(all(not(test), target_os = "none"))]
#[used]
#[link_section = ".limine_requests"]
static EXECUTABLE_CMDLINE_REQUEST: ExecutableCmdlineRequest = ExecutableCmdlineRequest::new();

#[cfg(all(not(test), target_os = "none"))]
#[used]
#[link_section = ".limine_requests"]
static MP_REQUEST: MpRequest = MpRequest::new();

#[cfg(all(not(test), target_os = "none"))]
static mut KERNEL_STORAGE: MaybeUninit<Kernel> = MaybeUninit::uninit();

/// Every CPU that has entered kernel code, BSP first.
static CPUS: hal_x86_64::CpuRegistry = hal_x86_64::CpuRegistry::new();

/// Number of CPUs the bootloader reported (0 before bring-up).
static CPU_TOTAL: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// LAPIC id of the boot processor.
static BSP_LAPIC_ID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Local APIC register window shared by every CPU (set once the HHDM is known).
static LAPIC: hal_x86_64::SharedLapic = hal_x86_64::SharedLapic::new();

/// Whether a remote secret was supplied at boot. Without one the remote
/// control ports are never opened: the compiled-in default is a constant
/// published in this repository, and a machine that accepts it is
/// controllable by anyone who has read the source.
static REMOTE_ENABLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Storage backend name, for the HTTP status page.
static STORAGE_BACKEND: hal_x86_64::SpinLock<&'static str> = hal_x86_64::SpinLock::new("none");

/// The network stack, once a virtio-net device has been found.
#[cfg(all(not(test), target_os = "none"))]
static NET: hal_x86_64::SpinLock<Option<bare_metal_net::NetStack>> =
    hal_x86_64::SpinLock::new(None);

/// Recently accepted remote message ids, so a captured datagram cannot be
/// replayed.
/// Eight callers, 128 nonces each. Per caller, because a shared window let
/// one caller's clock -- or one caller's far-future nonces -- lock every
/// other caller out for the life of the boot.
static REMOTE_REPLAY: hal_x86_64::SpinLock<remote_ipc::CallerReplayGuard<8, 128>> =
    hal_x86_64::SpinLock::new(remote_ipc::CallerReplayGuard::new());

/// Callers admitted to the remote command ports (`remote_callers=a,b` on the
/// command line); empty means any caller with a valid key.
static REMOTE_CALLERS: hal_x86_64::SpinLock<RemoteCallers> =
    hal_x86_64::SpinLock::new(RemoteCallers::ANY);

#[derive(Clone, Copy)]
struct RemoteCallers {
    names: [RemoteToken; 4],
    len: usize,
}

impl RemoteCallers {
    const ANY: Self = Self {
        names: [RemoteToken::DEFAULT; 4],
        len: 0,
    };

    fn allows(&self, caller: &str) -> bool {
        self.len == 0
            || self.names[..self.len]
                .iter()
                .any(|name| name.as_bytes() == caller.as_bytes())
    }

    /// `remote_callers=<name>[,<name>...]` from the kernel command line.
    fn from_cmdline(cmdline: &[u8]) -> Option<Self> {
        let text = core::str::from_utf8(cmdline).ok()?;
        let list = text
            .split_ascii_whitespace()
            .filter_map(|token| token.strip_prefix("remote_callers="))
            .last()?;
        let mut callers = Self::ANY;
        for name in list.split(',').filter(|n| !n.is_empty()).take(4) {
            callers.names[callers.len] = RemoteToken::from_str(name);
            callers.len += 1;
        }
        Some(callers)
    }
}

/// Shared secret for remote IPC tags (`remote_token=` on the command line).
static REMOTE_TOKEN: hal_x86_64::SpinLock<RemoteToken> =
    hal_x86_64::SpinLock::new(RemoteToken::DEFAULT);

#[derive(Clone, Copy)]
struct RemoteToken {
    bytes: [u8; 64],
    len: usize,
}

impl RemoteToken {
    const DEFAULT: Self = Self::from_str(remote_ipc::DEFAULT_REMOTE_TOKEN);

    const fn from_str(text: &str) -> Self {
        let src = text.as_bytes();
        let len = if src.len() > 64 { 64 } else { src.len() };
        let mut bytes = [0u8; 64];
        let mut i = 0;
        while i < len {
            bytes[i] = src[i];
            i += 1;
        }
        Self { bytes, len }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    /// `remote_token=<value>` from the kernel command line, if it names a
    /// real secret.
    ///
    /// A value that is still the build-time placeholder, or the development
    /// default, is treated as absent: both are constants published in this
    /// repository, and accepting either would let anyone who has read the
    /// source drive the machine.
    fn from_cmdline(cmdline: &[u8]) -> Option<Self> {
        let text = core::str::from_utf8(cmdline).ok()?;
        text.split_ascii_whitespace()
            .filter_map(|token| token.strip_prefix("remote_token="))
            .filter(|value| {
                !value.is_empty()
                    && !value.contains('@')
                    && *value != remote_ipc::DEFAULT_REMOTE_TOKEN
            })
            .last()
            .map(Self::from_str)
    }
}

/// Set once the kernel and its service tasks exist, so idle APs may poll them.
static KERNEL_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Kernel task polls that made progress, by CPU registry index.
static TASK_RUNS_BY_CPU: [core::sync::atomic::AtomicU64; hal_x86_64::MAX_CPUS] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const ZERO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    [ZERO; hal_x86_64::MAX_CPUS]
};

/// Diagnostics for the SMP paths (shown by `cpus`).
static PRESENT_FALLBACKS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static AP_KERNEL_POLLS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static AP_KERNEL_POLL_CYCLES: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static AP_KERNEL_POLL_MAX: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// Whether the boot CPU also runs command tasks (`smp bsp-commands on|off`);
/// off by default when other CPUs exist, so commands never stall the desktop.
static BSP_RUNS_COMMANDS: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
/// Whether idle application processors poll kernel tasks (`smp poll on|off`).
static AP_POLL_ENABLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

/// Jobs for application processors (`smp run <n>`).
static WORK: hal_x86_64::WorkQueue<32> = hal_x86_64::WorkQueue::new();

/// Job kind understood by `run_job`: wrapping sum of squares 1..=arg.
const JOB_SUM_OF_SQUARES: u32 = 1;
/// Job kind: convert present band `arg` from `PRESENT_BANDS`.
const JOB_PRESENT_BAND: u32 = 2;

/// Bands of the present in flight, indexed by job argument.
static PRESENT_BANDS: hal_x86_64::SpinLock<
    [framebuffer::PresentBand; framebuffer::MAX_PRESENT_BANDS],
> = hal_x86_64::SpinLock::new([framebuffer::PresentBand::EMPTY; framebuffer::MAX_PRESENT_BANDS]);

/// Incremented for every parallel present. A band job carries the generation
/// it was queued for, so a worker that wakes up late does not paint a band
/// belonging to a frame the boot CPU has already moved past.
static PRESENT_GENERATION: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Whether desktop presents are spread across the application processors.
static PARALLEL_PRESENT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

fn run_job(job: hal_x86_64::Job) -> u64 {
    match job.kind {
        JOB_SUM_OF_SQUARES => {
            (1..=job.arg).fold(0u64, |acc, i| acc.wrapping_add(i.wrapping_mul(i)))
        }
        JOB_PRESENT_BAND => {
            // arg packs the present generation and the band index.
            let generation = job.arg >> 8;
            let index = (job.arg & 0xFF) as usize;
            // Read the generation *with* the band, under the same lock, and
            // again after. Checking it first and taking the lock afterwards
            // left a window: a worker could pass the check, be preempted
            // while the boot CPU timed out, converted the band itself, bumped
            // the generation and published a later frame's buffers -- and
            // then wake up and convert a band it was never authorised for.
            // Harmless today only because the surfaces happen never to be
            // reallocated; a resize or a renderer rebuild makes it corruption.
            // Claim the band under the lock, *and register as a worker
            // while still holding it*. Checking the generation and then
            // converting afterwards is check-then-act however many times it
            // is re-checked: a worker can pass the last check, be preempted,
            // and have the boot CPU time out, bump the generation and
            // reallocate the surfaces before it touches them. The boot CPU
            // waits for this count to fall to zero before it moves on, so
            // there is no window left to lose.
            let band = {
                let slots = PRESENT_BANDS.lock();
                if generation != PRESENT_GENERATION.load(core::sync::atomic::Ordering::Acquire) {
                    // The boot CPU gave up waiting and moved on; those buffers
                    // may already describe a different frame.
                    return 0;
                }
                PRESENT_WORKERS.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
                slots[index % framebuffer::MAX_PRESENT_BANDS]
            };
            if band.rows() > 0 {
                // SAFETY: the generation was current when this band was
                // claimed, and the boot CPU will not reuse those buffers
                // until `PRESENT_WORKERS` returns to zero.
                unsafe { framebuffer::convert_rgba_rows(&band) };
            }
            PRESENT_WORKERS.fetch_sub(1, core::sync::atomic::Ordering::AcqRel);
            band.rows() as u64
        }
        _ => 0,
    }
}

/// Idle loop for an application processor: drain the work queue, then
/// sleep until a wake-up IPI. `cli` before the check and `sti; hlt` after
/// it close the lost-wake-up window.
#[cfg(all(not(test), target_os = "none"))]
fn ap_idle_loop(lapic_id: u32) -> ! {
    let mut timer_started = false;
    loop {
        // Start this CPU's timer once the boot CPU has calibrated it.
        if !timer_started {
            let initial = LAPIC_TIMER_INITIAL.load(core::sync::atomic::Ordering::Acquire);
            if initial != 0 {
                if let Some(mut apic) = LAPIC.get() {
                    apic.set_timer_divide(16);
                    apic.start_timer_periodic(LAPIC_TIMER_VECTOR, initial);
                    timer_started = true;
                }
            }
        }
        unsafe { asm!("cli", options(nomem, nostack)) };
        if let Some(job) = WORK.take() {
            unsafe { asm!("sti", options(nomem, nostack)) };
            let value = run_job(job);
            WORK.complete(job.id, lapic_id, value);
            continue;
        }
        unsafe { asm!("sti", options(nomem, nostack)) };

        // No band or job work: poll a kernel task if the boot CPU is not
        // inside the kernel right now.
        if KERNEL_READY.load(core::sync::atomic::Ordering::Acquire)
            && AP_POLL_ENABLED.load(core::sync::atomic::Ordering::Relaxed)
        {
            {
                let started = hal_x86_64::rdtsc();
                let mut serial = serial::SerialPort::new(serial::COM1);
                // SAFETY: KERNEL_READY guarantees initialisation; the kernel
                // is shared and internally locked.
                let kernel = unsafe { &*KERNEL_STORAGE.as_ptr() };
                let progressed = kernel.run_once(&mut serial);
                let spent = hal_x86_64::rdtsc().saturating_sub(started);
                AP_KERNEL_POLLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                AP_KERNEL_POLL_CYCLES.fetch_add(spent, core::sync::atomic::Ordering::Relaxed);
                AP_KERNEL_POLL_MAX.fetch_max(spent, core::sync::atomic::Ordering::Relaxed);
                if progressed {
                    continue;
                }
            }
        }

        // Sleep until an IPI or the timer, unless work arrived meanwhile.
        unsafe { asm!("cli", options(nomem, nostack)) };
        if WORK.pending() == 0 {
            unsafe { asm!("sti", "hlt", options(nomem, nostack)) };
        } else {
            unsafe { asm!("sti", options(nomem, nostack)) };
        }
    }
}

/// Entry point for application processors released by Limine.
///
/// Each AP arrives with interrupts disabled on its own 64 KiB stack. It
/// loads the shared IDT, registers itself, and parks in `hlt` until the
/// kernel has work to schedule on it.
#[cfg(all(not(test), target_os = "none"))]
unsafe extern "C" fn ap_entry(cpu: &limine::mp::Cpu) -> ! {
    let Some(index) = CPUS.register_within(cpu.lapic_id, MAX_TABLE_CPUS) else {
        // More CPUs than there are per-CPU tables. See below.
        halt_loop();
    };
    if !init_cpu_tables(index) {
        // Only MAX_TABLE_CPUS CPUs have a GDT and TSS here, while the CPU
        // registry holds many more. A CPU past that used to load the shared
        // IDT and join the idle loop while still on the bootloader's tables
        // -- and the IDT says vector 8 uses IST1, which lives in *this CPU's*
        // TSS. If the bootloader left IST1 zero, a double fault would load
        // RSP = 0 and triple-fault instead of printing the diagnostic the
        // exception handler exists to print. Park instead of joining: the
        // machine runs on fewer CPUs rather than resetting on the first
        // fault that reaches one of them.
        halt_loop();
    }
    load_idt();
    if let Some(mut apic) = LAPIC.get() {
        apic.enable(SPURIOUS_VECTOR);
    }
    ap_idle_loop(cpu.lapic_id)
}

/// Map the xAPIC register page uncached in the live page tables (the
/// bootloader's direct map does not cover MMIO holes) and return its
/// virtual address.
#[cfg(all(not(test), target_os = "none"))]
fn map_lapic_window(
    hhdm: u64,
    allocator: &mut FrameAllocator,
) -> Result<u64, hal_x86_64::MapError> {
    use hal_x86_64::paging::PageTableFlags;
    let phys = hal_x86_64::lapic::LAPIC_DEFAULT_PHYS;
    let virt = hhdm + phys;
    let cr3: u64;
    unsafe { asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };
    // SAFETY: frames come from the boot frame allocator, which only hands
    // out usable memory covered by the direct map.
    let mut mem = unsafe { hal_x86_64::HhdmMemory::new(hhdm, || allocator.allocate_frame()) };
    if hal_x86_64::mmio_map::translate(&mem, cr3, virt).is_none() {
        hal_x86_64::mmio_map::map_4k(
            &mut mem,
            cr3,
            virt,
            phys,
            PageTableFlags::WRITABLE
                | PageTableFlags::CACHE_DISABLE
                | PageTableFlags::WRITE_THROUGH,
        )?;
        unsafe { asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags)) };
    }
    Ok(virt)
}

/// Release the application processors and wait for them to register.
#[cfg(all(not(test), target_os = "none"))]
fn start_application_processors(
    serial: &mut serial::SerialPort,
    allocator: Option<&mut FrameAllocator>,
) {
    let Some(resp) = MP_REQUEST.get_response() else {
        CPU_TOTAL.store(1, Ordering::Release);
        kprintln!(serial, "SMP: no MP response, running on 1 CPU");
        return;
    };
    let bsp = resp.bsp_lapic_id();
    BSP_LAPIC_ID.store(bsp, Ordering::Release);
    if let (Some(hhdm), Some(allocator)) =
        (HHDM_REQUEST.get_response().map(|r| r.offset()), allocator)
    {
        match map_lapic_window(hhdm, allocator) {
            Ok(virt) => {
                // SAFETY: the window was just mapped uncached at `virt`.
                unsafe { LAPIC.set_base(virt as usize) };
                if let Some(mut apic) = LAPIC.get() {
                    apic.enable(SPURIOUS_VECTOR);
                    klog!(
                        serial,
                        "LAPIC: bsp id {} enabled at 0x{:x}\r\n",
                        apic.id(),
                        virt
                    );
                }
            }
            Err(err) => {
                klog!(serial, "LAPIC: map failed ({:?}); IPIs disabled\r\n", err);
            }
        }
    }
    let cpus = resp.cpus();
    CPU_TOTAL.store(cpus.len() as u32, Ordering::Release);
    for cpu in cpus {
        if cpu.lapic_id != bsp {
            cpu.goto_address.write(ap_entry);
        }
    }
    // Only MAX_TABLE_CPUS can join; the rest park themselves in `ap_entry`
    // because there is no GDT or TSS for them. Waiting for all of them would
    // spend fifty million polls and then report a timeout that is not one.
    let expected = cpus.len().min(MAX_TABLE_CPUS);
    let all_up = CPUS.wait_for(expected, 50_000_000);
    if cpus.len() > MAX_TABLE_CPUS {
        klog!(
            serial,
            "SMP: {} of {} CPUs parked; only {} have per-CPU tables\r\n",
            cpus.len() - MAX_TABLE_CPUS,
            cpus.len(),
            MAX_TABLE_CPUS
        );
    }
    klog!(
        serial,
        "SMP: {} of {} CPUs online (bsp lapic {}){}\r\n",
        CPUS.online(),
        cpus.len(),
        bsp,
        if all_up { "" } else { " [timeout]" }
    );
}

#[cfg(all(not(test), target_os = "none"))]
#[global_allocator]
static GLOBAL_HEAP: free_list_heap::FreeListHeap = free_list_heap::FreeListHeap::empty();

/// Host builds of the binary (integration tests build it) get a plain heap
/// object so the `mem`/telemetry paths compile; it is never installed as
/// the allocator there.
#[cfg(all(not(test), not(target_os = "none")))]
static GLOBAL_HEAP: free_list_heap::FreeListHeap = free_list_heap::FreeListHeap::empty();

#[cfg(all(not(test), target_os = "none"))]
#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    let mut serial = serial::SerialPort::new(serial::COM1);
    let _ = core::fmt::write(
        &mut serial,
        format_args!(
            "\r\n\r\nALLOCATION ERROR: size={} align={}\r\n",
            layout.size(),
            layout.align()
        ),
    );
    halt_loop()
}

const PAGE_SIZE: u64 = 4096;
const CHANNEL_CAPACITY: usize = 8;
const COMMAND_MAX: usize = 64;
/// Command output capacity; `net`/`cpus` status fits, and a full response
/// still fits one UDP datagram once base64-wrapped in a remote_ipc envelope.
const RESPONSE_MAX: usize = 512;
const ERROR_MAX: usize = 96;
const MAX_TASKS: usize = 8;
const MAX_CHANNELS: usize = 16;
const KEYBOARD_QUEUE_SIZE: usize = 256;

/// Bounded lock-free ring buffer for keyboard scancodes
///
/// This queue is written from IRQ context (push) and read from main loop (drain).
/// Drop policy: DropOldest - when full, oldest scancode is overwritten.
#[cfg(not(test))]
struct KeyboardEventQueue {
    buffer: [AtomicU8; KEYBOARD_QUEUE_SIZE],
    write_pos: AtomicU64,
    read_pos: AtomicU64,
}

#[cfg(not(test))]
impl KeyboardEventQueue {
    #[allow(clippy::declare_interior_mutable_const)]
    const fn new() -> Self {
        const ZERO: AtomicU8 = AtomicU8::new(0);
        Self {
            buffer: [ZERO; KEYBOARD_QUEUE_SIZE],
            write_pos: AtomicU64::new(0),
            read_pos: AtomicU64::new(0),
        }
    }

    /// Pushes a scancode from IRQ context.
    /// If queue is full, overwrites oldest entry (DropOldest policy).
    /// Returns true if an entry was dropped.
    fn push(&self, scancode: u8) -> bool {
        let write_idx =
            (self.write_pos.load(Ordering::Relaxed) % KEYBOARD_QUEUE_SIZE as u64) as usize;
        self.buffer[write_idx].store(scancode, Ordering::Release);

        let new_write = self.write_pos.load(Ordering::Relaxed).wrapping_add(1);
        self.write_pos.store(new_write, Ordering::Release);

        // If we've caught up to the read position, advance it (drop
        // oldest). A plain load-then-store raced with `pop`, which also
        // writes `read_pos` from the main loop: the IRQ can preempt `pop`
        // between its load and its store, and one of the two writes is then
        // lost -- a scancode delivered twice or skipped. Only reachable when
        // the queue is already overflowing, so the blast radius is one
        // keystroke, but a compare-exchange costs nothing here.
        let read = self.read_pos.load(Ordering::Acquire);
        if new_write.wrapping_sub(read) >= KEYBOARD_QUEUE_SIZE as u64 {
            let _ = self.read_pos.compare_exchange(
                read,
                read.wrapping_add(1),
                Ordering::AcqRel,
                Ordering::Relaxed,
            );
            true
        } else {
            false
        }
    }

    /// Pops a scancode from main loop context.
    /// Returns None if queue is empty.
    fn pop(&self) -> Option<u8> {
        let read = self.read_pos.load(Ordering::Acquire);
        let write = self.write_pos.load(Ordering::Acquire);

        if read == write {
            return None; // Empty
        }

        let read_idx = (read % KEYBOARD_QUEUE_SIZE as u64) as usize;
        let scancode = self.buffer[read_idx].load(Ordering::Acquire);
        // Compare-exchange for the same reason as `push`: the IRQ handler
        // also writes `read_pos` when the queue overflows, and it can
        // preempt this between the load above and this store. If it did,
        // that entry was dropped and this read is stale, so leaving
        // `read_pos` where the handler put it is the right answer.
        if self
            .read_pos
            .compare_exchange(
                read,
                read.wrapping_add(1),
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return None;
        }

        Some(scancode)
    }

    /// Checks if the queue has pending events without consuming them.
    fn has_pending(&self) -> bool {
        let read = self.read_pos.load(Ordering::Acquire);
        let write = self.write_pos.load(Ordering::Acquire);
        read != write
    }
}

#[derive(Copy, Clone)]
struct BootInfo {
    hhdm_offset: Option<u64>,
    kernel_phys: Option<u64>,
    kernel_virt: Option<u64>,
    mem_entries: usize,
    mem_total_kib: u64,
    mem_usable_kib: u64,
    framebuffer_addr: Option<*mut u8>,
    framebuffer_width: u64,
    framebuffer_height: u64,
    framebuffer_pitch: u64,
    framebuffer_bpp: u16,
    /// Display mode requested on the kernel command line (`display=...`).
    display_mode: Option<display_mode::DisplayMode>,
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
            framebuffer_addr: None,
            framebuffer_width: 0,
            framebuffer_height: 0,
            framebuffer_pitch: 0,
            framebuffer_bpp: 0,
            display_mode: None,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct TaskId(u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct ChannelId(u8);

impl ChannelId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct MessageId(u64);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct SchemaVersion {
    major: u16,
    minor: u16,
}

impl SchemaVersion {
    const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

const COMMAND_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum CommandErrorCode {
    InvalidCommand,
    InvalidArguments,
    Internal,
    ServiceUnavailable,
}

#[derive(Copy, Clone)]
struct CommandError {
    code: CommandErrorCode,
    len: usize,
    message: [u8; ERROR_MAX],
}

impl CommandError {
    fn new(code: CommandErrorCode, message: &str) -> Self {
        let mut error = Self {
            code,
            len: 0,
            message: [0; ERROR_MAX],
        };
        error.write_message(message);
        error
    }

    fn write_message(&mut self, message: &str) {
        let bytes = message.as_bytes();
        let len = bytes.len().min(ERROR_MAX);
        self.message[..len].copy_from_slice(&bytes[..len]);
        self.len = len;
    }

    fn as_str(&self) -> Option<&str> {
        str::from_utf8(&self.message[..self.len]).ok()
    }
}

#[derive(Copy, Clone)]
enum CommandStatus {
    Ok,
    Error(CommandError),
}

#[derive(Copy, Clone)]
struct CommandRequest {
    version: SchemaVersion,
    request_id: MessageId,
    reply_channel: ChannelId,
    len: usize,
    data: [u8; COMMAND_MAX],
}

impl CommandRequest {
    fn from_bytes(line: &[u8], request_id: MessageId, reply_channel: ChannelId) -> Option<Self> {
        if line.len() > COMMAND_MAX {
            return None;
        }
        let mut msg = Self {
            version: COMMAND_SCHEMA_VERSION,
            request_id,
            reply_channel,
            len: line.len(),
            data: [0; COMMAND_MAX],
        };
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
struct CommandResponse {
    version: SchemaVersion,
    correlation_id: MessageId,
    status: CommandStatus,
    len: usize,
    output: [u8; RESPONSE_MAX],
}

impl CommandResponse {
    fn ok(correlation_id: MessageId, output: &FixedBuffer<RESPONSE_MAX>) -> Self {
        let mut response = Self {
            version: COMMAND_SCHEMA_VERSION,
            correlation_id,
            status: CommandStatus::Ok,
            len: 0,
            output: [0; RESPONSE_MAX],
        };
        response.write_output(output.as_bytes());
        response
    }

    fn error(correlation_id: MessageId, error: CommandError) -> Self {
        Self {
            version: COMMAND_SCHEMA_VERSION,
            correlation_id,
            status: CommandStatus::Error(error),
            len: 0,
            output: [0; RESPONSE_MAX],
        }
    }

    fn write_output(&mut self, data: &[u8]) {
        let len = data.len().min(RESPONSE_MAX);
        self.output[..len].copy_from_slice(&data[..len]);
        self.len = len;
    }

    fn output_str(&self) -> Option<&str> {
        str::from_utf8(&self.output[..self.len]).ok()
    }
}

#[derive(Copy, Clone)]
enum KernelMessage {
    Empty,
    CommandRequest(CommandRequest),
    CommandResponse(CommandResponse),
}

impl KernelMessage {
    const fn empty() -> Self {
        Self::Empty
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum TaskDomain {
    Kernel,
    User,
}

struct TaskSlot {
    id: TaskId,
    domain: TaskDomain,
    time_slice: TimeSlice,
    kind: TaskKind,
}

impl TaskSlot {
    fn poll(&mut self, ctx: &mut KernelContext, serial: &mut serial::SerialPort) -> bool {
        self.time_slice.advance(1);
        if self.time_slice.should_preempt() {
            self.time_slice.reset();
        }
        self.kind.poll(ctx, serial)
    }
}

enum TaskKind {
    Console(ConsoleService),
    Command(CommandService),
}

impl TaskKind {
    fn poll(&mut self, ctx: &mut KernelContext, serial: &mut serial::SerialPort) -> bool {
        match self {
            TaskKind::Console(service) => service.poll(ctx, serial),
            TaskKind::Command(service) => service.poll(ctx, serial),
        }
    }

    fn set_task_id(&mut self, task_id: TaskId) {
        match self {
            TaskKind::Console(service) => service.task_id = task_id,
            TaskKind::Command(service) => service.task_id = task_id,
        }
    }
}

struct CooperativeScheduler {
    order: [TaskId; MAX_TASKS],
    count: usize,
    cursor: usize,
}

impl CooperativeScheduler {
    const fn new() -> Self {
        Self {
            order: [TaskId(0); MAX_TASKS],
            count: 0,
            cursor: 0,
        }
    }

    fn add_task(&mut self, id: TaskId) {
        if self.count < MAX_TASKS {
            self.order[self.count] = id;
            self.count += 1;
        }
    }

    fn next_task(&mut self) -> Option<TaskId> {
        if self.count == 0 {
            return None;
        }
        let id = self.order[self.cursor];
        self.cursor = (self.cursor + 1) % self.count;
        Some(id)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct TimeSlice {
    quantum_ticks: u64,
    used_ticks: u64,
}

impl TimeSlice {
    fn new(quantum_ticks: u64) -> Self {
        Self {
            quantum_ticks,
            used_ticks: 0,
        }
    }

    fn advance(&mut self, ticks: u64) {
        self.used_ticks = self.used_ticks.saturating_add(ticks);
    }

    fn should_preempt(&self) -> bool {
        self.quantum_ticks > 0 && self.used_ticks >= self.quantum_ticks
    }

    fn reset(&mut self) {
        self.used_ticks = 0;
    }
}

#[derive(Copy, Clone)]
struct Cap<T> {
    id: u32,
    _marker: PhantomData<T>,
}

impl<T> Cap<T> {
    fn new(id: u32) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    fn id(&self) -> u32 {
        self.id
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum KernelError {
    OutOfTasks,
    OutOfChannels,
    ChannelFull,
    ChannelEmpty,
    InvalidChannel,
    Unsupported,
}

trait KernelApiV0 {
    fn create_task(&mut self, name: &str, caps: &[Cap<()>]) -> Result<TaskId, KernelError>;
    fn create_channel(&mut self) -> Result<ChannelId, KernelError>;
    fn send(&mut self, channel: ChannelId, message: KernelMessage) -> Result<(), KernelError>;
    fn recv(&mut self, channel: ChannelId) -> Result<KernelMessage, KernelError>;
    fn grant(&mut self, _task: TaskId, _cap: Cap<()>) -> Result<(), KernelError> {
        Ok(())
    }
}

struct KernelContext<'a> {
    boot: &'a BootInfo,
    allocator: &'a hal_x86_64::SpinLock<Option<FrameAllocator>>,
    heap: &'a hal_x86_64::SpinLock<Option<BumpHeap>>,
    channels: &'a [hal_x86_64::SpinLock<Channel>; MAX_CHANNELS],
    next_message_id: &'a core::sync::atomic::AtomicU64,
}

impl KernelContext<'_> {
    fn next_message_id(&mut self) -> MessageId {
        MessageId(
            self.next_message_id
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed),
        )
    }

    fn try_recv(&mut self, channel: ChannelId) -> Option<KernelMessage> {
        if channel.index() >= MAX_CHANNELS {
            return None;
        }
        self.channels[channel.index()].lock().recv()
    }

    fn boot(&self) -> &BootInfo {
        self.boot
    }
}

impl KernelApiV0 for KernelContext<'_> {
    fn create_task(&mut self, _name: &str, _caps: &[Cap<()>]) -> Result<TaskId, KernelError> {
        Err(KernelError::Unsupported)
    }

    fn create_channel(&mut self) -> Result<ChannelId, KernelError> {
        Err(KernelError::Unsupported)
    }

    fn send(&mut self, channel: ChannelId, message: KernelMessage) -> Result<(), KernelError> {
        if channel.index() >= MAX_CHANNELS {
            return Err(KernelError::InvalidChannel);
        }
        self.channels[channel.index()]
            .lock()
            .send(message)
            .map_err(|_| KernelError::ChannelFull)
    }

    fn recv(&mut self, channel: ChannelId) -> Result<KernelMessage, KernelError> {
        if channel.index() >= MAX_CHANNELS {
            return Err(KernelError::InvalidChannel);
        }
        self.channels[channel.index()]
            .lock()
            .recv()
            .ok_or(KernelError::ChannelEmpty)
    }
}

/// Kernel state shared by every CPU. Each piece has its own lock, so tasks
/// polled on different CPUs run in parallel and only serialise on the
/// channel or slot they touch.
struct Kernel {
    boot: BootInfo,
    allocator: hal_x86_64::SpinLock<Option<FrameAllocator>>,
    heap: hal_x86_64::SpinLock<Option<BumpHeap>>,
    channels: [hal_x86_64::SpinLock<Channel>; MAX_CHANNELS],
    channel_count: core::sync::atomic::AtomicU8,
    next_message_id: core::sync::atomic::AtomicU64,
    scheduler: hal_x86_64::SpinLock<CooperativeScheduler>,
    tasks: [hal_x86_64::SpinLock<Option<TaskSlot>>; MAX_TASKS],
}

// SAFETY: every mutable part is behind a lock or atomic; `BootInfo` holds a
// framebuffer pointer that is only ever read through it.
unsafe impl Sync for Kernel {}
unsafe impl Send for Kernel {}

impl Kernel {
    /// Initializes a kernel directly in the provided storage.
    ///
    /// This avoids large stack allocations during early boot.
    unsafe fn init_in_place(
        storage: &mut MaybeUninit<Kernel>,
        boot: BootInfo,
        allocator: Option<FrameAllocator>,
        heap: Option<BumpHeap>,
    ) -> &mut Kernel {
        let ptr = storage.as_mut_ptr();

        core::ptr::addr_of_mut!((*ptr).boot).write(boot);
        core::ptr::addr_of_mut!((*ptr).allocator).write(hal_x86_64::SpinLock::new(allocator));
        core::ptr::addr_of_mut!((*ptr).heap).write(hal_x86_64::SpinLock::new(heap));
        core::ptr::addr_of_mut!((*ptr).channel_count).write(core::sync::atomic::AtomicU8::new(0));
        core::ptr::addr_of_mut!((*ptr).next_message_id)
            .write(core::sync::atomic::AtomicU64::new(1));
        core::ptr::addr_of_mut!((*ptr).scheduler)
            .write(hal_x86_64::SpinLock::new(CooperativeScheduler::new()));

        let channels_ptr =
            core::ptr::addr_of_mut!((*ptr).channels) as *mut hal_x86_64::SpinLock<Channel>;
        for idx in 0..MAX_CHANNELS {
            channels_ptr
                .add(idx)
                .write(hal_x86_64::SpinLock::new(Channel::new()));
        }

        let tasks_ptr =
            core::ptr::addr_of_mut!((*ptr).tasks) as *mut hal_x86_64::SpinLock<Option<TaskSlot>>;
        for idx in 0..MAX_TASKS {
            tasks_ptr.add(idx).write(hal_x86_64::SpinLock::new(None));
        }

        let kernel = &mut *ptr;
        let command_channel = kernel.create_channel().expect("command channel available");
        let response_channel = kernel.create_channel().expect("response channel available");

        let command_task = CommandService::new(command_channel);
        let console_task = ConsoleService::new(command_channel, response_channel);

        let _ = kernel.spawn_task(TaskDomain::User, TaskKind::Command(command_task));
        let _ = kernel.spawn_task(TaskDomain::User, TaskKind::Console(console_task));

        kernel
    }

    fn new(boot: BootInfo, allocator: Option<FrameAllocator>, heap: Option<BumpHeap>) -> Self {
        let mut kernel = Self {
            boot,
            allocator: hal_x86_64::SpinLock::new(allocator),
            heap: hal_x86_64::SpinLock::new(heap),
            channels: core::array::from_fn(|_| hal_x86_64::SpinLock::new(Channel::new())),
            channel_count: core::sync::atomic::AtomicU8::new(0),
            next_message_id: core::sync::atomic::AtomicU64::new(1),
            scheduler: hal_x86_64::SpinLock::new(CooperativeScheduler::new()),
            tasks: core::array::from_fn(|_| hal_x86_64::SpinLock::new(None)),
        };

        let command_channel = kernel.create_channel().expect("command channel available");
        let response_channel = kernel.create_channel().expect("response channel available");

        let command_task = CommandService::new(command_channel);
        let console_task = ConsoleService::new(command_channel, response_channel);

        let _ = kernel.spawn_task(TaskDomain::User, TaskKind::Command(command_task));
        let _ = kernel.spawn_task(TaskDomain::User, TaskKind::Console(console_task));

        kernel
    }

    /// Reserve a channel. Takes `&self` so no caller needs a unique
    /// reference to the kernel: the boot CPU and every application processor
    /// share one, and holding a `&mut` alongside those shared borrows is
    /// undefined behaviour regardless of the interior locks.
    fn create_channel(&self) -> Result<ChannelId, KernelError> {
        use core::sync::atomic::Ordering;
        let index = self
            .channel_count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                ((count as usize) < MAX_CHANNELS).then_some(count + 1)
            })
            .map_err(|_| KernelError::OutOfChannels)?;
        let id = ChannelId(index);
        self.channels[id.index()].lock().reset();
        Ok(id)
    }

    /// A context over the shared kernel state for one task poll or one
    /// piece of loop work; every access goes through the fine-grained locks.
    fn context(&self) -> KernelContext<'_> {
        KernelContext {
            boot: &self.boot,
            allocator: &self.allocator,
            heap: &self.heap,
            channels: &self.channels,
            next_message_id: &self.next_message_id,
        }
    }

    fn run_once(&self, serial: &mut serial::SerialPort) -> bool {
        self.run_once_filtered(serial, false)
    }

    /// Poll the next scheduled task unless another CPU is already running
    /// it. With `skip_commands`, command-service tasks are left to the
    /// application processors so a long command never stalls this CPU.
    fn run_once_filtered(&self, serial: &mut serial::SerialPort, skip_commands: bool) -> bool {
        let Some(task_id) = self.scheduler.lock().next_task() else {
            return false;
        };
        let index = task_id.0 as usize;
        let Some(mut slot) = self.tasks.get(index).and_then(|slot| slot.try_lock()) else {
            return false;
        };
        let Some(task) = slot.as_mut() else {
            return false;
        };
        if skip_commands && matches!(task.kind, TaskKind::Command(_)) {
            return false;
        }
        let mut ctx = self.context();
        let progressed = task.poll(&mut ctx, serial);
        #[cfg(all(not(test), target_os = "none"))]
        if progressed {
            if let Some(index) = current_cpu_index() {
                TASK_RUNS_BY_CPU[index].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            }
        }
        progressed
    }

    fn spawn_task(
        &mut self,
        domain: TaskDomain,
        mut kind: TaskKind,
    ) -> Result<TaskId, KernelError> {
        for (index, slot) in self.tasks.iter().enumerate() {
            let mut slot = slot.lock();
            if slot.is_none() {
                let id = TaskId(index as u32);
                kind.set_task_id(id);
                *slot = Some(TaskSlot {
                    id,
                    domain,
                    time_slice: TimeSlice::new(5),
                    kind,
                });
                self.scheduler.lock().add_task(id);
                return Ok(id);
            }
        }
        Err(KernelError::OutOfTasks)
    }
}

impl KernelApiV0 for Kernel {
    fn create_task(&mut self, _name: &str, _caps: &[Cap<()>]) -> Result<TaskId, KernelError> {
        Err(KernelError::Unsupported)
    }

    fn create_channel(&mut self) -> Result<ChannelId, KernelError> {
        Kernel::create_channel(self)
    }

    fn send(&mut self, channel: ChannelId, message: KernelMessage) -> Result<(), KernelError> {
        self.context().send(channel, message)
    }

    fn recv(&mut self, channel: ChannelId) -> Result<KernelMessage, KernelError> {
        self.context().recv(channel)
    }
}

struct ConsoleService {
    task_id: TaskId,
    command_channel: ChannelId,
    response_channel: ChannelId,
    buffer: [u8; COMMAND_MAX],
    len: usize,
    awaiting_response: bool,
}

impl ConsoleService {
    fn new(command_channel: ChannelId, response_channel: ChannelId) -> Self {
        Self {
            task_id: TaskId(0),
            command_channel,
            response_channel,
            buffer: [0; COMMAND_MAX],
            len: 0,
            awaiting_response: false,
        }
    }

    fn poll(&mut self, ctx: &mut KernelContext, serial: &mut serial::SerialPort) -> bool {
        let mut progressed = false;

        while let Some(message) = ctx.try_recv(self.response_channel) {
            if let KernelMessage::CommandResponse(response) = message {
                self.render_response(serial, &response);
                let _ = write!(serial, "> ");
                self.awaiting_response = false;
                progressed = true;
            }
        }

        if let Some(byte) = serial.try_read_byte() {
            progressed = true;
            match byte {
                b'\r' | b'\n' => {
                    let _ = serial.write_str("\r\n");
                    self.submit_command(ctx, serial);
                    self.len = 0;
                }
                0x08 | 0x7f => {
                    if self.len > 0 {
                        self.len -= 1;
                        let _ = serial.write_str("\x08 \x08");
                    }
                }
                byte => {
                    if self.len < self.buffer.len() {
                        self.buffer[self.len] = byte;
                        self.len += 1;
                        let _ = serial.write_byte(byte);
                    } else {
                        let _ = serial.write_str("\r\nerror: command too long\r\n> ");
                        self.len = 0;
                    }
                }
            }
        }

        progressed
    }

    fn submit_command(&mut self, ctx: &mut KernelContext, serial: &mut serial::SerialPort) {
        let request_id = ctx.next_message_id();
        let Some(request) =
            CommandRequest::from_bytes(&self.buffer[..self.len], request_id, self.response_channel)
        else {
            let _ = writeln!(serial, "error: command too long");
            return;
        };

        if ctx
            .send(self.command_channel, KernelMessage::CommandRequest(request))
            .is_err()
        {
            let _ = writeln!(serial, "error: command queue full");
            return;
        }

        self.awaiting_response = true;
    }

    fn render_response(&self, serial: &mut serial::SerialPort, response: &CommandResponse) {
        match response.status {
            CommandStatus::Ok => {
                if let Some(output) = response.output_str() {
                    let _ = serial.write_str(output);
                    let _ = serial.write_str("\r\n");
                }
            }
            CommandStatus::Error(err) => {
                let _ = serial.write_str("error: ");
                if let Some(msg) = err.as_str() {
                    let _ = serial.write_str(msg);
                } else {
                    let _ = serial.write_str("invalid error");
                }
                let _ = serial.write_str("\r\n");
            }
        }
    }
}

struct CommandService {
    task_id: TaskId,
    command_channel: ChannelId,
    poll_count: u64,
}

impl CommandService {
    fn new(command_channel: ChannelId) -> Self {
        Self {
            task_id: TaskId(0),
            command_channel,
            poll_count: 0,
        }
    }

    fn poll(&mut self, ctx: &mut KernelContext, serial: &mut serial::SerialPort) -> bool {
        let mut progressed = false;

        // The original scaffold slept a full PIT tick here on every other poll
        // ("syscall demo"); that stalled whichever CPU polled this task and
        // held the kernel lock for up to 10 ms.
        self.poll_count += 1;

        while let Some(message) = ctx.try_recv(self.command_channel) {
            progressed = true;
            if let KernelMessage::CommandRequest(request) = message {
                let response = self.handle_command(ctx, serial, &request);
                let _ = ctx.send(
                    request.reply_channel,
                    KernelMessage::CommandResponse(response),
                );
            }
        }
        progressed
    }

    /// `smp run <n>`: queue `n` jobs, wake the other CPUs, report who ran what.
    fn run_smp_jobs(&mut self, output: &mut FixedBuffer<RESPONSE_MAX>, jobs: u32) {
        // Only application processors take jobs; the boot CPU never calls
        // `WORK.take`. And unless `BSP_RUNS_COMMANDS` is set, *this command
        // is itself running on an AP* -- which then blocks in `wait`. So the
        // question is not "is there an AP" but "is there an AP other than
        // the one about to wait".
        //
        // Asking the former meant that on a two-CPU machine the sole AP
        // submitted jobs, woke a boot CPU that does not take them, and then
        // waited on itself. Every job ran out its timeout, serially: the
        // console answered nothing for twenty seconds on `smp run 4`, and
        // for two and a half minutes on `smp run 32`, while the desktop kept
        // repainting so it looked like the shell had hung.
        let online = CPUS.online();
        let takers = if BSP_RUNS_COMMANDS.load(core::sync::atomic::Ordering::Relaxed) {
            online.saturating_sub(1)
        } else {
            online.saturating_sub(2)
        };
        if takers == 0 {
            let _ = writeln!(
                output,
                "smp: no application processor free to run jobs ({online} online)"
            );
            return;
        }
        let before = hal_x86_64::lapic::IPI_COUNT.load(core::sync::atomic::Ordering::Relaxed);
        let mut ids = [u64::MAX; 32];
        for (i, slot) in ids.iter_mut().enumerate().take(jobs as usize) {
            let arg = 1_000 * (i as u64 + 1);
            *slot = WORK.submit(JOB_SUM_OF_SQUARES, arg).unwrap_or(u64::MAX);
        }
        #[cfg(not(test))]
        let woke = LAPIC
            .get()
            .map(|mut apic| apic.send_ipi_all_excluding_self(IPI_WAKE_VECTOR))
            .unwrap_or(false);
        #[cfg(test)]
        let woke = false;
        // Summary first: the response buffer is small, so the per-job lines
        // are the part that may be cut off.
        let mut done = 0;
        let mut lines = FixedBuffer::<RESPONSE_MAX>::new();
        for (i, id) in ids.iter().enumerate().take(jobs as usize) {
            if *id == u64::MAX {
                let _ = writeln!(lines, "  job{i} not queued");
                continue;
            }
            match WORK.wait(*id, SMP_JOB_SPINS) {
                Some(r) => {
                    done += 1;
                    let expected = run_job(hal_x86_64::Job {
                        id: *id,
                        kind: JOB_SUM_OF_SQUARES,
                        arg: 1_000 * (i as u64 + 1),
                    });
                    let _ = writeln!(
                        lines,
                        "  job{} cpu={} v={}{}",
                        i,
                        r.cpu,
                        r.value,
                        if r.value == expected { "" } else { " MISMATCH" }
                    );
                }
                None => {
                    let _ = writeln!(lines, "  job{} timeout", i);
                }
            }
        }
        let ipis =
            hal_x86_64::lapic::IPI_COUNT.load(core::sync::atomic::Ordering::Relaxed) - before;
        let _ = writeln!(
            output,
            "smp: {}/{} jobs done, ipi_sent={}, ipis_received={}",
            done, jobs, woke, ipis
        );
        let _ = output.write_str(core::str::from_utf8(lines.as_bytes()).unwrap_or(""));
    }

    fn handle_command(
        &mut self,
        ctx: &mut KernelContext,
        _serial: &mut serial::SerialPort,
        request: &CommandRequest,
    ) -> CommandResponse {
        let correlation_id = request.request_id;
        let Some(command) = request.as_str() else {
            return CommandResponse::error(
                correlation_id,
                CommandError::new(CommandErrorCode::InvalidCommand, "invalid utf-8"),
            );
        };
        let command = command.trim();
        if command.is_empty() {
            return CommandResponse::ok(correlation_id, &FixedBuffer::new());
        }

        let mut output = FixedBuffer::<RESPONSE_MAX>::new();

        match command {
            "help" => {
                let _ = writeln!(
                    output,
                    "commands: help, halt, boot, mem, cpus, alloc, heap, heap-alloc, ticks"
                );
            }
            cmd if cmd.starts_with("fault") => {
                // Deliberate CPU exceptions for testing the handlers.
                let kind = cmd.split_whitespace().nth(1).unwrap_or("");
                #[cfg(not(test))]
                match kind {
                    "pf" => unsafe {
                        let _ = core::ptr::read_volatile(0x10 as *const u64);
                    },
                    "ud" => unsafe { asm!("ud2", options(nomem, nostack)) },
                    "de" => unsafe {
                        asm!(
                            "xor edx, edx",
                            "mov eax, 1",
                            "xor ecx, ecx",
                            "div ecx",
                            out("eax") _, out("edx") _, out("ecx") _,
                            options(nomem, nostack)
                        )
                    },
                    _ => {}
                }
                let _ = writeln!(output, "usage: fault pf|ud|de");
            }
            "halt" => {
                #[cfg(not(test))]
                {
                    let _ = writeln!(output, "halting...");
                    let _response = CommandResponse::ok(correlation_id, &output);
                    halt_loop();
                }

                #[cfg(test)]
                {
                    return CommandResponse::error(
                        correlation_id,
                        CommandError::new(CommandErrorCode::ServiceUnavailable, "halt unavailable"),
                    );
                }
            }
            "boot" => {
                let boot = ctx.boot();
                match boot.hhdm_offset {
                    Some(offset) => {
                        let _ = writeln!(output, "hhdm: offset=0x{:x}", offset);
                    }
                    None => {
                        let _ = writeln!(output, "hhdm: unavailable");
                    }
                }
                match (boot.kernel_phys, boot.kernel_virt) {
                    (Some(phys), Some(virt)) => {
                        let _ = writeln!(output, "kernel: phys=0x{:x} virt=0x{:x}", phys, virt);
                    }
                    _ => {
                        let _ = writeln!(output, "kernel: address unavailable");
                    }
                }
            }
            #[cfg(all(not(test), target_os = "none"))]
            cmd if cmd.starts_with("net") => {
                let mut parts = cmd.split_whitespace();
                let _ = parts.next();
                let mut net = NET.lock();
                let Some(net) = net.as_mut() else {
                    let _ = writeln!(output, "net: no device");
                    return CommandResponse::ok(correlation_id, &output);
                };
                match (parts.next(), parts.next()) {
                    (Some("ping"), Some(ip)) => match net_stack::wire::parse_ipv4(ip) {
                        Some(ip) => {
                            let _ = net.ping(ip, &get_tick_count, &mut output);
                        }
                        None => {
                            let _ = writeln!(output, "net: bad address");
                        }
                    },
                    (Some("ping"), None) => {
                        let gw = net.ip();
                        let _ = gw;
                        let _ = net.ping([10, 0, 2, 2], &get_tick_count, &mut output);
                    }
                    (Some("poll"), _) => {
                        let event = net.poll();
                        let _ = writeln!(output, "net: polled ({event:?})");
                    }
                    (Some("dhcp"), _) => {
                        let _ = net.dhcp(&get_tick_count, &mut output);
                    }
                    (Some("renew"), _) => {
                        let _ = net.dhcp_renew(&get_tick_count, &mut output);
                    }
                    (Some("udp"), Some(ip)) => {
                        let port = parts.next().and_then(|p| p.parse::<u16>().ok());
                        let text = parts.next().unwrap_or("hello");
                        match (net_stack::wire::parse_ipv4(ip), port) {
                            (Some(ip), Some(port)) => {
                                let _ = net.udp_send(
                                    ip,
                                    port,
                                    text.as_bytes(),
                                    &get_tick_count,
                                    &mut output,
                                );
                            }
                            _ => {
                                let _ = writeln!(output, "usage: net udp <ip> <port> [text]");
                            }
                        }
                    }
                    _ => net.write_status(&mut output),
                }
            }
            cmd if cmd.starts_with("smp") => {
                let mut parts = cmd.split_whitespace();
                let _ = parts.next();
                let sub = parts.next();
                let arg = parts.next();
                match (sub, arg) {
                    (Some("run"), Some(n)) => match n.parse::<u32>() {
                        Ok(n) if n > 0 => self.run_smp_jobs(&mut output, n.min(32)),
                        _ => {
                            let _ = writeln!(output, "usage: smp run <jobs>");
                        }
                    },
                    (Some("present"), Some("on")) => {
                        PARALLEL_PRESENT.store(true, core::sync::atomic::Ordering::Relaxed);
                        let _ = writeln!(
                            output,
                            "smp: parallel present on ({} workers)",
                            present_workers()
                        );
                    }
                    (Some("diag"), _) => {
                        #[cfg(not(test))]
                        let bsp_ticks = get_tick_count();
                        #[cfg(test)]
                        let bsp_ticks = 0u64;
                        let _ = writeln!(
                            output,
                            "lapic timer: initial={} ({} Hz), bsp ticks={}",
                            LAPIC_TIMER_INITIAL.load(core::sync::atomic::Ordering::Relaxed),
                            LAPIC_TIMER_HZ,
                            bsp_ticks
                        );
                        let _ = writeln!(
                            output,
                            "smp diag: fallbacks={} ap_polls={} ap_cycles={} ap_max={}",
                            PRESENT_FALLBACKS.load(core::sync::atomic::Ordering::Relaxed),
                            AP_KERNEL_POLLS.load(core::sync::atomic::Ordering::Relaxed),
                            AP_KERNEL_POLL_CYCLES.load(core::sync::atomic::Ordering::Relaxed),
                            AP_KERNEL_POLL_MAX.load(core::sync::atomic::Ordering::Relaxed)
                        );
                    }
                    (Some("bsp-commands"), Some(state @ ("on" | "off"))) => {
                        BSP_RUNS_COMMANDS
                            .store(state == "on", core::sync::atomic::Ordering::Relaxed);
                        let _ = writeln!(output, "smp: boot cpu runs commands: {state}");
                    }
                    (Some("poll"), Some("on")) => {
                        AP_POLL_ENABLED.store(true, core::sync::atomic::Ordering::Relaxed);
                        let _ = writeln!(output, "smp: ap kernel polling on");
                    }
                    (Some("poll"), Some("off")) => {
                        AP_POLL_ENABLED.store(false, core::sync::atomic::Ordering::Relaxed);
                        let _ = writeln!(output, "smp: ap kernel polling off");
                    }
                    (Some("present"), Some("off")) => {
                        PARALLEL_PRESENT.store(false, core::sync::atomic::Ordering::Relaxed);
                        let _ = writeln!(output, "smp: parallel present off");
                    }
                    (Some("present"), _) => {
                        let on = PARALLEL_PRESENT.load(core::sync::atomic::Ordering::Relaxed);
                        let _ = writeln!(
                            output,
                            "smp: parallel present {} ({} workers)",
                            if on { "on" } else { "off" },
                            present_workers()
                        );
                    }
                    _ => {
                        let _ = writeln!(
                            output,
                            "usage: smp run <jobs> | present [on|off] | poll [on|off] | diag"
                        );
                    }
                }
            }
            cmd if cmd.starts_with("spin") => {
                // Busy-wait for up to 500 ticks: shows which CPU runs
                // commands and whether the desktop keeps rendering meanwhile.
                let ticks: u64 = cmd
                    .split_whitespace()
                    .nth(1)
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(100)
                    .min(500);
                #[cfg(not(test))]
                {
                    let start = get_tick_count();
                    while get_tick_count().saturating_sub(start) < ticks {
                        core::hint::spin_loop();
                    }
                }
                #[cfg(all(not(test), target_os = "none"))]
                let cpu = current_cpu_index();
                #[cfg(any(test, not(target_os = "none")))]
                let cpu: Option<usize> = None;
                let _ = writeln!(output, "spin: {ticks} ticks on cpu {cpu:?}");
            }
            "cpus" => {
                let total = CPU_TOTAL.load(core::sync::atomic::Ordering::Acquire);
                let _ = writeln!(
                    output,
                    "cpus: online={} total={} bsp_lapic={}",
                    CPUS.online(),
                    total,
                    BSP_LAPIC_ID.load(core::sync::atomic::Ordering::Acquire)
                );
                for index in 0..CPUS.online() {
                    if let Some(lapic) = CPUS.lapic_id(index) {
                        let _ = writeln!(
                            output,
                            "  cpu{index} lapic={lapic} ticks={} runs={}",
                            CPU_TICKS[index].load(core::sync::atomic::Ordering::Relaxed),
                            TASK_RUNS_BY_CPU[index].load(core::sync::atomic::Ordering::Relaxed)
                        );
                    }
                }
            }
            "mem" => {
                let boot = ctx.boot();
                let _ = writeln!(
                    output,
                    "memory: entries={} total={} KiB usable={} KiB",
                    boot.mem_entries, boot.mem_total_kib, boot.mem_usable_kib
                );
                #[cfg(not(test))]
                {
                    let heap = GLOBAL_HEAP.stats();
                    let _ = writeln!(
                        output,
                        "heap: used={} KiB free={} KiB total={} KiB allocs={} frees={} largest_free={} KiB blocks={}",
                        heap.used / 1024,
                        heap.free / 1024,
                        heap.total / 1024,
                        heap.allocations,
                        heap.frees,
                        heap.largest_free / 1024,
                        heap.free_blocks
                    );
                }
                if let Some(allocator) = ctx.allocator.lock().as_ref() {
                    let _ = writeln!(
                        output,
                        "allocator: ranges={} frames={} next=0x{:x} reclaimed={}",
                        allocator.range_count(),
                        allocator.total_frames(),
                        allocator.next_frame(),
                        allocator.reclaimed_count()
                    );
                } else {
                    let _ = writeln!(output, "allocator: unavailable");
                }
            }
            "alloc" => {
                if let Some(allocator) = ctx.allocator.lock().as_mut() {
                    if let Some(frame) = allocator.allocate_frame() {
                        if let Some(offset) = ctx.boot().hhdm_offset {
                            let virt = offset + frame;
                            let _ = writeln!(output, "frame: phys=0x{:x} virt=0x{:x}", frame, virt);
                        } else {
                            let _ = writeln!(output, "frame: phys=0x{:x}", frame);
                        }
                    } else {
                        return CommandResponse::error(
                            correlation_id,
                            CommandError::new(CommandErrorCode::Internal, "out of memory"),
                        );
                    }
                } else {
                    return CommandResponse::error(
                        correlation_id,
                        CommandError::new(
                            CommandErrorCode::ServiceUnavailable,
                            "allocator unavailable",
                        ),
                    );
                }
            }
            "heap" => {
                if let Some(heap) = ctx.heap.lock().as_ref() {
                    let stats = heap.stats();
                    let _ = writeln!(
                        output,
                        "heap: used={} bytes free={} bytes total={} allocs={}",
                        stats.used, stats.free, stats.total, stats.allocations
                    );
                } else {
                    let _ = writeln!(output, "heap: unavailable");
                }
            }
            "heap-alloc" => {
                if let Some(heap) = ctx.heap.lock().as_mut() {
                    match heap.alloc(64, 16, AllocationLifetime::KernelTransient) {
                        Some(record) => {
                            let _ = writeln!(
                                output,
                                "heap: allocated 64 bytes at 0x{:x} ({:?})",
                                record.start, record.lifetime
                            );
                        }
                        None => {
                            return CommandResponse::error(
                                correlation_id,
                                CommandError::new(CommandErrorCode::Internal, "heap out of memory"),
                            );
                        }
                    }
                } else {
                    return CommandResponse::error(
                        correlation_id,
                        CommandError::new(CommandErrorCode::ServiceUnavailable, "heap unavailable"),
                    );
                }
            }
            "ticks" => {
                #[cfg(not(test))]
                {
                    let ticks = get_tick_count();
                    let _ = writeln!(output, "kernel ticks: {} (at 100 Hz)", ticks);
                }
                #[cfg(test)]
                {
                    let _ = writeln!(output, "ticks: unavailable in test mode");
                }
            }
            _ => {
                return CommandResponse::error(
                    correlation_id,
                    CommandError::new(CommandErrorCode::InvalidCommand, "unknown command"),
                );
            }
        }

        CommandResponse::ok(correlation_id, &output)
    }
}

#[derive(Copy, Clone)]
struct FixedBuffer<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> FixedBuffer<N> {
    fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl<const N: usize> Write for FixedBuffer<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let available = N.saturating_sub(self.len);
        let len = bytes.len().min(available);
        self.buf[self.len..self.len + len].copy_from_slice(&bytes[..len]);
        self.len += len;
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct Channel {
    queue: [KernelMessage; CHANNEL_CAPACITY],
    head: usize,
    tail: usize,
    full: bool,
}

impl Channel {
    const fn new() -> Self {
        Self {
            queue: [KernelMessage::empty(); CHANNEL_CAPACITY],
            head: 0,
            tail: 0,
            full: false,
        }
    }

    fn reset(&mut self) {
        self.queue = [KernelMessage::empty(); CHANNEL_CAPACITY];
        self.head = 0;
        self.tail = 0;
        self.full = false;
    }

    fn send(&mut self, msg: KernelMessage) -> Result<(), ChannelError> {
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

    fn recv(&mut self) -> Option<KernelMessage> {
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
    reclaimed: [u64; 64],
    reclaimed_len: usize,
    reserved: [Range; 32],
    reserved_len: usize,
}

impl FrameAllocator {
    const fn new() -> Self {
        Self {
            ranges: [Range { start: 0, end: 0 }; 32],
            len: 0,
            current: 0,
            next: 0,
            reclaimed: [0; 64],
            reclaimed_len: 0,
            reserved: [Range { start: 0, end: 0 }; 32],
            reserved_len: 0,
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

    /// Exclude `[base, base + length)` from allocation. Returns false if the
    /// table is full, which the caller must report: a reserved range that is
    /// silently dropped lets the allocator place the heap, or a fresh page
    /// table, on top of the kernel image.
    #[must_use]
    fn add_reserved_range(&mut self, base: u64, length: u64) -> bool {
        // Outward, not inward. Rounding inward is right for a usable range --
        // it shrinks what may be handed out -- and exactly backwards for an
        // exclusion, where it shrinks what must not be. A reserved region
        // whose end was not page-aligned exposed its final partial page, and
        // one shorter than a page was discarded outright.
        let start = align_down(base, PAGE_SIZE);
        let end = align_up(base.saturating_add(length), PAGE_SIZE);
        if end <= start {
            return true;
        }
        if self.reserved_len >= self.reserved.len() {
            return false;
        }
        self.reserved[self.reserved_len] = Range { start, end };
        self.reserved_len += 1;
        true
    }

    fn reserved_range_count(&self) -> usize {
        self.reserved_len
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

    fn reclaimed_count(&self) -> usize {
        self.reclaimed_len
    }

    fn allocate_frame(&mut self) -> Option<u64> {
        if self.reclaimed_len > 0 {
            self.reclaimed_len -= 1;
            return Some(self.reclaimed[self.reclaimed_len]);
        }
        self.allocate_contiguous(1)
    }

    fn free_frame(&mut self, frame: u64) {
        if self.reclaimed_len >= self.reclaimed.len() {
            return;
        }
        self.reclaimed[self.reclaimed_len] = frame;
        self.reclaimed_len += 1;
    }

    /// A contiguous run of `pages` frames, or `None`.
    ///
    /// A failed search used to walk the cursor to the end of the last range
    /// and leave it there, so the allocator was exhausted by a request it
    /// had *refused*: every later call returned None whatever its size. The
    /// cursor is restored on failure, so asking for less after being told no
    /// works, and so does an ordinary `allocate_frame` afterwards.
    fn allocate_contiguous(&mut self, pages: u64) -> Option<u64> {
        if pages == 0 {
            return None;
        }
        let saved = (self.current, self.next);
        let found = self.allocate_contiguous_inner(pages);
        if found.is_none() {
            self.current = saved.0;
            self.next = saved.1;
        }
        found
    }

    fn allocate_contiguous_inner(&mut self, pages: u64) -> Option<u64> {
        let bytes = pages.saturating_mul(PAGE_SIZE);
        while self.current < self.len {
            let range = self.ranges[self.current];
            let mut start = if self.next < range.start {
                range.start
            } else {
                self.next
            };

            loop {
                let end = start.saturating_add(bytes);
                if end > range.end {
                    break;
                }
                if let Some(reserved) = self.first_reserved_overlap(start, end) {
                    start = reserved.end;
                    continue;
                }
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

    fn first_reserved_overlap(&self, start: u64, end: u64) -> Option<Range> {
        let mut i = 0usize;
        while i < self.reserved_len {
            let range = self.reserved[i];
            if start < range.end && end > range.start {
                return Some(range);
            }
            i += 1;
        }
        None
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum AllocationLifetime {
    KernelStatic,
    KernelTransient,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct AllocationRecord {
    start: usize,
    size: usize,
    lifetime: AllocationLifetime,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct AllocationStats {
    used: usize,
    free: usize,
    total: usize,
    allocations: usize,
}

trait KernelAllocator {
    fn alloc(
        &mut self,
        size: usize,
        align: usize,
        lifetime: AllocationLifetime,
    ) -> Option<AllocationRecord>;

    fn stats(&self) -> AllocationStats;
}

struct BumpHeap {
    start: usize,
    end: usize,
    next: core::cell::UnsafeCell<usize>,
    allocations: core::cell::UnsafeCell<usize>,
}

// SAFETY: BumpHeap uses interior mutability but is safe for concurrent use
// because we only run on a single CPU in kernel_bootstrap (no SMP yet).
unsafe impl Sync for BumpHeap {}

impl BumpHeap {
    const fn new(start: usize, size: usize) -> Self {
        Self {
            start,
            end: start.saturating_add(size),
            next: core::cell::UnsafeCell::new(start),
            allocations: core::cell::UnsafeCell::new(0),
        }
    }
}

impl KernelAllocator for BumpHeap {
    fn alloc(
        &mut self,
        size: usize,
        align: usize,
        lifetime: AllocationLifetime,
    ) -> Option<AllocationRecord> {
        // SAFETY: &mut self ensures exclusive access
        unsafe {
            let next = *self.next.get();
            let aligned = align_up_usize(next, align);
            let end = aligned.saturating_add(size);
            if end > self.end {
                return None;
            }
            *self.next.get() = end;
            *self.allocations.get() += 1;
            Some(AllocationRecord {
                start: aligned,
                size,
                lifetime,
            })
        }
    }

    fn stats(&self) -> AllocationStats {
        // SAFETY: read-only access is safe
        unsafe {
            let next = *self.next.get();
            let allocations = *self.allocations.get();
            AllocationStats {
                used: next.saturating_sub(self.start),
                free: self.end.saturating_sub(next),
                total: self.end.saturating_sub(self.start),
                allocations,
            }
        }
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

fn palette_query_hash(query: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in query.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_allocator_reclaims() {
        let mut allocator = FrameAllocator::new();
        allocator.add_range(0x1000, 0x9000);
        allocator.reset_cursor();

        let first = allocator.allocate_frame().unwrap();
        let second = allocator.allocate_frame().unwrap();
        allocator.free_frame(first);

        let reclaimed = allocator.allocate_frame().unwrap();
        assert_eq!(reclaimed, first);
        assert_ne!(reclaimed, second);
    }

    #[test]
    fn test_frame_allocator_excludes_reserved() {
        let mut allocator = FrameAllocator::new();
        allocator.add_range(0x1000, 0x9000);
        allocator.add_reserved_range(0x3000, 0x2000);
        allocator.reset_cursor();

        let a = allocator.allocate_frame().unwrap();
        let b = allocator.allocate_frame().unwrap();
        let c = allocator.allocate_frame().unwrap();

        assert_eq!(a, 0x1000);
        assert_eq!(b, 0x2000);
        assert_eq!(c, 0x5000);
    }

    #[test]
    fn a_refused_contiguous_request_does_not_exhaust_the_allocator() {
        // A failed search walked the cursor to the end of the last range and
        // left it there, so the allocator was spent by a request it had
        // refused. The heap asked for 32 MiB, was told no, and every smaller
        // size it tried afterwards was refused too -- not for lack of memory
        // but because the first "no" had consumed the allocator.
        let mut allocator = FrameAllocator::new();
        allocator.add_range(0x10_0000, 0x40_0000); // 4 MiB
        allocator.reset_cursor();

        assert!(
            allocator.allocate_contiguous(4096).is_none(),
            "16 MiB cannot fit in 4 MiB"
        );
        assert!(
            allocator.allocate_contiguous(256).is_some(),
            "1 MiB must still be available after a larger request was refused"
        );
        assert!(
            allocator.allocate_frame().is_some(),
            "and single frames must still come out"
        );
    }

    #[test]
    fn a_reserved_range_covers_every_page_it_touches() {
        // Reserved ranges were rounded *inward*, the rule that is correct for
        // usable ranges and exactly backwards for exclusions: it shrinks what
        // must not be handed out. `init_memory` feeds the kernel image into
        // this list -- the boot stack, the IDT, the per-CPU GDTs, the IST
        // stacks, the DMA area -- and Limine makes no alignment promise about
        // that entry. Its final partial page was handed to the heap or to a
        // fresh page table, which memsets it. Silent, and the symptom is an
        // arbitrary corruption much later.
        let mut allocator = FrameAllocator::new();
        allocator.add_range(0x10_0000, 0x10_0000);
        // A kernel image whose length is not a whole number of pages.
        allocator.add_reserved_range(0x10_0000, 0x2800);
        allocator.reset_cursor();

        let frame = allocator.allocate_frame().unwrap();
        assert!(
            frame >= 0x10_3000,
            "handed out 0x{frame:x}, which holds bytes 0x2000..0x2800 of a \
             reserved region"
        );
    }

    #[test]
    fn a_reserved_range_smaller_than_a_page_is_still_reserved() {
        // Rounding inward made `end <= start` for any sub-page region, and
        // the guard then dropped it entirely -- the allocator would place the
        // heap straight on top of it.
        let mut allocator = FrameAllocator::new();
        allocator.add_range(0x10_0000, 0x10_0000);
        allocator.add_reserved_range(0x10_0800, 0x400);
        assert_eq!(
            allocator.reserved_range_count(),
            1,
            "a reserved region shorter than a page was discarded"
        );
        allocator.reset_cursor();
        assert!(allocator.allocate_frame().unwrap() >= 0x10_1000);
    }

    #[test]
    fn a_dropped_reserved_range_is_reported_rather_than_swallowed() {
        // The table holds 32 entries and overflow returned quietly. Dropping
        // a usable range only loses memory; dropping a *reserved* one lets
        // the allocator place the heap, or a page table, on top of the kernel
        // image. `init_memory` had no way to notice.
        let mut allocator = FrameAllocator::new();
        for index in 0..40u64 {
            let accepted = allocator.add_reserved_range(index * 0x2000, 0x1000);
            assert_eq!(
                accepted,
                index < 32,
                "range {index} must report whether it was recorded"
            );
        }
    }

    #[test]
    fn a_present_band_wait_gives_up_inside_a_frame() {
        // The budget guards a job worth tens of microseconds and is paid once
        // per band, serially, on the CPU that draws the screen. At
        // 100,000,000 polls it was seconds. A present is paced at 100 Hz, so
        // anything approaching 10 ms is already too long.
        let queue = hal_x86_64::WorkQueue::<8>::new();
        let id = queue.submit(1, 0).expect("the queue must accept a job");

        let started = std::time::Instant::now();
        // Nothing ever takes it, so this always runs the budget out.
        assert!(queue.wait(id, PRESENT_BAND_SPINS).is_none());
        let waited = started.elapsed();

        assert!(
            waited < std::time::Duration::from_millis(50),
            "waiting out the budget took {waited:?}, which is longer than the \
             frame it is meant to fit inside"
        );
    }

    #[test]
    fn test_bump_heap_stats() {
        let mut heap = BumpHeap::new(0x1000, 0x1000);
        let stats = heap.stats();
        assert_eq!(stats.used, 0);
        assert_eq!(stats.free, 0x1000);

        let alloc = heap
            .alloc(64, 16, AllocationLifetime::KernelTransient)
            .unwrap();
        assert_eq!(alloc.size, 64);

        let stats = heap.stats();
        assert_eq!(stats.allocations, 1);
        assert!(stats.used >= 64);
    }

    #[test]
    fn test_time_slice_preemption() {
        let mut slice = TimeSlice::new(3);
        assert!(!slice.should_preempt());
        slice.advance(1);
        assert!(!slice.should_preempt());
        slice.advance(1);
        assert!(!slice.should_preempt());
        slice.advance(1);
        assert!(slice.should_preempt());
    }
}

#[cfg(not(test))]
pub mod serial {
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

        /// One byte, writer lock held. Used for the terminal echo, which is
        /// a single character and would otherwise land inside another CPU's
        /// line.
        pub fn write_byte(&mut self, byte: u8) -> fmt::Result {
            let _writer = SERIAL_LOCK.lock();
            self.write_byte_raw(byte)
        }

        fn write_byte_raw(&mut self, byte: u8) -> fmt::Result {
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

    /// One writer at a time: several CPUs print now.
    static SERIAL_LOCK: hal_x86_64::SpinLock<()> = hal_x86_64::SpinLock::new(());

    impl SerialPort {
        /// Write without taking the writer lock (fatal exception path only,
        /// and internally once the lock is already held).
        pub fn write_str_unlocked(&mut self, s: &str) -> fmt::Result {
            for byte in s.bytes() {
                if byte == b'\n' {
                    self.write_byte_raw(b'\r')?;
                }
                self.write_byte_raw(byte)?;
            }
            Ok(())
        }
    }

    /// Borrows the port without re-taking the writer lock, so one formatted
    /// write is emitted as a unit.
    struct Held<'a>(&'a mut SerialPort);

    impl fmt::Write for Held<'_> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.0.write_str_unlocked(s)
        }
    }

    impl fmt::Write for SerialPort {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let _writer = SERIAL_LOCK.lock();
            self.write_str_unlocked(s)
        }

        /// `writeln!` lowers to several `write_str` calls. Taking the lock
        /// per call let another CPU's line land in the middle of this one,
        /// so the whole formatted write is held instead.
        fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
            let _writer = SERIAL_LOCK.lock();
            fmt::Write::write_fmt(&mut Held(self), args)
        }
    }

    /// Serial writer for interrupt and exception context: never blocks on
    /// the lock the interrupted CPU might itself hold. `SpinLock` does not
    /// mask interrupts, so anything that can preempt a lock holder must use
    /// this rather than `SerialPort`'s `Write`.
    pub struct UnlockedSerial(pub SerialPort);

    impl fmt::Write for UnlockedSerial {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.0.write_str_unlocked(s)
        }
    }
}

#[cfg(test)]
pub mod serial {
    use std::fmt;

    pub const COM1: u16 = 0x3F8;

    #[derive(Default)]
    pub struct SerialPort {
        pub buffer: std::string::String,
    }

    impl SerialPort {
        pub fn new(_base: u16) -> Self {
            Self {
                buffer: std::string::String::new(),
            }
        }

        pub fn init(&mut self) {}

        pub fn write_byte(&mut self, byte: u8) -> fmt::Result {
            self.buffer.push(byte as char);
            Ok(())
        }

        pub fn try_read_byte(&mut self) -> Option<u8> {
            None
        }
    }

    impl fmt::Write for SerialPort {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.buffer.push_str(s);
            Ok(())
        }
    }
}

// Compiler intrinsics required for no_std bare-metal
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    unsafe {
        let c = c as u8;
        for i in 0..n {
            *dest.add(i) = c;
        }
    }
    dest
}

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        for i in 0..n {
            *dest.add(i) = *src.add(i);
        }
    }
    dest
}

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        if dest < src as *mut u8 {
            // Forward copy
            for i in 0..n {
                *dest.add(i) = *src.add(i);
            }
        } else {
            // Backward copy to handle overlap
            for i in (0..n).rev() {
                *dest.add(i) = *src.add(i);
            }
        }
    }
    dest
}

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    unsafe {
        for i in 0..n {
            let a = *s1.add(i);
            let b = *s2.add(i);
            if a != b {
                return a as i32 - b as i32;
            }
        }
    }
    0
}

// Rust language item for unwinding (we don't support it, but it's required)
#[cfg(all(not(test), target_os = "none"))]
#[no_mangle]
pub extern "C" fn rust_eh_personality() {
    // No-op: we don't support unwinding in bare-metal
}
