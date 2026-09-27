//! How a program and its card talk (PROC-005).
//!
//! A program with a card does not draw pixels. It describes what its
//! card shows -- a title, a footer, and a short list of widgets: fills,
//! outlines, text and buttons -- and the desk draws that in the desk's
//! own widgets, in the desk's theme. Colours are *roles* (surface, text,
//! accent, ...), not values, so every program follows the theme and none
//! can dress up as the desk's own chrome. A button stands for a key: a
//! click on it arrives as that key, exactly as if it were typed, so
//! everything a program does works with the mouse alone and the keyboard
//! alone.
//!
//! Back come events: a key, the card's size, the card closing.
//!
//! Views are small and bounded ([`VIEW_MAX`] bytes, [`OPS_MAX`] widgets)
//! and encoded without a heap, so a program needs none. The desk decodes
//! and checks every view (feature `decode`): a damaged view is refused,
//! never half drawn.

#![no_std]

#[cfg(feature = "decode")]
extern crate alloc;

/// Most bytes one view takes.
pub const VIEW_MAX: usize = 8192;
/// Most widgets in one view.
pub const OPS_MAX: usize = 256;
/// Most bytes in one text, label, title or footer.
pub const TEXT_MAX: usize = 120;
/// Bytes in one event.
pub const EVENT_BYTES: usize = 8;

const MAGIC: &[u8; 3] = b"PV1";

/// A colour, by what it is for -- or, for what the theme has no role
/// for (a game's tiles), by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The card's background.
    Surface,
    /// A raised area: a key, a field.
    Raised,
    Text,
    /// Secondary text.
    Muted,
    Accent,
    /// Text on an accent fill.
    OnAccent,
    /// Thin rules.
    Hairline,
    /// This colour, whatever the theme (PROC-008). Only inside the card:
    /// it cannot reach the desk's own chrome, so it cannot dress up as it.
    Rgb(u8, u8, u8),
}

/// The byte that says a colour by value follows.
const RGB: u8 = 0xFF;

impl Role {
    /// Its bytes and how many: a role's number, or [`RGB`] and the
    /// three values.
    fn encode(self) -> ([u8; 4], usize) {
        let n = match self {
            Role::Rgb(r, g, b) => return ([RGB, r, g, b], 4),
            Role::Surface => 0,
            Role::Raised => 1,
            Role::Text => 2,
            Role::Muted => 3,
            Role::Accent => 4,
            Role::OnAccent => 5,
            Role::Hairline => 6,
        };
        ([n, 0, 0, 0], 1)
    }

    pub fn from_u8(n: u8) -> Option<Role> {
        Some(match n {
            0 => Role::Surface,
            1 => Role::Raised,
            2 => Role::Text,
            3 => Role::Muted,
            4 => Role::Accent,
            5 => Role::OnAccent,
            6 => Role::Hairline,
            _ => return None,
        })
    }
}

/// How a button reads (the desk's `ButtonKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Plain = 0,
    Accent = 1,
    Quiet = 2,
    Primary = 3,
}

impl Kind {
    pub fn from_u8(n: u8) -> Option<Kind> {
        Some(match n {
            0 => Kind::Plain,
            1 => Kind::Accent,
            2 => Kind::Quiet,
            3 => Kind::Primary,
            _ => return None,
        })
    }
}

/// A rectangle in the card's canvas, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Area {
    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }
}

/// One widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op<'a> {
    Fill {
        area: Area,
        role: Role,
        radius: u8,
    },
    Outline {
        area: Area,
        role: Role,
        radius: u8,
        thickness: u8,
    },
    /// Text with its top-left at `(x, y)`; `scale` 1, 2 or 3.
    Text {
        x: u16,
        y: u16,
        role: Role,
        scale: u8,
        text: &'a str,
    },
    /// Text ending at `right`.
    TextRight {
        right: u16,
        y: u16,
        role: Role,
        scale: u8,
        text: &'a str,
    },
    /// A button that stands for `key`.
    Button {
        area: Area,
        kind: Kind,
        key: u8,
        label: &'a str,
    },
    /// A straight line, `thickness` pixels (1 to 8) wide: a drawn sign.
    Line {
        from: (u16, u16),
        to: (u16, u16),
        role: Role,
        thickness: u8,
    },
    /// Text centred in `area`: a tile's number, a clock's time.
    TextCentered {
        area: Area,
        role: Role,
        scale: u8,
        text: &'a str,
    },
    /// A place that stands for `key` without looking like a button: a
    /// calendar's day. The desk outlines it under the pointer.
    Hit {
        area: Area,
        key: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewError {
    /// The view does not fit in [`VIEW_MAX`] bytes.
    Full,
    TooManyOps,
    TextTooLong,
    NotAView,
    Truncated,
    BadRole(u8),
    BadKind(u8),
    BadScale(u8),
    BadThickness(u8),
    BadOp(u8),
    NotText,
    /// A widget reaching outside the card.
    OutOfCard,
    Trailing,
}

/// A view being written into a fixed buffer: no heap.
pub struct ViewWriter<'b> {
    buf: &'b mut [u8],
    len: usize,
    ops: u16,
    ops_at: usize,
    error: Option<ViewError>,
}

