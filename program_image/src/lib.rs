//! Program images (PROC-004): what a PandaGen program is, on disk and on
//! the way into memory.
//!
//! A program is not an executable file the kernel has to understand in
//! all its generality. It is a small, explicit image: a name, where to
//! start, the pieces of memory to lay out -- each saying whether it may
//! be written or run, never both -- and the capabilities the program
//! asks for. The asking is part of the image so it can be read before
//! anything runs: whoever starts a program sees what it wants, and grants
//! what they choose. A program cannot reach for anything else later; it
//! has no way to name it.
//!
//! The toolchain emits ELF, so `from_elf` turns a statically linked ELF
//! executable into an image at build time; the kernel only ever reads
//! images. Both directions are here, and tested on the host.
//!
//! The layout, little-endian throughout:
//!
//! ```text
//! magic   "PGX1"
//! entry   u64
//! name    u16 length, then UTF-8
//! asks    u16 count, then a u16 per capability asked for
//! pieces  u16 count, then per piece:
//!           at u64, size u64 (bytes in memory), access u8, then
//!           u64 length and that many bytes (the rest of `size` is zero)
//! ```

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

pub const MAGIC: &[u8; 4] = b"PGX1";
/// The lowest address a piece may be at: the first 4 MiB are left
/// unmapped, so a null pointer (and small offsets from one) fault.
pub const LOWEST: u64 = 0x40_0000;
/// Pieces end below here (the program's stack sits above them).
pub const HIGHEST: u64 = 0x7F_0000_0000;
/// The most memory an image may lay out.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_PIECES: usize = 16;
pub const MAX_ASKS: usize = 16;
pub const MAX_NAME: usize = 32;

/// A piece may be written.
pub const WRITE: u8 = 1;
/// A piece may be run.
pub const EXECUTE: u8 = 2;

/// Capabilities a program can ask for, by number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Ask {
    /// Lines to the Terminal that started it.
    Console = 1,
    /// A card on the desk that the program draws (`app_protocol`).
    Card = 2,
}

