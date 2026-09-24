//! Tiles (GFX-078): the sliding-tiles game, 2048, as a card.
//!
//! Arrows slide every tile; equal neighbours merge and score their sum; a
//! new 2 (or, one time in ten, a 4) lands on a free cell. The game is over
//! when no move changes anything. The randomness is a seeded xorshift, so
//! a test can play a whole game and know what it will see, and the desk
//! seeds it from the tick at launch so no two games are alike.
//!
//! Every move is also a `[ button ]` in the text, so it plays with the
//! pointer alone.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::timer::button_at;

pub const SIZE: usize = 4;
/// The content line the first row of cells is on (score, then the top
/// border).
pub const FIRST_CELL_LINE: usize = 2;
/// The controls: `[ Up ]` on one line, the rest on the next.
pub const UP_LINE: usize = 11;
pub const CONTROLS_LINE: usize = 12;

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

    /// A click on a control.
    pub fn click(&mut self, line: usize, column: usize) -> GameEffect {
        let lines = self.lines();
        let Some(text) = lines.get(line) else {
            return GameEffect::None;
        };
        let byte = match (line, button_at(text, column)) {
            (UP_LINE, Some(0)) => b'w',
            (CONTROLS_LINE, Some(0)) => b'a',
            (CONTROLS_LINE, Some(1)) => b's',
            (CONTROLS_LINE, Some(2)) => b'd',
            (CONTROLS_LINE, Some(3)) => b'n',
            _ => return GameEffect::None,
        };
        self.handle_byte(byte)
    }

    pub fn lines(&self) -> Vec<String> {
        let border = "+------".repeat(SIZE) + "+";
        let mut lines = alloc::vec![
            alloc::format!("Score {:<8}   Best {}", self.score, self.best),
            border.clone(),
        ];
        for row in &self.cells {
            let mut text = String::new();
            for value in row {
                if *value == 0 {
                    text.push_str("|      ");
                } else {
                    text.push_str(&alloc::format!("|{:>5} ", value));
                }
            }
            text.push('|');
            lines.push(text);
            lines.push(border.clone());
        }
        lines.push(String::new());
        lines.push("          [ Up ]".to_string());
        lines.push("[ Left ] [ Down ] [ Right ]   [ New ]".to_string());
        lines
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
        // Lines: the board's rows, and the buttons where the clicks go.
        let lines = game.lines();
        assert_eq!(lines[FIRST_CELL_LINE], "|    2 |    4 |    2 |    4 |");
        assert_eq!(lines[UP_LINE], "          [ Up ]");
        assert_eq!(button_at(&lines[CONTROLS_LINE], 31), Some(3));
        assert_eq!(game.click(CONTROLS_LINE, 31), GameEffect::Redraw, "New");
        assert_eq!(game.score(), 0);
        assert!(!game.over());
        // A seeded game is the same game every time.
        let mut a = Game::new(42);
        let b = Game::new(42);
        assert_eq!(a.cells(), b.cells());
        assert_eq!(a.handle_byte(0x1B), GameEffect::Close);
    }
}
