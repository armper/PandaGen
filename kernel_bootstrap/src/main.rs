#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
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

mod output;
mod workspace;

use core::fmt::Write;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::str;
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};

#[cfg(not(test))]
use core::arch::{asm, global_asm};
#[cfg(not(test))]
use core::panic::PanicInfo;
use limine_protocol::structures::memory_map_entry::EntryType;
use limine_protocol::{FramebufferRequest, HHDMRequest, KernelAddressRequest, MemoryMapRequest, Request};

#[cfg(not(test))]
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
    .skip 65536
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
        let addr = handler as usize;
        self.offset_low = (addr & 0xFFFF) as u16;
        self.offset_mid = ((addr >> 16) & 0xFFFF) as u16;
        self.offset_high = ((addr >> 32) & 0xFFFFFFFF) as u32;
        self.selector = selector;
        self.ist = 0;
        self.flags = IDT_PRESENT_INTERRUPT_GATE;
        self.reserved = 0;
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

#[cfg(not(test))]
const IDT_PRESENT_INTERRUPT_GATE: u8 = 0x8E; // Present, DPL=0, interrupt gate

#[cfg(not(test))]
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
"#
);

#[cfg(not(test))]
extern "C" {
    fn irq_timer_entry();
    fn irq_keyboard_entry();
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
    // Read scancode from PS/2 data port
    unsafe {
        let status = inb(0x64);
        if (status & 0x01) != 0 {
            let scancode = inb(0x60);
            KEYBOARD_EVENT_QUEUE.push(scancode);
        }

        // Send EOI to PIC
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

        let idtr = IdtPointer {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: core::ptr::addr_of!(IDT) as *const _ as u64,
        };

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

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    let mut serial = serial::SerialPort::new(serial::COM1);
    serial.init();
    kprintln!(serial, "PandaGen: kernel_bootstrap online");
    let boot = boot_info(&mut serial);
    let (allocator, heap) = init_memory(&mut serial, &boot);

    kprintln!(serial, "Initializing interrupts...");
    install_idt();
    klog!(
        serial,
        "IDT installed at 0x{:x}\r\n",
        core::ptr::addr_of!(IDT) as usize
    );

    init_pic();
    kprintln!(serial, "PIC remapped to IRQ base 32");

    init_pit();
    kprintln!(serial, "PIT configured for 100 Hz");

    unmask_timer_irq();
    unmask_keyboard_irq();
    enable_interrupts();
    kprintln!(
        serial,
        "Interrupts enabled, timer at 100 Hz, keyboard IRQ 1"
    );

    let kernel = unsafe {
        Kernel::init_in_place(
            &mut *core::ptr::addr_of_mut!(KERNEL_STORAGE),
            boot,
            allocator,
            heap,
        )
    };
    
    // Phase 64: Boot directly into workspace prompt
    kprintln!(serial, "\r\n=== PandaGen Workspace ===");
    kprintln!(serial, "Boot complete. Type 'help' for commands.\r\n");

    workspace_loop(&mut serial, kernel)
}

#[cfg(not(test))]
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
fn console_loop(serial: &mut serial::SerialPort, kernel: &mut Kernel) -> ! {
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
#[cfg(not(test))]
fn workspace_loop(serial: &mut serial::SerialPort, kernel: &mut Kernel) -> ! {
    // Get command and response channels from kernel
    let command_channel = ChannelId(0);
    let response_channel = ChannelId(1);
    
    let mut workspace = workspace::WorkspaceSession::new(command_channel, response_channel);
    let mut parser_state = Ps2ParserState::new();
    
    // Show initial prompt
    workspace.show_prompt(serial);
    
    loop {
        // Run kernel tasks
        let kernel_progressed = kernel.run_once(serial);
        
        // Process keyboard input
        let mut input_progressed = false;
        while let Some(scancode) = KEYBOARD_EVENT_QUEUE.pop() {
            if let Some(ch) = parser_state.process_scancode(scancode) {
                // Build kernel context
                let Kernel {
                    boot,
                    allocator,
                    heap,
                    channels,
                    next_message_id,
                    ..
                } = kernel;
                
                let mut ctx = KernelContext {
                    boot,
                    allocator,
                    heap,
                    channels,
                    next_message_id,
                };
                
                input_progressed = workspace.process_input(ch, &mut ctx, serial);
            }
        }
        
        // Check for responses from command service
        let Kernel {
            boot,
            allocator,
            heap,
            channels,
            next_message_id,
            ..
        } = kernel;
        
        let mut ctx = KernelContext {
            boot,
            allocator,
            heap,
            channels,
            next_message_id,
        };
        
        // Try to receive response
        if let Some(message) = ctx.try_recv(response_channel) {
            if let KernelMessage::CommandResponse(response) = message {
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
                workspace.show_prompt(serial);
            }
        }
        
        if !kernel_progressed && !input_progressed {
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
            if let Some(ch) = parser_state.process_scancode(scancode) {
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
        OUTPUT.render_to_serial(serial, &lines, cursor_line, cursor_col, Some(status), revision);
    }
}

/// PS/2 scancode parser state for translating to ASCII
#[cfg(not(test))]
struct Ps2ParserState {
    pending_e0: bool,
    shift_pressed: bool,
}

#[cfg(not(test))]
impl Ps2ParserState {
    fn new() -> Self {
        Self {
            pending_e0: false,
            shift_pressed: false,
        }
    }

    /// Process a scancode byte and return ASCII character if available
    fn process_scancode(&mut self, scancode: u8) -> Option<u8> {
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

        // Ignore E0-prefixed keys and break codes for now
        if self.pending_e0 || is_break {
            self.pending_e0 = false;
            return None;
        }

        self.pending_e0 = false;

        // Translate make code to ASCII
        let ascii = match code {
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
                if self.shift_pressed {
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
            0x39 => b' ',  // Space
            0x1C => b'\n', // Enter
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

        Some(ascii)
    }
}

#[cfg(not(test))]
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

        if let Some(map) = MEMORY_MAP_REQUEST
            .get_response()
            .and_then(|resp| resp.get_memory_map())
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

        // Request framebuffer from Limine
        match FRAMEBUFFER_REQUEST.get_response() {
            Some(fb_resp) => {
                if let Some(framebuffers) = fb_resp.get_framebuffers() {
                    if !framebuffers.is_empty() {
                        let fb = framebuffers[0];
                        info.framebuffer_addr = Some(fb.address);
                        info.framebuffer_width = fb.width;
                        info.framebuffer_height = fb.height;
                        info.framebuffer_pitch = fb.pitch;
                        info.framebuffer_bpp = fb.bpp;
                        kprintln!(serial, "framebuffer: {}x{} @ 0x{:x} ({} bpp)",
                            fb.width, fb.height, fb.address as usize, fb.bpp);
                    } else {
                        kprintln!(serial, "framebuffer: no framebuffer devices available");
                    }
                } else {
                    kprintln!(serial, "framebuffer: unavailable (failed to get list)");
                }
            }
            None => {
                kprintln!(serial, "framebuffer: unavailable (no response)");
            }
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

#[cfg(not(test))]
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
        match entry.kind {
            EntryType::Usable => allocator.add_range(entry.base, entry.length),
            EntryType::BootloaderReclaimable | EntryType::KernelAndModules => {
                allocator.add_reserved_range(entry.base, entry.length)
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

    let heap = match boot.hhdm_offset {
        Some(offset) => init_heap(serial, &mut allocator, offset),
        None => {
            let _ = writeln!(serial, "heap: skipped (no hhdm)");
            None
        }
    };

    (Some(allocator), heap)
}

#[cfg(not(test))]
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
    let virt_base = (hhdm_offset + phys_base) as usize;
    let size = (HEAP_PAGES * PAGE_SIZE) as usize;

    let heap = BumpHeap::new(virt_base, size);
    let _ = writeln!(serial, "heap: base=0x{:x} size={} bytes", virt_base, size);
    Some(heap)
}

#[cfg(not(test))]
#[used]
#[link_section = ".limine_requests"]
static HHDM_REQUEST: Request<HHDMRequest> = HHDMRequest::new().into();

#[cfg(not(test))]
#[used]
#[link_section = ".limine_requests"]
static MEMORY_MAP_REQUEST: Request<MemoryMapRequest> = MemoryMapRequest::new().into();

#[cfg(not(test))]
#[used]
#[link_section = ".limine_requests"]
static KERNEL_ADDRESS_REQUEST: Request<KernelAddressRequest> = KernelAddressRequest::new().into();

#[cfg(not(test))]
#[used]
#[link_section = ".limine_requests"]
static FRAMEBUFFER_REQUEST: Request<FramebufferRequest> = FramebufferRequest::new().into();

#[cfg(not(test))]
static mut KERNEL_STORAGE: MaybeUninit<Kernel> = MaybeUninit::uninit();

const PAGE_SIZE: u64 = 4096;
const CHANNEL_CAPACITY: usize = 8;
const COMMAND_MAX: usize = 64;
const RESPONSE_MAX: usize = 256;
const ERROR_MAX: usize = 96;
const MAX_TASKS: usize = 8;
const MAX_CHANNELS: usize = 16;
const KEYBOARD_QUEUE_SIZE: usize = 64;

/// Bounded lock-free ring buffer for keyboard scancodes
///
/// This queue is written from IRQ context (push) and read from main loop (drain).
/// Drop policy: DropOldest - when full, oldest scancode is overwritten.
#[cfg(not(test))]
struct KeyboardEventQueue {
    buffer: [AtomicU8; KEYBOARD_QUEUE_SIZE],
    write_pos: AtomicU8,
    read_pos: AtomicU8,
}

#[cfg(not(test))]
impl KeyboardEventQueue {
    #[allow(clippy::declare_interior_mutable_const)]
    const fn new() -> Self {
        const ZERO: AtomicU8 = AtomicU8::new(0);
        Self {
            buffer: [ZERO; KEYBOARD_QUEUE_SIZE],
            write_pos: AtomicU8::new(0),
            read_pos: AtomicU8::new(0),
        }
    }

    /// Pushes a scancode from IRQ context.
    /// If queue is full, overwrites oldest entry (DropOldest policy).
    fn push(&self, scancode: u8) {
        let write_idx = self.write_pos.load(Ordering::Relaxed) as usize % KEYBOARD_QUEUE_SIZE;
        self.buffer[write_idx].store(scancode, Ordering::Release);

        let new_write = self.write_pos.load(Ordering::Relaxed).wrapping_add(1);
        self.write_pos.store(new_write, Ordering::Release);

        // If we've caught up to read position, advance it (drop oldest)
        let read = self.read_pos.load(Ordering::Acquire);
        if new_write.wrapping_sub(read) >= KEYBOARD_QUEUE_SIZE as u8 {
            self.read_pos.store(read.wrapping_add(1), Ordering::Release);
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

        let read_idx = read as usize % KEYBOARD_QUEUE_SIZE;
        let scancode = self.buffer[read_idx].load(Ordering::Acquire);
        self.read_pos.store(read.wrapping_add(1), Ordering::Release);

        Some(scancode)
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
    framebuffer_width: u16,
    framebuffer_height: u16,
    framebuffer_pitch: u16,
    framebuffer_bpp: u16,
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
    allocator: &'a mut Option<FrameAllocator>,
    heap: &'a mut Option<BumpHeap>,
    channels: &'a mut [Channel; MAX_CHANNELS],
    next_message_id: &'a mut u64,
}

impl KernelContext<'_> {
    fn next_message_id(&mut self) -> MessageId {
        let id = *self.next_message_id;
        *self.next_message_id = (*self.next_message_id).saturating_add(1);
        MessageId(id)
    }

    fn try_recv(&mut self, channel: ChannelId) -> Option<KernelMessage> {
        if channel.index() >= MAX_CHANNELS {
            return None;
        }
        self.channels[channel.index()].recv()
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
            .send(message)
            .map_err(|_| KernelError::ChannelFull)
    }

    fn recv(&mut self, channel: ChannelId) -> Result<KernelMessage, KernelError> {
        if channel.index() >= MAX_CHANNELS {
            return Err(KernelError::InvalidChannel);
        }
        self.channels[channel.index()]
            .recv()
            .ok_or(KernelError::ChannelEmpty)
    }
}

struct Kernel {
    boot: BootInfo,
    allocator: Option<FrameAllocator>,
    heap: Option<BumpHeap>,
    channels: [Channel; MAX_CHANNELS],
    channel_count: u8,
    next_message_id: u64,
    scheduler: CooperativeScheduler,
    tasks: [Option<TaskSlot>; MAX_TASKS],
}

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
        core::ptr::addr_of_mut!((*ptr).allocator).write(allocator);
        core::ptr::addr_of_mut!((*ptr).heap).write(heap);
        core::ptr::addr_of_mut!((*ptr).channel_count).write(0);
        core::ptr::addr_of_mut!((*ptr).next_message_id).write(1);
        core::ptr::addr_of_mut!((*ptr).scheduler).write(CooperativeScheduler::new());

        let channels_ptr = core::ptr::addr_of_mut!((*ptr).channels) as *mut Channel;
        for idx in 0..MAX_CHANNELS {
            channels_ptr.add(idx).write(Channel::new());
        }

        let tasks_ptr = core::ptr::addr_of_mut!((*ptr).tasks) as *mut Option<TaskSlot>;
        for idx in 0..MAX_TASKS {
            tasks_ptr.add(idx).write(None);
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
            allocator,
            heap,
            channels: [Channel::new(); MAX_CHANNELS],
            channel_count: 0,
            next_message_id: 1,
            scheduler: CooperativeScheduler::new(),
            tasks: core::array::from_fn(|_| None),
        };

        let command_channel = kernel.create_channel().expect("command channel available");
        let response_channel = kernel.create_channel().expect("response channel available");

        let command_task = CommandService::new(command_channel);
        let console_task = ConsoleService::new(command_channel, response_channel);

        let _ = kernel.spawn_task(TaskDomain::User, TaskKind::Command(command_task));
        let _ = kernel.spawn_task(TaskDomain::User, TaskKind::Console(console_task));

        kernel
    }

    fn run_once(&mut self, serial: &mut serial::SerialPort) -> bool {
        let Kernel {
            boot,
            allocator,
            heap,
            channels,
            next_message_id,
            scheduler,
            tasks,
            ..
        } = self;

        let Some(task_id) = scheduler.next_task() else {
            return false;
        };
        let index = task_id.0 as usize;
        let Some(task) = tasks.get_mut(index).and_then(Option::as_mut) else {
            return false;
        };
        let mut ctx = KernelContext {
            boot,
            allocator,
            heap,
            channels,
            next_message_id,
        };
        task.poll(&mut ctx, serial)
    }

    fn spawn_task(
        &mut self,
        domain: TaskDomain,
        mut kind: TaskKind,
    ) -> Result<TaskId, KernelError> {
        if let Some((index, slot_ref)) = self
            .tasks
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
        {
            let id = TaskId(index as u32);
            kind.set_task_id(id);
            *slot_ref = Some(TaskSlot {
                id,
                domain,
                time_slice: TimeSlice::new(5),
                kind,
            });
            self.scheduler.add_task(id);
            Ok(id)
        } else {
            Err(KernelError::OutOfTasks)
        }
    }
}

impl KernelApiV0 for Kernel {
    fn create_task(&mut self, _name: &str, _caps: &[Cap<()>]) -> Result<TaskId, KernelError> {
        Err(KernelError::Unsupported)
    }

    fn create_channel(&mut self) -> Result<ChannelId, KernelError> {
        if self.channel_count as usize >= MAX_CHANNELS {
            return Err(KernelError::OutOfChannels);
        }
        let id = ChannelId(self.channel_count);
        self.channel_count = self.channel_count.saturating_add(1);
        self.channels[id.index()].reset();
        Ok(id)
    }

    fn send(&mut self, channel: ChannelId, message: KernelMessage) -> Result<(), KernelError> {
        if channel.index() >= MAX_CHANNELS {
            return Err(KernelError::InvalidChannel);
        }
        self.channels[channel.index()]
            .send(message)
            .map_err(|_| KernelError::ChannelFull)
    }

    fn recv(&mut self, channel: ChannelId) -> Result<KernelMessage, KernelError> {
        if channel.index() >= MAX_CHANNELS {
            return Err(KernelError::InvalidChannel);
        }
        self.channels[channel.index()]
            .recv()
            .ok_or(KernelError::ChannelEmpty)
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

        // Demonstrate syscalls (alternating between yield and sleep)
        #[cfg(not(test))]
        {
            if self.poll_count % 2 == 0 {
                sys_yield();
            } else {
                sys_sleep(1); // Sleep for 1 tick
            }
            self.poll_count += 1;
        }

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
                    "commands: help, halt, boot, mem, alloc, heap, heap-alloc, ticks"
                );
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
            "mem" => {
                let boot = ctx.boot();
                let _ = writeln!(
                    output,
                    "memory: entries={} total={} KiB usable={} KiB",
                    boot.mem_entries, boot.mem_total_kib, boot.mem_usable_kib
                );
                if let Some(allocator) = ctx.allocator.as_ref() {
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
                if let Some(allocator) = ctx.allocator.as_mut() {
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
                if let Some(heap) = ctx.heap.as_ref() {
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
                if let Some(heap) = ctx.heap.as_mut() {
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

    fn add_reserved_range(&mut self, base: u64, length: u64) {
        let start = align_up(base, PAGE_SIZE);
        let end = align_down(base.saturating_add(length), PAGE_SIZE);
        if end <= start || self.reserved_len >= self.reserved.len() {
            return;
        }
        self.reserved[self.reserved_len] = Range { start, end };
        self.reserved_len += 1;
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

    fn allocate_contiguous(&mut self, pages: u64) -> Option<u64> {
        if pages == 0 {
            return None;
        }
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
    next: usize,
    allocations: usize,
}

impl BumpHeap {
    fn new(start: usize, size: usize) -> Self {
        Self {
            start,
            end: start.saturating_add(size),
            next: start,
            allocations: 0,
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
        let aligned = align_up_usize(self.next, align);
        let end = aligned.saturating_add(size);
        if end > self.end {
            return None;
        }
        self.next = end;
        self.allocations += 1;
        Some(AllocationRecord {
            start: aligned,
            size,
            lifetime,
        })
    }

    fn stats(&self) -> AllocationStats {
        AllocationStats {
            used: self.next.saturating_sub(self.start),
            free: self.end.saturating_sub(self.next),
            total: self.end.saturating_sub(self.start),
            allocations: self.allocations,
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
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn rust_eh_personality() {
    // No-op: we don't support unwinding in bare-metal
}
