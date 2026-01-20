//! # Hardware Abstraction Layer (HAL)
//!
//! This crate defines hardware abstraction traits.
//!
//! ## Philosophy
//!
//! **Architecture must be fully abstracted and swappable.**
//!
//! No architecture-specific assumptions should leak into core logic.
//! The HAL provides traits that architecture-specific crates implement.
//!
//! ## Design Principles
//!
//! 1. **No x86-specific assumptions**: Core logic must work on any architecture
//! 2. **Trait-based**: All hardware operations go through traits
//! 3. **Minimal unsafe**: Hardware access requires unsafe, but keep it isolated
//! 4. **Testable**: HAL can be mocked for testing

pub mod block_device;
pub mod cpu;
pub mod framebuffer;
pub mod interrupts;
pub mod keyboard;
pub mod keyboard_translation;
pub mod memory;
pub mod timer;

pub use block_device::{BlockDevice, BlockError, BLOCK_SIZE};
#[cfg(feature = "alloc")]
pub use block_device::RamDisk;
pub use cpu::CpuHal;
pub use framebuffer::{Framebuffer, FramebufferInfo, PixelFormat};
pub use interrupts::InterruptHal;
pub use keyboard::{HalKeyEvent, HalScancode, KeyboardDevice};
pub use keyboard_translation::{scancode_to_keycode, KeyboardTranslator};
pub use memory::MemoryHal;
pub use timer::{TimerDevice, TimerInterrupt};
