//! Calculator (GFX-075, GFX-081): real keys, a display, a tape.
//!
//! The arithmetic is exact decimal, not floating point: values are `i128`
//! scaled by [`SCALE`] (twelve decimal places), so `0.1 + 0.2` is `0.3`
//! and a kernel without a floating-point unit never needs one. Anything
//! that would not fit says "Too big" rather than wrapping.
//!
//! The card is drawn with the widget layer: the expression small and the
//! result twice the font's size, right-aligned; a grid of rounded keys;
//! the tape in the muted tone below. A click lands on a key through the
//! [`Ui`]'s hit rectangles and arrives here as the same character the
//! keyboard would send, so both reach [`Calculator::press`].

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use view_types::PixelRect;

use crate::widgets::{grid, rect, ButtonKind, Palette, Ui, GLYPH_H};

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
pub const DISPLAY_H: u32 = 64;
const GAP: u32 = 6;
const KEY_ROWS_H: u32 = 244;
const TAPE_PITCH: u32 = GLYPH_H + 2;

/// Where things go on a canvas of a given size (GFX-081): the display,
/// the twenty keys, the tape. Pure geometry, so a test and the desk agree
/// on where a key is.
#[derive(Debug, Clone)]
pub struct Layout {
    pub width: u32,
    pub display: PixelRect,
    pub keys: Vec<PixelRect>,
    pub tape_top: u32,
    pub tape_rows: u32,
}

impl Layout {
    pub fn new(width: u32, height: u32) -> Self {
        let keys_top = DISPLAY_H + 8;
        let keys = grid(rect(0, keys_top as i32, width, KEY_ROWS_H), 4, 5, GAP);
        let tape_top = keys_top + KEY_ROWS_H + 12;
        let tape_rows = height.saturating_sub(tape_top) / TAPE_PITCH;
        Self {
            width,
            display: rect(0, 0, width, DISPLAY_H),
            keys,
            tape_top,
            tape_rows,
        }
    }

    /// The rectangle of key `ch`.
    pub fn key_rect(&self, ch: char) -> Option<PixelRect> {
        KEYS.iter()
            .flatten()
            .position(|k| *k == ch)
            .and_then(|i| self.keys.get(i).copied())
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

    /// The card, drawn (GFX-081): the display, the keys, the tape.
    /// `hover` is the pointer in canvas pixels, if it is over the card.
    pub fn ui(&self, width: u32, height: u32, palette: Palette, hover: Option<(i32, i32)>) -> Ui {
        let layout = Layout::new(width, height);
        let mut ui = Ui::new(palette, hover);
        let p = *ui.palette();
        // The display: a raised well, the expression small above the
        // result large, both right-aligned; an error reads in the accent.
        ui.fill(layout.display, p.raised, 8);
        let right = width as i32 - 10;
        let expr_ink = if self.fresh { p.muted } else { p.text };
        ui.text_right(right, 8, &self.expr, expr_ink, 1);
        let (shown, ink) = match &self.result {
            Some(Ok(value)) => (value.clone(), p.text),
            Some(Err(why)) => (why.clone(), p.accent),
            None => (String::new(), p.text),
        };
        let scale = if shown.len() > 16 { 1 } else { 2 };
        ui.text_right(right, 28, &shown, ink, scale);
        // The keys.
        for (cell, key) in layout.keys.iter().zip(KEYS.iter().flatten()) {
            let kind = match key {
                '=' => ButtonKind::Primary,
                '/' | '*' | '-' | '+' => ButtonKind::Accent,
                'C' | '<' | '(' | ')' => ButtonKind::Quiet,
                _ => ButtonKind::Plain,
            };
            // Divide and multiply are drawn as the signs people know
            // (GFX-096); the keys they stand for are the ones typed.
            let drawn = matches!(key, '/' | '*');
            let label = if drawn {
                String::from(" ")
            } else {
                key.to_string()
            };
            ui.button(*cell, &label, *key as u8, kind);
            if drawn {
                let cx = (cell.x + cell.width / 2) as i32;
                let cy = (cell.y + cell.height / 2) as i32;
                if *key == '*' {
                    ui.line(cx - 6, cy - 6, cx + 6, cy + 6, p.accent, 2);
                    ui.line(cx - 6, cy + 6, cx + 6, cy - 6, p.accent, 2);
                } else {
                    ui.line(cx - 8, cy, cx + 8, cy, p.accent, 2);
                    ui.fill(rect(cx - 2, cy - 8, 4, 4), p.accent, 2);
                    ui.fill(rect(cx - 2, cy + 5, 4, 4), p.accent, 2);
                }
            }
        }
        // The tape, newest first, in the muted tone.
        for (i, (expr, value)) in self.tape.iter().take(layout.tape_rows as usize).enumerate() {
            let y = (layout.tape_top + i as u32 * TAPE_PITCH) as i32;
            ui.text_right(right, y, &alloc::format!("{expr} = {value}"), p.muted, 1);
        }
        ui
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
    use super::*;
    use services_gui_host::Theme;
    use view_types::DrawOp;

    fn value(text: &str) -> String {
        format_fixed(evaluate(text).unwrap())
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
        let palette = Palette::from_theme(&Theme::DEFAULT);
        let layout = Layout::new(284, 400);
        assert_eq!(layout.keys.len(), 20);
        assert_eq!(layout.key_rect('7'), Some(rect(0, 122, 66, 44)));
        assert_eq!(layout.key_rect('='), Some(rect(216, 272, 66, 44)));
        assert_eq!(layout.tape_rows, 4);
        let mut calc = Calculator::new();
        // Click 7, +, 8, = through the drawn keys.
        for key in ['7', '+', '8', '='] {
            let cell = layout.key_rect(key).unwrap();
            let hit = calc
                .ui(284, 400, palette, None)
                .hit(cell.x as i32 + 5, cell.y as i32 + 5)
                .expect("a key there");
            assert_eq!(hit as char, key);
            calc.press(hit as char);
        }
        assert_eq!(calc.shown(), "15");
        assert_eq!(calc.tape()[0], ("7+8".to_string(), "15".to_string()));
        // The display shows the result at twice the size; the tape once.
        let ops = calc.ui(284, 400, palette, None).into_ops();
        assert!(ops.iter().any(
            |op| matches!(op, DrawOp::Text { text, style, .. } if text == "15" && style.scale == 2)
        ));
        assert!(ops.iter().any(|op| matches!(
            op,
            DrawOp::Text { text, style, .. } if text == "7+8 = 15" && style.scale == 1
        )));
        // Between the keys there is nothing to hit.
        assert_eq!(calc.ui(284, 400, palette, None).hit(68, 130), None);
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
}