impl Ask {
    pub fn from_u16(n: u16) -> Option<Ask> {
        match n {
            1 => Some(Ask::Console),
            2 => Some(Ask::Card),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Ask::Console => "console",
            Ask::Card => "a card",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub at: u64,
    pub size: u64,
    pub access: u8,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub name: String,
    pub entry: u64,
    pub asks: Vec<Ask>,
    pub pieces: Vec<Piece>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageError {
    NotAnImage,
    Truncated,
    BadName,
    UnknownAsk(u16),
    TooMany,
    /// A piece outside [`LOWEST`, `HIGHEST`), or wrapping.
    OutOfRange,
    /// Two pieces share a page.
    Overlap,
    /// Writable and runnable at once.
    WriteAndExecute,
    /// More bytes than the piece's size.
    Overfull,
    TooBig,
    /// The entry is not in a runnable piece.
    BadEntry,
    /// Trailing bytes after the last piece.
    Trailing,
    // From ELF:
    NotElf,
    /// Not a static x86-64 executable.
    UnsupportedElf,
}

const PAGE: u64 = 4096;

fn page_down(x: u64) -> u64 {
    x & !(PAGE - 1)
}

fn page_up(x: u64) -> Option<u64> {
    Some(x.checked_add(PAGE - 1)? & !(PAGE - 1))
}

impl Image {
    /// Check everything the kernel relies on, before any of it is mapped.
    pub fn validate(&self) -> Result<(), ImageError> {
        if self.name.is_empty()
            || self.name.len() > MAX_NAME
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(ImageError::BadName);
        }
        if self.pieces.len() > MAX_PIECES || self.asks.len() > MAX_ASKS {
            return Err(ImageError::TooMany);
        }
        let mut total = 0u64;
        let mut spans: Vec<(u64, u64)> = Vec::new();
        for piece in &self.pieces {
            if piece.access & !(WRITE | EXECUTE) != 0 || piece.access == WRITE | EXECUTE {
                return Err(ImageError::WriteAndExecute);
            }
            if piece.bytes.len() as u64 > piece.size {
                return Err(ImageError::Overfull);
            }
            let end = piece
                .at
                .checked_add(piece.size)
                .ok_or(ImageError::OutOfRange)?;
            if piece.at < LOWEST || end > HIGHEST || piece.size == 0 {
                return Err(ImageError::OutOfRange);
            }
            let span = (
                page_down(piece.at),
                page_up(end).ok_or(ImageError::OutOfRange)?,
            );
            if spans.iter().any(|&(a, b)| span.0 < b && a < span.1) {
                return Err(ImageError::Overlap);
            }
            spans.push(span);
            total += span.1 - span.0;
            if total > MAX_BYTES {
                return Err(ImageError::TooBig);
            }
        }
        let runnable = self
            .pieces
            .iter()
            .any(|p| p.access & EXECUTE != 0 && p.at <= self.entry && self.entry < p.at + p.size);
        if !runnable {
            return Err(ImageError::BadEntry);
        }
        Ok(())
    }

    /// The image as bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.entry.to_le_bytes());
        out.extend_from_slice(&(self.name.len() as u16).to_le_bytes());
        out.extend_from_slice(self.name.as_bytes());
        out.extend_from_slice(&(self.asks.len() as u16).to_le_bytes());
        for ask in &self.asks {
            out.extend_from_slice(&(*ask as u16).to_le_bytes());
        }
        out.extend_from_slice(&(self.pieces.len() as u16).to_le_bytes());
        for piece in &self.pieces {
            out.extend_from_slice(&piece.at.to_le_bytes());
            out.extend_from_slice(&piece.size.to_le_bytes());
            out.push(piece.access);
            out.extend_from_slice(&(piece.bytes.len() as u64).to_le_bytes());
            out.extend_from_slice(&piece.bytes);
        }
        out
    }

    /// Read and validate an image.
    pub fn parse(bytes: &[u8]) -> Result<Image, ImageError> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != MAGIC {
            return Err(ImageError::NotAnImage);
        }
        let entry = r.u64()?;
        let name_len = r.u16()? as usize;
        if name_len > MAX_NAME {
            return Err(ImageError::BadName);
        }
        let name = core::str::from_utf8(r.take(name_len)?)
            .map_err(|_| ImageError::BadName)?
            .into();
        let asks_n = r.u16()? as usize;
        if asks_n > MAX_ASKS {
            return Err(ImageError::TooMany);
        }
        let mut asks = Vec::with_capacity(asks_n);
        for _ in 0..asks_n {
            let n = r.u16()?;
            asks.push(Ask::from_u16(n).ok_or(ImageError::UnknownAsk(n))?);
        }
        let pieces_n = r.u16()? as usize;
        if pieces_n > MAX_PIECES {
            return Err(ImageError::TooMany);
        }
        let mut pieces = Vec::with_capacity(pieces_n);
        for _ in 0..pieces_n {
            let at = r.u64()?;
            let size = r.u64()?;
            let access = r.u8()?;
            let len = r.u64()?;
            if len > size || len > MAX_BYTES {
                return Err(ImageError::Overfull);
            }
            let bytes = r.take(len as usize)?.to_vec();
            pieces.push(Piece {
                at,
                size,
                access,
                bytes,
            });
        }
        if r.at != bytes.len() {
            return Err(ImageError::Trailing);
        }
        let image = Image {
            name,
            entry,
            asks,
            pieces,
        };
        image.validate()?;
        Ok(image)
    }

