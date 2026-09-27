//! `ticker`: say the time three times, half a second apart, then check
//! what it may not do -- a handle it never had -- and end. A program that
//! mostly sleeps, and so costs the machine nothing between.

#![no_std]
#![no_main]

use pandagen_app::{entry, say, Console, Error, Handle};

entry!(main);

fn main(console: Console) -> u64 {
    for n in 1..=3 {
        let _ = say!(console, "tick {n} at {} ms", pandagen_app::time_ms());
        pandagen_app::sleep_ms(500);
    }
    let stranger = Console(Handle(5));
    match stranger.send(b"hello?") {
        Err(Error::NoSuchHandle) => {
            let _ = say!(console, "handle 5: not mine, so not there");
        }
        other => {
            let _ = say!(console, "handle 5 answered {other:?}");
            return 1;
        }
    }
    0
}
