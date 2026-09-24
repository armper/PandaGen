//! Calculator (GFX-075): a card with keys you can click and a tape.
//!
//! The arithmetic is exact decimal, not floating point: values are `i128`
//! scaled by [`SCALE`] (twelve decimal places), so `0.1 + 0.2` is `0.3`
//! and a kernel without a floating-point unit never needs one. Anything
//! that would not fit says "Too big" rather than wrapping.
//!
//! The card is text, like every card: the expression and its result on
//! the first two lines, then the key grid, then the tape of what was
//! worked out before. A click on a key is mapped back from the grid's
//! line and column, so the pointer and the keyboard reach the same
//! [`Calculator::press`].

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Fixed-point scale: twelve decimal places.
pub const SCALE: i128 = 1_000_000_000_000;
/// How many results the tape keeps.
pub const TAPE: usize = 6;
/// The display's width in characters; expression and result are
/// right-aligned to it, and the key grid sits under it.
pub const WIDTH: usize = 23;

/// The key grid, as drawn: five rows of four. `C` clears, `<` deletes
/// the last character, `=` works the expression out.
pub const KEYS: [[char; 4]; 5] = [
    ['C', '(', ')', '/'],
    ['7', '8', '9', '*'],
    ['4', '5', '6', '-'],
    ['1', '2', '3', '+'],
    ['0', '.', '<', '='],
];
/// The content line the first key row is on: expression, result, a gap.
pub const FIRST_KEY_LINE: usize = 3;
/// Each key is `[ x ]` and a space: six columns.
const KEY_WIDTH: usize = 6;

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

    /// The key under content `line`, `column`, if that is a key.
    pub fn key_at(line: usize, column: usize) -> Option<char> {
        let row = line.checked_sub(FIRST_KEY_LINE)?;
        if column % KEY_WIDTH >= KEY_WIDTH - 1 {
            return None; // the space between keys
        }
        KEYS.get(row)?.get(column / KEY_WIDTH).copied()
    }

    /// A click on the card's content.
    pub fn click(&mut self, line: usize, column: usize) -> bool {
        match Self::key_at(line, column) {
            Some(key) => self.press(key),
            None => false,
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

    /// The card's lines: the display, the keys, the tape.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = alloc::vec![
            alloc::format!("{:>WIDTH$}", self.expr),
            alloc::format!("{:>WIDTH$}", self.shown()),
            String::new(),
        ];
        for row in KEYS.iter() {
            let cells: Vec<String> = row.iter().map(|k| alloc::format!("[ {k} ]")).collect();
            lines.push(cells.join(" "));
        }
        if !self.tape.is_empty() {
            lines.push(String::new());
            for (expr, value) in &self.tape {
                let entry = alloc::format!("{expr} = {value}");
                lines.push(alloc::format!("{entry:>WIDTH$}"));
            }
        }
        lines
    }

    pub fn footer(&self) -> String {
        if self.tape.is_empty() {
            "Type or click the keys   Enter works it out   Esc clears".to_string()
        } else {
            alloc::format!(
                "{} on the tape   Enter works it out   Esc clears",
                self.tape.len()
            )
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
    fn keys_clicked_and_typed_reach_the_same_expression_and_the_tape() {
        let mut calc = Calculator::new();
        // Click 7, +, 8, = on the grid.
        assert_eq!(Calculator::key_at(FIRST_KEY_LINE + 1, 0), Some('7'));
        assert_eq!(Calculator::key_at(FIRST_KEY_LINE + 1, 4), Some('7'));
        assert_eq!(Calculator::key_at(FIRST_KEY_LINE + 1, 5), None, "the gap");
        assert_eq!(Calculator::key_at(FIRST_KEY_LINE + 3, 18), Some('+'));
        assert_eq!(Calculator::key_at(0, 0), None);
        assert!(calc.click(FIRST_KEY_LINE + 1, 2));
        assert!(calc.click(FIRST_KEY_LINE + 3, 20));
        assert!(calc.click(FIRST_KEY_LINE + 1, 8));
        assert!(calc.click(FIRST_KEY_LINE + 4, 20));
        assert_eq!(calc.shown(), "15");
        let lines = calc.lines();
        assert!(lines[0].ends_with("15"));
        assert_eq!(lines[FIRST_KEY_LINE], "[ C ] [ ( ] [ ) ] [ / ]");
        assert!(lines.last().unwrap().ends_with("7+8 = 15"));
        // An operator carries the result on; Enter is =; x is *.
        calc.handle_byte(b'x');
        calc.handle_byte(b'2');
        calc.handle_byte(b'\n');
        assert_eq!(calc.shown(), "30");
        assert_eq!(
            calc.footer(),
            "2 on the tape   Enter works it out   Esc clears"
        );
        // A digit after a result starts over; Backspace edits; Esc clears.
        calc.handle_byte(b'4');
        calc.handle_byte(b'2');
        calc.handle_byte(0x08);
        assert!(calc.lines()[0].ends_with(" 4"));
        calc.handle_byte(b'/');
        calc.handle_byte(b'0');
        calc.handle_byte(b'\n');
        assert_eq!(calc.shown(), "Divide by zero");
        assert!(calc.handle_byte(0x1B));
        assert_eq!(calc.lines()[0].trim(), "");
        assert!(!calc.handle_byte(b'\n'), "nothing to work out");
    }
}
