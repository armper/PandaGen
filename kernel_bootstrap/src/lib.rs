#![cfg_attr(not(test), no_std)]

//! Kernel bootstrap library
//! 
//! This library contains testable components from the kernel bootstrap,
//! particularly the minimal editor.

#[cfg(not(test))]
extern crate alloc;

pub mod minimal_editor;

// Tests are in the test module
#[cfg(test)]
mod minimal_editor_tests;
