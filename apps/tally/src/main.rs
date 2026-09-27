//! `tally`: a tally counter with a card of its own (PROC-005).
//!
//! The first program that draws on the desk. It describes its card --
//! the count, large, and three buttons -- and the desk draws that in the
//! desk's theme; the buttons stand for keys, so a click and a key press
//! are the same thing to it. It asks for two things: the console (handle
//! 0), to say goodbye on, and a card (handle 1).

#![no_std]
#![no_main]

use pandagen_app::{
    entry, line, say, Area, Card, Console, Event, Handle, Kind, Op, Role, ViewWriter,
};

entry!(main);

/// Its handles, in the order its image asks for them.
const CARD: Card = Card(Handle(1));

fn main(console: Console) -> u64 {
    let (mut w, mut h) = (0u16, 0u16);
    let mut count: i64 = 0;
    loop {
        match CARD.next_event() {
            Event::Size { w: nw, h: nh } => (w, h) = (nw, nh),
            Event::Key(b'+' | b'=' | b' ') => count += 1,
            Event::Key(b'-' | b'_') => count -= 1,
            Event::Key(b'0') => count = 0,
            Event::Key(_) => continue,
            Event::Closed => {
                let _ = say!(console, "counted to {count}; goodbye");
                return 0;
            }
            _ => continue,
        }
        if w > 0 && h > 0 {
            show(count, w, h);
        }
    }
}

/// The card: the count in the middle, three buttons along the bottom.
fn show(count: i64, w: u16, h: u16) {
    let mut buffer = [0u8; 1024];
    let title = line!("Tally: {count}");
    let mut view = ViewWriter::new(
        &mut buffer,
        title.as_str(),
        "+ or Space adds one, - takes one away, 0 starts again",
    );
    let pad = 16u16;
    let buttons_h = 48u16.min(h / 3);
    let number = line!("{count}");
    // Scale 2 is 16 pixels a character across, 32 high.
    let text_w = number.as_str().len() as u16 * 16;
    let top = h.saturating_sub(buttons_h + pad) / 2;
    view.op(Op::Fill {
        area: Area::new(
            pad,
            pad,
            w.saturating_sub(2 * pad),
            h.saturating_sub(buttons_h + 3 * pad),
        ),
        role: Role::Raised,
        radius: 12,
    })
    .op(Op::Text {
        x: (w.saturating_sub(text_w)) / 2,
        y: top.saturating_sub(16),
        role: Role::Text,
        scale: 2,
        text: number.as_str(),
    });
    let cell = w.saturating_sub(4 * pad) / 3;
    let y = h.saturating_sub(buttons_h + pad);
    for (i, (label, key, kind)) in [
        ("-", b'-', Kind::Accent),
        ("Reset", b'0', Kind::Quiet),
        ("+", b'+', Kind::Primary),
    ]
    .into_iter()
    .enumerate()
    {
        view.op(Op::Button {
            area: Area::new(pad + i as u16 * (cell + pad), y, cell, buttons_h),
            kind,
            key,
            label,
        });
    }
    if let Ok(bytes) = view.finish() {
        let _ = CARD.present(bytes);
    }
}
