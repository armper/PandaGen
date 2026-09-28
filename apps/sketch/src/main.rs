//! Sketch, as a program (PROC-012).
//!
//! It asks for its card (handle 0) and exactly one document, `sketch`, to
//! read and write (handle 1). The pointer comes to it as events on its
//! card -- a press, the moves while pressed, the release -- and the
//! drawing goes back as one path a stroke. It saves a second after the
//! last change, and at once if its card closes with a change waiting.
//! Everything else is `sketch_core`, tested on the host.

#![no_std]
#![no_main]

extern crate alloc;

use pandagen_app::{entry, Card, Console, Documents, Event, Handle, Message, ViewWriter, VIEW_MAX};
use sketch_core::{SketchEffect, SketchView, SKETCH_FILE};

entry!(main);

/// Its handles, in the order its image asks for them.
const CARD: Card = Card(Handle(0));
const DRAWING: Documents = Documents(Handle(1));

/// The machine's time in ticks (a hundredth of a second).
fn now() -> u64 {
    pandagen_app::time_ms() / 10
}

fn main(_nothing: Console) -> u64 {
    let mut sketch = SketchView::new();
    let _ = DRAWING.read(SKETCH_FILE);
    let (mut w, mut h) = (0u16, 0u16);
    let mut view_buffer = alloc::vec![0u8; VIEW_MAX];
    // A full drawing's document is ~100 KB: room for it each way.
    let mut message = alloc::vec![0u8; 128 * 1024];
    let mut write_buffer = alloc::vec![0u8; 128 * 1024];
    let save = |content: &str, buffer: &mut [u8]| {
        let _ = DRAWING.write(SKETCH_FILE, content.as_bytes(), buffer);
    };
    loop {
        let event = if sketch.is_dirty() {
            CARD.event_within(250)
        } else {
            Some(CARD.next_event())
        };
        if let Some(content) = sketch.save_due(now()) {
            save(&content, &mut write_buffer);
        }
        match event {
            Some(Event::Size { w: nw, h: nh }) => (w, h) = (nw, nh),
            Some(Event::Pointer { kind, x, y }) => {
                if !sketch.pointer(kind, x, y) {
                    continue;
                }
            }
            Some(Event::Key(byte)) => match sketch.handle_byte(byte) {
                SketchEffect::None => continue,
                SketchEffect::Redraw => {}
                SketchEffect::Close => {
                    if sketch.is_dirty() {
                        save(&sketch.content(), &mut write_buffer);
                    }
                    return 0;
                }
            },
            Some(Event::Message) => {
                while let Ok(n) = pandagen_app::receive(&mut message) {
                    if n == 0 {
                        break;
                    }
                    match pandagen_app::message::decode(&message[..n]) {
                        Some(Message::Document { content, .. }) => {
                            sketch.load(core::str::from_utf8(content).unwrap_or(""))
                        }
                        Some(Message::Absent { .. }) => sketch.load(""),
                        _ => {}
                    }
                }
            }
            Some(Event::Closed) => {
                if sketch.is_dirty() {
                    save(&sketch.content(), &mut write_buffer);
                }
                return 0;
            }
            Some(_) | None => {}
        }
        if w == 0 || h == 0 {
            continue;
        }
        let footer = sketch.footer();
        let mut view = ViewWriter::new(&mut view_buffer, "Sketch", &footer);
        sketch.draw(w, h, &mut view);
        if let Ok(bytes) = view.finish() {
            let _ = CARD.present(bytes);
        }
    }
}
