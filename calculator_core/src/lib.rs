//! The Calculator (GFX-075, GFX-081; a program since PROC-006): exact
//! decimal arithmetic, a display, twenty keys and a tape.
//!
//! The arithmetic is exact decimal, not floating point: values are `i128`
//! scaled by [`SCALE`] (twelve decimal places), so `0.1 + 0.2` is `0.3`
//! and a machine without a floating-point unit never needs one. Anything
//! that would not fit says "Too big" rather than wrapping.
//!
//! This used to be a card compiled into the kernel. It is a program now
//! (`apps/calculator`), running in ring 3 with a card of its own; this
//! crate is everything it does that can be tested on the host -- the
//! arithmetic, the keys, and the card, described as an `app_protocol`
//! view: the expression small and the result large, right-aligned; a
//! grid of rounded keys, the operators drawn as the signs people know;
//! the tape in the muted tone below. A click on a key arrives as the
//! same character the keyboard would send.

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use app_protocol::{Area, Kind, Op, Role, ViewWriter};

/// Fixed-point scale: twelve decimal places.
pub const SCALE: i128 = 1_000_000_000_000;
/// How many results the tape keeps.
pub const TAPE: usize = 6;
/// Longest expression.
pub const WIDTH: usize = 24;

/// The key grid: five rows of four. `C` clears, `<` deletes the last
/// character, `=` works the expression out.
pub const KEYS: [[char; 4]; 5] = [
    ['C', '(', ')', '/'],
    ['7', '8', '9', '*'],
    ['4', '5', '6', '-'],
    ['1', '2', '3', '+'],
    ['0', '.', '<', '='],
];
/// The display's height in canvas pixels.
pub const DISPLAY_H: u16 = 80;
const GAP: u16 = 6;
const KEY_ROWS_H: u16 = 244;
/// The font's cell height; a tape line is two pixels more.
const GLYPH_H: u16 = 16;
const TAPE_PITCH: u16 = GLYPH_H + 2;

/// Where things go on a canvas of a given size: the display, the twenty
/// keys, the tape. Pure geometry, so a test and the card agree on where a
/// key is.
#[derive(Debug, Clone)]
pub struct Layout {
    pub width: u16,
    pub display: Area,
    pub keys: Vec<Area>,
    pub tape_top: u16,
    pub tape_rows: u16,
}

impl Layout {
    pub fn new(width: u16, height: u16) -> Self {
        let keys_top = DISPLAY_H + 8;
        let (cols, rows) = (4u16, 5u16);
        let cell_w = width.saturating_sub(GAP * (cols - 1)) / cols;
        let cell_h = KEY_ROWS_H.saturating_sub(GAP * (rows - 1)) / rows;
        let mut keys = Vec::with_capacity(20);
        for r in 0..rows {
            for c in 0..cols {
                keys.push(Area::new(
                    c * (cell_w + GAP),
                    keys_top + r * (cell_h + GAP),
                    cell_w,
                    cell_h,
                ));
            }
        }
        let tape_top = keys_top + KEY_ROWS_H + 12;
        let tape_rows = height.saturating_sub(tape_top) / TAPE_PITCH;
        Self {
            width,
            display: Area::new(0, 0, width, DISPLAY_H),
            keys,
            tape_top,
            tape_rows,
        }
    }

    /// The rectangle of key `ch`.
    pub fn key_area(&self, ch: char) -> Option<Area> {
        KEYS.iter()
            .flatten()
            .position(|k| *k == ch)
            .and_then(|i| self.keys.get(i).copied())
    }

    /// Whether a canvas this size has room for the card at all.
    pub fn fits(width: u16, height: u16) -> bool {
        width >= 4 * 24 + 3 * GAP && height >= DISPLAY_H + 8 + KEY_ROWS_H
    }
}

