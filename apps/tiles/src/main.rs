//! Tiles (2048), as a program (PROC-008).
//!
//! It asks for its card and nothing else; the machine's generator seeds
//! each game (a call anyone may make: it reveals nothing and reaches
//! nothing). Everything it does is `tiles_core`, tested on the host.

#![no_std]
#![no_main]

extern crate alloc;

use pandagen_app::{entry, Card, Console, Event, Handle, ViewWriter, VIEW_MAX};
use tiles_core::{Game, GameEffect};

entry!(main);

/// Its only handle.
const CARD: Card = Card(Handle(0));

fn main(_nothing: Console) -> u64 {
    let mut game = Game::new(pandagen_app::random_u64());
    let (mut w, mut h) = (0u16, 0u16);
    let mut buffer = alloc::vec![0u8; VIEW_MAX];
    loop {
        match CARD.next_event() {
            Event::Size { w: nw, h: nh } => (w, h) = (nw, nh),
            Event::Key(byte) => match game.handle_byte(byte) {
                GameEffect::Close => return 0,
                GameEffect::Redraw => {}
                GameEffect::None => continue,
            },
            Event::Closed => return 0,
            _ => continue,
        }
        if w == 0 || h == 0 {
            continue;
        }
        let footer = game.footer();
        let mut view = ViewWriter::new(&mut buffer, "Tiles", &footer);
        game.draw(w, h, &mut view);
        if let Ok(bytes) = view.finish() {
            let _ = CARD.present(bytes);
        }
    }
}