    /// An image from a statically linked x86-64 ELF executable: its
    /// loadable segments become pieces (writable or runnable, as the
    /// segment says; a segment that is both is refused).
    pub fn from_elf(elf: &[u8], name: &str, asks: &[Ask]) -> Result<Image, ImageError> {
        let r = |at: usize, n: usize| elf.get(at..at + n).ok_or(ImageError::Truncated);
        let u16_at = |at| Ok::<_, ImageError>(u16::from_le_bytes(r(at, 2)?.try_into().unwrap()));
        let u32_at = |at| Ok::<_, ImageError>(u32::from_le_bytes(r(at, 4)?.try_into().unwrap()));
        let u64_at = |at| Ok::<_, ImageError>(u64::from_le_bytes(r(at, 8)?.try_into().unwrap()));
        if r(0, 4)? != b"\x7fELF" {
            return Err(ImageError::NotElf);
        }
        // 64-bit, little-endian, an executable (not position-independent),
        // for x86-64.
        let (class, data) = (r(4, 1)?[0], r(5, 1)?[0]);
        if class != 2 || data != 1 || u16_at(16)? != 2 || u16_at(18)? != 62 {
            return Err(ImageError::UnsupportedElf);
        }
        let entry = u64_at(24)?;
        let phoff = u64_at(32)? as usize;
        let phentsize = u16_at(54)? as usize;
        let phnum = u16_at(56)? as usize;
        if phentsize < 56 {
            return Err(ImageError::UnsupportedElf);
        }
        let mut pieces = Vec::new();
        for i in 0..phnum {
            let h = phoff + i * phentsize;
            const PT_LOAD: u32 = 1;
            const PT_INTERP: u32 = 3;
            match u32_at(h)? {
                PT_LOAD => {}
                PT_INTERP => return Err(ImageError::UnsupportedElf),
                _ => continue,
            }
            let flags = u32_at(h + 4)?;
            let offset = u64_at(h + 8)? as usize;
            let vaddr = u64_at(h + 16)?;
            let filesz = u64_at(h + 32)? as usize;
            let memsz = u64_at(h + 40)?;
            if memsz == 0 {
                continue;
            }
            let (x, w) = (flags & 1 != 0, flags & 2 != 0);
            let access = match (w, x) {
                (true, true) => return Err(ImageError::WriteAndExecute),
                (true, false) => WRITE,
                (false, true) => EXECUTE,
                (false, false) => 0,
            };
            // Pages come zeroed, so trailing zeros need not be carried.
            let mut bytes = r(offset, filesz)?.to_vec();
            while bytes.last() == Some(&0) {
                bytes.pop();
            }
            pieces.push(Piece {
                at: vaddr,
                size: memsz,
                access,
                bytes,
            });
        }
        let image = Image {
            name: name.into(),
            entry,
            asks: asks.to_vec(),
            pieces,
        };
        image.validate()?;
        Ok(image)
    }

