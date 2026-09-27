#![cfg_attr(not(test), no_std)]

//! Kernel bootstrap library
//!
//! This library contains testable components from the kernel bootstrap,
//! particularly the minimal editor.

#[cfg(not(test))]
extern crate alloc;

#[cfg(test)]
extern crate std;

pub mod desk;
pub mod desktop_frame;
pub mod display_mode;
pub mod display_sink;
pub mod free_list_heap;
pub mod launcher;
pub mod line_edit;
pub mod minimal_editor;
pub mod notepad;
pub mod optimized_render;
pub mod palette_overlay;
pub mod present_policy;
pub mod program_card;
pub mod program_store;
pub mod render_stats;
pub mod rtc;
pub mod sched;
pub mod sketch;
pub mod speaker;
pub mod supervision;
pub mod syscall_abi;
pub mod web;
pub mod widgets;

// Storage modules (available in both test and non-test)
pub mod access_card;
pub mod access_shell;
pub mod audit_card;
pub mod bare_metal_editor_io;
pub mod bare_metal_storage;
pub mod guard;
pub mod sharing;
pub mod sign_in;

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
