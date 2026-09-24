//! Timer (GFX-077): a stopwatch with laps, and a countdown that says so
//! when it is done.
//!
//! Time is the kernel's tick, 100 Hz, handed in by the desk: the card
//! never reads a clock itself, so `cargo test` can run a whole countdown
//! in no time at all. When a countdown reaches zero the desk raises a
//! notice -- through the notices centre like everything else the desk
//! says -- so the card need not be on screen, or even on this space, to
//! be heard.
//!
//! Every control is a `[ button ]` in the text, hit by line and column,
//! and every button has a key.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Ticks per second.
pub const HZ: u64 = 100;
/// The countdown presets, in minutes, as the card offers them.
pub const PRESETS: [u64; 4] = [1, 5, 10, 25];
/// The content line the controls are on, and the presets.
pub const CONTROLS_LINE: usize = 3;
pub const PRESETS_LINE: usize = 5;
pub const LAPS_LINE: usize = 7;
/// Laps kept.
pub const MAX_LAPS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Stopwatch,
    /// Counting down from this many ticks.
    Countdown(u64),
}

/// What a key or click asks the desk to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerEffect {
    None,
    Redraw,
    Close,
}

/// The timer's whole state.
#[derive(Debug, Clone)]
pub struct TimerView {
    pub mode: Mode,
    /// Ticks accumulated while running before the last pause.
    banked: u64,
    /// The tick the current run started at, while running.
    started: Option<u64>,
    /// The last tick the desk told us about.
    now: u64,
    laps: Vec<u64>,
    /// The countdown reached zero and the desk was told.
    fired: bool,
}

impl Default for TimerView {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerView {
    pub fn new() -> Self {
        Self {
            mode: Mode::Stopwatch,
            banked: 0,
            started: None,
            now: 0,
            laps: Vec::new(),
            fired: false,
        }
    }

    pub fn running(&self) -> bool {
        self.started.is_some()
    }

    /// Ticks run so far.
    pub fn elapsed(&self) -> u64 {
        self.banked
            + self
                .started
                .map(|s| self.now.saturating_sub(s))
                .unwrap_or(0)
    }

    /// What the big line shows: elapsed, or what is left.
    pub fn shown_ticks(&self) -> u64 {
        match self.mode {
            Mode::Stopwatch => self.elapsed(),
            Mode::Countdown(total) => total.saturating_sub(self.elapsed()),
        }
    }

    /// The desk's clock. Returns a message once when a countdown ends.
    pub fn poll(&mut self, now: u64) -> Option<String> {
        self.now = now;
        if let Mode::Countdown(total) = self.mode {
            if self.running() && self.elapsed() >= total && !self.fired {
                self.fired = true;
                self.banked = total;
                self.started = None;
                return Some(alloc::format!("Timer: {} up", describe(total)));
            }
        }
        None
    }

    fn start_or_pause(&mut self) {
        if let Some(started) = self.started.take() {
            self.banked += self.now.saturating_sub(started);
        } else {
            if let Mode::Countdown(total) = self.mode {
                if self.banked >= total {
                    // Done: Start runs it again.
                    self.banked = 0;
                    self.fired = false;
                }
            }
            self.started = Some(self.now);
        }
    }

    fn reset(&mut self) {
        self.banked = 0;
        self.started = None;
        self.laps.clear();
        self.fired = false;
    }

    fn lap(&mut self) {
        if self.mode == Mode::Stopwatch && self.running() {
            self.laps.insert(0, self.elapsed());
            self.laps.truncate(MAX_LAPS);
        }
    }

    fn set_countdown(&mut self, minutes: u64) {
        self.mode = Mode::Countdown(minutes * 60 * HZ);
        self.reset();
        self.started = Some(self.now);
    }

