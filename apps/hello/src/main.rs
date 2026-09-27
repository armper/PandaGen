//! `hello`: the program that is not on the boot image (PROC-010).
//!
//! It exists to be installed: fetched over the network with `install`,
//! checked, kept on disk, and run from there -- the way programs will
//! come to the machine.

#![no_std]
#![no_main]

use pandagen_app::{entry, say, Console};

entry!(main);

fn main(console: Console) -> u64 {
    let _ = say!(console, "hello from a program installed over the network");
    0
}