/// The calculator's whole state.
#[derive(Debug, Clone, Default)]
pub struct Calculator {
    expr: String,
    /// The last `=`: what it showed, or why it could not.
    result: Option<Result<String, String>>,
    /// Older results, newest first: `(expression, result)`.
    tape: Vec<(String, String)>,
    /// The expression is a result: a digit starts over, an operator
    /// continues from it.
    fresh: bool,
}

impl Calculator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tape(&self) -> &[(String, String)] {
        &self.tape
    }

    /// One key: a digit, an operator, `(`, `)`, `.`, `C`, `<` or `=`.
    /// Returns whether anything changed.
    pub fn press(&mut self, key: char) -> bool {
        match key {
            'C' | 'c' => {
                let changed = !self.expr.is_empty() || self.result.is_some();
                self.expr.clear();
                self.result = None;
                self.fresh = false;
                changed
            }
            '<' => {
                if self.fresh {
                    self.expr.clear();
                    self.fresh = false;
                } else {
                    self.expr.pop();
                }
                self.result = None;
                true
            }
            '=' => {
                if self.expr.is_empty() {
                    return false;
                }
                match evaluate(&self.expr) {
                    Ok(value) => {
                        let shown = format_fixed(value);
                        self.tape.insert(0, (self.expr.clone(), shown.clone()));
                        self.tape.truncate(TAPE);
                        self.expr = shown.clone();
                        self.result = Some(Ok(shown));
                        self.fresh = true;
                    }
                    Err(why) => {
                        self.result = Some(Err(why.to_string()));
                        self.fresh = false;
                    }
                }
                true
            }
            '0'..='9' | '.' | '(' | ')' | '+' | '-' | '*' | '/' => {
                if self.fresh {
                    // A digit after a result starts again; an operator
                    // carries the result on.
                    if matches!(key, '0'..='9' | '.' | '(') {
                        self.expr.clear();
                    }
                    self.fresh = false;
                }
                if self.expr.len() >= WIDTH {
                    return false;
                }
                self.expr.push(key);
                self.result = None;
                true
            }
            _ => false,
        }
    }

    /// A key from the keyboard: Enter is `=`, Backspace `<`, Esc `C`, and
    /// `x` is `*` because that is how people type it.
    pub fn handle_byte(&mut self, byte: u8) -> bool {
        match byte {
            b'\n' | b'\r' => self.press('='),
            0x08 => self.press('<'),
            0x1B => self.press('C'),
            b'x' | b'X' => self.press('*'),
            0x20..=0x7E => self.press(byte as char),
            _ => false,
        }
    }

    /// What the result line says: the value, the error, or nothing.
    pub fn shown(&self) -> String {
        match &self.result {
            Some(Ok(value)) => value.clone(),
            Some(Err(why)) => why.clone(),
            None => String::new(),
        }
    }

    /// The card, as a view for a `width` x `height` canvas.
    pub fn draw(&self, width: u16, height: u16, view: &mut ViewWriter) {
        if !Layout::fits(width, height) {
            view.op(Op::Text {
                x: 8,
                y: 8,
                role: Role::Muted,
                scale: 1,
                text: "Make me bigger",
            });
            return;
        }
        let layout = Layout::new(width, height);
        // The display: a raised well, the expression small above the
        // result large, both right-aligned; an error reads in the accent.
        view.op(Op::Fill {
            area: layout.display,
            role: Role::Raised,
            radius: 8,
        });
        let right = width - 10;
        let expr_role = if self.fresh { Role::Muted } else { Role::Text };
        if !self.expr.is_empty() {
            view.op(Op::TextRight {
                right,
                y: 6,
                role: expr_role,
                scale: 1,
                text: &self.expr,
            });
        }
        let (shown, role) = match &self.result {
            Some(Ok(value)) => (value.as_str(), Role::Text),
            Some(Err(why)) => (why.as_str(), Role::Accent),
            None => ("", Role::Text),
        };
        // The result as large as it fits: three times the font for a
        // number, smaller for a long one or a sentence.
        let scale: u8 = match shown.len() {
            0..=10 => 3,
            11..=16 => 2,
            _ => 1,
        };
        if !shown.is_empty() {
            view.op(Op::TextRight {
                right,
                y: DISPLAY_H - 8 - 16 * scale as u16,
                role,
                scale,
                text: shown,
            });
        }
        // The keys.
        let mut label = [0u8; 4];
        for (cell, key) in layout.keys.iter().zip(KEYS.iter().flatten()) {
            let kind = match key {
                '=' => Kind::Primary,
                '/' | '*' | '-' | '+' => Kind::Accent,
                'C' | '<' | '(' | ')' => Kind::Quiet,
                _ => Kind::Plain,
            };
            // The operators are drawn as the signs people know, all four
            // in one weight; the keys they stand for are the ones typed.
            let drawn = matches!(key, '/' | '*' | '+' | '-');
            let text = if drawn {
                " "
            } else {
                key.encode_utf8(&mut label)
            };
            view.op(Op::Button {
                area: *cell,
                kind,
                key: *key as u8,
                label: text,
            });
            if drawn {
                let (cx, cy) = (cell.x + cell.w / 2, cell.y + cell.h / 2);
                let line = |view: &mut ViewWriter, from: (u16, u16), to: (u16, u16)| {
                    view.op(Op::Line {
                        from,
                        to,
                        role: Role::Accent,
                        thickness: 2,
                    });
                };
                match key {
                    '*' => {
                        line(view, (cx - 6, cy - 6), (cx + 6, cy + 6));
                        line(view, (cx - 6, cy + 6), (cx + 6, cy - 6));
                    }
                    '+' => {
                        line(view, (cx - 8, cy), (cx + 8, cy));
                        line(view, (cx, cy - 8), (cx, cy + 8));
                    }
                    '-' => line(view, (cx - 8, cy), (cx + 8, cy)),
                    _ => {
                        line(view, (cx - 8, cy), (cx + 8, cy));
                        view.op(Op::Fill {
                            area: Area::new(cx - 2, cy - 8, 4, 4),
                            role: Role::Accent,
                            radius: 2,
                        });
                        view.op(Op::Fill {
                            area: Area::new(cx - 2, cy + 5, 4, 4),
                            role: Role::Accent,
                            radius: 2,
                        });
                    }
                }
            }
        }
        // The tape, newest first, in the muted tone.
        for (i, (expr, value)) in self.tape.iter().take(layout.tape_rows as usize).enumerate() {
            let line = alloc::format!("{expr} = {value}");
            view.op(Op::TextRight {
                right,
                y: layout.tape_top + i as u16 * TAPE_PITCH,
                role: Role::Muted,
                scale: 1,
                text: &line,
            });
        }
    }

    pub fn footer(&self) -> String {
        if self.tape.is_empty() {
            "Enter works it out   Esc clears".to_string()
        } else {
            alloc::format!("{} on the tape   Esc clears", self.tape.len())
        }
    }
}

