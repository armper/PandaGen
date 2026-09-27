//! The Timer, as a program (PROC-008).
//!
//! It asks for two things: its card (handle 0) and notices (handle 1),
//! to say a countdown is up wherever the person is. Everything it does is
//! `timer_core`, tested on the host; this is the loop that joins it to
//! the card and the clock. While it runs it redraws ten times a second;
//! otherwise it sleeps until something happens.

#![no_std]
#![no_main]

extern crate alloc;

use pandagen_app::{entry, Card, Console, Event, Handle, Notices, ViewWriter, VIEW_MAX};
use timer_core::{TimerEffect, TimerView};

entry!(main);

/// Its handles, in the order its image asks for them.
const CARD: Card = Card(Handle(0));
const NOTICES: Notices = Notices(Handle(1));

/// The machine's time in the Timer's ticks (a hundredth of a second).
fn now() -> u64 {
    pandagen_app::time_ms() / 10
}

fn main(_nothing: Console) -> u64 {
    let mut timer = TimerView::new();
    timer.poll(now());
    let (mut w, mut h) = (0u16, 0u16);
    let mut buffer = alloc::vec![0u8; VIEW_MAX];
    loop {
        // Running, it wakes ten times a second to move the time on.
        let event = if timer.running() {
            CARD.event_within(100)
        } else {
            Some(CARD.next_event())
        };
        if let Some(message) = timer.poll(now()) {
            let _ = NOTICES.say(&message);
        }
        match event {
            Some(Event::Size { w: nw, h: nh }) => (w, h) = (nw, nh),
            Some(Event::Key(byte)) => match timer.handle_byte(byte) {
                TimerEffect::Close => return 0,
                TimerEffect::Redraw => {
                    timer.poll(now());
                }
                TimerEffect::None => continue,
            },
            Some(Event::Closed) => return 0,
            None => {}
        }
        if w == 0 || h == 0 {
            continue;
        }
        let (title, footer) = (timer.title(), timer.footer());
        let mut view = ViewWriter::new(&mut buffer, &title, &footer);
        timer.draw(w, h, &mut view);
        if let Ok(bytes) = view.finish() {
            let _ = CARD.present(bytes);
        }
    }
}
