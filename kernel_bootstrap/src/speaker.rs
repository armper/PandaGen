//! Sound (GFX-087): the PC speaker, played from the kernel's loop.
//!
//! The machine has no audio device the kernel knows; it has the speaker,
//! which PIT channel 2 drives at a frequency and the keyboard controller's
//! port 0x61 gates on and off. That is enough for what a desk says out
//! loud: a tick under a button, a chime when a timer is up, a blip for a
//! notice. Notes are queued and the loop polls them by the tick, so a
//! chime never blocks the kernel.
//!
//! Host-testable: everything goes through [`PortIo`], and a fake port
//! records what was written.

extern crate alloc;

use alloc::vec::Vec;
use hal_x86_64::PortIo;

/// The PIT's input clock.
pub const PIT_HZ: u32 = 1_193_182;
const PIT_COMMAND: u16 = 0x43;
const PIT_CHANNEL_2: u16 = 0x42;
/// Channel 2, lobyte/hibyte, square wave, binary.
const PIT_SQUARE_WAVE_CH2: u8 = 0b1011_0110;
/// The keyboard controller's port B: bit 0 gates channel 2, bit 1 the
/// speaker.
const SPEAKER_GATE: u16 = 0x61;
const GATE_BITS: u8 = 0b11;

/// A note: a frequency in Hz (0 is a rest) and a length in ticks.
pub type Note = (u32, u64);

/// What the desk says out loud (GFX-087).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// A short tick under a pressed button.
    Click,
    /// Three rising notes: a countdown is up.
    Chime,
    /// A soft blip: a notice arrived.
    Blip,
}

impl Sound {
    pub fn notes(self) -> &'static [Note] {
        match self {
            Sound::Click => &[(1_800, 2)],
            Sound::Chime => &[(880, 12), (0, 3), (1_108, 12), (0, 3), (1_318, 24)],
            Sound::Blip => &[(660, 6)],
        }
    }
}

/// The speaker: what is queued, what is sounding, when it ends.
pub struct Speaker<P: PortIo> {
    port: P,
    queue: Vec<Note>,
    /// The tick the current note ends at, while one sounds or rests.
    until: Option<u64>,
    gated: bool,
}

impl<P: PortIo> Speaker<P> {
    pub fn new(port: P) -> Self {
        Self {
            port,
            queue: Vec::new(),
            until: None,
            gated: false,
        }
    }

    /// The PIT divisor for `hz`, clamped to what sixteen bits can say.
    pub fn divisor(hz: u32) -> u16 {
        (PIT_HZ / hz.max(19)).clamp(2, u16::MAX as u32) as u16
    }

    /// Queue `notes` after whatever is playing. A click while a chime
    /// sounds waits its turn; nothing is cut short.
    pub fn play(&mut self, notes: &[Note]) {
        // Bounded: a runaway caller cannot pile up minutes of tone.
        if self.queue.len() + notes.len() <= 64 {
            self.queue.extend_from_slice(notes);
        }
    }

    pub fn is_playing(&self) -> bool {
        self.until.is_some() || !self.queue.is_empty()
    }

    /// Called every loop with the tick: end a note that is over, start
    /// the next one. Cheap when idle: one compare.
    pub fn poll(&mut self, now: u64) {
        if let Some(until) = self.until {
            if now < until {
                return;
            }
            self.until = None;
        }
        match self.queue.first().copied() {
            Some((hz, ticks)) => {
                self.queue.remove(0);
                if hz == 0 {
                    self.gate(false);
                } else {
                    let divisor = Self::divisor(hz);
                    self.port.outb(PIT_COMMAND, PIT_SQUARE_WAVE_CH2);
                    self.port.outb(PIT_CHANNEL_2, (divisor & 0xFF) as u8);
                    self.port.outb(PIT_CHANNEL_2, (divisor >> 8) as u8);
                    self.gate(true);
                }
                self.until = Some(now + ticks.max(1));
            }
            None => self.gate(false),
        }
    }

    fn gate(&mut self, on: bool) {
        if self.gated == on {
            return;
        }
        let current = self.port.inb(SPEAKER_GATE);
        let next = if on {
            current | GATE_BITS
        } else {
            current & !GATE_BITS
        };
        self.port.outb(SPEAKER_GATE, next);
        self.gated = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A port that remembers its writes and answers a quiet port B.
    #[derive(Default)]
    struct Recorder {
        writes: Vec<(u16, u8)>,
    }

    impl PortIo for Recorder {
        fn inb(&mut self, _port: u16) -> u8 {
            0b0000_1100
        }
        fn outb(&mut self, port: u16, value: u8) {
            self.writes.push((port, value));
        }
    }

    #[test]
    fn a_chime_is_played_note_by_note_on_the_tick_and_the_gate_closes_after() {
        let mut speaker = Speaker::new(Recorder::default());
        assert_eq!(Speaker::<Recorder>::divisor(1_000), 1_193);
        assert_eq!(
            Speaker::<Recorder>::divisor(1),
            (PIT_HZ / 19) as u16,
            "floored"
        );
        speaker.play(Sound::Chime.notes());
        assert!(speaker.is_playing());
        speaker.poll(100);
        // The first note: mode, divisor low, divisor high, then the gate.
        let d = Speaker::<Recorder>::divisor(880);
        assert_eq!(
            speaker.port.writes,
            alloc::vec![
                (0x43, 0b1011_0110),
                (0x42, (d & 0xFF) as u8),
                (0x42, (d >> 8) as u8),
                (0x61, 0b0000_1111),
            ]
        );
        // Not over yet: nothing more is written.
        speaker.poll(105);
        assert_eq!(speaker.port.writes.len(), 4);
        // Polled every tick, the chime runs its course: three notes of
        // four writes, two rests and the final silence of one each.
        for tick in 106..=160 {
            speaker.poll(tick);
        }
        assert_eq!(speaker.port.writes.len(), 4 + 1 + 4 + 1 + 4 + 1);
        assert_eq!(
            speaker.port.writes[4],
            (0x61, 0b0000_1100),
            "the first rest"
        );
        assert_eq!(speaker.port.writes.last(), Some(&(0x61, 0b0000_1100)));
        assert!(!speaker.is_playing());
        // Idle polls write nothing.
        speaker.poll(500);
        assert_eq!(speaker.port.writes.len(), 15);
        // A click queued behind a blip waits its turn; the queue is bounded.
        speaker.play(Sound::Blip.notes());
        speaker.play(Sound::Click.notes());
        speaker.poll(600);
        speaker.poll(603);
        assert!(speaker.is_playing(), "the blip still sounds");
        speaker.poll(606);
        let d = Speaker::<Recorder>::divisor(1_800);
        assert!(speaker.port.writes.contains(&(0x42, (d & 0xFF) as u8)));
        for _ in 0..100 {
            speaker.play(Sound::Click.notes());
        }
        assert!(speaker.queue.len() <= 64);
    }
}