impl<'b> ViewWriter<'b> {
    /// Start a view with its card's `title` and `footer`.
    pub fn new(buf: &'b mut [u8], title: &str, footer: &str) -> Self {
        let mut w = Self {
            buf,
            len: 0,
            ops: 0,
            ops_at: 0,
            error: None,
        };
        w.bytes(MAGIC);
        w.text(title);
        w.text(footer);
        w.ops_at = w.len;
        w.bytes(&[0, 0]);
        w
    }

    fn bytes(&mut self, b: &[u8]) {
        if self.error.is_some() {
            return;
        }
        let end = self.len + b.len();
        if end > self.buf.len() || end > VIEW_MAX {
            self.error = Some(ViewError::Full);
            return;
        }
        self.buf[self.len..end].copy_from_slice(b);
        self.len = end;
    }

    fn text(&mut self, t: &str) {
        if t.len() > TEXT_MAX {
            self.error.get_or_insert(ViewError::TextTooLong);
            return;
        }
        self.bytes(&[t.len() as u8]);
        self.bytes(t.as_bytes());
    }

    fn role(&mut self, role: Role) {
        let (bytes, n) = role.encode();
        self.bytes(&bytes[..n]);
    }

    fn area(&mut self, a: Area) {
        for v in [a.x, a.y, a.w, a.h] {
            self.bytes(&v.to_le_bytes());
        }
    }

    /// Add a widget.
    pub fn op(&mut self, op: Op) -> &mut Self {
        if self.ops as usize >= OPS_MAX {
            self.error.get_or_insert(ViewError::TooManyOps);
            return self;
        }
        match op {
            Op::Fill { area, role, radius } => {
                self.bytes(&[1]);
                self.area(area);
                self.role(role);
                self.bytes(&[radius]);
            }
            Op::Outline {
                area,
                role,
                radius,
                thickness,
            } => {
                self.bytes(&[2]);
                self.area(area);
                self.role(role);
                self.bytes(&[radius, thickness]);
            }
            Op::Text {
                x,
                y,
                role,
                scale,
                text,
            } => {
                self.bytes(&[3]);
                self.bytes(&x.to_le_bytes());
                self.bytes(&y.to_le_bytes());
                self.role(role);
                self.bytes(&[scale]);
                self.text(text);
            }
            Op::TextRight {
                right,
                y,
                role,
                scale,
                text,
            } => {
                self.bytes(&[4]);
                self.bytes(&right.to_le_bytes());
                self.bytes(&y.to_le_bytes());
                self.role(role);
                self.bytes(&[scale]);
                self.text(text);
            }
            Op::Button {
                area,
                kind,
                key,
                label,
            } => {
                self.bytes(&[5]);
                self.area(area);
                self.bytes(&[kind as u8, key]);
                self.text(label);
            }
            Op::Line {
                from,
                to,
                role,
                thickness,
            } => {
                self.bytes(&[6]);
                for v in [from.0, from.1, to.0, to.1] {
                    self.bytes(&v.to_le_bytes());
                }
                self.role(role);
                self.bytes(&[thickness]);
            }
            Op::TextCentered {
                area,
                role,
                scale,
                text,
            } => {
                self.bytes(&[7]);
                self.area(area);
                self.role(role);
                self.bytes(&[scale]);
                self.text(text);
            }
            Op::Hit { area, key } => {
                self.bytes(&[8]);
                self.area(area);
                self.bytes(&[key]);
            }
        }
        self.ops += 1;
        self
    }

    /// The finished view, or what went wrong first.
    pub fn finish(self) -> Result<&'b [u8], ViewError> {
        if let Some(e) = self.error {
            return Err(e);
        }
        let ops = self.ops.to_le_bytes();
        self.buf[self.ops_at..self.ops_at + 2].copy_from_slice(&ops);
        Ok(&self.buf[..self.len])
    }
}

