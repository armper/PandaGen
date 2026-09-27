//! The Calculator, as a program (PROC-006).
//!
//! It was a card compiled into the kernel; now it runs in ring 3 in an
//! address space of its own and asks for exactly one thing: a card (its
//! handle 0). It cannot reach a file, the network or the console -- it
//! was never given them. Everything it does is `calculator_core`, tested
//! on the host; this is the loop that joins it to its card.

#![no_std]
#![no_main]

extern crate alloc;

use calculator_core::Calculator;
use pandagen_app::{entry, Card, Console, Event, Handle, ViewWriter, VIEW_MAX};

entry!(main);

/// Its only handle.
const CARD: Card = Card(Handle(0));

fn main(_nothing: Console) -> u64 {
    let mut calc = Calculator::new();
    let (mut w, mut h) = (0u16, 0u16);
    let mut buffer = alloc::vec![0u8; VIEW_MAX];
    loop {
        match CARD.next_event() {
            Event::Size { w: nw, h: nh } => (w, h) = (nw, nh),
            Event::Key(byte) => {
                if !calc.handle_byte(byte) {
                    continue;
                }
            }
            Event::Closed => return 0,
        }
        if w == 0 || h == 0 {
            continue;
        }
        let footer = calc.footer();
        let mut view = ViewWriter::new(&mut buffer, "Calculator", &footer);
        calc.draw(w, h, &mut view);
        if let Ok(bytes) = view.finish() {
            let _ = CARD.present(bytes);
        }
    }
}
