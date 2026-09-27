//! The Timer (GFX-077; a program since PROC-008): a stopwatch with laps,
//! and a countdown that says so when it is done.
//!
//! Time is ticks, 100 a second, handed in by whoever runs it: the Timer
//! never reads a clock itself, so `cargo test` can run a whole countdown
//! in no time at all. The program (`apps/timer`) hands it the machine's
//! time; when a countdown reaches zero, [`TimerView::poll`] returns what
//! to say, and the program says it through its Notices capability -- so
//! the card need not be on screen, or even on this space, to be heard.
//!
//! Every control is a button that stands for a key, so the pointer and
//! the keyboard reach the same code; the time is drawn three times the
//! font's size, and a countdown shows what is left as a bar.

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use app_protocol::{Area, Kind, Op, Role, ViewWriter};

/// Ticks per second.
pub const HZ: u64 = 100;
/// The countdown presets, in minutes, as the card offers them.
pub const PRESETS: [u64; 4] = [1, 5, 10, 25];
/// Where the card's parts go, top to bottom, in canvas pixels.
pub const TIME_TOP: u16 = 20;
pub const BAR_TOP: u16 = 76;
pub const CONTROLS_TOP: u16 = 96;
pub const PRESETS_TOP: u16 = 172;
pub const LAPS_TOP: u16 = 222;
const LAP_PITCH: u16 = 18;
/// Laps kept.
pub const MAX_LAPS: usize = 6;

/// Keys the Timer answers besides its buttons.
pub const CTRL_W: u8 = 0x17;
pub const ESC: u8 = 0x1B;

/// `area` cut into `cols` cells with `gap` between them.
fn row(area: Area, cols: u16, gap: u16) -> Vec<Area> {
    let w = area.w.saturating_sub(gap * (cols - 1)) / cols;
    (0..cols)
        .map(|c| Area::new(area.x + c * (w + gap), area.y, w, area.h))
        .collect()
}

/// The controls' and presets' rectangles for a canvas `width` wide.
#[derive(Debug, Clone)]
pub struct TimerLayout {
    pub controls: Vec<Area>,
    pub presets: Vec<Area>,
}