/// Work `expr` out: `+ - * /`, parentheses, unary minus, decimals.
pub fn evaluate(expr: &str) -> Result<i128, &'static str> {
    let chars: Vec<char> = expr.chars().filter(|c| *c != ' ').collect();
    let mut parser = Parser { chars, at: 0 };
    let value = parser.sum()?;
    if parser.at < parser.chars.len() {
        return Err(if parser.chars[parser.at] == ')' {
            "Unbalanced ( )"
        } else {
            "Unexpected character"
        });
    }
    Ok(value)
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn sum(&mut self) -> Result<i128, &'static str> {
        let mut value = self.product()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.at += 1;
            let right = self.product()?;
            value = if op == '+' {
                value.checked_add(right)
            } else {
                value.checked_sub(right)
            }
            .ok_or("Too big")?;
        }
        Ok(value)
    }

    fn product(&mut self) -> Result<i128, &'static str> {
        let mut value = self.factor()?;
        while let Some(op @ ('*' | '/')) = self.peek() {
            self.at += 1;
            let right = self.factor()?;
            value = if op == '*' {
                value
                    .checked_mul(right)
                    .map(|v| v / SCALE)
                    .ok_or("Too big")?
            } else {
                if right == 0 {
                    return Err("Divide by zero");
                }
                value
                    .checked_mul(SCALE)
                    .map(|v| v / right)
                    .ok_or("Too big")?
            };
        }
        Ok(value)
    }

    fn factor(&mut self) -> Result<i128, &'static str> {
        match self.peek() {
            Some('-') => {
                self.at += 1;
                self.factor()?.checked_neg().ok_or("Too big")
            }
            Some('(') => {
                self.at += 1;
                let value = self.sum()?;
                if self.peek() != Some(')') {
                    return Err("Unbalanced ( )");
                }
                self.at += 1;
                Ok(value)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            Some(_) => Err("Unexpected character"),
            None => Err("Incomplete"),
        }
    }

    fn number(&mut self) -> Result<i128, &'static str> {
        let mut whole: i128 = 0;
        let mut fraction: i128 = 0;
        let mut place = SCALE;
        let mut seen_dot = false;
        let mut digits = 0;
        while let Some(c) = self.peek() {
            if c == '.' {
                if seen_dot {
                    return Err("Two points in one number");
                }
                seen_dot = true;
            } else if let Some(d) = c.to_digit(10) {
                digits += 1;
                if seen_dot {
                    place /= 10;
                    fraction += d as i128 * place;
                } else {
                    whole = whole
                        .checked_mul(10)
                        .and_then(|w| w.checked_add(d as i128))
                        .ok_or("Too big")?;
                }
            } else {
                break;
            }
            self.at += 1;
        }
        if digits == 0 {
            return Err("Incomplete");
        }
        whole
            .checked_mul(SCALE)
            .and_then(|w| w.checked_add(fraction))
            .ok_or("Too big")
    }
}

