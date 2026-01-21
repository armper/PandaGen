//! VGA text console integration
//!
//! This module provides VGA text console initialization for the kernel.
//! It maps physical VGA memory (0xB8000) using HHDM offset and creates
//! a VgaConsole instance.

use crate::BootInfo;
use console_vga::VgaConsole;

/// VGA text buffer physical address
pub const VGA_TEXT_BUFFER_PHYS: u64 = 0xB8000;

/// VGA text buffer size (80x25 * 2 bytes per cell = 4000 bytes)
pub const VGA_TEXT_BUFFER_SIZE: usize = 80 * 25 * 2;

/// Initialize VGA console from boot information
///
/// # Safety
///
/// The caller must ensure:
/// - HHDM offset is valid
/// - VGA memory is accessible
/// - No other references to VGA memory exist
///
/// Returns None if HHDM offset is not available.
pub unsafe fn init_vga_console(boot_info: &BootInfo) -> Option<VgaConsole> {
    let hhdm_offset = boot_info.hhdm_offset?;

    // Calculate virtual address using HHDM offset
    let vga_virt = (hhdm_offset + VGA_TEXT_BUFFER_PHYS) as usize;

    // Create VGA console
    Some(VgaConsole::new(vga_virt))
}
