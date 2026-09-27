//! `fragile`: a program that breaks when asked (PROC-007).
//!
//! Its card has one button, Break, which writes through a null pointer.
//! The CPU refuses (the first 4 MiB of every program are unmapped), the
//! kernel ends the program, and the desk -- its supervisor -- starts it
//! again into the same card, up to three times a minute; then the card
//! says it has given up and offers to start it again.

#![no_std]
#![no_main]

use pandagen_app::{entry, line, Area, Card, Console, Event, Handle, Kind, Op, Role, ViewWriter};

entry!(main);

/// Its only handle.
const CARD: Card = Card(Handle(0));

fn main(_nothing: Console) -> u64 {
    let started = pandagen_app::time_ms();
    loop {
        match CARD.next_event() {
            Event::Size { w, h } => show(w, h, started),
            Event::Key(b'b') => {
                // SAFETY: none -- that is the point. Address 0x10 is not
                // this program's; the kernel ends it here.
                unsafe { core::ptr::write_volatile(0x10 as *mut u64, 1) };
            }
            Event::Key(_) => {}
            Event::Closed => return 0,
        }
    }
}

fn show(w: u16, h: u16, started: u64) {
    let mut buffer = [0u8; 1024];
    let mut view = ViewWriter::new(
        &mut buffer,
        "Fragile",
        "B breaks it; the desk starts it again",
    );
    let when = line!("Started at {started} ms");
    view.op(Op::Text {
        x: 16,
        y: 16,
        role: Role::Text,
        scale: 1,
        text: when.as_str(),
    });
    let bw = 160u16.min(w.saturating_sub(32));
    view.op(Op::Button {
        area: Area::new(w.saturating_sub(bw) / 2, h / 2, bw, 44.min(h / 3)),
        kind: Kind::Primary,
        key: b'b',
        label: "Break",
    });
    if let Ok(bytes) = view.finish() {
        let _ = CARD.present(bytes);
    }
}
