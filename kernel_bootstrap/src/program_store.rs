//! Programs on disk (PROC-010): where programs live once they are
//! installed, and how they get there.
//!
//! A program is a document: its image, kept in storage as `<name>.pgx`
//! (of kind "program"), so it has versions like any document, shows in
//! Files, and is the person's -- installed, updated and removed like
//! anything else they keep. `run` loads it from there.
//!
//! Programs arrive two ways. The machine's boot image carries the
//! programs it ships with; at boot each is **installed** if the disk has
//! no program of that name, and **updated** (a new version, the old one
//! kept) if the disk's differs -- so a newer machine brings newer
//! programs, and a person's disk is where programs are run from either
//! way. And `install <url>` fetches an image over the network, checks it
//! as the loader will, and keeps it.
//!
//! This is the policy, and it is plain code so it runs under `cargo
//! test`; the kernel's loop does the reading and writing.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use program_image::Image;

/// The suffix of a program's document.
pub const SUFFIX: &str = ".pgx";
/// The kind (schema) programs are kept as.
pub const KIND: &str = "program";

/// The document a program named `name` is kept in.
pub fn file_name(name: &str) -> String {
    alloc::format!("{name}{SUFFIX}")
}

/// The program a document is, if it is one: `calendar.pgx` -> `calendar`.
pub fn program_name(file: &str) -> Option<&str> {
    let name = file.strip_suffix(SUFFIX)?;
    let fine = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    fine.then_some(name)
}

/// The installed programs among a listing of document names, sorted.
pub fn installed<'a>(files: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut names: Vec<&str> = files.into_iter().filter_map(program_name).collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// What to do at boot with a program the boot image carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seed {
    /// The disk has none: install it.
    Install,
    /// The disk's differs: a new version, the old one kept.
    Update,
    /// The disk's is this one.
    Keep,
}

pub fn seed(on_disk: Option<&[u8]>, shipped: &[u8]) -> Seed {
    match on_disk {
        None => Seed::Install,
        Some(bytes) if bytes == shipped => Seed::Keep,
        Some(_) => Seed::Update,
    }
}

/// Check bytes fetched to be installed, as the loader will check them
/// before running: the image, parsed and validated. The reason, as the
/// Terminal says it, when they are not a program.
pub fn check(bytes: &[u8]) -> Result<Image, String> {
    Image::parse(bytes).map_err(|why| alloc::format!("install: not a program image ({why:?})"))
}

/// A program, in a line: "ticker (16 KiB, asks for console)".
pub fn describe(image: &Image) -> String {
    let asks: Vec<String> = image.asks.iter().map(|a| a.describe()).collect();
    alloc::format!(
        "{} ({} KiB, asks for {})",
        image.name,
        image.memory() / 1024,
        if asks.is_empty() {
            String::from("nothing")
        } else {
            asks.join(", ")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use program_image::{Ask, Piece, EXECUTE};

    fn image(name: &str) -> Image {
        Image {
            name: name.into(),
            entry: 0x40_0000,
            asks: alloc::vec![Ask::Console],
            pieces: alloc::vec![Piece {
                at: 0x40_0000,
                size: 0x4000,
                access: EXECUTE,
                bytes: alloc::vec![0x90; 64],
            }],
        }
    }

    #[test]
    fn a_program_is_a_document_named_for_it() {
        assert_eq!(file_name("calendar"), "calendar.pgx");
        assert_eq!(program_name("calendar.pgx"), Some("calendar"));
        assert_eq!(program_name("calendar"), None);
        assert_eq!(program_name(".pgx"), None);
        assert_eq!(program_name("../x.pgx"), None);
        assert_eq!(
            installed([
                "memo",
                "tiles.pgx",
                "calendar.pgx",
                "2026-09-27",
                "tiles.pgx"
            ]),
            alloc::vec!["calendar", "tiles"]
        );
    }

    #[test]
    fn the_boot_image_installs_or_updates_and_never_rewrites_the_same() {
        assert_eq!(seed(None, b"new"), Seed::Install);
        assert_eq!(seed(Some(b"new"), b"new"), Seed::Keep);
        assert_eq!(seed(Some(b"old"), b"new"), Seed::Update);
    }

    #[test]
    fn only_a_real_image_is_installed() {
        let bytes = image("ticker").to_bytes();
        let checked = check(&bytes).unwrap();
        assert_eq!(describe(&checked), "ticker (16 KiB, asks for console)");
        assert!(check(b"<html>Not found</html>")
            .unwrap_err()
            .contains("NotAnImage"));
        // Cut short, as a fetch past its limit would be.
        assert!(check(&bytes[..bytes.len() - 1]).is_err());
    }
}