    /// A key: Space starts or pauses, `l` laps, `r` resets, `s` is the
    /// stopwatch, `1`..`4` start a preset countdown.
    pub fn handle_byte(&mut self, byte: u8) -> TimerEffect {
        use crate::notepad::{CTRL_W, ESC};
        match byte {
            b' ' | b'\n' | b'\r' => self.start_or_pause(),
            b'l' | b'L' => self.lap(),
            b'r' | b'R' => self.reset(),
            b's' | b'S' => {
                self.mode = Mode::Stopwatch;
                self.reset();
            }
            b'1'..=b'4' => self.set_countdown(PRESETS[(byte - b'1') as usize]),
            CTRL_W | ESC => return TimerEffect::Close,
            _ => return TimerEffect::None,
        }
        TimerEffect::Redraw
    }

    /// A click on the content: a control or a preset.
    pub fn click(&mut self, line: usize, column: usize) -> TimerEffect {
        let lines = self.lines();
        let Some(text) = lines.get(line) else {
            return TimerEffect::None;
        };
        let Some(index) = button_at(text, column) else {
            return TimerEffect::None;
        };
        match (line, index) {
            (CONTROLS_LINE, 0) => self.start_or_pause(),
            (CONTROLS_LINE, 1) => {
                if self.mode == Mode::Stopwatch {
                    self.lap()
                } else {
                    self.mode = Mode::Stopwatch;
                    self.reset();
                }
            }
            (CONTROLS_LINE, 2) => self.reset(),
            (PRESETS_LINE, i) if i < PRESETS.len() => self.set_countdown(PRESETS[i]),
            _ => return TimerEffect::None,
        }
        TimerEffect::Redraw
    }

    pub fn lines(&self) -> Vec<String> {
        let heading = match self.mode {
            Mode::Stopwatch => "Stopwatch".to_string(),
            Mode::Countdown(total) => alloc::format!("Countdown  {}", describe(total)),
        };
        let start = if self.running() { "Pause" } else { "Start" };
        let second = if self.mode == Mode::Stopwatch {
            "Lap"
        } else {
            "Stopwatch"
        };
        let presets: Vec<String> = PRESETS.iter().map(|m| alloc::format!("[ {m}m ]")).collect();
        let mut lines = alloc::vec![
            heading,
            alloc::format!("        {}", format_ticks(self.shown_ticks())),
            String::new(),
            alloc::format!("[ {start} ] [ {second} ] [ Reset ]"),
            String::new(),
            alloc::format!("Countdown  {}", presets.join(" ")),
            String::new(),
        ];
        if !self.laps.is_empty() {
            lines.push("Laps".to_string());
            let count = self.laps.len();
            for (i, lap) in self.laps.iter().enumerate() {
                let previous = self.laps.get(i + 1).copied().unwrap_or(0);
                lines.push(alloc::format!(
                    "  {:>2}   {}   +{}",
                    count - i,
                    format_ticks(*lap),
                    format_ticks(lap - previous)
                ));
            }
        } else if self.fired {
            lines.push("Done".to_string());
        }
        lines
    }

