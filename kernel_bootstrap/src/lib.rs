#![cfg_attr(not(test), no_std)]

//! Kernel bootstrap library
//!
//! This library contains testable components from the kernel bootstrap,
//! particularly the minimal editor.

#[cfg(not(test))]
extern crate alloc;

#[cfg(test)]
extern crate std;

pub mod display_sink;
pub mod minimal_editor;
pub mod optimized_render;
pub mod palette_overlay;
pub mod render_stats;

// Storage modules (available in both test and non-test)
pub mod bare_metal_editor_io;
pub mod bare_metal_storage;

// Workspace platform adapter (test-only for now until no_std dependencies resolved)
#[cfg(test)]
pub mod workspace_platform;

// Tests are in the test module
#[cfg(test)]
mod minimal_editor_tests;

#[cfg(test)]
mod parity_tests;

#[cfg(test)]
mod bare_metal_storage_tests;
