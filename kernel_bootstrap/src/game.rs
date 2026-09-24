//! Tiles (GFX-078): the sliding-tiles game, 2048, as a card.
//!
//! Arrows slide every tile; equal neighbours merge and score their sum; a
//! new 2 (or, one time in ten, a 4) lands on a free cell. The game is over
//! when no move changes anything. The randomness is a seeded xorshift, so
//! a test can play a whole game and know what it will see, and the desk
//! seeds it from the tick at launch so no two games are alike.
//!
//! The board is drawn (GFX-082): coloured tiles on a grid, the moves as
//! real buttons under it, so it plays with the pointer alone.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use view_types::{Color, PixelRect};

use crate::widgets::{grid, rect, ButtonKind, Palette, Ui};

pub const SIZE: usize = 4;
/// The board's top, under the score line, and the buttons' height.
pub const BOARD_TOP: i32 = 24;
pub const BUTTONS_H: u32 = 44;

/// The board's cells and the five buttons for a canvas of a size
/// (GFX-082): what the drawing and the desk's hit test agree on.
#[derive(Debug, Clone)]
pub struct GameLayout {
    pub board: PixelRect,
    pub cells: Vec<PixelRect>,
    /// Left, Up, Down, Right, New.
    pub buttons: Vec<PixelRect>,
}

impl GameLayout {
    pub fn new(width: u32, height: u32) -> Self {
        let side = width.min(height.saturating_sub(BOARD_TOP as u32 + BUTTONS_H + 12));
        let board = rect(((width - side) / 2) as i32, BOARD_TOP, side, side);
        Self {
            board,
            cells: grid(board, SIZE as u32, SIZE as u32, 6),
            buttons: grid(
                rect(0, height as i32 - BUTTONS_H as i32, width, BUTTONS_H),
                5,
                1,
                6,
            ),
        }
    }
}

