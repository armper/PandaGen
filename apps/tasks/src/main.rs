//! Tasks, as a program (PROC-011).
//!
//! It asks for its card (handle 0) and exactly one document, `tasks`, to
//! read and write (handle 1) -- nothing else a person keeps. It reads the
//! list when it starts, saves it a second after the last change, and saves
//! at once if its card is closed while a change is waiting. Everything
//! else is `tasks_core`, tested on the host.

#![no_std]
#![no_main]

extern crate alloc;

use pandagen_app::{entry, Card, Console, Documents, Event, Handle, Message, ViewWriter, VIEW_MAX};
use tasks_core::{TasksEffect, TasksView, TASKS_FILE};

entry!(main);

/// Its handles, in the order its image asks for them.
const CARD: Card = Card(Handle(0));
const LIST: Documents = Documents(Handle(1));

/// The machine's time in ticks (a hundredth of a second).
fn now() -> u64 {
    pandagen_app::time_ms() / 10
}

fn main(_nothing: Console) -> u64 {
    let mut tasks = TasksView::new();
    let _ = LIST.read(TASKS_FILE);
    let (mut w, mut h) = (0u16, 0u16);
    let mut view_buffer = alloc::vec![0u8; VIEW_MAX];
    let mut message = alloc::vec![0u8; 8192];
    let mut write_buffer = alloc::vec![0u8; 8192];
    let save = |content: &str, buffer: &mut [u8]| {
        let _ = LIST.write(TASKS_FILE, content.as_bytes(), buffer);
    };
    loop {
        // A change waiting to be saved wakes it to see whether it is due.
        let event = if tasks.is_dirty() {
            CARD.event_within(250)
        } else {
            Some(CARD.next_event())
        };
        if let Some(content) = tasks.save_due(now()) {
            save(&content, &mut write_buffer);
        }
        match event {
            Some(Event::Size { w: nw, h: nh }) => (w, h) = (nw, nh),
            Some(Event::Key(byte)) => match tasks.handle_byte(byte) {
                TasksEffect::None => continue,
                TasksEffect::Redraw => {}
                TasksEffect::Close => {
                    if tasks.is_dirty() {
                        save(&tasks.content(), &mut write_buffer);
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
                            tasks.load(core::str::from_utf8(content).unwrap_or(""))
                        }
                        Some(Message::Absent { .. }) => tasks.load(""),
                        _ => {}
                    }
                }
            }
            Some(Event::Closed) => {
                if tasks.is_dirty() {
                    save(&tasks.content(), &mut write_buffer);
                }
                return 0;
            }
            Some(_) | None => {}
        }
        if w == 0 || h == 0 {
            continue;
        }
        let footer = tasks.footer();
        let mut view = ViewWriter::new(&mut view_buffer, "Tasks", &footer);
        tasks.draw(w, h, &mut view);
        if let Ok(bytes) = view.finish() {
            let _ = CARD.present(bytes);
        }
    }
}