/// Something that happened to a program's card. More kinds may come: a
/// program ignores what it does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A key: typed, or a button that stands for it clicked.
    Key(u8),
    /// The card's canvas is this size (sent first, and on every change).
    Size { w: u16, h: u16 },
    /// The card was closed; the program ends next.
    Closed,
    /// A message is waiting for the program (PROC-009): an answer to
    /// something it asked, taken with `receive`.
    Message,
}

impl Event {
    pub fn encode(self) -> [u8; EVENT_BYTES] {
        let mut b = [0u8; EVENT_BYTES];
        match self {
            Event::Key(k) => {
                b[0] = 1;
                b[1] = k;
            }
            Event::Size { w, h } => {
                b[0] = 2;
                b[2..4].copy_from_slice(&w.to_le_bytes());
                b[4..6].copy_from_slice(&h.to_le_bytes());
            }
            Event::Closed => b[0] = 3,
            Event::Message => b[0] = 4,
        }
        b
    }

    pub fn decode(b: &[u8; EVENT_BYTES]) -> Option<Event> {
        Some(match b[0] {
            1 => Event::Key(b[1]),
            2 => Event::Size {
                w: u16::from_le_bytes([b[2], b[3]]),
                h: u16::from_le_bytes([b[4], b[5]]),
            },
            3 => Event::Closed,
            4 => Event::Message,
            _ => return None,
        })
    }
}

/// A view as the desk keeps it: owned, checked.
#[cfg(feature = "decode")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub title: alloc::string::String,
    pub footer: alloc::string::String,
    pub ops: alloc::vec::Vec<OwnedOp>,
}

#[cfg(feature = "decode")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedOp {
    Fill {
        area: Area,
        role: Role,
        radius: u8,
    },
    Outline {
        area: Area,
        role: Role,
        radius: u8,
        thickness: u8,
    },
    Text {
        x: u16,
        y: u16,
        role: Role,
        scale: u8,
        text: alloc::string::String,
    },
    TextRight {
        right: u16,
        y: u16,
        role: Role,
        scale: u8,
        text: alloc::string::String,
    },
    Button {
        area: Area,
        kind: Kind,
        key: u8,
        label: alloc::string::String,
    },
    Line {
        from: (u16, u16),
        to: (u16, u16),
        role: Role,
        thickness: u8,
    },
    TextCentered {
        area: Area,
        role: Role,
        scale: u8,
        text: alloc::string::String,
    },
    Hit {
        area: Area,
        key: u8,
    },
}