/// A scaled value as text: no trailing zeros, no point when whole.
pub fn format_fixed(value: i128) -> String {
    let negative = value < 0;
    let magnitude = value.unsigned_abs();
    let whole = magnitude / SCALE as u128;
    let fraction = magnitude % SCALE as u128;
    let mut text = String::new();
    if negative {
        text.push('-');
    }
    text.push_str(&whole.to_string());
    if fraction != 0 {
        let digits = alloc::format!("{fraction:012}");
        text.push('.');
        text.push_str(digits.trim_end_matches('0'));
    }
    text
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use app_protocol::{OwnedOp, View};

    fn value(text: &str) -> String {
        format_fixed(evaluate(text).unwrap())
    }

    fn view_of(calc: &Calculator, w: u16, h: u16) -> View {
        let mut buf = [0u8; app_protocol::VIEW_MAX];
        let mut view = ViewWriter::new(&mut buf, "Calculator", &calc.footer());
        calc.draw(w, h, &mut view);
        View::decode(view.finish().unwrap(), w, h).expect("a view the desk takes")
    }

    /// The key a click at `(x, y)` would be, as the desk's hit test finds
    /// it: the last button drawn there.
    fn hit(view: &View, x: u16, y: u16) -> Option<u8> {
        view.ops.iter().rev().find_map(|op| match op {
            OwnedOp::Button { area, key, .. }
                if x >= area.x && y >= area.y && x < area.x + area.w && y < area.y + area.h =>
            {
                Some(*key)
            }
            _ => None,
        })
    }

    #[test]
    fn arithmetic_is_exact_decimal_with_precedence_and_parentheses() {
        assert_eq!(value("1+2*3"), "7");
        assert_eq!(value("(1+2)*3"), "9");
        assert_eq!(value("0.1+0.2"), "0.3");
        assert_eq!(value("10/4"), "2.5");
        assert_eq!(value("1/3"), "0.333333333333");
        assert_eq!(value("-2*-3"), "6");
        assert_eq!(value("2-5"), "-3");
        assert_eq!(value(".5*4"), "2");
        assert_eq!(value("1000000*1000000"), "1000000000000");
        assert_eq!(evaluate("1/0"), Err("Divide by zero"));
        assert_eq!(evaluate("(1+2"), Err("Unbalanced ( )"));
        assert_eq!(evaluate("1+2)"), Err("Unbalanced ( )"));
        assert_eq!(evaluate("1+"), Err("Incomplete"));
        assert_eq!(evaluate("1.2.3"), Err("Two points in one number"));
        assert_eq!(
            evaluate("99999999999999999999*99999999999999999999"),
            Err("Too big")
        );
    }

    #[test]
    fn keys_are_real_buttons_hit_by_pixel_and_typed_keys_reach_the_same_code() {
        let layout = Layout::new(284, 400);
        assert_eq!(layout.keys.len(), 20);
        assert_eq!(layout.key_area('7'), Some(Area::new(0, 138, 66, 44)));
        assert_eq!(layout.key_area('='), Some(Area::new(216, 288, 66, 44)));
        assert_eq!(layout.tape_rows, 3);
        let mut calc = Calculator::new();
        // Click 7, +, 8, = through the drawn keys.
        for key in ['7', '+', '8', '='] {
            let cell = layout.key_area(key).unwrap();
            let got = hit(&view_of(&calc, 284, 400), cell.x + 5, cell.y + 5).expect("a key");
            assert_eq!(got as char, key);
            calc.press(got as char);
        }
        assert_eq!(calc.shown(), "15");
        assert_eq!(calc.tape()[0], ("7+8".to_string(), "15".to_string()));
        // The display shows the result at three times the size; the tape once.
        let view = view_of(&calc, 284, 400);
        assert!(view
            .ops
            .iter()
            .any(|op| matches!(op, OwnedOp::TextRight { text, scale: 3, .. } if text == "15")));
        assert!(view.ops.iter().any(
            |op| matches!(op, OwnedOp::TextRight { text, scale: 1, .. } if text == "7+8 = 15")
        ));
        // The operators are drawn signs.
        assert!(view.ops.iter().any(|op| matches!(op, OwnedOp::Line { .. })));
        // Between the keys there is nothing to hit.
        assert_eq!(hit(&view, 68, 130), None);
        // An operator carries the result on; Enter is =; x is *.
        calc.handle_byte(b'x');
        calc.handle_byte(b'2');
        calc.handle_byte(b'\n');
        assert_eq!(calc.shown(), "30");
        assert_eq!(calc.footer(), "2 on the tape   Esc clears");
        // A digit after a result starts over; Backspace edits; Esc clears.
        calc.handle_byte(b'4');
        calc.handle_byte(b'2');
        calc.handle_byte(0x08);
        calc.handle_byte(b'/');
        calc.handle_byte(b'0');
        calc.handle_byte(b'\n');
        assert_eq!(calc.shown(), "Divide by zero");
        assert!(calc.handle_byte(0x1B));
        assert_eq!(calc.shown(), "");
        assert!(!calc.handle_byte(b'\n'), "nothing to work out");
    }

    #[test]
    fn a_full_tape_and_a_long_result_still_make_a_view_the_desk_takes() {
        let mut calc = Calculator::new();
        for _ in 0..10 {
            for b in b"123456789/7\n" {
                calc.handle_byte(*b);
            }
        }
        for (w, h) in [(284, 400), (480, 600), (1200, 800)] {
            let view = view_of(&calc, w, h);
            assert!(view.ops.len() > 20);
        }
        // Too small: it says so, and never draws outside the card.
        let view = view_of(&calc, 60, 60);
        assert_eq!(view.ops.len(), 1);
    }
}