impl TimerLayout {
    pub fn new(width: u16) -> Self {
        Self {
            controls: row(Area::new(0, CONTROLS_TOP, width, 44), 3, 6),
            presets: row(Area::new(0, PRESETS_TOP, width, 36), 4, 6),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Stopwatch,
    /// Counting down from this many ticks.
    Countdown(u64),
}

/// What a key or click asks for.
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
    /// The last tick it was told about.
    now: u64,
    laps: Vec<u64>,
    /// The countdown reached zero and it was said.
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

    /// The time, in ticks. Returns what to say, once, when a countdown
    /// ends.
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

    /// The card's title: the time while it runs.
    pub fn title(&self) -> String {
        if self.running() {
            alloc::format!("Timer - {}", format_ticks(self.shown_ticks()))
        } else {
            "Timer".to_string()
        }
    }

    /// The card: the mode, the time large, a bar of what is left while
    /// counting down, the controls, the presets, the laps.
    pub fn draw(&self, width: u16, height: u16, view: &mut ViewWriter) {
        if width < 200 || height < PRESETS_TOP + 36 {
            view.op(Op::Text {
                x: 0,
                y: 0,
                role: Role::Muted,
                scale: 1,
                text: "Make me bigger",
            });
            return;
        }
        let layout = TimerLayout::new(width);
        let heading = match self.mode {
            Mode::Stopwatch => "Stopwatch".to_string(),
            Mode::Countdown(total) => alloc::format!("Countdown  {}", describe(total)),
        };
        view.op(Op::Text {
            x: 0,
            y: 0,
            role: Role::Muted,
            scale: 1,
            text: &heading,
        });
        let time = format_ticks(self.shown_ticks());
        view.op(Op::TextCentered {
            area: Area::new(0, TIME_TOP, width, 48),
            role: if self.fired { Role::Accent } else { Role::Text },
            scale: 3,
            text: &time,
        });
        if let Mode::Countdown(total) = self.mode {
            // What is left, as a bar.
            view.op(Op::Fill {
                area: Area::new(0, BAR_TOP, width, 8),
                role: Role::Raised,
                radius: 4,
            });
            let left = self.shown_ticks().min(total);
            let filled = ((width as u64 * left) / total.max(1)) as u16;
            if filled > 0 {
                view.op(Op::Fill {
                    area: Area::new(0, BAR_TOP, filled, 8),
                    role: Role::Accent,
                    radius: 4,
                });
            }
        }
        let (start, start_kind) = if self.running() {
            ("Pause", Kind::Plain)
        } else {
            ("Start", Kind::Primary)
        };
        let button = |view: &mut ViewWriter, area: Area, label: &str, key: u8, kind: Kind| {
            view.op(Op::Button {
                area,
                kind,
                key,
                label,
            });
        };
        button(view, layout.controls[0], start, b' ', start_kind);
        if self.mode == Mode::Stopwatch {
            button(view, layout.controls[1], "Lap", b'l', Kind::Plain);
        } else {
            button(view, layout.controls[1], "Stopwatch", b's', Kind::Plain);
        }
        button(view, layout.controls[2], "Reset", b'r', Kind::Quiet);
        view.op(Op::Text {
            x: 0,
            y: PRESETS_TOP - 20,
            role: Role::Muted,
            scale: 1,
            text: "Countdown",
        });
        for (i, (cell, minutes)) in layout.presets.iter().zip(PRESETS.iter()).enumerate() {
            let label = alloc::format!("{minutes}m");
            button(view, *cell, &label, b'1' + i as u8, Kind::Accent);
        }
        let mut y = LAPS_TOP;
        if !self.laps.is_empty() {
            view.op(Op::Text {
                x: 0,
                y,
                role: Role::Muted,
                scale: 1,
                text: "Laps",
            });
            y += LAP_PITCH;
            let count = self.laps.len();
            for (i, lap) in self.laps.iter().enumerate() {
                if y + LAP_PITCH > height {
                    break;
                }
                let previous = self.laps.get(i + 1).copied().unwrap_or(0);
                let line = alloc::format!(
                    "{:>2}   {}   +{}",
                    count - i,
                    format_ticks(*lap),
                    format_ticks(lap - previous)
                );
                view.op(Op::Text {
                    x: 8,
                    y,
                    role: Role::Text,
                    scale: 1,
                    text: &line,
                });
                y += LAP_PITCH;
            }
        } else if self.fired && y + LAP_PITCH <= height {
            view.op(Op::Text {
                x: 0,
                y,
                role: Role::Accent,
                scale: 1,
                text: "Done",
            });
        }
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
    extern crate std;
    use super::*;
    use app_protocol::{OwnedOp, View};

    fn view_of(timer: &TimerView) -> View {
        let mut buf = [0u8; app_protocol::VIEW_MAX];
        let mut view = ViewWriter::new(&mut buf, &timer.title(), &timer.footer());
        timer.draw(384, 360, &mut view);
        View::decode(view.finish().unwrap(), 384, 360).expect("a view the desk takes")
    }

    fn texts(timer: &TimerView) -> Vec<String> {
        view_of(timer)
            .ops
            .into_iter()
            .filter_map(|op| match op {
                OwnedOp::Text { text, .. } | OwnedOp::TextCentered { text, .. } => Some(text),
                OwnedOp::Button { label, .. } => Some(label),
                _ => None,
            })
            .collect()
    }

    fn hit(timer: &TimerView, x: u16, y: u16) -> Option<u8> {
        view_of(timer).ops.iter().rev().find_map(|op| match op {
            OwnedOp::Button { area, key, .. }
                if x >= area.x && y >= area.y && x < area.x + area.w && y < area.y + area.h =>
            {
                Some(*key)
            }
            _ => None,
        })
    }

    #[test]
    fn the_stopwatch_runs_on_the_ticks_it_is_given_pauses_and_laps() {
        let mut timer = TimerView::new();
        timer.poll(1_000);
        assert!(texts(&timer).contains(&"00:00.0".to_string()));
        assert_eq!(timer.handle_byte(b' '), TimerEffect::Redraw);
        timer.poll(1_230);
        assert!(texts(&timer).contains(&"00:02.3".to_string()));
        assert_eq!(timer.title(), "Timer - 00:02.3");
        timer.handle_byte(b'l');
        timer.poll(1_500);
        timer.handle_byte(b'l');
        let shown = texts(&timer);
        assert!(shown.contains(&"Laps".to_string()));
        assert!(
            shown.contains(&" 2   00:05.0   +00:02.7".to_string()),
            "{shown:?}"
        );
        assert!(
            shown.contains(&" 1   00:02.3   +00:02.3".to_string()),
            "{shown:?}"
        );
        // Pause holds; resume continues from where it was.
        timer.handle_byte(b' ');
        timer.poll(9_000);
        assert_eq!(timer.elapsed(), 500);
        assert!(texts(&timer).contains(&"Start".to_string()));
        timer.handle_byte(b' ');
        timer.poll(9_100);
        assert_eq!(timer.elapsed(), 600);
        assert!(texts(&timer).contains(&"Pause".to_string()));
        assert_eq!(format_ticks(3_661 * HZ), "1:01:01");
        timer.handle_byte(b'r');
        assert_eq!(timer.elapsed(), 0);
        assert!(!texts(&timer).contains(&"Laps".to_string()));
    }

    #[test]
    fn a_countdown_says_so_once_when_it_is_up_and_buttons_are_hit_by_pixel() {
        let mut timer = TimerView::new();
        timer.poll(0);
        let layout = TimerLayout::new(384);
        assert_eq!(layout.controls.len(), 3);
        assert_eq!(layout.presets.len(), 4);
        // The second preset, clicked: five minutes, running at once.
        let cell = layout.presets[1];
        let key = hit(&timer, cell.x + 3, cell.y + 3).expect("a preset there");
        assert_eq!(key, b'2');
        assert_eq!(hit(&timer, cell.x - 2, cell.y + 3), None, "the gap");
        assert_eq!(timer.handle_byte(key), TimerEffect::Redraw);
        assert_eq!(timer.mode, Mode::Countdown(5 * 60 * HZ));
        assert!(timer.running());
        let shown = texts(&timer);
        assert!(shown.contains(&"Countdown  5 minutes".to_string()));
        assert!(shown.contains(&"05:00.0".to_string()));
        assert_eq!(timer.poll(100), None);
        assert!(texts(&timer).contains(&"04:59.0".to_string()));
        // The bar: a full-width track and an accent fill nearly as wide.
        let bars: Vec<u16> = view_of(&timer)
            .ops
            .iter()
            .filter_map(|op| match op {
                OwnedOp::Fill { area, .. } if area.y == BAR_TOP => Some(area.w),
                _ => None,
            })
            .collect();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0], 384);
        assert!((380..384).contains(&bars[1]), "{bars:?}");
        assert_eq!(
            timer.poll(5 * 60 * HZ + 7),
            Some("Timer: 5 minutes up".to_string())
        );
        assert_eq!(timer.poll(5 * 60 * HZ + 200), None, "said once");
        assert!(!timer.running());
        let shown = texts(&timer);
        assert!(shown.contains(&"00:00.0".to_string()));
        assert!(shown.contains(&"Done".to_string()));
        assert!(timer.footer().starts_with("Done"));
        // Reset, then Start runs it again from the top.
        let reset = layout.controls[2];
        let key = hit(&timer, reset.x + 3, reset.y + 3).unwrap();
        assert_eq!(key, b'r');
        timer.handle_byte(key);
        assert_eq!(timer.elapsed(), 0);
        timer.handle_byte(b' ');
        timer.poll(5 * 60 * HZ + 300);
        assert!(texts(&timer).contains(&"04:59.0".to_string()));
        // The second control is "Stopwatch" while counting down.
        let second = layout.controls[1];
        assert!(texts(&timer).contains(&"Stopwatch".to_string()));
        let key = hit(&timer, second.x + 3, second.y + 3).unwrap();
        assert_eq!(key, b's');
        timer.handle_byte(key);
        assert_eq!(timer.mode, Mode::Stopwatch);
        assert_eq!(timer.handle_byte(ESC), TimerEffect::Close);
        assert_eq!(describe(90 * HZ), "90 seconds");
        assert_eq!(describe(60 * HZ), "1 minute");
    }

    #[test]
    fn six_laps_on_a_short_card_never_draw_outside_it() {
        let mut timer = TimerView::new();
        timer.poll(0);
        timer.handle_byte(b' ');
        for t in 1..=10u64 {
            timer.poll(t * 100);
            timer.handle_byte(b'l');
        }
        for (w, h) in [
            (384u16, 120u16),
            (384, 240),
            (384, 300),
            (384, 360),
            (100, 400),
        ] {
            let mut buf = [0u8; app_protocol::VIEW_MAX];
            let mut view = ViewWriter::new(&mut buf, "", "");
            timer.draw(w, h, &mut view);
            // Laps stop at the card's foot; too small a card says so.
            let decoded = View::decode(view.finish().unwrap(), w, h);
            assert!(decoded.is_ok(), "{w}x{h}: {decoded:?}");
        }
    }
}
