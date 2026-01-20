#![no_std]
#![no_main]

use core::arch::{asm, global_asm};
use core::fmt::Write;
use core::panic::PanicInfo;
use core::str;

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
    let _ = writeln!(serial, "Type 'help' for commands.");
    let _ = write!(serial, "> ");

    console_loop(&mut serial)
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

fn console_loop(serial: &mut serial::SerialPort) -> ! {
    let mut buffer = [0u8; 128];
    let mut len = 0usize;

    loop {
        if let Some(byte) = serial.try_read_byte() {
            match byte {
                b'\r' | b'\n' => {
                    let _ = serial.write_str("\r\n");
                    handle_command(serial, &buffer[..len]);
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

fn handle_command(serial: &mut serial::SerialPort, line: &[u8]) {
    let Ok(command) = str::from_utf8(line) else {
        let _ = writeln!(serial, "error: invalid utf-8");
        return;
    };
    let command = command.trim();
    if command.is_empty() {
        return;
    }

    match command {
        "help" => {
            let _ = writeln!(serial, "commands: help, halt");
        }
        "halt" => {
            let _ = writeln!(serial, "halting...");
            halt_loop();
        }
        _ => {
            let _ = writeln!(serial, "unknown command: {}", command);
        }
    }
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
