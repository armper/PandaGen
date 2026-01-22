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
pub mod render_stats;

#[cfg(not(test))]
pub mod bare_metal_storage;

#[cfg(not(test))]
pub mod bare_metal_editor_io;

// Tests are in the test module
#[cfg(test)]
mod minimal_editor_tests;

#[cfg(test)]
mod parity_tests;