    pub fn footer(&self) -> String {
        match self.mode {
            Mode::Stopwatch => "Space starts and pauses   L laps   R resets".to_string(),
            Mode::Countdown(_) if self.fired => {
                "Done, said in Notices   Start runs it again".to_string()
            }
            Mode::Countdown(_) => "Space pauses   R resets   S stopwatch".to_string(),
        }
    }
}

/// Which `[ button ]` on `text` covers `column`, counting from 0.
pub fn button_at(text: &str, column: usize) -> Option<usize> {
    let mut index = 0;
    let mut open: Option<usize> = None;
    for (i, ch) in text.chars().enumerate() {
        match ch {
            '[' => open = Some(i),
            ']' => {
                if let Some(start) = open.take() {
                    if (start..=i).contains(&column) {
                        return Some(index);
                    }
                    index += 1;
                }
            }
            _ => {}
        }
    }
    None
}

/// `mm:ss.t`, or `h:mm:ss` past an hour.
pub fn format_ticks(ticks: u64) -> String {
    let tenths = (ticks / (HZ / 10)) % 10;
    let secs = ticks / HZ;
    if secs >= 3600 {
        alloc::format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
    } else {
        alloc::format!("{:02}:{:02}.{}", secs / 60, secs % 60, tenths)
    }
}

/// `5 minutes`, `1 minute`, `90 seconds`.
pub fn describe(ticks: u64) -> String {
    let secs = ticks / HZ;
    match secs {
        60 => "1 minute".to_string(),
        s if s % 60 == 0 => alloc::format!("{} minutes", s / 60),
        s => alloc::format!("{s} seconds"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stopwatch_runs_on_the_desks_ticks_pauses_and_laps() {
        let mut timer = TimerView::new();
        timer.poll(1_000);
        assert_eq!(timer.lines()[1].trim(), "00:00.0");
        assert_eq!(timer.handle_byte(b' '), TimerEffect::Redraw);
        timer.poll(1_230);
        assert_eq!(timer.lines()[1].trim(), "00:02.3");
        timer.handle_byte(b'l');
        timer.poll(1_500);
        timer.handle_byte(b'l');
        assert_eq!(timer.lines()[LAPS_LINE], "Laps");
        assert_eq!(timer.lines()[LAPS_LINE + 1], "   2   00:05.0   +00:02.7");
        assert_eq!(timer.lines()[LAPS_LINE + 2], "   1   00:02.3   +00:02.3");
        // Pause holds; resume continues from where it was.
        timer.handle_byte(b' ');
        timer.poll(9_000);
        assert_eq!(timer.elapsed(), 500);
        assert!(timer.lines()[CONTROLS_LINE].starts_with("[ Start ]"));
        timer.handle_byte(b' ');
        timer.poll(9_100);
        assert_eq!(timer.elapsed(), 600);
        assert!(timer.lines()[CONTROLS_LINE].starts_with("[ Pause ]"));
        assert_eq!(format_ticks(3_661 * HZ), "1:01:01");
        timer.handle_byte(b'r');
        assert_eq!(timer.elapsed(), 0);
        assert_eq!(timer.lines().len(), LAPS_LINE);
    }

    #[test]
    fn a_countdown_says_so_once_when_it_is_up_and_buttons_are_hit_by_column() {
        let mut timer = TimerView::new();
        timer.poll(0);
        // The presets line: "Countdown  [ 1m ] [ 5m ] [ 10m ] [ 25m ]".
        let presets = timer.lines()[PRESETS_LINE].clone();
        assert_eq!(button_at(&presets, 11), Some(0));
        assert_eq!(button_at(&presets, 10), None, "the gap before");
        assert_eq!(button_at(&presets, 19), Some(1));
        assert_eq!(button_at(&presets, 35), Some(3));
        assert_eq!(timer.click(PRESETS_LINE, 19), TimerEffect::Redraw);
        assert_eq!(timer.mode, Mode::Countdown(5 * 60 * HZ));
        assert!(timer.running());
        assert_eq!(timer.lines()[0], "Countdown  5 minutes");
        assert_eq!(timer.lines()[1].trim(), "05:00.0");
        assert_eq!(timer.poll(100), None);
        assert_eq!(timer.lines()[1].trim(), "04:59.0");
        assert_eq!(
            timer.poll(5 * 60 * HZ + 7),
            Some("Timer: 5 minutes up".to_string())
        );
        assert_eq!(timer.poll(5 * 60 * HZ + 200), None, "said once");
        assert!(!timer.running());
        assert_eq!(timer.lines()[1].trim(), "00:00.0");
        assert_eq!(timer.lines()[LAPS_LINE], "Done");
        assert!(timer.footer().starts_with("Done"));
        // Start runs it again from the top.
        timer.click(CONTROLS_LINE, 26);
        assert_eq!(timer.elapsed(), 0);
        timer.handle_byte(b' ');
        timer.poll(5 * 60 * HZ + 300);
        assert_eq!(timer.lines()[1].trim(), "04:59.0");
        // The second control is "Stopwatch" while counting down.
        assert!(timer.lines()[CONTROLS_LINE].contains("[ Stopwatch ]"));
        timer.click(CONTROLS_LINE, 12);
        assert_eq!(timer.mode, Mode::Stopwatch);
        assert_eq!(timer.handle_byte(0x1B), TimerEffect::Close);
        assert_eq!(describe(90 * HZ), "90 seconds");
        assert_eq!(describe(60 * HZ), "1 minute");
    }
}