#[cfg(feature = "decode")]
impl View {
    /// Read a view and check it against a canvas of `w` x `h`: every
    /// widget inside it, every text valid and short, nothing left over.
    pub fn decode(bytes: &[u8], w: u16, h: u16) -> Result<View, ViewError> {
        use alloc::string::String;
        use alloc::vec::Vec;
        struct R<'a> {
            b: &'a [u8],
            at: usize,
        }
        impl<'a> R<'a> {
            fn take(&mut self, n: usize) -> Result<&'a [u8], ViewError> {
                let out = self
                    .b
                    .get(self.at..self.at + n)
                    .ok_or(ViewError::Truncated)?;
                self.at += n;
                Ok(out)
            }
            fn u8(&mut self) -> Result<u8, ViewError> {
                Ok(self.take(1)?[0])
            }
            fn u16(&mut self) -> Result<u16, ViewError> {
                let b = self.take(2)?;
                Ok(u16::from_le_bytes([b[0], b[1]]))
            }
            fn text(&mut self) -> Result<String, ViewError> {
                let n = self.u8()? as usize;
                if n > TEXT_MAX {
                    return Err(ViewError::TextTooLong);
                }
                let t = core::str::from_utf8(self.take(n)?).map_err(|_| ViewError::NotText)?;
                if t.chars().any(char::is_control) {
                    return Err(ViewError::NotText);
                }
                Ok(String::from(t))
            }
            fn role(&mut self) -> Result<Role, ViewError> {
                let n = self.u8()?;
                if n == RGB {
                    return Ok(Role::Rgb(self.u8()?, self.u8()?, self.u8()?));
                }
                Role::from_u8(n).ok_or(ViewError::BadRole(n))
            }
            fn scale(&mut self) -> Result<u8, ViewError> {
                match self.u8()? {
                    s @ (1..=3) => Ok(s),
                    s => Err(ViewError::BadScale(s)),
                }
            }
        }
        if bytes.len() > VIEW_MAX {
            return Err(ViewError::Full);
        }
        let mut r = R { b: bytes, at: 0 };
        if r.take(3)? != MAGIC {
            return Err(ViewError::NotAView);
        }
        let title = r.text()?;
        let footer = r.text()?;
        let n = r.u16()? as usize;
        if n > OPS_MAX {
            return Err(ViewError::TooManyOps);
        }
        let inside = |a: Area| {
            (a.x as u32 + a.w as u32) <= w as u32 && (a.y as u32 + a.h as u32) <= h as u32
        };
        let area = |r: &mut R| -> Result<Area, ViewError> {
            let a = Area::new(r.u16()?, r.u16()?, r.u16()?, r.u16()?);
            if inside(a) {
                Ok(a)
            } else {
                Err(ViewError::OutOfCard)
            }
        };
        let point = |x: u16, y: u16| {
            if x <= w && y <= h {
                Ok(())
            } else {
                Err(ViewError::OutOfCard)
            }
        };
        let mut ops = Vec::with_capacity(n);
        for _ in 0..n {
            let op = match r.u8()? {
                1 => OwnedOp::Fill {
                    area: area(&mut r)?,
                    role: r.role()?,
                    radius: r.u8()?,
                },
                2 => OwnedOp::Outline {
                    area: area(&mut r)?,
                    role: r.role()?,
                    radius: r.u8()?,
                    thickness: r.u8()?,
                },
                3 => {
                    let (x, y) = (r.u16()?, r.u16()?);
                    point(x, y)?;
                    OwnedOp::Text {
                        x,
                        y,
                        role: r.role()?,
                        scale: r.scale()?,
                        text: r.text()?,
                    }
                }
                4 => {
                    let (right, y) = (r.u16()?, r.u16()?);
                    point(right, y)?;
                    OwnedOp::TextRight {
                        right,
                        y,
                        role: r.role()?,
                        scale: r.scale()?,
                        text: r.text()?,
                    }
                }
                5 => {
                    let a = area(&mut r)?;
                    let k = r.u8()?;
                    OwnedOp::Button {
                        area: a,
                        kind: Kind::from_u8(k).ok_or(ViewError::BadKind(k))?,
                        key: r.u8()?,
                        label: r.text()?,
                    }
                }
                6 => {
                    let (x0, y0, x1, y1) = (r.u16()?, r.u16()?, r.u16()?, r.u16()?);
                    point(x0, y0)?;
                    point(x1, y1)?;
                    let role = r.role()?;
                    let thickness = r.u8()?;
                    if !(1..=8).contains(&thickness) {
                        return Err(ViewError::BadThickness(thickness));
                    }
                    OwnedOp::Line {
                        from: (x0, y0),
                        to: (x1, y1),
                        role,
                        thickness,
                    }
                }
                7 => OwnedOp::TextCentered {
                    area: area(&mut r)?,
                    role: r.role()?,
                    scale: r.scale()?,
                    text: r.text()?,
                },
                8 => OwnedOp::Hit {
                    area: area(&mut r)?,
                    key: r.u8()?,
                },
                other => return Err(ViewError::BadOp(other)),
            };
            ops.push(op);
        }
        if r.at != bytes.len() {
            return Err(ViewError::Trailing);
        }
        Ok(View { title, footer, ops })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    fn sample(buf: &mut [u8]) -> &[u8] {
        let mut v = ViewWriter::new(buf, "Counter", "Space adds one");
        v.op(Op::Fill {
            area: Area::new(0, 0, 200, 100),
            role: Role::Raised,
            radius: 6,
        })
        .op(Op::Text {
            x: 10,
            y: 10,
            role: Role::Text,
            scale: 2,
            text: "42",
        })
        .op(Op::Button {
            area: Area::new(10, 50, 80, 40),
            kind: Kind::Primary,
            key: b' ',
            label: "Add one",
        });
        v.finish().unwrap()
    }

    #[test]
    fn events_round_trip() {
        for e in [
            Event::Key(b'x'),
            Event::Size { w: 640, h: 480 },
            Event::Closed,
            Event::Message,
        ] {
            assert_eq!(Event::decode(&e.encode()), Some(e));
        }
        assert_eq!(Event::decode(&[9; 8]), None);
    }

    #[test]
    fn a_full_buffer_is_an_error_not_a_cut_view() {
        let mut small = [0u8; 20];
        let mut v = ViewWriter::new(&mut small, "Title", "");
        v.op(Op::Text {
            x: 0,
            y: 0,
            role: Role::Text,
            scale: 1,
            text: "far too much text for twenty bytes",
        });
        assert_eq!(v.finish(), Err(ViewError::Full));
        let mut buf = [0u8; 64];
        let long = "x".repeat(TEXT_MAX + 1);
        let v = ViewWriter::new(&mut buf, &long, "");
        assert_eq!(v.finish(), Err(ViewError::TextTooLong));
    }

    #[test]
    fn too_many_widgets_is_refused() {
        let mut buf = [0u8; VIEW_MAX];
        let mut v = ViewWriter::new(&mut buf, "", "");
        for _ in 0..=OPS_MAX {
            v.op(Op::Fill {
                area: Area::new(0, 0, 1, 1),
                role: Role::Surface,
                radius: 0,
            });
        }
        assert_eq!(v.finish(), Err(ViewError::TooManyOps));
    }

    #[cfg(feature = "decode")]
    #[test]
    fn a_view_survives_the_round_trip() {
        let mut buf = [0u8; 256];
        let bytes = sample(&mut buf);
        let view = View::decode(bytes, 400, 300).unwrap();
        assert_eq!(view.title, "Counter");
        assert_eq!(view.footer, "Space adds one");
        assert_eq!(view.ops.len(), 3);
        assert!(matches!(
            &view.ops[2],
            OwnedOp::Button { key: b' ', label, kind: Kind::Primary, .. } if label == "Add one"
        ));
    }

    #[cfg(feature = "decode")]
    #[test]
    fn lines_are_drawn_signs_inside_the_card() {
        let mut buf = [0u8; 128];
        let mut v = ViewWriter::new(&mut buf, "", "");
        v.op(Op::Line {
            from: (10, 20),
            to: (30, 20),
            role: Role::Accent,
            thickness: 2,
        });
        let bytes = v.finish().unwrap().to_vec();
        let view = View::decode(&bytes, 100, 100).unwrap();
        assert_eq!(
            view.ops[0],
            OwnedOp::Line {
                from: (10, 20),
                to: (30, 20),
                role: Role::Accent,
                thickness: 2
            }
        );
        assert_eq!(View::decode(&bytes, 20, 100), Err(ViewError::OutOfCard));
        let mut thick = bytes.clone();
        let last = thick.len() - 1;
        thick[last] = 40;
        assert_eq!(
            View::decode(&thick, 100, 100),
            Err(ViewError::BadThickness(40))
        );
    }

    #[cfg(feature = "decode")]
    #[test]
    fn a_colour_by_value_survives_the_trip() {
        let mut buf = [0u8; 64];
        let mut v = ViewWriter::new(&mut buf, "", "");
        v.op(Op::Fill {
            area: Area::new(0, 0, 10, 10),
            role: Role::Rgb(237, 194, 46),
            radius: 6,
        })
        .op(Op::Text {
            x: 0,
            y: 0,
            role: Role::Muted,
            scale: 1,
            text: "2048",
        });
        let bytes = v.finish().unwrap().to_vec();
        let view = View::decode(&bytes, 20, 20).unwrap();
        assert!(matches!(
            view.ops[0],
            OwnedOp::Fill {
                role: Role::Rgb(237, 194, 46),
                radius: 6,
                ..
            }
        ));
        assert!(matches!(
            view.ops[1],
            OwnedOp::Text {
                role: Role::Muted,
                ..
            }
        ));
    }

    #[cfg(feature = "decode")]
    #[test]
    fn a_widget_outside_the_card_is_refused() {
        let mut buf = [0u8; 256];
        let bytes = sample(&mut buf);
        assert_eq!(View::decode(bytes, 150, 300), Err(ViewError::OutOfCard));
        assert_eq!(View::decode(bytes, 400, 80), Err(ViewError::OutOfCard));
    }

    #[cfg(feature = "decode")]
    #[test]
    fn damaged_views_are_refused_never_misread() {
        let mut buf = [0u8; 256];
        let bytes = sample(&mut buf).to_vec();
        for cut in 0..bytes.len() {
            assert!(View::decode(&bytes[..cut], 400, 300).is_err(), "cut {cut}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(View::decode(&longer, 400, 300), Err(ViewError::Trailing));
        for i in 0..bytes.len() {
            for flip in [0x01u8, 0x80, 0xFF] {
                let mut b = bytes.clone();
                b[i] ^= flip;
                let _ = View::decode(&b, 400, 300);
            }
        }
        // A control character in text (an escape, say) is not text.
        let mut buf = [0u8; 64];
        let mut v = ViewWriter::new(&mut buf, "a\x1bb", "");
        v.op(Op::Fill {
            area: Area::new(0, 0, 1, 1),
            role: Role::Surface,
            radius: 0,
        });
        let b = v.finish().unwrap();
        assert_eq!(View::decode(b, 10, 10), Err(ViewError::NotText));
    }
}
