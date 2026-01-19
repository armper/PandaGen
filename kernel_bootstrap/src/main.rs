#![no_std]
#![no_main]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;

// Provide a small, deterministic stack and jump into Rust.
global_asm!(
    r#"
.section .text.entry, "ax"
.global _start
.extern rust_main
.intel_syntax noprefix
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
    .att_syntax prefix
"#
);

#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    halt_loop()
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
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