/// A tile's fill and ink by its value: warm for the low ones, gold for
/// the high ones, the classic way.
pub fn tile_colors(value: u32) -> (Color, Color) {
    let dark = Color::rgb(119, 110, 101);
    let light = Color::rgb(249, 246, 242);
    match value {
        2 => (Color::rgb(238, 228, 218), dark),
        4 => (Color::rgb(237, 224, 200), dark),
        8 => (Color::rgb(242, 177, 121), light),
        16 => (Color::rgb(245, 149, 99), light),
        32 => (Color::rgb(246, 124, 95), light),
        64 => (Color::rgb(246, 94, 59), light),
        128 => (Color::rgb(237, 207, 114), light),
        256 => (Color::rgb(237, 204, 97), light),
        512 => (Color::rgb(237, 200, 80), light),
        1024 => (Color::rgb(237, 197, 63), light),
        2048 => (Color::rgb(237, 194, 46), light),
        _ => (Color::rgb(60, 58, 50), light),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEffect {
    None,
    Redraw,
    Close,
}

#[derive(Debug, Clone)]
pub struct Game {
    cells: [[u32; SIZE]; SIZE],
    score: u32,
    best: u32,
    rng: u64,
    moves: u32,
}

impl Game {
    /// A new board with two tiles, from `seed`.
    pub fn new(seed: u64) -> Self {
        let mut game = Self {
            cells: [[0; SIZE]; SIZE],
            score: 0,
            best: 0,
            rng: seed | 1,
            moves: 0,
        };
        game.spawn();
        game.spawn();
        game
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    pub fn cells(&self) -> &[[u32; SIZE]; SIZE] {
        &self.cells
    }

    fn next_random(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn spawn(&mut self) {
        let free: Vec<(usize, usize)> = (0..SIZE)
            .flat_map(|r| (0..SIZE).map(move |c| (r, c)))
            .filter(|(r, c)| self.cells[*r][*c] == 0)
            .collect();
        if free.is_empty() {
            return;
        }
        let pick = (self.next_random() % free.len() as u64) as usize;
        let value = if self.next_random() % 10 == 0 { 4 } else { 2 };
        let (r, c) = free[pick];
        self.cells[r][c] = value;
    }

    fn start_over(&mut self) {
        self.cells = [[0; SIZE]; SIZE];
        self.score = 0;
        self.moves = 0;
        self.spawn();
        self.spawn();
    }

    /// Slide `dir`; returns whether anything moved.
    pub fn slide(&mut self, dir: Dir) -> bool {
        let before = self.cells;
        let mut gained = 0;
        for i in 0..SIZE {
            let line: Vec<u32> = (0..SIZE).map(|j| self.get(dir, i, j)).collect();
            let (merged, score) = slide_line(&line);
            gained += score;
            for (j, value) in merged.iter().enumerate() {
                self.set(dir, i, j, *value);
            }
        }
        if self.cells == before {
            return false;
        }
        self.score += gained;
        self.best = self.best.max(self.score);
        self.moves += 1;
        self.spawn();
        true
    }

    /// Cell `j` along line `i` reading in the direction tiles move.
    fn coords(dir: Dir, i: usize, j: usize) -> (usize, usize) {
        match dir {
            Dir::Left => (i, j),
            Dir::Right => (i, SIZE - 1 - j),
            Dir::Up => (j, i),
            Dir::Down => (SIZE - 1 - j, i),
        }
    }

    fn get(&self, dir: Dir, i: usize, j: usize) -> u32 {
        let (r, c) = Self::coords(dir, i, j);
        self.cells[r][c]
    }

    fn set(&mut self, dir: Dir, i: usize, j: usize, value: u32) {
        let (r, c) = Self::coords(dir, i, j);
        self.cells[r][c] = value;
    }

    /// No move would change the board.
    pub fn over(&self) -> bool {
        [Dir::Up, Dir::Down, Dir::Left, Dir::Right]
            .iter()
            .all(|dir| {
                (0..SIZE).all(|i| {
                    let line: Vec<u32> = (0..SIZE).map(|j| self.get(*dir, i, j)).collect();
                    slide_line(&line).0 == line
                })
            })
    }

    pub fn won(&self) -> bool {
        self.cells.iter().flatten().any(|v| *v >= 2048)
    }

    /// Arrows slide, `n` starts over.
    pub fn handle_byte(&mut self, byte: u8) -> GameEffect {
        use crate::notepad::{CTRL_W, ESC, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_UP};
        let dir = match byte {
            KEY_UP | b'w' | b'k' => Dir::Up,
            KEY_DOWN | b's' | b'j' => Dir::Down,
            KEY_LEFT | b'a' | b'h' => Dir::Left,
            KEY_RIGHT | b'd' | b'l' => Dir::Right,
            b'n' | b'N' => {
                self.start_over();
                return GameEffect::Redraw;
            }
            CTRL_W | ESC => return GameEffect::Close,
            _ => return GameEffect::None,
        };
        if self.slide(dir) {
            GameEffect::Redraw
        } else {
            GameEffect::None
        }
    }

    /// The card, drawn (GFX-082): the score, the board as coloured
    /// tiles, the moves as buttons. `hover` is the pointer in canvas
    /// pixels, if over the card.
    pub fn ui(&self, width: u32, height: u32, palette: Palette, hover: Option<(i32, i32)>) -> Ui {
        let layout = GameLayout::new(width, height);
        let mut ui = Ui::new(palette, hover);
        let p = *ui.palette();
        ui.text(0, 2, &alloc::format!("Score {}", self.score), p.text, 1);
        ui.text_right(
            width as i32,
            2,
            &alloc::format!("Best {}", self.best),
            p.muted,
            1,
        );
        for (cell, value) in layout.cells.iter().zip(self.cells.iter().flatten()) {
            if *value == 0 {
                ui.fill(*cell, p.raised, 6);
                continue;
            }
            let (fill, ink) = tile_colors(*value);
            ui.fill(*cell, fill, 6);
            let label = value.to_string();
            let scale = if label.len() <= 3 { 2 } else { 1 };
            ui.text_centered(cell, &label, ink, scale);
        }
        let buttons: [(&str, u8, ButtonKind); 5] = [
            ("Left", b'a', ButtonKind::Accent),
            ("Up", b'w', ButtonKind::Accent),
            ("Down", b's', ButtonKind::Accent),
            ("Right", b'd', ButtonKind::Accent),
            ("New", b'n', ButtonKind::Quiet),
        ];
        for (cell, (label, key, kind)) in layout.buttons.iter().zip(buttons.iter()) {
            ui.button(*cell, label, *key, *kind);
        }
        ui
    }

    pub fn footer(&self) -> String {
        if self.over() {
            alloc::format!("No moves left after {}   N restarts", self.moves)
        } else if self.won() {
            alloc::format!("2048 in {} moves   keep going, or N", self.moves)
        } else {
            alloc::format!("{} moves   arrows slide   N restarts", self.moves)
        }
    }
}

/// One line, sliding towards index 0: tiles close up, equal neighbours
/// merge once, and the score is the sum of what merged.
pub fn slide_line(line: &[u32]) -> (Vec<u32>, u32) {
    let mut out: Vec<u32> = Vec::new();
    let mut score = 0;
    let mut pending: Option<u32> = None;
    for value in line.iter().copied().filter(|v| *v != 0) {
        match pending {
            Some(p) if p == value => {
                out.push(p * 2);
                score += p * 2;
                pending = None;
            }
            Some(p) => {
                out.push(p);
                pending = Some(value);
            }
            None => pending = Some(value),
        }
    }
    if let Some(p) = pending {
        out.push(p);
    }
    out.resize(line.len(), 0);
    (out, score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use services_gui_host::Theme;
    use view_types::DrawOp;

    #[test]
    fn a_line_slides_and_merges_once_per_pair() {
        assert_eq!(slide_line(&[2, 2, 0, 0]), (alloc::vec![4, 0, 0, 0], 4));
        assert_eq!(slide_line(&[2, 2, 2, 2]), (alloc::vec![4, 4, 0, 0], 8));
        assert_eq!(slide_line(&[0, 2, 0, 2]), (alloc::vec![4, 0, 0, 0], 4));
        assert_eq!(
            slide_line(&[4, 4, 8, 0]),
            (alloc::vec![8, 8, 0, 0], 8),
            "no cascade"
        );
        assert_eq!(slide_line(&[2, 4, 2, 4]), (alloc::vec![2, 4, 2, 4], 0));
    }

    #[test]
    fn the_board_slides_in_four_directions_scores_and_knows_the_end() {
        let palette = Palette::from_theme(&Theme::DEFAULT);
        let mut game = Game::new(7);
        let tiles = game.cells().iter().flatten().filter(|v| **v != 0).count();
        assert_eq!(tiles, 2, "two tiles to start");
        // A board set by hand, then one move each way.
        game.cells = [[2, 0, 0, 2], [0, 0, 0, 0], [4, 0, 0, 0], [4, 0, 0, 0]];
        assert!(game.slide(Dir::Left));
        assert_eq!(game.cells[0][0], 4);
        assert_eq!(game.score(), 4);
        game.cells = [[2, 0, 0, 0], [2, 0, 0, 0], [4, 0, 0, 0], [4, 0, 0, 0]];
        assert!(game.slide(Dir::Down));
        assert_eq!((game.cells[3][0], game.cells[2][0]), (8, 4));
        game.cells = [[0, 0, 2, 2], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        assert!(game.slide(Dir::Right));
        assert_eq!(game.cells[0][3], 4);
        game.cells = [[0; 4], [0; 4], [0; 4], [2, 2, 2, 2]];
        assert!(game.slide(Dir::Up));
        assert_eq!(game.cells[0], [2, 2, 2, 2]);
        // Nothing moves: no spawn, no score.
        game.cells = [[2, 4, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 2]];
        let score = game.score();
        assert!(!game.slide(Dir::Left));
        assert_eq!(game.score(), score);
        assert!(game.over());
        assert!(game.footer().starts_with("No moves left"));
        // Drawn: sixteen tiles with their values at twice the size, and
        // five buttons; the New button, hit by pixel, starts over.
        let layout = GameLayout::new(324, 340);
        assert_eq!(layout.cells.len(), 16);
        assert_eq!(layout.board, rect(32, BOARD_TOP, 260, 260));
        assert_eq!(layout.buttons.len(), 5);
        let ops = game.ui(324, 340, palette, None).into_ops();
        let big: Vec<&String> = ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Text { text, style, .. }
                    if style.scale == 2 && text.parse::<u32>().is_ok() =>
                {
                    Some(text)
                }
                _ => None,
            })
            .collect();
        assert_eq!(big.len(), 16);
        let new = layout.buttons[4];
        let key = game
            .ui(324, 340, palette, None)
            .hit(new.x as i32 + 4, new.y as i32 + 4)
            .expect("the New button");
        assert_eq!(key, b'n');
        assert_eq!(game.handle_byte(key), GameEffect::Redraw);
        assert_eq!(game.score(), 0);
        assert!(!game.over());
        assert_eq!(
            game.ui(324, 340, palette, None).hit(0, BOARD_TOP + 10),
            None,
            "the board is not a button"
        );
        // A seeded game is the same game every time.
        let mut a = Game::new(42);
        let b = Game::new(42);
        assert_eq!(a.cells(), b.cells());
        assert_eq!(a.handle_byte(0x1B), GameEffect::Close);
        assert_eq!(tile_colors(2048).0, Color::rgb(237, 194, 46));
    }
}