    /// Bytes of memory the pieces take, whole pages.
    pub fn memory(&self) -> u64 {
        self.pieces
            .iter()
            .map(|p| page_up(p.at + p.size).unwrap_or(0) - page_down(p.at))
            .sum()
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ImageError> {
        let end = self.at.checked_add(n).ok_or(ImageError::Truncated)?;
        let out = self.bytes.get(self.at..end).ok_or(ImageError::Truncated)?;
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, ImageError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, ImageError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, ImageError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use alloc::vec;

    fn hello() -> Image {
        Image {
            name: "hello".into(),
            entry: 0x40_0010,
            asks: vec![Ask::Console],
            pieces: vec![
                Piece {
                    at: 0x40_0000,
                    size: 0x1800,
                    access: EXECUTE,
                    bytes: vec![0x90; 0x1234],
                },
                Piece {
                    at: 0x40_2000,
                    size: 0x3000,
                    access: WRITE,
                    bytes: vec![7; 16],
                },
            ],
        }
    }

    #[test]
    fn an_image_survives_the_round_trip() {
        let image = hello();
        let bytes = image.to_bytes();
        assert_eq!(Image::parse(&bytes), Ok(image.clone()));
        assert_eq!(image.memory(), 0x2000 + 0x3000);
    }

    #[test]
    fn writable_and_runnable_at_once_is_refused() {
        let mut image = hello();
        image.pieces[1].access = WRITE | EXECUTE;
        assert_eq!(image.validate(), Err(ImageError::WriteAndExecute));
        image.pieces[1].access = 0x80;
        assert_eq!(image.validate(), Err(ImageError::WriteAndExecute));
    }

    #[test]
    fn pieces_stay_in_the_programs_range_and_apart() {
        let mut image = hello();
        image.pieces[1].at = 0; // the null page
        assert_eq!(image.validate(), Err(ImageError::OutOfRange));
        image.pieces[1].at = 0xffff_8000_0000_0000; // the kernel's half
        assert_eq!(image.validate(), Err(ImageError::OutOfRange));
        image.pieces[1].at = u64::MAX - 10; // wrapping
        assert_eq!(image.validate(), Err(ImageError::OutOfRange));
        image.pieces[1].at = 0x40_1800; // sharing a page with the code
        assert_eq!(image.validate(), Err(ImageError::Overlap));
        let mut image = hello();
        image.pieces[1].size = MAX_BYTES;
        assert_eq!(image.validate(), Err(ImageError::TooBig));
    }

    #[test]
    fn the_entry_must_be_in_runnable_code() {
        let mut image = hello();
        image.entry = 0x40_2000; // the data
        assert_eq!(image.validate(), Err(ImageError::BadEntry));
    }

    #[test]
    fn damaged_images_are_refused_not_misread() {
        let bytes = hello().to_bytes();
        for cut in 0..bytes.len() {
            assert!(Image::parse(&bytes[..cut]).is_err(), "cut at {cut}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(Image::parse(&longer), Err(ImageError::Trailing));
        let mut other = bytes.clone();
        other[0] = b'X';
        assert_eq!(Image::parse(&other), Err(ImageError::NotAnImage));
        // An ask nobody knows.
        let mut image = hello();
        image.asks.clear();
        let mut b = image.to_bytes();
        let at = 4 + 8 + 2 + 5;
        b[at..at + 2].copy_from_slice(&1u16.to_le_bytes());
        b.splice(at + 2..at + 2, 99u16.to_le_bytes());
        assert_eq!(Image::parse(&b), Err(ImageError::UnknownAsk(99)));
        // Every single-byte change either parses to a valid image or is
        // refused -- never a panic.
        for i in 0..bytes.len().min(64) {
            let mut b = bytes.clone();
            b[i] ^= 0xFF;
            let _ = Image::parse(&b);
        }
    }

    /// A minimal ELF executable: one code segment, one data segment.
    fn elf(code_flags: u32) -> Vec<u8> {
        let mut e = vec![0u8; 64 + 2 * 56];
        e[0..4].copy_from_slice(b"\x7fELF");
        e[4] = 2;
        e[5] = 1;
        e[16..18].copy_from_slice(&2u16.to_le_bytes());
        e[18..20].copy_from_slice(&62u16.to_le_bytes());
        e[24..32].copy_from_slice(&0x40_0000u64.to_le_bytes());
        e[32..40].copy_from_slice(&64u64.to_le_bytes());
        e[54..56].copy_from_slice(&56u16.to_le_bytes());
        e[56..58].copy_from_slice(&2u16.to_le_bytes());
        let data_at = e.len();
        e.extend_from_slice(&[0xC3; 32]); // code
        e.extend_from_slice(&[1, 2, 0, 0]); // data, ending in zeros
        let ph = |e: &mut Vec<u8>,
                  i: usize,
                  flags: u32,
                  off: usize,
                  vaddr: u64,
                  filesz: u64,
                  memsz: u64| {
            let h = 64 + i * 56;
            e[h..h + 4].copy_from_slice(&1u32.to_le_bytes());
            e[h + 4..h + 8].copy_from_slice(&flags.to_le_bytes());
            e[h + 8..h + 16].copy_from_slice(&(off as u64).to_le_bytes());
            e[h + 16..h + 24].copy_from_slice(&vaddr.to_le_bytes());
            e[h + 32..h + 40].copy_from_slice(&filesz.to_le_bytes());
            e[h + 40..h + 48].copy_from_slice(&memsz.to_le_bytes());
        };
        ph(&mut e, 0, code_flags, data_at, 0x40_0000, 32, 32);
        ph(&mut e, 1, 4 | 2, data_at + 32, 0x40_1000, 4, 0x2000);
        e
    }

    #[test]
    fn an_elf_executable_becomes_an_image() {
        let image = Image::from_elf(&elf(4 | 1), "tiny", &[Ask::Console]).unwrap();
        assert_eq!(image.entry, 0x40_0000);
        assert_eq!(image.pieces.len(), 2);
        assert_eq!(image.pieces[0].access, EXECUTE);
        assert_eq!(image.pieces[0].bytes, vec![0xC3; 32]);
        assert_eq!(image.pieces[1].access, WRITE);
        assert_eq!(image.pieces[1].size, 0x2000);
        assert_eq!(
            image.pieces[1].bytes,
            vec![1, 2],
            "trailing zeros are implied"
        );
        assert_eq!(Image::parse(&image.to_bytes()), Ok(image));
    }

    #[test]
    fn elf_that_is_not_ours_to_run_is_refused() {
        assert_eq!(
            Image::from_elf(&elf(4 | 2 | 1), "x", &[]),
            Err(ImageError::WriteAndExecute)
        );
        let mut pie = elf(5);
        pie[16] = 3; // position-independent (ET_DYN)
        assert_eq!(
            Image::from_elf(&pie, "x", &[]),
            Err(ImageError::UnsupportedElf)
        );
        assert_eq!(Image::from_elf(b"MZ..", "x", &[]), Err(ImageError::NotElf));
        let e = elf(5);
        assert_eq!(
            Image::from_elf(&e[..100], "x", &[]),
            Err(ImageError::Truncated)
        );
        assert_eq!(
            Image::from_elf(&e, "bad name!", &[]),
            Err(ImageError::BadName)
        );
    }
}
