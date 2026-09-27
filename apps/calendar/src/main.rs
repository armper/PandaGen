//! The Calendar, as a program (PROC-009).
//!
//! It asks for its card (handle 0) and the documents named like days,
//! `####-##-##` (handle 1): their names, for the dots, and the right to
//! have one opened in a Notepad. It cannot read or write a note, or see
//! any other document -- the desk's Notepad does the writing, and the
//! person sees it happen. Everything else is `calendar_core`, tested on
//! the host.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use calendar_core::{CalendarEffect, CalendarView, Date};
use pandagen_app::{entry, Card, Console, Documents, Event, Handle, ViewWriter, VIEW_MAX};

entry!(main);

/// Its handles, in the order its image asks for them.
const CARD: Card = Card(Handle(0));
const DAYS: Documents = Documents(Handle(1));

fn today() -> Option<Date> {
    pandagen_app::date().map(|(y, m, d)| Date::new(y, m, d))
}

fn main(_nothing: Console) -> u64 {
    let mut calendar = CalendarView::new(today());
    let _ = DAYS.list();
    let (mut w, mut h) = (0u16, 0u16);
    let mut view_buffer = alloc::vec![0u8; VIEW_MAX];
    let mut message = alloc::vec![0u8; 8192];
    loop {
        // The day may turn over while it is open.
        let event = CARD.event_within(60_000);
        calendar.set_today(today());
        match event {
            Some(Event::Size { w: nw, h: nh }) => (w, h) = (nw, nh),
            Some(Event::Key(byte)) => match calendar.handle_byte(byte) {
                CalendarEffect::None => continue,
                CalendarEffect::Redraw => {}
                CalendarEffect::Close => return 0,
                CalendarEffect::OpenNote { name, .. } => {
                    let _ = DAYS.open(&name);
                    continue;
                }
            },
            Some(Event::Message) => {
                // The day-named documents, one per line.
                while let Ok(n) = pandagen_app::receive(&mut message) {
                    if n == 0 {
                        break;
                    }
                    let text = String::from_utf8_lossy(&message[..n]);
                    let names: Vec<&str> = text.lines().collect();
                    calendar.set_file_names(&names);
                }
            }
            Some(Event::Closed) => return 0,
            Some(_) | None => {}
        }
        if w == 0 || h == 0 {
            continue;
        }
        let footer = calendar.footer();
        let mut view = ViewWriter::new(&mut view_buffer, "Calendar", &footer);
        calendar.draw(w, h, &mut view);
        if let Ok(bytes) = view.finish() {
            let _ = CARD.present(bytes);
        }
    }
}
