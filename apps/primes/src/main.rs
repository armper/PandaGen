//! `primes`: count the primes below a million, flat out, and say how
//! long it took -- a program that never waits, written in Rust against
//! `pandagen_app`, loaded from its image and run in ring 3.

#![no_std]
#![no_main]

use pandagen_app::{entry, say, Console};

entry!(main);

fn main(console: Console) -> u64 {
    let started = pandagen_app::time_ms();
    let limit = 1_000_000u32;
    let count = (2..limit).filter(|&n| is_prime(n)).count();
    let took = pandagen_app::time_ms() - started;
    let _ = say!(console, "{count} primes below {limit}, in {took} ms");
    0
}

fn is_prime(n: u32) -> bool {
    if n < 4 {
        return n >= 2;
    }
    if n.is_multiple_of(2) {
        return false;
    }
    let mut d = 3;
    while d * d <= n {
        if n.is_multiple_of(d) {
            return false;
        }
        d += 2;
    }
    true
}
