//! The desk (GFX-050/052): a window manager for cards, a dock and a top bar.
//!
//! Apps own windows; the shell stays out of the way. This module holds the
//! pure part -- where the windows are, which one has focus, what is being
//! dragged or resized -- and turns pointer deliveries and key bytes into
//! changes to that state. It builds the `DesktopWindow` list the compositor
//! paints; the kernel loop does the painting, the presenting, the file I/O
//! and the console it asks for.
//!
//! Everything here is host-testable: nothing touches hardware.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use graphics_rasterizer::RasterRect;
use input_types::{PointerButton, PointerEventKind};
use services_gui_host::{
    Delivery, DesktopTab, DesktopWindow, DesktopWindowLayer, DesktopWindowRole, HitRegion,
    NoticeLevel, ShellNotice, SurfaceRect, Theme, WindowStyle,
};
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

use crate::notepad::{Notepad, NotepadEffect};

pub const TOP_BAR_HEIGHT: usize = 28;
pub const DOCK_HEIGHT: usize = 56;
pub const DOCK_MARGIN: usize = 12;
pub const NOTEPAD_SIZE: (usize, usize) = (720, 480);
pub const TERMINAL_SIZE: (usize, usize) = (800, 520);
/// Successive windows open offset by this much.
pub const CASCADE_STEP: usize = 32;
/// The smallest a card can be resized to.
pub const MIN_CARD_SIZE: (usize, usize) = (240, 140);
/// How close to an edge a drag must end to snap there.
pub const SNAP_MARGIN: usize = 6;
/// Ctrl+Tab, as the parser delivers it.
pub const KEY_CTRL_TAB: u8 = 0x85;
/// Ctrl+Space, as the parser delivers it: the palette.
pub const KEY_CTRL_SPACE: u8 = 0x86;
/// The palette card (GFX-053).
pub const PALETTE_WIDTH: usize = 560;
pub const PALETTE_MAX_ROWS: usize = 8;
/// Notice cards (GFX-053).
pub const NOTICE_WIDTH: usize = 340;
/// The one font's advance, for fitting text to a card's width.
pub const GLYPH_WIDTH: usize = 8;
pub const NOTICE_MARGIN: usize = 12;
/// How long a desk-raised notice stays, in ticks (100 Hz).
pub const NOTICE_TTL_TICKS: u64 = 400;
pub const FILES_SIZE: (usize, usize) = (680, 440);

/// The apps the dock offers, in dock order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeskApp {
    Notepad,
    /// The real filesystem, as a list: Enter or a click opens a file in a
    /// Notepad.
    Files,
    /// The machine's console -- the `WS >` prompt and everything it can do --
    /// as a card, so the desk never has to be left to reach it.
    Terminal,
    /// Themes and accents, previewed live and kept on disk (GFX-059).
    Look,
}

impl DeskApp {
    pub const ALL: [DeskApp; 4] = [
        DeskApp::Notepad,
        DeskApp::Files,
        DeskApp::Terminal,
        DeskApp::Look,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            DeskApp::Notepad => "Notepad",
            DeskApp::Files => "Files",
            DeskApp::Terminal => "Terminal",
            DeskApp::Look => "Look",
        }
    }

    /// Two letters for the dock tile; there are no icons in this tree.
    pub const fn monogram(self) -> &'static str {
        match self {
            DeskApp::Notepad => "Np",
            DeskApp::Files => "Fi",
            DeskApp::Terminal => "Tm",
            DeskApp::Look => "Lk",
        }
    }

    const fn size(self) -> (usize, usize) {
        match self {
            DeskApp::Notepad => NOTEPAD_SIZE,
            DeskApp::Files => FILES_SIZE,
            DeskApp::Terminal => TERMINAL_SIZE,
            DeskApp::Look => LOOK_SIZE,
        }
    }
}

/// The Look card (GFX-059).
pub const LOOK_SIZE: (usize, usize) = (420, 360);

/// What the desk looks like: a preset and an accent, by name (GFX-059).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookChoice {
    pub theme: String,
    pub accent: String,
}

impl Default for LookChoice {
    fn default() -> Self {
        Self {
            theme: "Dusk".to_string(),
            accent: "Mint".to_string(),
        }
    }
}

impl LookChoice {
    pub fn theme(&self) -> Theme {
        let base = Theme::named(&self.theme).unwrap_or(Theme::DESK);
        match Theme::accent_named(&self.accent) {
            Some(accent) => base.with_accent(accent),
            None => base,
        }
    }

    /// The on-disk form: two lines, `theme=` and `accent=`.
    pub fn to_text(&self) -> String {
        alloc::format!("theme={}\naccent={}\n", self.theme, self.accent)
    }

    /// Parse the on-disk form; unknown names fall back to the defaults, so
    /// a hand-edited or damaged file cannot leave the desk unreadable.
    pub fn from_text(text: &str) -> Self {
        let mut choice = Self::default();
        for line in text.lines() {
            if let Some(name) = line.strip_prefix("theme=") {
                if let Some((canonical, _)) = Theme::PRESETS
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
                {
                    choice.theme = canonical.to_string();
                }
            } else if let Some(name) = line.strip_prefix("accent=") {
                if let Some((canonical, _)) = Theme::ACCENTS
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
                {
                    choice.accent = canonical.to_string();
                }
            }
        }
        choice
    }
}

/// The Look app's state: which row is under the highlight. Rows are the
/// presets, then the accents; moving previews at once, Enter keeps.
#[derive(Debug, Clone, Default)]
pub struct LookView {
    pub row: usize,
}

impl LookView {
    const THEME_ROWS: usize = Theme::PRESETS.len();
    const ROWS: usize = Theme::PRESETS.len() + Theme::ACCENTS.len();

    /// The content lines: a heading, the presets, a gap, a heading, the
    /// accents. `line_of_row` maps a row to its line for the highlight.
    fn lines(kept: &LookChoice, preview: &LookChoice) -> Vec<String> {
        let mark = |name: &str, current: &str| {
            if name.eq_ignore_ascii_case(current) {
                alloc::format!("  * {name}")
            } else {
                alloc::format!("    {name}")
            }
        };
        let mut lines = alloc::vec!["Theme".to_string()];
        for (name, _) in Theme::PRESETS.iter() {
            let mut line = mark(name, &preview.theme);
            if name.eq_ignore_ascii_case(&kept.theme) && !name.eq_ignore_ascii_case(&preview.theme)
            {
                line.push_str("   (kept)");
            }
            lines.push(line);
        }
        lines.push(String::new());
        lines.push("Accent".to_string());
        for (name, _) in Theme::ACCENTS.iter() {
            let mut line = mark(name, &preview.accent);
            if name.eq_ignore_ascii_case(&kept.accent)
                && !name.eq_ignore_ascii_case(&preview.accent)
            {
                line.push_str("   (kept)");
            }
            lines.push(line);
        }
        lines
    }

    fn line_of_row(row: usize) -> usize {
        if row < Self::THEME_ROWS {
            1 + row
        } else {
            3 + row
        }
    }

    /// The inverse of `line_of_row`: headings and the gap are no row.
    fn row_of_line(line: usize) -> Option<usize> {
        (0..Self::ROWS).find(|row| Self::line_of_row(*row) == line)
    }

    /// The choice the highlighted row stands for, given the current one.
    fn choice_at(&self, current: &LookChoice) -> LookChoice {
        let mut choice = current.clone();
        if self.row < Self::THEME_ROWS {
            choice.theme = Theme::PRESETS[self.row].0.to_string();
        } else {
            choice.accent = Theme::ACCENTS[self.row - Self::THEME_ROWS].0.to_string();
        }
        choice
    }

    /// The row that stands for `choice`'s theme (so the card opens on it).
    fn row_of(choice: &LookChoice) -> usize {
        Theme::PRESETS
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(&choice.theme))
            .unwrap_or(0)
    }
}

impl DeskWindow {
    /// The header chips (GFX-056): each is a key the app already answers,
    /// so the pointer and the keyboard reach the same code. Files' depend
    /// on its state (GFX-057).
    pub fn actions(&self) -> Vec<(String, u8)> {
        match &self.state {
            AppState::Notepad(notepad) if notepad.browsing_history() => alloc::vec![
                ("Older".to_string(), crate::notepad::KEY_LEFT),
                ("Newer".to_string(), crate::notepad::KEY_RIGHT),
                ("Restore".to_string(), b'\n'),
                ("Back".to_string(), crate::notepad::ESC),
            ],
            AppState::Notepad(notepad) if notepad.prompt_open() == Some("find") => alloc::vec![
                ("Next".to_string(), b'\n'),
                ("Close".to_string(), crate::notepad::ESC),
            ],
            AppState::Notepad(notepad) if notepad.prompt_open().is_some() => {
                alloc::vec![("Cancel".to_string(), crate::notepad::ESC)]
            }
            AppState::Notepad(_) => alloc::vec![
                ("Save".to_string(), crate::notepad::CTRL_S),
                ("Save as".to_string(), crate::notepad::CTRL_SHIFT_S),
                ("Open".to_string(), crate::notepad::CTRL_O),
                ("Find".to_string(), crate::notepad::CTRL_F),
                ("History".to_string(), crate::notepad::CTRL_Y),
            ],
            AppState::Files(files) => files.actions(),
            AppState::Terminal => Vec::new(),
            AppState::Look(_) => alloc::vec![
                ("Keep".to_string(), b'\n'),
                ("Revert".to_string(), crate::notepad::ESC),
            ],
        }
    }
}

/// Ctrl+R: refresh the Files listing.
pub const CTRL_R: u8 = 0x12;

/// One entry of the root directory, as the kernel lists it (GFX-056).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    /// "file", "folder" or "object".
    pub kind: &'static str,
    /// Seconds since the epoch when last written; 0 when never stamped.
    pub modified_at: u64,
    /// What the content is, when the writer said (GFX-057).
    pub schema: Option<String>,
    pub tags: Vec<String>,
    /// In the bin.
    pub trashed: bool,
    /// How many earlier contents are kept.
    pub versions: usize,
}

impl FileEntry {
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_string(),
            kind: "file",
            ..Self::default()
        }
    }

    /// The type as a word: from the schema, never from the name.
    pub fn kind_label(&self) -> &'static str {
        match (self.kind, self.schema.as_deref()) {
            ("folder", _) => "Folder",
            (_, Some("text/plain")) => "Text",
            (_, Some(_)) => "Data",
            (_, None) => "File",
        }
    }
}

/// `12 B`, `1.5 KB`, `2.0 MB`.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        alloc::format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        alloc::format!("{}.{} KB", bytes / 1024, (bytes % 1024) * 10 / 1024)
    } else {
        let mb = bytes / (1024 * 1024);
        alloc::format!("{}.{} MB", mb, (bytes % (1024 * 1024)) * 10 / (1024 * 1024))
    }
}

/// A one-line prompt in the Files footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesPrompt {
    /// A name for a new, empty file.
    New(String),
    /// A new name for the selected file.
    Rename(String),
    /// Tags to add (and `-tag` to remove) on the selected file.
    Tag(String),
    /// Remove the selected file from the bin for good? Enter confirms.
    ConfirmPurge,
}

/// Ctrl+E: rename ("edit the name"). Ctrl+K: tag. Ctrl+B: the bin.
pub const CTRL_E: u8 = 0x05;
pub const CTRL_K: u8 = 0x0B;
pub const CTRL_B: u8 = 0x02;

/// The Files app's state (GFX-057): what the kernel listed, a filter typed
/// straight into the card, a sort, whether the bin is showing, the
/// selection within what is visible, and any prompt in the footer. There
/// are no folders: a file is found by name or tag, and by recency.
#[derive(Debug, Clone, Default)]
pub struct FilesView {
    pub entries: Vec<FileEntry>,
    /// Index into [`FilesView::visible`].
    pub selection: usize,
    pub loaded: bool,
    pub scroll: usize,
    pub prompt: Option<FilesPrompt>,
    /// Typed into the card: matches names and tags, case-insensitively.
    pub filter: String,
    /// Sorted by name rather than by recency.
    pub by_name: bool,
    /// Showing the bin instead of the files.
    pub bin: bool,
}

impl FilesView {
    /// Indices into `entries` that the card shows, in display order.
    pub fn visible(&self) -> Vec<usize> {
        let filter = self.filter.to_ascii_lowercase();
        let mut shown: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.trashed == self.bin)
            // Names starting with '.' are the system's (the desk's look);
            // Files is for the person's files.
            .filter(|(_, e)| !e.name.starts_with('.'))
            .filter(|(_, e)| {
                filter.is_empty()
                    || e.name.to_ascii_lowercase().contains(&filter)
                    || e.tags
                        .iter()
                        .any(|t| t.to_ascii_lowercase().contains(&filter))
            })
            .map(|(i, _)| i)
            .collect();
        if self.by_name {
            shown.sort_by(|a, b| self.entries[*a].name.cmp(&self.entries[*b].name));
        } else {
            shown.sort_by(|a, b| {
                self.entries[*b]
                    .modified_at
                    .cmp(&self.entries[*a].modified_at)
                    .then_with(|| self.entries[*a].name.cmp(&self.entries[*b].name))
            });
        }
        shown
    }

    fn select(&mut self, delta: isize) {
        let count = self.visible().len();
        if count == 0 {
            self.selection = 0;
            return;
        }
        let last = count as isize - 1;
        self.selection = (self.selection as isize + delta).clamp(0, last) as usize;
    }

    pub fn selected(&self) -> Option<&FileEntry> {
        self.visible()
            .get(self.selection)
            .map(|index| &self.entries[*index])
    }

    /// The footer: a prompt while one is open, otherwise everything known
    /// about the selected entry -- the details live where the eye already
    /// is, not in a dialog.
    fn footer(&self) -> String {
        match &self.prompt {
            Some(FilesPrompt::New(name)) => {
                return alloc::format!("New: {name}_   a name, no suffix needed   (Enter, Esc)")
            }
            Some(FilesPrompt::Rename(name)) => {
                return alloc::format!("Rename to: {name}_   (Enter renames, Esc cancels)")
            }
            Some(FilesPrompt::Tag(tags)) => {
                return alloc::format!(
                    "Tags: {tags}_   space-separated, -tag removes   (Enter, Esc)"
                )
            }
            Some(FilesPrompt::ConfirmPurge) => {
                let name = self.selected().map(|e| e.name.as_str()).unwrap_or("");
                return alloc::format!("Remove {name} for good?   Enter removes, Esc keeps it");
            }
            None => {}
        }
        if !self.loaded {
            return "Reading the filesystem...".to_string();
        }
        let mut text = String::new();
        if !self.filter.is_empty() {
            text.push_str(&alloc::format!(
                "Filter: {}_   {} of {}   ",
                self.filter,
                self.visible().len(),
                self.entries
                    .iter()
                    .filter(|e| e.trashed == self.bin)
                    .count()
            ));
        }
        match self.selected() {
            Some(entry) => {
                text.push_str(&alloc::format!(
                    "{}   {}   {}",
                    entry.name,
                    entry.kind_label(),
                    format_size(entry.size)
                ));
                if !entry.tags.is_empty() {
                    text.push_str(&alloc::format!("   #{}", entry.tags.join(" #")));
                }
                text.push_str(&alloc::format!(
                    "   written {}",
                    crate::rtc::format_unix_minutes(entry.modified_at)
                ));
                if entry.versions > 0 {
                    text.push_str(&alloc::format!("   {} earlier", entry.versions));
                }
                text.push_str(if self.bin {
                    "   Enter restores"
                } else {
                    "   Enter opens"
                });
            }
            None if self.bin => text.push_str("The bin is empty"),
            None if !self.filter.is_empty() => text.push_str("Nothing matches"),
            None => {
                text.push_str("No files yet   Ctrl+N makes one, or just start typing to search")
            }
        }
        text
    }

    /// One row: name, tags, type, size and time in columns that fit
    /// `columns` character cells. The name column takes what the fixed
    /// columns leave; narrow cards lose the rightmost columns first.
    fn line(entry: &FileEntry, columns: usize) -> String {
        let when = crate::rtc::format_unix_minutes(entry.modified_at);
        let size = format_size(entry.size);
        let tags: String = entry.tags.iter().map(|t| alloc::format!("#{t} ")).collect();
        let tags: String = tags.trim_end().chars().take(14).collect();
        let clipped = |width: usize| -> String { entry.name.chars().take(width).collect() };
        // " {:<14} {:<6} {:>8}  {:16}" after the name.
        const TAIL_FULL: usize = 1 + 14 + 1 + 6 + 1 + 8 + 2 + 16;
        const TAIL_SHORT: usize = 1 + 8;
        if columns >= TAIL_FULL + 12 {
            let name_w = columns - TAIL_FULL;
            alloc::format!(
                "{:<name_w$} {:<14} {:<6} {:>8}  {}",
                clipped(name_w),
                tags,
                entry.kind_label(),
                size,
                when
            )
        } else if columns >= TAIL_SHORT + 8 {
            let name_w = columns - TAIL_SHORT;
            alloc::format!("{:<name_w$} {:>8}", clipped(name_w), size)
        } else {
            clipped(columns)
        }
    }

    /// The header chips for this state (GFX-057): the bin has its own.
    fn actions(&self) -> Vec<(String, u8)> {
        let sort = if self.by_name { "A-Z" } else { "Recent" };
        if self.bin {
            alloc::vec![
                ("Restore".to_string(), b'\n'),
                ("Remove".to_string(), crate::notepad::KEY_DELETE),
                (sort.to_string(), crate::notepad::CTRL_S),
                ("Files".to_string(), CTRL_B),
            ]
        } else {
            alloc::vec![
                ("New".to_string(), crate::notepad::CTRL_N),
                ("Rename".to_string(), CTRL_E),
                ("Tag".to_string(), CTRL_K),
                ("Bin it".to_string(), crate::notepad::KEY_DELETE),
                (sort.to_string(), crate::notepad::CTRL_S),
                ("Bin".to_string(), CTRL_B),
            ]
        }
    }
}

/// `work draft -old` -> add `work`, `draft`; remove `old`.
pub fn parse_tag_edit(text: &str) -> (Vec<String>, Vec<String>) {
    let mut add = Vec::new();
    let mut remove = Vec::new();
    for word in text.split_whitespace() {
        let word = word.trim_start_matches('#');
        if let Some(rest) = word.strip_prefix('-') {
            if !rest.is_empty() {
                remove.push(rest.to_ascii_lowercase());
            }
        } else if !word.is_empty() {
            add.push(word.to_ascii_lowercase());
        }
    }
    (add, remove)
}

/// Break `text` into lines of at most `columns` characters at spaces; a
/// word longer than a line is cut. A notice card is 340px wide and the
/// workspace's messages are longer than that -- the first build showed
/// "Unknown command: files. Type 'help' for h" and stopped.
pub fn wrap_words(text: &str, columns: usize) -> Vec<String> {
    let columns = columns.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let mut word = word;
        while !word.is_empty() {
            let used = line.chars().count();
            let sep = usize::from(!line.is_empty());
            let len = word.chars().count();
            if used + sep + len <= columns {
                if sep == 1 {
                    line.push(' ');
                }
                line.push_str(word);
                break;
            }
            if used > 0 {
                lines.push(core::mem::take(&mut line));
                continue;
            }
            let cut = word
                .char_indices()
                .nth(columns)
                .map(|(i, _)| i)
                .unwrap_or(word.len());
            lines.push(word[..cut].to_string());
            word = &word[cut..];
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// What an open window holds.
#[derive(Debug, Clone)]
pub enum AppState {
    Notepad(Notepad),
    Files(FilesView),
    /// The console's state lives in the workspace; the card only shows it.
    Terminal,
    Look(LookView),
}

/// One open window.
#[derive(Debug, Clone)]
pub struct DeskWindow {
    pub id: ViewId,
    pub app: DeskApp,
    pub bounds: RasterRect,
    pub z: usize,
    /// The bounds before a snap, so the next header drag un-snaps to them.
    pub restore: Option<RasterRect>,
    /// Tucked into the dock: not drawn, not focusable, waiting on its tile.
    pub tucked: bool,
    pub state: AppState,
}

impl DeskWindow {
    pub fn notepad(&self) -> Option<&Notepad> {
        match &self.state {
            AppState::Notepad(notepad) => Some(notepad),
            _ => None,
        }
    }

    pub fn notepad_mut(&mut self) -> Option<&mut Notepad> {
        match &mut self.state {
            AppState::Notepad(notepad) => Some(notepad),
            _ => None,
        }
    }

    pub fn files(&self) -> Option<&FilesView> {
        match &self.state {
            AppState::Files(files) => Some(files),
            _ => None,
        }
    }

    pub fn look_mut(&mut self) -> Option<&mut LookView> {
        match &mut self.state {
            AppState::Look(look) => Some(look),
            _ => None,
        }
    }

    pub fn files_mut(&mut self) -> Option<&mut FilesView> {
        match &mut self.state {
            AppState::Files(files) => Some(files),
            _ => None,
        }
    }
}

/// A header drag in progress: which window, and where inside its header
/// the pointer grabbed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drag {
    id: ViewId,
    grab_x: usize,
    grab_y: usize,
}

/// A corner resize in progress: which window, and how far inside the
/// corner the pointer grabbed it, so the corner stays under the pointer
/// rather than jumping to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Resize {
    id: ViewId,
    grab_dx: usize,
    grab_dy: usize,
}

/// What the desk asks the kernel to do after handling input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeskRequest {
    /// Perform this file operation for the window with `id`, then call
    /// [`Desk::io_done`].
    Io { id: ViewId, effect: NotepadEffect },
    /// A key for the console, which the workspace owns.
    Terminal(u8),
    /// List the filesystem, then call [`Desk::files_listed`] for `id`.
    ListFiles { id: ViewId },
    /// Leave the desk for the text console.
    TextConsole,
    /// Scroll the console's view: positive notches towards older lines.
    TerminalScroll { notches: i32 },
    /// Create `name` empty, then list again for `id` and open it (GFX-056).
    CreateFile { id: ViewId, name: String },
    /// Give `from` the name `to`, then list again for `id`.
    RenameFile {
        id: ViewId,
        from: String,
        to: String,
    },
    /// Put `name` in the bin (or take it out), then list again (GFX-057).
    TrashFile {
        id: ViewId,
        name: String,
        trashed: bool,
    },
    /// Remove `name` for good, then list again.
    PurgeFile { id: ViewId, name: String },
    /// Add and remove tags on `name`, then list again.
    TagFile {
        id: ViewId,
        name: String,
        add: Vec<String>,
        remove: Vec<String>,
    },
    /// Read kept version `index` of `name` for the Notepad `id`, then call
    /// [`Desk::version_loaded`] (GFX-058).
    ReadVersion {
        id: ViewId,
        name: String,
        index: usize,
    },
    /// Write the desk's look to disk (GFX-059).
    SaveLook { text: String },
    /// Write the recently used files to disk (GFX-060); read back with
    /// the look.
    SaveRecent { text: String },
    /// Find lines containing `query` in the person's text files, then call
    /// [`Desk::search_results`] (GFX-061).
    SearchFiles { query: String },
    /// Read the desk's look from disk, then call [`Desk::apply_look`].
    LoadLook,
}

/// One row of the palette: what it does and how it is spelled (GFX-053).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    NewNotepad,
    NewTerminal,
    OpenFiles,
    Save,
    SaveAs,
    Open,
    CloseWindow,
    NextWindow,
    SnapLeft,
    SnapRight,
    Maximise,
    Tuck,
    TextConsole,
    SelectAll,
    Copy,
    Cut,
    Paste,
    Find,
    ToggleTheme,
    History,
}

impl PaletteAction {
    pub const ALL: [PaletteAction; 20] = [
        PaletteAction::NewNotepad,
        PaletteAction::NewTerminal,
        PaletteAction::OpenFiles,
        PaletteAction::Save,
        PaletteAction::SaveAs,
        PaletteAction::Open,
        PaletteAction::CloseWindow,
        PaletteAction::NextWindow,
        PaletteAction::SnapLeft,
        PaletteAction::SnapRight,
        PaletteAction::Maximise,
        PaletteAction::Tuck,
        PaletteAction::TextConsole,
        PaletteAction::SelectAll,
        PaletteAction::Copy,
        PaletteAction::Cut,
        PaletteAction::Paste,
        PaletteAction::Find,
        PaletteAction::ToggleTheme,
        PaletteAction::History,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            PaletteAction::NewNotepad => "New Notepad window",
            PaletteAction::NewTerminal => "New Terminal",
            PaletteAction::OpenFiles => "Files",
            PaletteAction::Save => "Save",
            PaletteAction::SaveAs => "Save as...",
            PaletteAction::Open => "Open file...",
            PaletteAction::CloseWindow => "Close window",
            PaletteAction::NextWindow => "Next window",
            PaletteAction::SnapLeft => "Snap left",
            PaletteAction::SnapRight => "Snap right",
            PaletteAction::Maximise => "Fill the desk",
            PaletteAction::Tuck => "Tuck into the dock",
            PaletteAction::TextConsole => "Switch to the text console",
            PaletteAction::SelectAll => "Select all",
            PaletteAction::Copy => "Copy",
            PaletteAction::Cut => "Cut",
            PaletteAction::Paste => "Paste",
            PaletteAction::Find => "Find...",
            PaletteAction::ToggleTheme => "Look: themes and accents",
            PaletteAction::History => "Earlier versions of this document",
        }
    }

    pub const fn shortcut(self) -> &'static str {
        match self {
            PaletteAction::NewNotepad => "Ctrl+N",
            PaletteAction::NewTerminal => "Ctrl+T",
            PaletteAction::OpenFiles => "",
            PaletteAction::Save => "Ctrl+S",
            PaletteAction::SaveAs => "",
            PaletteAction::Open => "Ctrl+O",
            PaletteAction::CloseWindow => "Ctrl+W",
            PaletteAction::NextWindow => "Ctrl+Tab",
            PaletteAction::SnapLeft => "Ctrl+Left",
            PaletteAction::SnapRight => "Ctrl+Right",
            PaletteAction::Maximise => "Ctrl+Up",
            PaletteAction::Tuck => "drag onto the dock",
            PaletteAction::TextConsole => "",
            PaletteAction::SelectAll => "Ctrl+A",
            PaletteAction::Copy => "Ctrl+C",
            PaletteAction::Cut => "Ctrl+X",
            PaletteAction::Paste => "Ctrl+V",
            PaletteAction::Find => "Ctrl+F",
            PaletteAction::ToggleTheme => "",
            PaletteAction::History => "Ctrl+Y",
        }
    }

    /// Whether the action needs a focused Notepad.
    const fn needs_notepad(self) -> bool {
        matches!(
            self,
            PaletteAction::Save
                | PaletteAction::SaveAs
                | PaletteAction::Open
                | PaletteAction::SelectAll
                | PaletteAction::Copy
                | PaletteAction::Cut
                | PaletteAction::Paste
                | PaletteAction::Find
                | PaletteAction::History
        )
    }

    /// Whether the action needs any focused window.
    const fn needs_window(self) -> bool {
        matches!(
            self,
            PaletteAction::CloseWindow
                | PaletteAction::SnapLeft
                | PaletteAction::SnapRight
                | PaletteAction::Maximise
                | PaletteAction::Tuck
        )
    }
}

/// The palette while it is open (GFX-053): a query and the actions that
/// match it, one selected. There are no menus anywhere on the desk; this is
/// the one place every action is listed, with its shortcut beside it.
#[derive(Debug, Clone, Default)]
pub struct Palette {
    pub query: String,
    pub selection: usize,
    /// Lines inside files that contain the query, as the kernel found them
    /// for this query (GFX-061): `(file, line)`. Empty until it answers.
    pub hits: Vec<(String, String)>,
    /// The query the hits were found for; stale hits are not shown.
    pub hits_for: String,
}

/// Content search starts at this many characters, so one letter does not
/// read every file.
pub const SEARCH_MIN_CHARS: usize = 2;
/// How many lines a content search returns at most.
pub const SEARCH_MAX_HITS: usize = 6;

/// One row of the palette (GFX-060): an action, or a file opened lately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteRow {
    Action(PaletteAction),
    /// Open this file in a Notepad.
    Recent(String),
    /// A line inside a file that contains the query: open the file there.
    Hit {
        file: String,
        line: String,
    },
}

impl PaletteRow {
    pub fn label(&self) -> String {
        match self {
            PaletteRow::Action(action) => action.label().to_string(),
            PaletteRow::Recent(name) => alloc::format!("Open {name}"),
            PaletteRow::Hit { file, line } => {
                let line: String = line.trim().chars().take(40).collect();
                alloc::format!("{file}: {line}")
            }
        }
    }

    pub fn shortcut(&self) -> &'static str {
        match self {
            PaletteRow::Action(action) => action.shortcut(),
            PaletteRow::Recent(_) => "recent",
            PaletteRow::Hit { .. } => "in file",
        }
    }
}

/// How many recently used files the palette offers.
pub const RECENT_FILES: usize = 5;

impl Palette {
    /// The rows that match the query, in order: the files used lately
    /// first, then the actions, filtered for the state of the desk so the
    /// list never offers something that would do nothing.
    pub fn matches(
        &self,
        has_window: bool,
        has_notepad: bool,
        recent: &[String],
    ) -> Vec<PaletteRow> {
        let query = self.query.to_ascii_lowercase();
        let fits =
            |label: &str| query.is_empty() || label.to_ascii_lowercase().contains(query.as_str());
        let mut rows: Vec<PaletteRow> = recent
            .iter()
            .filter(|name| fits(&alloc::format!("Open {name}")))
            .map(|name| PaletteRow::Recent(name.clone()))
            .collect();
        if self.hits_for == self.query {
            rows.extend(self.hits.iter().map(|(file, line)| PaletteRow::Hit {
                file: file.clone(),
                line: line.clone(),
            }));
        }
        rows.extend(
            PaletteAction::ALL
                .iter()
                .copied()
                .filter(|action| !(action.needs_notepad() && !has_notepad))
                .filter(|action| !(action.needs_window() && !has_window))
                .filter(|action| fits(action.label()))
                .map(PaletteRow::Action),
        );
        rows
    }
}

/// A notice the desk raised itself (a save that landed), with its expiry.
#[derive(Debug, Clone)]
struct DeskNotice {
    notice: ShellNotice,
    expires_at: u64,
}

/// The console as the Terminal card shows it, built by the kernel from the
/// workspace: the lines that fit, and the caret on the prompt line.
#[derive(Debug, Clone, Default)]
pub struct TerminalView {
    pub lines: Vec<String>,
    /// `(line, character column)` of the caret within `lines`.
    pub cursor: Option<(usize, usize)>,
    pub status: String,
}

/// The whole desk.
#[derive(Debug, Clone)]
pub struct Desk {
    width: usize,
    height: usize,
    top_bar_id: ViewId,
    dock_id: ViewId,
    windows: Vec<DeskWindow>,
    focus: Option<ViewId>,
    drag: Option<Drag>,
    resize: Option<Resize>,
    next_z: usize,
    opened: usize,
    pointer: Option<(usize, usize)>,
    hovered_tile: Option<usize>,
    palette: Option<Palette>,
    palette_id: ViewId,
    /// One clipboard for every card (GFX-054).
    clipboard: String,
    /// The look kept on disk (GFX-059)...
    look: LookChoice,
    /// ...and the one being previewed from the Look card, if any.
    look_preview: Option<LookChoice>,
    /// Files opened or saved lately, newest first (GFX-060).
    recent: Vec<String>,
    /// Notepads whose pending save is an autosave: no notice when it lands.
    quiet_saves: Vec<ViewId>,
    /// A pointer drag selecting text in this card.
    text_select: Option<ViewId>,
    notices: Vec<DeskNotice>,
    notice_ids: Vec<ViewId>,
    /// Shell notices the person clicked away (GFX-063): hidden until the
    /// workspace's list changes.
    dismissed_shell: Vec<ShellNotice>,
    /// The workspace's notices as of the last frame, so a click can
    /// dismiss them.
    last_shell_notices: Vec<ShellNotice>,
    /// The last file listing, for save-as and open autocomplete.
    file_names_cache: Vec<String>,
}

impl Desk {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            top_bar_id: ViewId::new(),
            dock_id: ViewId::new(),
            windows: Vec::new(),
            focus: None,
            drag: None,
            resize: None,
            next_z: 1,
            opened: 0,
            pointer: None,
            hovered_tile: None,
            palette: None,
            palette_id: ViewId::new(),
            clipboard: String::new(),
            look: LookChoice::default(),
            look_preview: None,
            recent: Vec::new(),
            quiet_saves: Vec::new(),
            text_select: None,
            notices: Vec::new(),
            notice_ids: (0..4).map(|_| ViewId::new()).collect(),
            dismissed_shell: Vec::new(),
            last_shell_notices: Vec::new(),
            file_names_cache: Vec::new(),
        }
    }

    pub fn palette_open(&self) -> bool {
        self.palette.is_some()
    }

    /// Raise a notice card for a few seconds (GFX-053).
    pub fn notify(&mut self, level: NoticeLevel, text: impl Into<String>, now: u64) {
        self.notices.push(DeskNotice {
            notice: ShellNotice::new(level, text),
            expires_at: now.saturating_add(NOTICE_TTL_TICKS),
        });
        let max = self.notice_ids.len();
        if self.notices.len() > max {
            let overflow = self.notices.len() - max;
            self.notices.drain(..overflow);
        }
    }

    /// The kernel answers a `ListFiles` request.
    pub fn files_listed(&mut self, id: ViewId, entries: Vec<FileEntry>) {
        let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        self.file_names_cache = names.clone();
        if let Some(files) = self.window_mut(id).and_then(|w| w.files_mut()) {
            // Keep the selection on the same name across a refresh.
            let keep = files.selected().map(|e| e.name.clone());
            files.entries = entries;
            files.loaded = true;
            let visible = files.visible();
            files.selection = keep
                .and_then(|name| visible.iter().position(|i| files.entries[*i].name == name))
                .unwrap_or(0);
            files.select(0);
        }
        if let Some(notepad) = self.window_mut(id).and_then(|w| w.notepad_mut()) {
            notepad.set_file_names(names);
        }
    }

    /// A header chip was pressed: the app's key for it (GFX-056).
    fn card_action(&mut self, id: ViewId, index: usize) -> (Option<DeskRequest>, bool) {
        let Some(actions) = self.window(id).map(|w| w.actions()) else {
            return (None, false);
        };
        let Some((_, byte)) = actions.get(index) else {
            return (None, false);
        };
        self.handle_app_key(id, *byte)
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// The theme the desk is drawn with: the preview while one is open,
    /// otherwise what is kept.
    pub fn theme(&self) -> Theme {
        self.look_preview.as_ref().unwrap_or(&self.look).theme()
    }

    /// The kept look, by name.
    pub fn look(&self) -> &LookChoice {
        &self.look
    }

    /// The kernel read the look file (GFX-059); an unreadable or missing
    /// file leaves the defaults.
    pub fn apply_look(&mut self, text: Option<&str>) {
        if let Some(text) = text {
            self.look = LookChoice::from_text(text);
        }
        self.look_preview = None;
    }

    /// The kernel read the recent-files list (GFX-060): one name a line.
    pub fn apply_recent(&mut self, text: Option<&str>) {
        if let Some(text) = text {
            self.recent = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .take(RECENT_FILES)
                .map(String::from)
                .collect();
        }
    }

    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// `name` was just used: to the front of the list, bounded. Returns the
    /// request that keeps the list on disk when it changed.
    fn touch_recent(&mut self, name: &str) -> Option<DeskRequest> {
        if self.recent.first().map(String::as_str) == Some(name) {
            return None;
        }
        self.recent.retain(|n| n != name);
        self.recent.insert(0, name.to_string());
        self.recent.truncate(RECENT_FILES);
        let mut text = self.recent.join("\n");
        text.push('\n');
        Some(DeskRequest::SaveRecent { text })
    }

    /// Every tick (GFX-060): documents that have sat still save
    /// themselves. The saves are quiet -- no notice card -- because a card
    /// for something the person did not ask for is noise.
    pub fn tick(&mut self, now: u64) -> Vec<DeskRequest> {
        let mut requests = Vec::new();
        for window in &mut self.windows {
            if let AppState::Notepad(notepad) = &mut window.state {
                if let Some(effect) = notepad.autosave_due(now) {
                    requests.push(DeskRequest::Io {
                        id: window.id,
                        effect,
                    });
                    if !self.quiet_saves.contains(&window.id) {
                        self.quiet_saves.push(window.id);
                    }
                }
            }
        }
        requests
    }

    pub fn focus(&self) -> Option<ViewId> {
        self.focus
    }

    pub fn window(&self, id: ViewId) -> Option<&DeskWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn window_mut(&mut self, id: ViewId) -> Option<&mut DeskWindow> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    pub fn focused_window(&self) -> Option<&DeskWindow> {
        self.focus.and_then(|id| self.window(id))
    }

    /// Whether a key typed now belongs to an app rather than the shell.
    /// Whether the console has a card open, and if so how many content rows
    /// it shows -- so the kernel can build a [`TerminalView`] that fits.
    pub fn terminal_rows(&self) -> Option<usize> {
        let window = self.windows.iter().find(|w| w.app == DeskApp::Terminal)?;
        Some(Self::card_rows(window.bounds))
    }

    pub fn set_pointer(&mut self, position: Option<(usize, usize)>) {
        self.pointer = position;
    }

    /// The area cards may occupy: below the top bar, above the dock.
    fn work_area(&self) -> RasterRect {
        let top = TOP_BAR_HEIGHT;
        let bottom = self.height.saturating_sub(DOCK_HEIGHT + DOCK_MARGIN * 2);
        RasterRect::new(0, top, self.width, bottom.saturating_sub(top))
    }

    fn card_rows(bounds: RasterRect) -> usize {
        DesktopWindow::card(
            ViewFrame::new(
                ViewId::new(),
                ViewKind::TextBuffer,
                0,
                ViewContent::text_buffer(Vec::new()),
                0,
            ),
            bounds,
        )
        .with_footer(Some(String::new()))
        .content_rows()
    }

    /// Open `app` in a new card, cascaded from the last, and focus it.
    pub fn launch(&mut self, app: DeskApp) -> ViewId {
        let (w, h) = app.size();
        let area = self.work_area();
        let w = w.min(area.width);
        let h = h.min(area.height);
        let step = (self.opened % 6) * CASCADE_STEP;
        let x =
            (area.x + area.width.saturating_sub(w) / 2 + step).min(area.right().saturating_sub(w));
        let y = (area.y + 16 + step).min(area.bottom().saturating_sub(h).max(area.y));
        let id = ViewId::new();
        let z = self.next_z;
        self.next_z += 1;
        self.opened += 1;
        self.windows.push(DeskWindow {
            id,
            app,
            bounds: RasterRect::new(x, y, w, h),
            z,
            restore: None,
            tucked: false,
            state: match app {
                DeskApp::Notepad => AppState::Notepad(Notepad::new()),
                DeskApp::Files => AppState::Files(FilesView::default()),
                DeskApp::Terminal => AppState::Terminal,
                DeskApp::Look => AppState::Look(LookView {
                    row: LookView::row_of(&self.look),
                }),
            },
        });
        self.focus = Some(id);
        id
    }

    /// Tuck `id` into the dock: hidden until its tile is clicked.
    pub fn tuck(&mut self, id: ViewId) {
        if let Some(window) = self.window_mut(id) {
            window.tucked = true;
        }
        if self.focus == Some(id) {
            self.focus = self
                .windows
                .iter()
                .filter(|w| !w.tucked)
                .max_by_key(|w| w.z)
                .map(|w| w.id);
        }
    }

    fn untuck(&mut self, id: ViewId) {
        if let Some(window) = self.window_mut(id) {
            window.tucked = false;
        }
        self.raise(id);
    }

    /// Back to the size a window had before it was snapped, if it was.
    fn unsnap(&mut self, id: ViewId) -> bool {
        let area = self.work_area();
        if let Some(window) = self.window_mut(id) {
            if let Some(restore) = window.restore.take() {
                window.bounds = RasterRect::new(
                    restore.x.min(area.right().saturating_sub(restore.width)),
                    restore.y.max(area.y),
                    restore.width.min(area.width),
                    restore.height.min(area.height),
                );
                return true;
            }
        }
        false
    }

    /// Snap the focused window from the palette.
    fn snap_focused(&mut self, how: PaletteAction) {
        let area = self.work_area();
        let Some(id) = self.focus else {
            return;
        };
        let target = match how {
            PaletteAction::SnapLeft => RasterRect::new(area.x, area.y, area.width / 2, area.height),
            PaletteAction::SnapRight => RasterRect::new(
                area.x + area.width / 2,
                area.y,
                area.width - area.width / 2,
                area.height,
            ),
            _ => area,
        };
        if let Some(window) = self.window_mut(id) {
            if window.restore.is_none() {
                window.restore = Some(window.bounds);
            }
            window.bounds = target;
        }
    }

    /// Run a palette action. Returns what the kernel must do, if anything.
    fn run_action(&mut self, action: PaletteAction) -> Option<DeskRequest> {
        match action {
            PaletteAction::NewNotepad => {
                self.launch(DeskApp::Notepad);
                None
            }
            PaletteAction::NewTerminal => {
                self.launch(DeskApp::Terminal);
                None
            }
            PaletteAction::OpenFiles => {
                let id = self.launch(DeskApp::Files);
                Some(DeskRequest::ListFiles { id })
            }
            PaletteAction::Save => self.forward_to_notepad(crate::notepad::CTRL_S),
            PaletteAction::SaveAs => self.forward_to_notepad(crate::notepad::CTRL_SHIFT_S),
            PaletteAction::Open => self.forward_to_notepad(crate::notepad::CTRL_O),
            PaletteAction::SelectAll => self.forward_to_notepad(crate::notepad::CTRL_A),
            PaletteAction::Copy => self.forward_to_notepad(crate::notepad::CTRL_C),
            PaletteAction::Cut => self.forward_to_notepad(crate::notepad::CTRL_X),
            PaletteAction::Paste => self.forward_to_notepad(crate::notepad::CTRL_V),
            PaletteAction::Find => self.forward_to_notepad(crate::notepad::CTRL_F),
            PaletteAction::History => self.forward_to_notepad(crate::notepad::CTRL_Y),
            PaletteAction::CloseWindow => {
                if let Some(id) = self.focus {
                    self.close(id);
                }
                None
            }
            PaletteAction::NextWindow => {
                self.cycle_focus();
                None
            }
            PaletteAction::SnapLeft | PaletteAction::SnapRight | PaletteAction::Maximise => {
                self.snap_focused(action);
                None
            }
            PaletteAction::Tuck => {
                if let Some(id) = self.focus {
                    self.tuck(id);
                }
                None
            }
            PaletteAction::TextConsole => Some(DeskRequest::TextConsole),
            PaletteAction::ToggleTheme => {
                self.open_or_raise(DeskApp::Look);
                None
            }
        }
    }

    fn forward_to_notepad(&mut self, byte: u8) -> Option<DeskRequest> {
        let id = self.focus?;
        let (request, _) = self.handle_app_key(id, byte);
        request
    }

    /// A content search for the palette's query, when it is long enough.
    fn search_request(palette: &Palette) -> Option<DeskRequest> {
        (palette.query.trim().len() >= SEARCH_MIN_CHARS).then(|| DeskRequest::SearchFiles {
            query: palette.query.trim().to_string(),
        })
    }

    /// The kernel found these lines for `query` (GFX-061). Shown only while
    /// the palette still asks the same thing.
    pub fn search_results(&mut self, query: &str, hits: Vec<(String, String)>) {
        if let Some(palette) = self.palette.as_mut() {
            palette.hits = hits;
            palette.hits_for = query.to_string();
        }
    }

    /// A key while the palette is open.
    fn handle_palette_key(&mut self, byte: u8) -> (Option<DeskRequest>, bool) {
        let has_window = self.focused_window().is_some();
        let has_notepad = self.focused_window().and_then(|w| w.notepad()).is_some();
        let recent = self.recent.clone();
        let query = self
            .palette
            .as_ref()
            .map(|p| p.query.clone())
            .unwrap_or_default();
        let Some(palette) = self.palette.as_mut() else {
            return (None, false);
        };
        match byte {
            crate::notepad::ESC | KEY_CTRL_SPACE | 0x10 => {
                self.palette = None;
                (None, true)
            }
            crate::notepad::KEY_UP => {
                palette.selection = palette.selection.saturating_sub(1);
                (None, true)
            }
            crate::notepad::KEY_DOWN => {
                let count = palette.matches(has_window, has_notepad, &recent).len();
                palette.selection = (palette.selection + 1).min(count.saturating_sub(1));
                (None, true)
            }
            crate::notepad::BACKSPACE => {
                palette.query.pop();
                palette.selection = 0;
                (Self::search_request(palette), true)
            }
            b'\n' | b'\r' => {
                let matches = palette.matches(has_window, has_notepad, &recent);
                let chosen = matches.get(palette.selection).cloned();
                self.palette = None;
                match chosen {
                    Some(PaletteRow::Action(action)) => (self.run_action(action), true),
                    Some(PaletteRow::Recent(name)) => {
                        let id = self.launch(DeskApp::Notepad);
                        (
                            Some(DeskRequest::Io {
                                id,
                                effect: NotepadEffect::Open { path: name },
                            }),
                            true,
                        )
                    }
                    Some(PaletteRow::Hit { file, .. }) => {
                        // Open the file and land on the line: the Notepad
                        // finds the query once the content arrives.
                        let id = self.launch(DeskApp::Notepad);
                        if let Some(notepad) = self.window_mut(id).and_then(|w| w.notepad_mut()) {
                            notepad.find_on_open(&query);
                        }
                        (
                            Some(DeskRequest::Io {
                                id,
                                effect: NotepadEffect::Open { path: file },
                            }),
                            true,
                        )
                    }
                    None => (None, true),
                }
            }
            0x20..=0x7E => {
                if palette.query.len() < 40 {
                    palette.query.push(byte as char);
                    palette.selection = 0;
                }
                (Self::search_request(palette), true)
            }
            _ => (None, false),
        }
    }

    /// Bring `id` to the front and give it focus.
    pub fn raise(&mut self, id: ViewId) {
        let z = self.next_z;
        if let Some(window) = self.window_mut(id) {
            window.z = z;
            window.tucked = false;
            self.next_z += 1;
            self.focus = Some(id);
        }
    }

    pub fn close(&mut self, id: ViewId) {
        if self.window(id).map(|w| w.app) == Some(DeskApp::Look) {
            self.look_preview = None;
        }
        self.windows.retain(|w| w.id != id);
        if self.drag.map(|d| d.id) == Some(id) {
            self.drag = None;
        }
        if self.resize.map(|r| r.id) == Some(id) {
            self.resize = None;
        }
        if self.focus == Some(id) {
            // The top-most remaining window takes focus.
            self.focus = self
                .windows
                .iter()
                .filter(|w| !w.tucked)
                .max_by_key(|w| w.z)
                .map(|w| w.id);
        }
    }

    /// Focus the next window in z order (Ctrl+Tab). Tucked windows are
    /// skipped; the dock is how they come back.
    pub fn cycle_focus(&mut self) -> bool {
        if self.windows.iter().filter(|w| !w.tucked).count() < 2 {
            return false;
        }
        // The lowest window comes to the top, so repeated presses walk the
        // whole stack.
        let lowest = self
            .windows
            .iter()
            .filter(|w| !w.tucked)
            .min_by_key(|w| w.z)
            .map(|w| w.id);
        if let Some(id) = lowest {
            self.raise(id);
        }
        true
    }

    /// A dock tile was pressed: focus the app's window if it has one, else
    /// launch it.
    /// Bring an app's window to the front, or open one.
    pub fn open_or_raise(&mut self, app: DeskApp) -> ViewId {
        let existing = self
            .windows
            .iter()
            .filter(|w| w.app == app)
            .max_by_key(|w| w.z)
            .map(|w| w.id);
        match existing {
            Some(id) => {
                self.raise(id);
                id
            }
            None => self.launch(app),
        }
    }

    pub fn activate_dock_tile(&mut self, index: usize) -> Option<DeskRequest> {
        let app = DeskApp::ALL.get(index).copied()?;
        // A tucked window of this app comes back first.
        let tucked = self
            .windows
            .iter()
            .filter(|w| w.app == app && w.tucked)
            .max_by_key(|w| w.z)
            .map(|w| w.id);
        if let Some(id) = tucked {
            self.untuck(id);
            return None;
        }
        let existing = self
            .windows
            .iter()
            .filter(|w| w.app == app)
            .max_by_key(|w| w.z)
            .map(|w| w.id);
        match existing {
            Some(id) => {
                self.raise(id);
                None
            }
            None => {
                let id = self.launch(app);
                (app == DeskApp::Files).then_some(DeskRequest::ListFiles { id })
            }
        }
    }

    /// Snap a window whose drag ended at the desk's edge (GFX-052): left or
    /// right half, or the whole work area from the top edge.
    fn snap_if_at_edge(&mut self, id: ViewId, px: usize, py: usize) -> bool {
        let area = self.work_area();
        let target = if px < SNAP_MARGIN {
            Some(RasterRect::new(area.x, area.y, area.width / 2, area.height))
        } else if px + SNAP_MARGIN >= self.width {
            Some(RasterRect::new(
                area.x + area.width / 2,
                area.y,
                area.width - area.width / 2,
                area.height,
            ))
        } else if py <= area.y + SNAP_MARGIN {
            Some(area)
        } else {
            None
        };
        let Some(target) = target else {
            return false;
        };
        if let Some(window) = self.window_mut(id) {
            if window.restore.is_none() {
                window.restore = Some(window.bounds);
            }
            window.bounds = target;
            return true;
        }
        false
    }

    /// Apply routed pointer deliveries. Returns whether the screen changed.
    pub fn handle_deliveries(&mut self, deliveries: &[Delivery]) -> bool {
        let (_, changed) = self.handle_deliveries_with_requests(deliveries);
        changed
    }

    /// As `handle_deliveries`, also returning what the kernel must do.
    pub fn handle_deliveries_with_requests(
        &mut self,
        deliveries: &[Delivery],
    ) -> (Vec<DeskRequest>, bool) {
        let mut requests = Vec::new();
        let mut changed = false;
        for delivery in deliveries {
            match delivery {
                Delivery::FocusChanged { current, .. } => {
                    if let Some(id) = current {
                        if self.window(*id).is_some() {
                            self.raise(*id);
                            changed = true;
                        }
                    }
                }
                Delivery::Leave { target } if *target == self.dock_id => {
                    if self.hovered_tile.take().is_some() {
                        changed = true;
                    }
                }
                Delivery::Pointer { target, event, hit } => {
                    let press = event.is_press(PointerButton::Primary);
                    let release = event.is_release(PointerButton::Primary);
                    let region = hit.map(|h| h.region);
                    let px = event.position.x.max(0) as usize;
                    let py = event.position.y.max(0) as usize;

                    if *target == self.dock_id {
                        let tile = match region {
                            Some(HitRegion::DockTile { index }) => Some(index),
                            _ => None,
                        };
                        if self.hovered_tile != tile {
                            self.hovered_tile = tile;
                            changed = true;
                        }
                        if let (true, Some(index)) = (press, tile) {
                            if let Some(request) = self.activate_dock_tile(index) {
                                requests.push(request);
                            }
                            changed = true;
                        }
                        continue;
                    }
                    // A row of the palette runs on a click (GFX-063).
                    if press && *target == self.palette_id {
                        if let (Some(HitRegion::Content { line, .. }), Some(palette)) =
                            (region, self.palette.as_mut())
                        {
                            palette.selection = line;
                            let (request, _) = self.handle_palette_key(b'\n');
                            requests.extend(request);
                        }
                        changed = true;
                        continue;
                    }
                    // A click anywhere while the palette is open closes it.
                    if press && self.palette.is_some() && *target != self.palette_id {
                        self.palette = None;
                        changed = true;
                    }
                    // The bar is the desk's own handle: a click opens the
                    // palette, the same as Ctrl+Space (GFX-063).
                    if press && *target == self.top_bar_id {
                        self.palette = Some(Palette::default());
                        changed = true;
                        continue;
                    }
                    // A notice goes away when clicked (GFX-063).
                    if press && self.notice_ids.contains(target) {
                        self.notices.clear();
                        let shell = core::mem::take(&mut self.last_shell_notices);
                        self.dismissed_shell = shell;
                        changed = true;
                        continue;
                    }
                    if self.window(*target).is_none() {
                        continue;
                    }

                    match (press, release, region, &event.kind) {
                        (true, _, Some(HitRegion::Close), _) => {
                            self.close(*target);
                            changed = true;
                        }
                        (true, _, Some(HitRegion::Action { index }), _) => {
                            self.raise(*target);
                            self.focus = Some(*target);
                            let (request, _) = self.card_action(*target, index);
                            requests.extend(request);
                            changed = true;
                        }
                        (true, _, Some(HitRegion::Resize), _) => {
                            self.raise(*target);
                            if let Some(window) = self.window(*target) {
                                self.resize = Some(Resize {
                                    id: *target,
                                    grab_dx: window.bounds.right().saturating_sub(px),
                                    grab_dy: window.bounds.bottom().saturating_sub(py),
                                });
                            }
                            changed = true;
                        }
                        (true, _, Some(HitRegion::Header), _) => {
                            self.raise(*target);
                            if let Some(window) = self.window_mut(*target) {
                                // A snapped card un-snaps under the pointer.
                                if let Some(restore) = window.restore.take() {
                                    let half = restore.width / 2;
                                    window.bounds = RasterRect::new(
                                        px.saturating_sub(half),
                                        window.bounds.y,
                                        restore.width,
                                        restore.height,
                                    );
                                }
                                self.drag = Some(Drag {
                                    id: *target,
                                    grab_x: px.saturating_sub(window.bounds.x),
                                    grab_y: py.saturating_sub(window.bounds.y),
                                });
                            }
                            changed = true;
                        }
                        (true, _, Some(HitRegion::Content { line, column }), _) => {
                            self.raise(*target);
                            if let Some(notepad) =
                                self.window_mut(*target).and_then(|w| w.notepad_mut())
                            {
                                notepad.place_cursor(line, column);
                                self.text_select = Some(*target);
                            }
                            let mut open = false;
                            if let Some(files) =
                                self.window_mut(*target).and_then(|w| w.files_mut())
                            {
                                let row = files.scroll + line;
                                if row < files.visible().len() {
                                    open = files.selection == row;
                                    files.selection = row;
                                }
                            }
                            if open {
                                requests.extend(self.open_files_selection(*target));
                            }
                            // Look: a click on a row previews it; a click on
                            // the row already under the highlight keeps it.
                            let look_row = self
                                .window(*target)
                                .filter(|w| w.app == DeskApp::Look)
                                .and_then(|_| LookView::row_of_line(line));
                            if let Some(row) = look_row {
                                let current = self
                                    .look_preview
                                    .clone()
                                    .unwrap_or_else(|| self.look.clone());
                                let mut keep = false;
                                if let Some(look) =
                                    self.window_mut(*target).and_then(|w| w.look_mut())
                                {
                                    keep = look.row == row;
                                    look.row = row;
                                    if !keep {
                                        self.look_preview = Some(look.choice_at(&current));
                                    }
                                }
                                if keep {
                                    let (request, _) = self.handle_app_key(*target, b'\n');
                                    requests.extend(request);
                                }
                            }
                            changed = true;
                        }
                        (true, _, _, _) => {
                            self.raise(*target);
                            changed = true;
                        }
                        (_, _, _, PointerEventKind::Wheel { dy, .. }) => {
                            if let Some(notepad) =
                                self.window_mut(*target).and_then(|w| w.notepad_mut())
                            {
                                notepad.scroll_by(-(*dy) * 3);
                                changed = true;
                            } else if self.window(*target).map(|w| w.app) == Some(DeskApp::Terminal)
                            {
                                // The console's scrollback is the
                                // workspace's; ask for it.
                                requests.push(DeskRequest::TerminalScroll { notches: *dy * 3 });
                                changed = true;
                            }
                        }
                        (_, _, _, PointerEventKind::Move { .. }) => {
                            if self.text_select == Some(*target) {
                                if let Some(HitRegion::Content { line, column }) = region {
                                    if let Some(notepad) =
                                        self.window_mut(*target).and_then(|w| w.notepad_mut())
                                    {
                                        notepad.extend_selection_to(line, column);
                                        changed = true;
                                    }
                                }
                            } else if let Some(drag) = self.drag.filter(|d| d.id == *target) {
                                let (width, height) = (self.width, self.height);
                                if let Some(window) = self.window_mut(*target) {
                                    let max_x = width.saturating_sub(window.bounds.width);
                                    let max_y = height.saturating_sub(window.bounds.height);
                                    window.bounds.x = px.saturating_sub(drag.grab_x).min(max_x);
                                    window.bounds.y = py
                                        .saturating_sub(drag.grab_y)
                                        .max(TOP_BAR_HEIGHT)
                                        .min(max_y);
                                    changed = true;
                                }
                            } else if let Some(resize) = self.resize.filter(|r| r.id == *target) {
                                let (width, height) = (self.width, self.height);
                                if let Some(window) = self.window_mut(*target) {
                                    let (min_w, min_h) = MIN_CARD_SIZE;
                                    window.restore = None;
                                    window.bounds.width = (px + resize.grab_dx)
                                        .saturating_sub(window.bounds.x)
                                        .max(min_w)
                                        .min(width.saturating_sub(window.bounds.x));
                                    window.bounds.height = (py + resize.grab_dy)
                                        .saturating_sub(window.bounds.y)
                                        .max(min_h)
                                        .min(height.saturating_sub(window.bounds.y));
                                    changed = true;
                                }
                            }
                        }
                        (_, true, _, _) => {
                            if self.drag.map(|d| d.id) == Some(*target) {
                                self.drag = None;
                                // Dropped on the dock: tucked away (GFX-053).
                                let dock_top =
                                    self.height.saturating_sub(DOCK_HEIGHT + DOCK_MARGIN);
                                if py >= dock_top {
                                    self.tuck(*target);
                                    changed = true;
                                } else {
                                    changed |= self.snap_if_at_edge(*target, px, py);
                                }
                            }
                            if self.resize.map(|r| r.id) == Some(*target) {
                                self.resize = None;
                            }
                            if self.text_select == Some(*target) {
                                self.text_select = None;
                            }
                        }
                        _ => {}
                    }
                }
                Delivery::Capture { transition, .. } => {
                    if matches!(transition, input_types::PointerCapture::Lost) {
                        self.drag = None;
                        self.resize = None;
                    }
                }
                _ => {}
            }
        }
        (requests, changed)
    }

    /// A key for the focused app. Returns what the kernel must do, if
    /// anything, and whether the screen changed.
    pub fn handle_key(&mut self, byte: u8) -> (Option<DeskRequest>, bool) {
        // The palette first: it is modal while open, and Ctrl+Space or
        // Ctrl+P opens it from anywhere.
        if self.palette.is_some() {
            return self.handle_palette_key(byte);
        }
        if byte == KEY_CTRL_SPACE || byte == 0x10 {
            self.palette = Some(Palette::default());
            return (None, true);
        }
        // Launching is global. The first build only answered Ctrl+T with
        // nothing focused, so with a Notepad open the keystroke fell into
        // the Notepad and did nothing -- a shortcut that only works when
        // there is nothing to use it on. Ctrl+N stays the Notepad's own
        // "new document" while a Notepad is focused; from a Terminal, or
        // from the bare desk, it opens one.
        if byte == KEY_CTRL_TAB {
            return (None, self.cycle_focus());
        }
        // The window keys (GFX-062): Ctrl+Left/Right snap to a half,
        // Ctrl+Up fills the desk, Ctrl+Down goes back to the size before
        // any of those -- the keyboard's version of dragging to an edge.
        if let Some(how) = match byte {
            crate::notepad::KEY_CTRL_LEFT => Some(Some(PaletteAction::SnapLeft)),
            crate::notepad::KEY_CTRL_RIGHT => Some(Some(PaletteAction::SnapRight)),
            crate::notepad::KEY_CTRL_UP => Some(Some(PaletteAction::Maximise)),
            crate::notepad::KEY_CTRL_DOWN => Some(None),
            _ => None,
        } {
            let Some(id) = self.focus else {
                return (None, false);
            };
            let changed = match how {
                Some(how) => {
                    self.snap_focused(how);
                    true
                }
                None => self.unsnap(id),
            };
            return (None, changed);
        }
        if byte == crate::notepad::CTRL_T {
            self.launch(DeskApp::Terminal);
            return (None, true);
        }
        let Some(id) = self.focus else {
            // The bare desk. Ctrl+N opens a Notepad, and a printable key
            // opens the palette with that key already typed: the desk with
            // nothing on it is a search box, not a place keys vanish into.
            // (They used to: the kernel sent bare-desk keys through a second
            // entry point that knew three shortcuts and nothing else, so the
            // first Ctrl+Space of a session went to the invisible console.)
            if byte == crate::notepad::CTRL_N {
                self.launch(DeskApp::Notepad);
                return (None, true);
            }
            if (0x20..0x7f).contains(&byte) {
                self.palette = Some(Palette {
                    query: String::from(byte as char),
                    ..Palette::default()
                });
                return (None, true);
            }
            return (None, false);
        };
        if byte == crate::notepad::CTRL_N
            && self.window(id).map(|w| w.app) == Some(DeskApp::Terminal)
        {
            self.launch(DeskApp::Notepad);
            return (None, true);
        }
        self.handle_app_key(id, byte)
    }

    /// Open the Files card's selected entry in a fresh Notepad; the kernel
    /// reads the file and reports back to that window. Enter does this, and
    /// so does a click on the row that is already selected -- one click
    /// selects, the next opens, and there is no double-click clock to beat.
    fn open_files_selection(&mut self, id: ViewId) -> Option<DeskRequest> {
        let files = self.window_mut(id).and_then(|w| w.files_mut())?;
        let name = files.selected().map(|e| e.name.clone())?;
        let notepad_id = self.launch(DeskApp::Notepad);
        Some(DeskRequest::Io {
            id: notepad_id,
            effect: NotepadEffect::Open { path: name },
        })
    }

    /// A key for window `id`'s app.
    fn handle_app_key(&mut self, id: ViewId, byte: u8) -> (Option<DeskRequest>, bool) {
        let clipboard = self.clipboard.clone();
        let current_look = self
            .look_preview
            .clone()
            .unwrap_or_else(|| self.look.clone());
        let kept_row = LookView::row_of(&self.look);
        let Some(window) = self.window_mut(id) else {
            return (None, false);
        };
        match &mut window.state {
            AppState::Terminal => {
                if byte == crate::notepad::CTRL_W {
                    self.close(id);
                    return (None, true);
                }
                if byte == crate::notepad::KEY_PAGE_UP {
                    return (Some(DeskRequest::TerminalScroll { notches: 10 }), true);
                }
                if byte == crate::notepad::KEY_PAGE_DOWN {
                    return (Some(DeskRequest::TerminalScroll { notches: -10 }), true);
                }
                (Some(DeskRequest::Terminal(byte)), true)
            }
            AppState::Files(files) => {
                // A prompt owns the keys while it is open.
                if let Some(prompt) = files.prompt.clone() {
                    return Self::files_prompt_key(files, id, prompt, byte);
                }
                let in_bin = files.bin;
                match byte {
                    crate::notepad::KEY_UP => {
                        files.select(-1);
                        (None, true)
                    }
                    crate::notepad::KEY_DOWN => {
                        files.select(1);
                        (None, true)
                    }
                    crate::notepad::KEY_PAGE_UP => {
                        files.select(-10);
                        (None, true)
                    }
                    crate::notepad::KEY_PAGE_DOWN => {
                        files.select(10);
                        (None, true)
                    }
                    crate::notepad::KEY_HOME => {
                        files.selection = 0;
                        (None, true)
                    }
                    crate::notepad::KEY_END => {
                        files.selection = files.visible().len().saturating_sub(1);
                        (None, true)
                    }
                    CTRL_R => (Some(DeskRequest::ListFiles { id }), true),
                    crate::notepad::CTRL_S => {
                        files.by_name = !files.by_name;
                        files.selection = 0;
                        (None, true)
                    }
                    CTRL_B => {
                        files.bin = !files.bin;
                        files.selection = 0;
                        files.scroll = 0;
                        (None, true)
                    }
                    crate::notepad::CTRL_N if !in_bin => {
                        files.prompt = Some(FilesPrompt::New(String::new()));
                        (None, true)
                    }
                    CTRL_E if !in_bin => match files.selected() {
                        Some(entry) => {
                            files.prompt = Some(FilesPrompt::Rename(entry.name.clone()));
                            (None, true)
                        }
                        None => (None, false),
                    },
                    CTRL_K if !in_bin => match files.selected() {
                        Some(_) => {
                            files.prompt = Some(FilesPrompt::Tag(String::new()));
                            (None, true)
                        }
                        None => (None, false),
                    },
                    crate::notepad::KEY_DELETE => match files.selected() {
                        Some(entry) if in_bin => {
                            let _ = entry;
                            files.prompt = Some(FilesPrompt::ConfirmPurge);
                            (None, true)
                        }
                        Some(entry) => (
                            Some(DeskRequest::TrashFile {
                                id,
                                name: entry.name.clone(),
                                trashed: true,
                            }),
                            true,
                        ),
                        None => (None, false),
                    },
                    crate::notepad::BACKSPACE => {
                        if files.filter.pop().is_some() {
                            files.selection = 0;
                            (None, true)
                        } else {
                            (None, false)
                        }
                    }
                    crate::notepad::ESC => {
                        if files.filter.is_empty() {
                            (None, false)
                        } else {
                            files.filter.clear();
                            files.selection = 0;
                            (None, true)
                        }
                    }
                    crate::notepad::CTRL_W => {
                        self.close(id);
                        (None, true)
                    }
                    b'\n' | b'\r' if in_bin => match files.selected() {
                        Some(entry) => (
                            Some(DeskRequest::TrashFile {
                                id,
                                name: entry.name.clone(),
                                trashed: false,
                            }),
                            true,
                        ),
                        None => (None, false),
                    },
                    b'\n' | b'\r' => match self.open_files_selection(id) {
                        Some(request) => (Some(request), true),
                        None => (None, false),
                    },
                    // Typing is searching: the filter is the card's search
                    // line, and it matches tags as well as names.
                    0x20..=0x7E if files.filter.len() < 40 => {
                        files.filter.push(byte as char);
                        files.selection = 0;
                        files.scroll = 0;
                        (None, true)
                    }
                    _ => (None, false),
                }
            }
            AppState::Look(look) => {
                let current = current_look;
                match byte {
                    crate::notepad::KEY_UP | crate::notepad::KEY_DOWN => {
                        let row = if byte == crate::notepad::KEY_UP {
                            look.row.saturating_sub(1)
                        } else {
                            (look.row + 1).min(LookView::ROWS - 1)
                        };
                        look.row = row;
                        let preview = look.choice_at(&current);
                        self.look_preview = Some(preview);
                        (None, true)
                    }
                    b'\n' | b'\r' => {
                        // Keep: what is previewed becomes the look, on disk.
                        let chosen = self.look_preview.take().unwrap_or(current);
                        let text = chosen.to_text();
                        self.look = chosen;
                        (Some(DeskRequest::SaveLook { text }), true)
                    }
                    crate::notepad::ESC => {
                        look.row = kept_row;
                        self.look_preview = None;
                        (None, true)
                    }
                    crate::notepad::CTRL_W => {
                        self.look_preview = None;
                        self.close(id);
                        (None, true)
                    }
                    _ => (None, false),
                }
            }
            AppState::Notepad(notepad) => match if byte == crate::notepad::CTRL_V {
                notepad.paste(&clipboard)
            } else {
                notepad.handle_byte(byte)
            } {
                NotepadEffect::None => (None, false),
                NotepadEffect::Redraw => (None, true),
                NotepadEffect::ListFiles => (Some(DeskRequest::ListFiles { id }), true),
                NotepadEffect::Copy(text) => {
                    self.clipboard = text;
                    (None, true)
                }
                NotepadEffect::Version { path, index } => (
                    Some(DeskRequest::ReadVersion {
                        id,
                        name: path,
                        index,
                    }),
                    true,
                ),
                NotepadEffect::Close => {
                    self.close(id);
                    (None, true)
                }
                effect @ (NotepadEffect::Save { .. } | NotepadEffect::Open { .. }) => {
                    (Some(DeskRequest::Io { id, effect }), true)
                }
            },
        }
    }

    /// A key while a Files prompt is open.
    fn files_prompt_key(
        files: &mut FilesView,
        id: ViewId,
        prompt: FilesPrompt,
        byte: u8,
    ) -> (Option<DeskRequest>, bool) {
        let selected = files.selected().map(|e| e.name.clone());
        match (prompt, byte) {
            (_, crate::notepad::ESC) => {
                files.prompt = None;
                (None, true)
            }
            (FilesPrompt::ConfirmPurge, b'\n' | b'\r') => {
                files.prompt = None;
                match selected {
                    Some(name) => (Some(DeskRequest::PurgeFile { id, name }), true),
                    None => (None, true),
                }
            }
            (FilesPrompt::ConfirmPurge, _) => (None, false),
            (FilesPrompt::Tag(text), b'\n' | b'\r') => {
                files.prompt = None;
                let (add, remove) = parse_tag_edit(&text);
                match selected {
                    Some(name) if !(add.is_empty() && remove.is_empty()) => (
                        Some(DeskRequest::TagFile {
                            id,
                            name,
                            add,
                            remove,
                        }),
                        true,
                    ),
                    _ => (None, true),
                }
            }
            (FilesPrompt::New(name) | FilesPrompt::Rename(name), b'\n' | b'\r') => {
                let name = name.trim().to_string();
                let renaming = matches!(files.prompt, Some(FilesPrompt::Rename(_)));
                if name.is_empty() {
                    return (None, false);
                }
                files.prompt = None;
                if renaming {
                    let from = selected.unwrap_or_default();
                    if from == name {
                        return (None, true);
                    }
                    (Some(DeskRequest::RenameFile { id, from, to: name }), true)
                } else {
                    (Some(DeskRequest::CreateFile { id, name }), true)
                }
            }
            (
                FilesPrompt::New(mut text)
                | FilesPrompt::Rename(mut text)
                | FilesPrompt::Tag(mut text),
                byte,
            ) => {
                let kind = files.prompt.clone();
                match byte {
                    crate::notepad::BACKSPACE => {
                        text.pop();
                    }
                    0x20..=0x7E if text.len() < 64 => text.push(byte as char),
                    _ => return (None, false),
                }
                files.prompt = Some(match kind {
                    Some(FilesPrompt::Rename(_)) => FilesPrompt::Rename(text),
                    Some(FilesPrompt::Tag(_)) => FilesPrompt::Tag(text),
                    _ => FilesPrompt::New(text),
                });
                (None, true)
            }
        }
    }

    /// The kernel delivers a kept version to the Notepad that asked.
    pub fn version_loaded(
        &mut self,
        id: ViewId,
        index: usize,
        total: usize,
        content: Option<&str>,
        when: u64,
    ) {
        if let Some(notepad) = self.window_mut(id).and_then(|w| w.notepad_mut()) {
            notepad.show_version(index, total, content, when);
        }
    }

    /// The kernel reports a file operation's outcome, and the desk says so
    /// with a notice card.
    pub fn io_done(
        &mut self,
        id: ViewId,
        effect: &NotepadEffect,
        result: Result<Option<String>, String>,
        now: u64,
    ) -> Option<DeskRequest> {
        let quiet = if let Some(at) = self.quiet_saves.iter().position(|q| *q == id) {
            self.quiet_saves.remove(at);
            matches!(effect, NotepadEffect::Save { .. })
        } else {
            false
        };
        let follow_up = match (effect, &result) {
            (NotepadEffect::Save { path, .. }, Ok(_))
            | (NotepadEffect::Open { path }, Ok(Some(_))) => self.touch_recent(path),
            _ => None,
        };
        let (level, text) = match (effect, &result) {
            (NotepadEffect::Save { path, .. }, Ok(_)) => {
                (NoticeLevel::Info, alloc::format!("Saved {path}"))
            }
            (NotepadEffect::Open { path }, Ok(Some(_))) => {
                (NoticeLevel::Info, alloc::format!("Opened {path}"))
            }
            (NotepadEffect::Open { path }, Ok(None)) => {
                (NoticeLevel::Warning, alloc::format!("Not found: {path}"))
            }
            (_, Err(err)) => (NoticeLevel::Error, alloc::format!("{err}")),
            _ => (NoticeLevel::Info, String::new()),
        };
        if let Some(notepad) = self.window_mut(id).and_then(|w| w.notepad_mut()) {
            notepad.io_done(effect, result);
            if quiet {
                notepad.set_status("Saved itself");
            }
        }
        if !text.is_empty() && !quiet {
            self.notify(level, text, now);
        }
        follow_up
    }

    /// The window list for the compositor, top bar and dock included.
    ///
    /// `clock` is the top bar's right-hand text; `terminal` is the console
    /// as the kernel built it for the Terminal card, if one is open.
    pub fn windows(
        &mut self,
        clock: &str,
        caret_visible: bool,
        terminal: Option<&TerminalView>,
    ) -> Vec<DesktopWindow> {
        self.windows_at(clock, caret_visible, terminal, u64::MAX / 2, &[])
    }

    /// As `windows`, with the time (for notice expiry) and the workspace's
    /// own notices to show as cards.
    pub fn windows_at(
        &mut self,
        clock: &str,
        caret_visible: bool,
        terminal: Option<&TerminalView>,
        now: u64,
        shell_notices: &[ShellNotice],
    ) -> Vec<DesktopWindow> {
        self.notices.retain(|n| n.expires_at > now);
        self.last_shell_notices = shell_notices.to_vec();
        let mut out = Vec::with_capacity(self.windows.len() + 8);

        // Cards.
        let focus = self.focus;
        let kept_look = self.look.clone();
        let look_preview = self.look_preview.clone();
        for window in &mut self.windows {
            if window.tucked {
                continue;
            }
            let rows = Self::card_rows(window.bounds);
            let focused = focus == Some(window.id);
            let mut highlight = None;
            let mut selection = Vec::new();
            let (lines, title, footer, cursor) = match &mut window.state {
                AppState::Notepad(notepad) => {
                    let lines = notepad.viewport_lines(rows);
                    let cursor = notepad.viewport_cursor();
                    selection = notepad.viewport_selection(rows);
                    (lines, notepad.title(), notepad.footer(), cursor)
                }
                AppState::Files(files) => {
                    let visible = files.visible();
                    files.selection = files.selection.min(visible.len().saturating_sub(1));
                    if files.selection < files.scroll {
                        files.scroll = files.selection;
                    } else if rows > 0 && files.selection >= files.scroll + rows {
                        files.scroll = files.selection + 1 - rows;
                    }
                    let columns = window
                        .bounds
                        .width
                        .saturating_sub(services_gui_host::CARD_PADDING * 2)
                        / GLYPH_WIDTH;
                    let lines: Vec<String> = visible
                        .iter()
                        .skip(files.scroll)
                        .take(rows)
                        .map(|index| FilesView::line(&files.entries[*index], columns))
                        .collect();
                    if !visible.is_empty() {
                        highlight = Some(files.selection - files.scroll);
                    }
                    let title = if files.bin { "Files - Bin" } else { "Files" };
                    (lines, title.to_string(), files.footer(), None)
                }
                AppState::Terminal => {
                    let view = terminal.cloned().unwrap_or_default();
                    (view.lines, "Terminal".to_string(), view.status, view.cursor)
                }
                AppState::Look(look) => {
                    let preview = look_preview.clone().unwrap_or_else(|| kept_look.clone());
                    let lines = LookView::lines(&kept_look, &preview);
                    highlight = Some(LookView::line_of_row(look.row));
                    let footer = if look_preview.is_some() {
                        alloc::format!(
                            "Previewing {} + {}   Enter keeps it   Esc goes back",
                            preview.theme,
                            preview.accent
                        )
                    } else {
                        alloc::format!(
                            "{} + {}   arrows preview, and it changes as you go",
                            preview.theme,
                            preview.accent
                        )
                    };
                    (lines, "Look".to_string(), footer, None)
                }
            };
            let mut frame = ViewFrame::new(
                window.id,
                ViewKind::TextBuffer,
                0,
                ViewContent::text_buffer(lines),
                0,
            );
            frame.title = Some(title);
            if caret_visible && focused {
                if let Some((line, column)) = cursor {
                    frame.cursor = Some(CursorPosition::new(line, column));
                }
            }
            let mut card = DesktopWindow::card(frame, window.bounds)
                .with_z_index(window.z)
                .with_footer(Some(footer))
                .with_highlight(highlight)
                .with_selection(selection)
                .with_actions(
                    window
                        .actions()
                        .into_iter()
                        .map(|(label, _)| label)
                        .collect(),
                );
            if focused {
                card = card.focused();
            }
            out.push(card);
        }

        // The palette: a card near the top, above everything, without a
        // close glyph -- Esc or a click elsewhere closes it.
        if let Some(palette) = &self.palette {
            let has_window = focus.is_some();
            let has_notepad = self
                .windows
                .iter()
                .any(|w| focus == Some(w.id) && w.notepad().is_some());
            let matches = palette.matches(has_window, has_notepad, &self.recent);
            let rows = matches.len().min(PALETTE_MAX_ROWS);
            let mut lines = Vec::with_capacity(rows + 1);
            let inner = PALETTE_WIDTH.saturating_sub(services_gui_host::CARD_PADDING * 2)
                / services_gui_host::RASTER_CELL_WIDTH;
            for action in matches.iter().take(rows) {
                let label = action.label();
                let shortcut = action.shortcut();
                let pad =
                    inner.saturating_sub(label.chars().count() + shortcut.chars().count() + 2);
                let mut line = label.clone();
                for _ in 0..pad {
                    line.push(' ');
                }
                line.push_str("  ");
                line.push_str(shortcut);
                lines.push(line);
            }
            if matches.is_empty() {
                lines.push("No actions match".to_string());
            }
            let height = services_gui_host::CARD_HEADER_HEIGHT
                + services_gui_host::CARD_PADDING * 2
                + lines.len() * services_gui_host::CARD_LINE_HEIGHT
                + services_gui_host::CARD_FOOTER_HEIGHT;
            let mut frame = ViewFrame::new(
                self.palette_id,
                ViewKind::Panel,
                0,
                ViewContent::text_buffer(lines),
                0,
            );
            frame.title = Some(alloc::format!("> {}_", palette.query));
            let mut card = DesktopWindow::card(
                frame,
                RasterRect::new(
                    self.width.saturating_sub(PALETTE_WIDTH) / 2,
                    TOP_BAR_HEIGHT + 40,
                    PALETTE_WIDTH,
                    height,
                ),
            )
            .with_role(DesktopWindowRole::Palette)
            .with_z_index(usize::MAX / 2)
            .with_footer(Some("Enter runs   Esc closes   type to filter".to_string()))
            .with_highlight(
                (!matches.is_empty()).then_some(palette.selection.min(rows.saturating_sub(1))),
            );
            card.closable = false;
            card.focused = true;
            out.push(card);
        }

        // Notices: small cards at the top right, newest highest. The
        // workspace's own (a display switch, an error) and the desk's (a
        // save that landed) share one look.
        let mut all: Vec<ShellNotice> = shell_notices
            .iter()
            .filter(|n| !self.dismissed_shell.contains(n))
            .cloned()
            .collect();
        all.extend(self.notices.iter().map(|n| n.notice.clone()));
        let mut y = TOP_BAR_HEIGHT + NOTICE_MARGIN;
        let columns = (NOTICE_WIDTH - services_gui_host::CARD_PADDING * 2) / GLYPH_WIDTH;
        for (index, notice) in all.iter().rev().take(self.notice_ids.len()).enumerate() {
            let lines = wrap_words(&notice.text, columns);
            let height = services_gui_host::CARD_HEADER_HEIGHT
                + services_gui_host::CARD_PADDING * 2
                + services_gui_host::CARD_LINE_HEIGHT * lines.len().max(1);
            let mut frame = ViewFrame::new(
                self.notice_ids[index],
                ViewKind::Panel,
                0,
                ViewContent::text_buffer(lines),
                0,
            );
            frame.title = Some(notice.card_title());
            let mut card = DesktopWindow::card(
                frame,
                RasterRect::new(
                    self.width.saturating_sub(NOTICE_WIDTH + NOTICE_MARGIN),
                    y,
                    NOTICE_WIDTH,
                    height,
                ),
            )
            .with_role(DesktopWindowRole::Notification)
            .with_z_index(usize::MAX / 2 - 1);
            card.closable = false;
            out.push(card);
            y += height + NOTICE_MARGIN;
        }

        // Top bar: the focused app's name on the left, the clock on the right.
        // With nothing open the bar says how to begin, once; a desk with
        // no hint on it is a desk the first visitor stares at.
        let left = match (
            self.hovered_tile.and_then(|i| DeskApp::ALL.get(i)),
            self.focused_window(),
        ) {
            // The dock has no labels; the bar says what the tile under the
            // pointer is, and whether it is running (GFX-062).
            (Some(app), _) => {
                let running = self.windows.iter().filter(|w| w.app == *app).count();
                match running {
                    0 => alloc::format!("{}   -   click to open", app.name()),
                    1 => alloc::format!("{}   -   open", app.name()),
                    n => alloc::format!("{}   -   {n} open", app.name()),
                }
            }
            (None, Some(w)) => w.app.name().to_string(),
            (None, None) if self.windows.iter().all(|w| w.tucked) => {
                "PandaGen   -   type to search, Ctrl+Space for everything".to_string()
            }
            (None, None) => "PandaGen".to_string(),
        };
        let mut bar_frame = ViewFrame::new(
            self.top_bar_id,
            ViewKind::StatusLine,
            0,
            ViewContent::text_buffer(alloc::vec![clock.to_string()]),
            0,
        );
        bar_frame.title = Some(left);
        out.push(
            DesktopWindow::new(bar_frame, SurfaceRect::new(0, 0, 0, 0))
                .with_role(DesktopWindowRole::Status)
                .with_layer(DesktopWindowLayer::System)
                .with_style(WindowStyle::TopBar)
                .with_pixel_rect(RasterRect::new(0, 0, self.width, TOP_BAR_HEIGHT)),
        );

        // Dock: one tile per app, the running ones marked, the hovered one lit.
        let tabs: Vec<DesktopTab> = DeskApp::ALL
            .iter()
            .enumerate()
            .map(|(index, app)| {
                let mut tab =
                    DesktopTab::new(app.monogram(), self.windows.iter().any(|w| w.app == *app));
                tab.hovered = self.hovered_tile == Some(index);
                tab.tucked = self.windows.iter().any(|w| w.app == *app && w.tucked);
                tab
            })
            .collect();
        let count = tabs.len();
        let pill_width = count * services_gui_host::DOCK_TILE
            + count.saturating_sub(1) * services_gui_host::DOCK_TILE_GAP
            + 32;
        let dock_frame = ViewFrame::new(
            self.dock_id,
            ViewKind::Panel,
            0,
            ViewContent::text_buffer(Vec::new()),
            0,
        );
        out.push(
            DesktopWindow::new(dock_frame, SurfaceRect::new(0, 0, 0, 0))
                .with_role(DesktopWindowRole::Launcher)
                .with_layer(DesktopWindowLayer::System)
                .with_style(WindowStyle::Dock)
                .with_tabs(tabs)
                .with_pixel_rect(RasterRect::new(
                    self.width.saturating_sub(pill_width) / 2,
                    self.height.saturating_sub(DOCK_HEIGHT + DOCK_MARGIN),
                    pill_width,
                    DOCK_HEIGHT,
                )),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use input_types::{ButtonState, Modifiers, PointerButtons, PointerEvent, PointerPosition};
    use services_gui_host::{Compositor, DesktopInputRouter};

    fn pointer(kind: PointerEventKind, x: i32, y: i32, buttons: PointerButtons) -> PointerEvent {
        PointerEvent::new(kind, PointerPosition::new(x, y), buttons, Modifiers::NONE)
    }

    fn press(x: i32, y: i32) -> PointerEvent {
        pointer(
            PointerEventKind::Button {
                button: PointerButton::Primary,
                state: ButtonState::Pressed,
            },
            x,
            y,
            PointerButtons::PRIMARY,
        )
    }

    fn release(x: i32, y: i32) -> PointerEvent {
        pointer(
            PointerEventKind::Button {
                button: PointerButton::Primary,
                state: ButtonState::Released,
            },
            x,
            y,
            PointerButtons::none(),
        )
    }

    fn moved(x: i32, y: i32, buttons: PointerButtons) -> PointerEvent {
        pointer(
            PointerEventKind::Move {
                delta: input_types::PointerDelta::new(0, 0),
            },
            x,
            y,
            buttons,
        )
    }

    fn wheel(x: i32, y: i32, dy: i32) -> PointerEvent {
        pointer(
            PointerEventKind::Wheel { dx: 0, dy },
            x,
            y,
            PointerButtons::none(),
        )
    }

    /// Drive one event through the real router against the desk's own
    /// window list, then apply the deliveries.
    fn route(desk: &mut Desk, router: &mut DesktopInputRouter, event: PointerEvent) -> bool {
        let compositor = Compositor::new();
        let windows = desk.windows("00:00", true, None);
        let deliveries = router.route(&compositor, &windows, event);
        desk.handle_deliveries(&deliveries)
    }

    fn drag(desk: &mut Desk, router: &mut DesktopInputRouter, from: (i32, i32), to: (i32, i32)) {
        route(desk, router, press(from.0, from.1));
        route(desk, router, moved(to.0, to.1, PointerButtons::PRIMARY));
        route(desk, router, release(to.0, to.1));
    }

    #[test]
    fn ctrl_n_and_ctrl_t_on_the_bare_desk_open_apps() {
        let mut desk = Desk::new(1280, 800);
        assert!(desk.handle_key(crate::notepad::CTRL_N).1);
        assert!(desk.handle_key(crate::notepad::CTRL_T).1);
        assert_eq!(desk.window_count(), 2);
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );
        assert!(desk.terminal_rows().is_some());
    }

    /// The kernel used to route bare-desk keys through a second entry point
    /// that knew three shortcuts, so the first Ctrl+Space of a session --
    /// before any card had focus -- went to the invisible console, and so
    /// did everything typed after it. There is one entry point now, and a
    /// printable key on the bare desk is the start of a palette search.
    #[test]
    fn typing_on_the_bare_desk_opens_the_palette_with_the_key_typed() {
        let mut desk = Desk::new(1280, 800);
        assert!(!desk.palette_open());
        let (request, changed) = desk.handle_key(b't');
        assert!(request.is_none() && changed && desk.palette_open());
        for byte in b"erm" {
            desk.handle_key(*byte);
        }
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        assert!(palette.frame.title.as_deref().unwrap().contains("term"));
        desk.handle_key(b'\n');
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );

        // A control byte the bare desk has no use for is not taken.
        let mut desk = Desk::new(1280, 800);
        assert_eq!(desk.handle_key(0x01), (None, false));
        assert!(!desk.palette_open());
    }

    /// The first build answered the launch shortcuts only with nothing
    /// focused, so with a Notepad open Ctrl+T fell into the Notepad and did
    /// nothing. Launching is global; Ctrl+N is the Notepad's own inside one.
    #[test]
    fn launch_shortcuts_work_whatever_is_focused() {
        let mut desk = Desk::new(1280, 800);
        desk.handle_key(crate::notepad::CTRL_N);
        assert_eq!(desk.focused_window().map(|w| w.app), Some(DeskApp::Notepad));

        // Ctrl+T from inside the Notepad opens a Terminal.
        let (_, changed) = desk.handle_key(crate::notepad::CTRL_T);
        assert!(changed);
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );

        // Ctrl+N from the Terminal opens a second Notepad...
        desk.handle_key(crate::notepad::CTRL_N);
        assert_eq!(desk.window_count(), 3);
        assert_eq!(desk.focused_window().map(|w| w.app), Some(DeskApp::Notepad));

        // ...while inside a Notepad it is "new document", not a new window.
        for byte in b"draft" {
            desk.handle_key(*byte);
        }
        desk.handle_key(crate::notepad::CTRL_N);
        desk.handle_key(crate::notepad::CTRL_N); // confirm the discard
        assert_eq!(desk.window_count(), 3);
        assert_eq!(
            desk.focused_window()
                .and_then(|w| w.notepad())
                .map(|n| n.content()),
            Some(String::new())
        );
    }

    #[test]
    fn the_palette_lists_filters_and_runs_actions() {
        let mut desk = Desk::new(1280, 800);
        // Ctrl+Space opens it from the bare desk; it is a card.
        let (_, changed) = desk.handle_key(KEY_CTRL_SPACE);
        assert!(changed && desk.palette_open());
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        assert_eq!(palette.style, WindowStyle::Card);
        // With nothing open, window-only actions are not offered.
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(listed.iter().any(|l| l.starts_with("New Notepad window")));
        assert!(!listed.iter().any(|l| l.starts_with("Close window")));

        // Typing filters; Enter runs the selected row.
        for byte in b"term" {
            desk.handle_key(*byte);
        }
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        assert!(palette.frame.title.as_deref().unwrap().contains("term"));
        let (request, _) = desk.handle_key(b'\n');
        assert!(request.is_none());
        assert!(!desk.palette_open());
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );

        // Esc closes without running.
        desk.handle_key(0x10);
        assert!(desk.palette_open());
        desk.handle_key(crate::notepad::ESC);
        assert!(!desk.palette_open());

        // "Switch to the text console" is a request to the kernel.
        desk.handle_key(KEY_CTRL_SPACE);
        for byte in b"text" {
            desk.handle_key(*byte);
        }
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(request, Some(DeskRequest::TextConsole));
    }

    #[test]
    fn dropping_a_card_on_the_dock_tucks_it_and_the_tile_brings_it_back() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        let bounds = desk.window(id).unwrap().bounds;
        drag(
            &mut desk,
            &mut router,
            ((bounds.x + 100) as i32, (bounds.y + 10) as i32),
            (640, 790),
        );
        assert!(desk.window(id).unwrap().tucked);
        assert_eq!(desk.focus(), None);
        let windows = desk.windows("", true, None);
        assert!(
            !windows.iter().any(|w| w.frame.view_id == id),
            "a tucked card is still drawn"
        );
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert!(dock.tabs[0].active && dock.tabs[0].tucked);

        // Ctrl+Tab skips it; the dock tile brings it back.
        assert!(!desk.cycle_focus());
        let pill = dock.bounds();
        let first_x = (pill.x + (pill.width - 184) / 2 + 20) as i32;
        let y = (pill.y + pill.height / 2) as i32;
        route(&mut desk, &mut router, press(first_x, y));
        assert!(!desk.window(id).unwrap().tucked);
        assert_eq!(desk.focus(), Some(id));
    }

    #[test]
    fn files_lists_the_filesystem_and_enter_opens_in_a_notepad() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let windows = desk.windows("", true, None);
        let pill = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap()
            .bounds();
        let files_x = (pill.x + (pill.width - 184) / 2 + 20 + 48) as i32;
        let y = (pill.y + pill.height / 2) as i32;
        let compositor = Compositor::new();
        let deliveries = router.route(&compositor, &windows, press(files_x, y));
        let (requests, _) = desk.handle_deliveries_with_requests(&deliveries);
        let id = desk.focused_window().map(|w| w.id).unwrap();
        assert_eq!(requests, alloc::vec![DeskRequest::ListFiles { id }]);

        desk.files_listed(
            id,
            alloc::vec![FileEntry::named("a.txt"), FileEntry::named("b.txt")],
        );
        desk.handle_key(crate::notepad::KEY_DOWN);
        let (request, _) = desk.handle_key(b'\n');
        let notepad_id = desk.focused_window().map(|w| w.id).unwrap();
        assert_ne!(notepad_id, id);
        assert_eq!(
            request,
            Some(DeskRequest::Io {
                id: notepad_id,
                effect: NotepadEffect::Open {
                    path: "b.txt".to_string()
                }
            })
        );
        assert_eq!(desk.window_count(), 2);
    }

    #[test]
    fn notices_wrap_to_the_card_instead_of_running_off_it() {
        assert_eq!(
            wrap_words("Saved n.txt", 40),
            alloc::vec!["Saved n.txt".to_string()]
        );
        assert_eq!(
            wrap_words("Unknown command: files. Type 'help' for help.", 20),
            alloc::vec![
                "Unknown command:".to_string(),
                "files. Type 'help'".to_string(),
                "for help.".to_string()
            ]
        );
        assert_eq!(
            wrap_words("abcdefghij", 4),
            alloc::vec!["abcd".to_string(), "efgh".to_string(), "ij".to_string()]
        );
        assert_eq!(wrap_words("", 4), alloc::vec![String::new()]);

        let mut desk = Desk::new(1280, 800);
        let shell = alloc::vec![ShellNotice::new(
            NoticeLevel::Warning,
            "Unknown command: files. Type 'help' for help.",
        )];
        let windows = desk.windows_at("", true, None, 5000, &shell);
        let notice = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Notification)
            .unwrap();
        let lines = match &notice.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(lines.len() >= 2, "{lines:?}");
        assert!(lines
            .iter()
            .all(|l| l.len() * GLYPH_WIDTH <= NOTICE_WIDTH - 16));
        assert_eq!(
            notice.bounds().height,
            services_gui_host::CARD_HEADER_HEIGHT
                + 16
                + services_gui_host::CARD_LINE_HEIGHT * lines.len()
        );
    }

    #[test]
    fn in_files_one_click_selects_and_a_click_on_the_selection_opens() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Files);
        desk.files_listed(
            id,
            alloc::vec![FileEntry::named("a.txt"), FileEntry::named("b.txt")],
        );
        let bounds = desk.window(id).unwrap().bounds;
        let text_x = (bounds.x + services_gui_host::CARD_PADDING + 4) as i32;
        let row_y = |row: usize| {
            (bounds.y
                + services_gui_host::CARD_HEADER_HEIGHT
                + services_gui_host::CARD_PADDING
                + services_gui_host::CARD_LINE_HEIGHT * row
                + 4) as i32
        };
        let compositor = Compositor::new();
        let windows = desk.windows("", true, None);
        let deliveries = router.route(&compositor, &windows, press(text_x, row_y(1)));
        let (requests, _) = desk.handle_deliveries_with_requests(&deliveries);
        assert!(requests.is_empty(), "the first click only selects");
        route(&mut desk, &mut router, release(text_x, row_y(1)));
        assert_eq!(
            desk.window(id).unwrap().files().map(|f| f.selection),
            Some(1)
        );

        let windows = desk.windows("", true, None);
        let deliveries = router.route(&compositor, &windows, press(text_x, row_y(1)));
        let (requests, _) = desk.handle_deliveries_with_requests(&deliveries);
        let notepad_id = desk.focused_window().map(|w| w.id).unwrap();
        assert_eq!(
            requests,
            alloc::vec![DeskRequest::Io {
                id: notepad_id,
                effect: NotepadEffect::Open {
                    path: "b.txt".to_string()
                }
            }]
        );
    }

    #[test]
    fn the_clipboard_is_the_desks_and_every_notepad_shares_it() {
        let mut desk = Desk::new(1280, 800);
        let first = desk.launch(DeskApp::Notepad);
        for byte in b"copy me" {
            desk.handle_key(*byte);
        }
        desk.handle_key(crate::notepad::CTRL_A);
        let (request, changed) = desk.handle_key(crate::notepad::CTRL_C);
        assert!(request.is_none() && changed);
        assert_eq!(desk.clipboard, "copy me");

        let second = desk.launch(DeskApp::Notepad);
        assert_ne!(first, second);
        desk.handle_key(crate::notepad::CTRL_V);
        assert_eq!(
            desk.window(second).unwrap().notepad().unwrap().content(),
            "copy me"
        );

        // The selection is drawn: the card carries spans for the compositor.
        desk.handle_key(crate::notepad::CTRL_A);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == second).unwrap();
        assert_eq!(card.selection_spans, alloc::vec![(0, 0, 7)]);
        let other = windows.iter().find(|w| w.frame.view_id == first).unwrap();
        assert!(
            other.selection_spans.is_empty() || other.selection_spans == alloc::vec![(0, 0, 7)]
        );
    }

    #[test]
    fn dragging_across_a_notepads_text_selects_it() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        for byte in b"hello world" {
            desk.handle_key(*byte);
        }
        let bounds = desk.window(id).unwrap().bounds;
        let text_x = |col: usize| {
            (bounds.x
                + services_gui_host::CARD_PADDING
                + col * services_gui_host::RASTER_CELL_WIDTH
                + 2) as i32
        };
        let y = (bounds.y
            + services_gui_host::CARD_HEADER_HEIGHT
            + services_gui_host::CARD_PADDING
            + 4) as i32;
        drag(&mut desk, &mut router, (text_x(6), y), (text_x(11), y));
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert_eq!(notepad.selected_text().as_deref(), Some("world"));

        // After the release, moving the pointer does not grow it.
        route(
            &mut desk,
            &mut router,
            moved(text_x(0), y, PointerButtons::none()),
        );
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert_eq!(notepad.selected_text().as_deref(), Some("world"));

        // Editing actions are palette rows only over a Notepad.
        desk.handle_key(KEY_CTRL_SPACE);
        for byte in b"find" {
            desk.handle_key(*byte);
        }
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(
            listed.iter().any(|l| l.starts_with("Find...")),
            "{listed:?}"
        );
        desk.handle_key(b'\n');
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert!(
            notepad.footer().starts_with("Find: world_"),
            "{}",
            notepad.footer()
        );
    }

    #[test]
    fn the_terminal_card_scrolls_the_workspaces_scrollback_by_wheel_and_page_keys() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Terminal);
        let bounds = desk.window(id).unwrap().bounds;
        let (x, y) = ((bounds.x + 100) as i32, (bounds.y + 100) as i32);
        let compositor = Compositor::new();
        let windows = desk.windows("", true, None);
        let deliveries = router.route(&compositor, &windows, wheel(x, y, 2));
        let (requests, changed) = desk.handle_deliveries_with_requests(&deliveries);
        assert!(changed);
        assert_eq!(
            requests,
            alloc::vec![DeskRequest::TerminalScroll { notches: 6 }]
        );
        assert_eq!(
            desk.handle_key(crate::notepad::KEY_PAGE_UP).0,
            Some(DeskRequest::TerminalScroll { notches: 10 })
        );
        assert_eq!(
            desk.handle_key(crate::notepad::KEY_PAGE_DOWN).0,
            Some(DeskRequest::TerminalScroll { notches: -10 })
        );
    }

    #[test]
    fn the_look_card_previews_as_you_move_keeps_on_enter_and_reverts_on_esc() {
        let mut desk = Desk::new(1280, 800);
        assert_eq!(desk.theme(), Theme::DESK);
        let windows = desk.windows("", true, None);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        assert!(bar.frame.title.as_deref().unwrap().contains("Ctrl+Space"));

        // The palette opens Look.
        desk.handle_key(KEY_CTRL_SPACE);
        for byte in b"look" {
            desk.handle_key(*byte);
        }
        desk.handle_key(b'\n');
        let id = desk.focused_window().map(|w| w.id).unwrap();
        assert_eq!(desk.window(id).unwrap().app, DeskApp::Look);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(card.actions, alloc::vec!["Keep", "Revert"]);
        assert_eq!(
            card.highlight_line,
            Some(1),
            "opens on the kept theme, Dusk"
        );

        // Down twice: Ember, previewed at once, not kept.
        desk.handle_key(crate::notepad::KEY_DOWN);
        desk.handle_key(crate::notepad::KEY_DOWN);
        assert_eq!(desk.theme(), Theme::EMBER.with_accent(Theme::ACCENTS[0].1));
        assert_eq!(desk.look().theme, "Dusk");
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert!(card
            .footer
            .as_deref()
            .unwrap()
            .starts_with("Previewing Ember + Mint"));
        // Esc reverts.
        desk.handle_key(crate::notepad::ESC);
        assert_eq!(desk.theme(), Theme::DESK);

        // Down to Ember again, then on to the accents: Sky. Enter keeps and
        // asks the kernel to write it.
        for _ in 0..(LookView::THEME_ROWS + 1) {
            desk.handle_key(crate::notepad::KEY_DOWN);
        }
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::SaveLook {
                text: "theme=Mono\naccent=Sky\n".to_string()
            })
        );
        assert_eq!(desk.look().theme, "Mono");
        assert_eq!(desk.theme(), Theme::MONO.with_accent(Theme::ACCENTS[1].1));

        // What was written reads back; nonsense reads as the defaults.
        let mut fresh = Desk::new(1280, 800);
        fresh.apply_look(Some("theme=mono\naccent=SKY\n"));
        assert_eq!(fresh.look(), desk.look());
        fresh.apply_look(Some("theme=plaid\n"));
        assert_eq!(fresh.look(), &LookChoice::default());
        fresh.apply_look(None);
        assert_eq!(fresh.look(), &LookChoice::default());

        // Closing the card drops any preview.
        desk.handle_key(crate::notepad::KEY_UP);
        assert_ne!(desk.theme(), desk.look().theme());
        desk.close(id);
        assert_eq!(desk.theme(), desk.look().theme());

        // A card with focus names itself in the bar.
        desk.launch(DeskApp::Notepad);
        let windows = desk.windows("", true, None);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        assert_eq!(bar.frame.title.as_deref(), Some("Notepad"));
    }

    #[test]
    fn a_still_document_saves_itself_quietly_and_the_palette_remembers_it() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        desk.window_mut(id)
            .unwrap()
            .notepad_mut()
            .unwrap()
            .load(Some("memo".to_string()), "");
        desk.handle_key(b'h');
        assert!(
            desk.tick(1_000).is_empty(),
            "the first tick only starts the clock"
        );
        assert!(desk
            .tick(1_000 + crate::notepad::AUTOSAVE_IDLE_TICKS - 1)
            .is_empty());
        let requests = desk.tick(1_000 + crate::notepad::AUTOSAVE_IDLE_TICKS);
        let effect = NotepadEffect::Save {
            path: "memo".to_string(),
            content: "h".to_string(),
        };
        assert_eq!(
            requests,
            alloc::vec![DeskRequest::Io {
                id,
                effect: effect.clone()
            }]
        );
        // It lands quietly: no notice, a status, and the file is recent.
        let follow_up = desk.io_done(id, &effect, Ok(None), 2_000);
        assert_eq!(
            follow_up,
            Some(DeskRequest::SaveRecent {
                text: "memo\n".to_string()
            })
        );
        let windows = desk.windows_at("", true, None, 2_000, &[]);
        assert!(!windows
            .iter()
            .any(|w| w.role == DesktopWindowRole::Notification));
        assert!(desk
            .window(id)
            .unwrap()
            .notepad()
            .unwrap()
            .footer()
            .contains("Saved itself"));
        assert_eq!(desk.recent(), &["memo".to_string()]);

        // A save the person asked for still says so.
        let follow_up = desk.io_done(id, &effect, Ok(None), 3_000);
        assert_eq!(follow_up, None, "already at the front");
        let windows = desk.windows_at("", true, None, 3_000, &[]);
        assert!(windows
            .iter()
            .any(|w| w.role == DesktopWindowRole::Notification));

        // The palette lists it first, and Enter opens it in a new Notepad.
        desk.handle_key(KEY_CTRL_SPACE);
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(listed[0].starts_with("Open memo"), "{listed:?}");
        let (request, _) = desk.handle_key(b'\n');
        let new_id = desk.focused_window().map(|w| w.id).unwrap();
        assert_ne!(new_id, id);
        assert_eq!(
            request,
            Some(DeskRequest::Io {
                id: new_id,
                effect: NotepadEffect::Open {
                    path: "memo".to_string()
                }
            })
        );

        // The list reads back, bounded and newest first.
        let mut fresh = Desk::new(1280, 800);
        fresh.apply_recent(Some("a\nb\nc\nd\ne\nf\ng\n"));
        assert_eq!(fresh.recent().len(), RECENT_FILES);
        assert_eq!(fresh.recent()[0], "a");
    }

    #[test]
    fn the_palette_searches_inside_files_and_enter_opens_the_file_on_the_line() {
        let mut desk = Desk::new(1280, 800);
        desk.handle_key(KEY_CTRL_SPACE);
        // One letter is not a search; two are.
        assert_eq!(desk.handle_key(b'q').0, None);
        assert_eq!(
            desk.handle_key(b'u').0,
            Some(DeskRequest::SearchFiles {
                query: "qu".to_string()
            })
        );
        desk.search_results(
            "qu",
            alloc::vec![("memo".to_string(), "  the quick fox".to_string())],
        );
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(listed[0].starts_with("memo: the quick fox"), "{listed:?}");
        assert!(listed[0].trim_end().ends_with("in file"), "{listed:?}");

        // A stale answer is not shown once the query moves on.
        desk.handle_key(b'i');
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(!listed.iter().any(|l| l.starts_with("memo:")), "{listed:?}");
        desk.search_results(
            "qui",
            alloc::vec![("memo".to_string(), "the quick fox".to_string())],
        );

        // Enter opens the file in a Notepad that will find the query.
        let (request, _) = desk.handle_key(b'\n');
        let id = desk.focused_window().map(|w| w.id).unwrap();
        assert_eq!(
            request,
            Some(DeskRequest::Io {
                id,
                effect: NotepadEffect::Open {
                    path: "memo".to_string()
                }
            })
        );
        desk.io_done(
            id,
            &NotepadEffect::Open {
                path: "memo".to_string(),
            },
            Ok(Some("slow\nthe quick fox\n".to_string())),
            100,
        );
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert_eq!(notepad.selected_text().as_deref(), Some("qui"));
        assert_eq!(notepad.cursor().row, 1);
    }

    #[test]
    fn ctrl_arrows_snap_fill_and_put_a_window_back() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        let before = desk.window(id).unwrap().bounds;
        let area = desk.work_area();
        assert!(desk.handle_key(crate::notepad::KEY_CTRL_LEFT).1);
        let bounds = desk.window(id).unwrap().bounds;
        assert_eq!((bounds.x, bounds.width), (area.x, area.width / 2));
        desk.handle_key(crate::notepad::KEY_CTRL_RIGHT);
        let bounds = desk.window(id).unwrap().bounds;
        assert_eq!(bounds.x, area.x + area.width / 2);
        desk.handle_key(crate::notepad::KEY_CTRL_UP);
        assert_eq!(desk.window(id).unwrap().bounds, area);
        // Ctrl+Down: the size before the first snap, not the last one.
        desk.handle_key(crate::notepad::KEY_CTRL_DOWN);
        assert_eq!(desk.window(id).unwrap().bounds, before);
        // Nothing snapped, nothing to put back.
        assert!(!desk.handle_key(crate::notepad::KEY_CTRL_DOWN).1);
        // With nothing focused the keys do nothing.
        desk.close(id);
        assert_eq!(
            desk.handle_key(crate::notepad::KEY_CTRL_LEFT),
            (None, false)
        );
    }

    #[test]
    fn the_bar_names_the_dock_tile_under_the_pointer() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        desk.launch(DeskApp::Notepad);
        let windows = desk.windows("", true, None);
        let pill = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap()
            .bounds();
        let first_x = (pill.x + (pill.width - 184) / 2 + 20) as i32;
        let y = (pill.y + pill.height / 2) as i32;
        route(
            &mut desk,
            &mut router,
            moved(first_x, y, PointerButtons::none()),
        );
        let bar_title = |desk: &mut Desk| {
            desk.windows("", true, None)
                .iter()
                .find(|w| w.style == WindowStyle::TopBar)
                .and_then(|w| w.frame.title.clone())
                .unwrap()
        };
        assert_eq!(bar_title(&mut desk), "Notepad   -   open");
        route(
            &mut desk,
            &mut router,
            moved(first_x + 48, y, PointerButtons::none()),
        );
        assert_eq!(bar_title(&mut desk), "Files   -   click to open");
        // Off the dock: the focused card's name again.
        route(
            &mut desk,
            &mut router,
            moved(640, 400, PointerButtons::none()),
        );
        assert_eq!(bar_title(&mut desk), "Notepad");
    }

    #[test]
    fn the_pointer_alone_runs_the_palette_dismisses_notices_and_picks_a_look() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let compositor = Compositor::new();
        // The bar opens the palette.
        route(&mut desk, &mut router, press(300, 14));
        route(&mut desk, &mut router, release(300, 14));
        assert!(desk.palette_open());
        // Row 1 is "New Terminal" on the bare desk; a click runs it.
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let (ox, oy, pitch) = palette.card_text_origin();
        let deliveries = router.route(
            &compositor,
            &windows,
            press((ox + 4) as i32, (oy + pitch + 4) as i32),
        );
        desk.handle_deliveries_with_requests(&deliveries);
        route(
            &mut desk,
            &mut router,
            release((ox + 4) as i32, (oy + pitch + 4) as i32),
        );
        assert!(!desk.palette_open());
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );

        // A notice goes when clicked.
        desk.notify(NoticeLevel::Info, "hello", 100);
        let windows = desk.windows_at("", true, None, 100, &[]);
        let notice = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Notification)
            .unwrap();
        let b = notice.bounds();
        let deliveries = router.route(
            &compositor,
            &windows,
            press((b.x + 20) as i32, (b.y + 40) as i32),
        );
        desk.handle_deliveries_with_requests(&deliveries);
        route(
            &mut desk,
            &mut router,
            release((b.x + 20) as i32, (b.y + 40) as i32),
        );
        let windows = desk.windows_at("", true, None, 101, &[]);
        assert!(!windows
            .iter()
            .any(|w| w.role == DesktopWindowRole::Notification));

        // Look: click a theme row to preview, click it again to keep.
        let id = desk.launch(DeskApp::Look);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let (ox, oy, pitch) = card.card_text_origin();
        let ember_line = LookView::line_of_row(2);
        let (cx, cy) = ((ox + 40) as i32, (oy + pitch * ember_line + 4) as i32);
        let click = press(cx, cy);
        let deliveries = router.route(&compositor, &windows, click);
        desk.handle_deliveries_with_requests(&deliveries);
        route(&mut desk, &mut router, release(cx, cy));
        assert_eq!(desk.theme(), Theme::EMBER.with_accent(Theme::ACCENTS[0].1));
        assert_eq!(desk.look().theme, "Dusk");
        let windows = desk.windows("", true, None);
        let deliveries = router.route(&compositor, &windows, click);
        let (requests, _) = desk.handle_deliveries_with_requests(&deliveries);
        assert_eq!(desk.look().theme, "Ember");
        assert!(matches!(
            requests.first(),
            Some(DeskRequest::SaveLook { .. })
        ));
        // A heading is no row.
        let heading = press((ox + 4) as i32, (oy + 4) as i32);
        let windows = desk.windows("", true, None);
        let deliveries = router.route(&compositor, &windows, heading);
        let (requests, _) = desk.handle_deliveries_with_requests(&deliveries);
        assert!(requests.is_empty());
        assert_eq!(desk.look().theme, "Ember");
    }

    #[test]
    fn notepad_chips_follow_its_mode() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        desk.handle_key(crate::notepad::CTRL_F);
        assert_eq!(desk.window(id).unwrap().actions()[0].0, "Next");
        desk.handle_key(crate::notepad::ESC);
        desk.handle_key(crate::notepad::CTRL_O);
        assert_eq!(desk.window(id).unwrap().actions()[0].0, "Cancel");
        desk.handle_key(crate::notepad::ESC);
        desk.window_mut(id)
            .unwrap()
            .notepad_mut()
            .unwrap()
            .load(Some("m".to_string()), "x");
        desk.handle_key(crate::notepad::CTRL_Y);
        desk.version_loaded(id, 0, 1, Some("old"), 0);
        let labels: Vec<String> = desk
            .window(id)
            .unwrap()
            .actions()
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        assert_eq!(labels, alloc::vec!["Older", "Newer", "Restore", "Back"]);
        // The Back chip is Esc.
        let (_, byte) = desk.window(id).unwrap().actions()[3].clone();
        desk.handle_app_key(id, byte);
        assert!(!desk
            .window(id)
            .unwrap()
            .notepad()
            .unwrap()
            .browsing_history());
    }

    #[test]
    fn files_hides_the_systems_dot_names() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Files);
        desk.files_listed(
            id,
            alloc::vec![FileEntry::named(".look"), FileEntry::named("memo")],
        );
        let files = desk.window(id).unwrap().files().unwrap();
        assert_eq!(files.visible().len(), 1);
        assert_eq!(files.selected().map(|e| e.name.as_str()), Some("memo"));
    }

    #[test]
    fn files_shows_tags_type_size_and_time_and_narrows_to_the_name() {
        let entry = FileEntry {
            name: "notes".to_string(),
            size: 1536,
            kind: "file",
            modified_at: 1_789_946_225,
            schema: Some("text/plain".to_string()),
            tags: alloc::vec!["work".to_string()],
            trashed: false,
            versions: 2,
        };
        let line = FilesView::line(&entry, 90);
        assert!(line.starts_with("notes"), "{line}");
        assert!(line.contains("#work") && line.contains("Text"), "{line}");
        assert!(
            line.contains("1.5 KB") && line.contains("2026-09-20 23:17"),
            "{line}"
        );
        let narrow = FilesView::line(&entry, 40);
        assert!(
            narrow.contains("1.5 KB") && !narrow.contains("2026"),
            "{narrow}"
        );
        assert_eq!(FilesView::line(&entry, 3), "not");
        assert_eq!(format_size(12), "12 B");
        assert_eq!(FileEntry::named("x").kind_label(), "File");

        let mut files = FilesView::default();
        files.entries = alloc::vec![entry];
        files.loaded = true;
        let footer = files.footer();
        assert!(
            footer.contains("Text") && footer.contains("#work"),
            "{footer}"
        );
        assert!(
            footer.contains("2 earlier") && footer.contains("written 2026"),
            "{footer}"
        );
        files.entries.clear();
        assert!(files.footer().contains("start typing"));
    }

    #[test]
    fn typing_in_files_filters_by_name_or_tag_and_recency_sorts_first() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Files);
        let mut old = FileEntry::named("alpha");
        old.modified_at = 100;
        let mut new = FileEntry::named("zeta");
        new.modified_at = 900;
        new.tags = alloc::vec!["work".to_string()];
        let mut binned = FileEntry::named("gone");
        binned.trashed = true;
        desk.files_listed(id, alloc::vec![old, new, binned]);
        let names = |desk: &Desk| -> Vec<String> {
            let files = desk.window(id).unwrap().files().unwrap();
            files
                .visible()
                .iter()
                .map(|i| files.entries[*i].name.clone())
                .collect()
        };
        // Recent first, the bin hidden.
        assert_eq!(
            names(&desk),
            alloc::vec!["zeta".to_string(), "alpha".to_string()]
        );
        // Ctrl+S: by name.
        desk.handle_key(crate::notepad::CTRL_S);
        assert_eq!(
            names(&desk),
            alloc::vec!["alpha".to_string(), "zeta".to_string()]
        );
        // Typing filters by tag too; Backspace and Esc undo it.
        for byte in b"wor" {
            desk.handle_key(*byte);
        }
        assert_eq!(names(&desk), alloc::vec!["zeta".to_string()]);
        let footer = desk.window(id).unwrap().files().unwrap().footer();
        assert!(footer.starts_with("Filter: wor_   1 of 2"), "{footer}");
        desk.handle_key(crate::notepad::ESC);
        assert_eq!(names(&desk).len(), 2);
        // The bin shows only what is in it, with its own chips.
        desk.handle_key(CTRL_B);
        assert_eq!(names(&desk), alloc::vec!["gone".to_string()]);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(card.frame.title.as_deref(), Some("Files - Bin"));
        assert_eq!(
            card.actions,
            alloc::vec!["Restore", "Remove", "A-Z", "Files"]
        );
        // Enter restores; Delete asks, then removes for good.
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::TrashFile {
                id,
                name: "gone".to_string(),
                trashed: false
            })
        );
        desk.handle_key(crate::notepad::KEY_DELETE);
        assert!(desk
            .window(id)
            .unwrap()
            .files()
            .unwrap()
            .footer()
            .starts_with("Remove gone for good?"));
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::PurgeFile {
                id,
                name: "gone".to_string()
            })
        );
    }

    #[test]
    fn files_creates_renames_tags_and_bins_through_footer_prompts() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Files);
        desk.files_listed(
            id,
            alloc::vec![FileEntry::named("a"), FileEntry::named("b")],
        );
        // Ctrl+N: a name, Enter. No suffix.
        desk.handle_key(crate::notepad::CTRL_N);
        for byte in b"memo" {
            desk.handle_key(*byte);
        }
        let footer = desk.window(id).unwrap().files().unwrap().footer();
        assert!(footer.starts_with("New: memo_"), "{footer}");
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::CreateFile {
                id,
                name: "memo".to_string()
            })
        );
        // Delete bins without asking: it is reversible.
        desk.handle_key(crate::notepad::KEY_DOWN);
        let (request, _) = desk.handle_key(crate::notepad::KEY_DELETE);
        assert_eq!(
            request,
            Some(DeskRequest::TrashFile {
                id,
                name: "b".to_string(),
                trashed: true
            })
        );
        // Rename starts from the current name; an unchanged name is a no-op.
        desk.handle_key(CTRL_E);
        assert!(desk
            .window(id)
            .unwrap()
            .files()
            .unwrap()
            .footer()
            .starts_with("Rename to: b_"));
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(request, None);
        desk.handle_key(CTRL_E);
        desk.handle_key(crate::notepad::BACKSPACE);
        desk.handle_key(b'c');
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::RenameFile {
                id,
                from: "b".to_string(),
                to: "c".to_string()
            })
        );
        // Tags: words add, -word removes, # is optional.
        desk.handle_key(CTRL_K);
        for byte in b"Work #draft -old" {
            desk.handle_key(*byte);
        }
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::TagFile {
                id,
                name: "b".to_string(),
                add: alloc::vec!["work".to_string(), "draft".to_string()],
                remove: alloc::vec!["old".to_string()],
            })
        );
        // Ctrl+R lists again; a refresh keeps the selection by name.
        assert_eq!(
            desk.handle_key(CTRL_R).0,
            Some(DeskRequest::ListFiles { id })
        );
        desk.files_listed(
            id,
            alloc::vec![
                FileEntry::named("a"),
                FileEntry::named("b"),
                FileEntry::named("memo")
            ],
        );
        assert_eq!(
            desk.window(id)
                .unwrap()
                .files()
                .unwrap()
                .selected()
                .map(|e| e.name.as_str()),
            Some("b")
        );
    }

    #[test]
    fn header_chips_run_the_apps_own_keys() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        desk.handle_key(b'x');
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(
            card.actions,
            alloc::vec!["Save", "Save as", "Open", "Find", "History"]
        );
        let save = card.action_rects()[0].expect("the Save chip is placed");
        let compositor = Compositor::new();
        let deliveries = router.route(
            &compositor,
            &windows,
            press((save.x + 3) as i32, (save.y + 3) as i32),
        );
        let (requests, changed) = desk.handle_deliveries_with_requests(&deliveries);
        assert!(changed);
        // An unnamed document: Save opens the name prompt, which asks for
        // the listing to complete against.
        assert_eq!(requests, alloc::vec![DeskRequest::ListFiles { id }]);
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert!(
            notepad.footer().starts_with("Save as:"),
            "{}",
            notepad.footer()
        );

        let files = desk.launch(DeskApp::Files);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == files).unwrap();
        assert_eq!(
            card.actions,
            alloc::vec!["New", "Rename", "Tag", "Bin it", "Recent", "Bin"]
        );
        let terminal = desk.launch(DeskApp::Terminal);
        let windows = desk.windows("", true, None);
        let card = windows
            .iter()
            .find(|w| w.frame.view_id == terminal)
            .unwrap();
        assert!(card.actions.is_empty());
    }

    #[test]
    fn the_history_chip_asks_the_kernel_and_the_answer_reaches_the_notepad() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        desk.window_mut(id)
            .unwrap()
            .notepad_mut()
            .unwrap()
            .load(Some("memo".to_string()), "now");
        let (request, _) = desk.handle_key(crate::notepad::CTRL_Y);
        assert_eq!(
            request,
            Some(DeskRequest::ReadVersion {
                id,
                name: "memo".to_string(),
                index: 0
            })
        );
        desk.version_loaded(id, 0, 1, Some("before"), 0);
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert!(notepad.browsing_history());
        assert_eq!(notepad.content(), "before");
        // Enter restores: a Save request for what is shown.
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::Io {
                id,
                effect: NotepadEffect::Save {
                    path: "memo".to_string(),
                    content: "before".to_string()
                }
            })
        );
    }

    #[test]
    fn a_landed_save_raises_a_notice_that_expires() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        let effect = NotepadEffect::Save {
            path: "n.txt".to_string(),
            content: String::new(),
        };
        desk.io_done(id, &effect, Ok(None), 1000);
        let windows = desk.windows_at("", true, None, 1000, &[]);
        let notice = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Notification)
            .expect("a notice card");
        assert!(
            matches!(&notice.frame.content, ViewContent::TextBuffer { lines } if lines[0] == "Saved n.txt")
        );
        assert!(!notice.closable);

        let windows = desk.windows_at("", true, None, 1000 + NOTICE_TTL_TICKS + 1, &[]);
        assert!(
            !windows
                .iter()
                .any(|w| w.role == DesktopWindowRole::Notification),
            "the notice did not expire"
        );

        // The workspace's own notices show the same way.
        let shell = alloc::vec![ShellNotice::new(
            NoticeLevel::Info,
            "Switching display to desk."
        )];
        let windows = desk.windows_at("", true, None, 5000, &shell);
        assert!(windows
            .iter()
            .any(|w| w.role == DesktopWindowRole::Notification));
    }

    #[test]
    fn the_empty_desk_has_a_top_bar_and_a_dock_and_nothing_else() {
        let mut desk = Desk::new(1280, 800);
        let windows = desk.windows("12:34", true, None);
        assert_eq!(windows.len(), 2);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        assert!(
            matches!(&bar.frame.content, ViewContent::TextBuffer { lines } if lines[0] == "12:34")
        );
        assert!(windows.iter().any(|w| w.style == WindowStyle::Dock));
    }

    #[test]
    fn clicking_the_dock_tile_launches_notepad_once_and_then_focuses_it() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let windows = desk.windows("", true, None);
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        // Four tiles: 4*40 + 3*8 = 184 wide, centred in the pill; the
        // first tile's centre is 20px into that row.
        let pill = dock.bounds();
        let first_x = (pill.x + (pill.width - 184) / 2 + 20) as i32;
        let y = (pill.y + pill.height / 2) as i32;

        assert!(route(&mut desk, &mut router, press(first_x, y)));
        route(&mut desk, &mut router, release(first_x, y));
        assert_eq!(desk.window_count(), 1);
        assert_eq!(desk.focused_window().map(|w| w.app), Some(DeskApp::Notepad));

        // A second click does not open a second Notepad.
        route(&mut desk, &mut router, press(first_x, y));
        route(&mut desk, &mut router, release(first_x, y));
        assert_eq!(desk.window_count(), 1);
    }

    #[test]
    fn hovering_a_dock_tile_lights_it_and_leaving_the_dock_clears_it() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let windows = desk.windows("", true, None);
        let pill = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap()
            .bounds();
        let first_x = (pill.x + (pill.width - 184) / 2 + 20) as i32;
        let y = (pill.y + pill.height / 2) as i32;

        assert!(route(
            &mut desk,
            &mut router,
            moved(first_x, y, PointerButtons::none())
        ));
        let windows = desk.windows("", true, None);
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert!(dock.tabs[0].hovered && !dock.tabs[1].hovered);

        route(
            &mut desk,
            &mut router,
            moved(10, 300, PointerButtons::none()),
        );
        let windows = desk.windows("", true, None);
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert!(!dock.tabs[0].hovered);
    }

    #[test]
    fn a_card_drags_by_its_header_and_stays_on_the_desk() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        let before = desk.window(id).unwrap().bounds;

        let grab = ((before.x + 100) as i32, (before.y + 10) as i32);
        drag(&mut desk, &mut router, grab, (grab.0 + 50, grab.1 + 40));
        let after = desk.window(id).unwrap().bounds;
        assert_eq!((after.x, after.y), (before.x + 50, before.y + 40));

        // Dragging far off-screen clamps to the desk.
        drag(
            &mut desk,
            &mut router,
            (after.x as i32 + 100, after.y as i32 + 10),
            (1200, 700),
        );
        let clamped = desk.window(id).unwrap().bounds;
        assert!(clamped.right() <= 1280 && clamped.bottom() <= 800);
    }

    #[test]
    fn a_drag_to_the_edge_snaps_and_the_next_drag_unsnaps() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        let before = desk.window(id).unwrap().bounds;
        let area = desk.work_area();

        // To the left edge: the left half.
        drag(
            &mut desk,
            &mut router,
            ((before.x + 100) as i32, (before.y + 10) as i32),
            (2, 300),
        );
        let snapped = desk.window(id).unwrap().bounds;
        assert_eq!(
            (snapped.x, snapped.width),
            (0, area.width / 2),
            "not the left half"
        );
        assert_eq!(snapped.height, area.height);

        // To the top: the whole work area.
        drag(
            &mut desk,
            &mut router,
            (100, (snapped.y + 10) as i32),
            (600, TOP_BAR_HEIGHT as i32 + 2),
        );
        let maxed = desk.window(id).unwrap().bounds;
        assert_eq!(maxed, area, "not maximised");

        // Dragging it away restores the original size under the pointer.
        drag(
            &mut desk,
            &mut router,
            (600, (maxed.y + 10) as i32),
            (640, 400),
        );
        let restored = desk.window(id).unwrap().bounds;
        assert_eq!(
            (restored.width, restored.height),
            (before.width, before.height)
        );
    }

    #[test]
    fn the_corner_grip_resizes_within_limits() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        let before = desk.window(id).unwrap().bounds;
        let grip = ((before.right() - 4) as i32, (before.bottom() - 4) as i32);

        drag(&mut desk, &mut router, grip, (grip.0 - 100, grip.1 - 60));
        let smaller = desk.window(id).unwrap().bounds;
        assert_eq!(
            (smaller.width, smaller.height),
            (before.width - 100, before.height - 60)
        );

        // Never below the minimum, whatever the pointer does.
        let grip = ((smaller.right() - 4) as i32, (smaller.bottom() - 4) as i32);
        drag(&mut desk, &mut router, grip, (10, 10));
        let floor = desk.window(id).unwrap().bounds;
        assert_eq!((floor.width, floor.height), MIN_CARD_SIZE);
    }

    #[test]
    fn clicking_in_the_text_places_the_caret_and_the_wheel_scrolls() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        for i in 0..60 {
            for byte in alloc::format!("line {i}").bytes() {
                desk.handle_key(byte);
            }
            desk.handle_key(b'\n');
        }
        let bounds = desk.window(id).unwrap().bounds;
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let (ox, oy, pitch) = card.card_text_origin();

        // Wheel up scrolls the view back towards the top.
        let rows = Desk::card_rows(bounds);
        route(
            &mut desk,
            &mut router,
            wheel((ox + 40) as i32, (oy + 40) as i32, 10),
        );
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let first_shown = match &card.frame.content {
            ViewContent::TextBuffer { lines } => lines[0].clone(),
            _ => panic!(),
        };
        assert_ne!(
            first_shown,
            alloc::format!("line {}", 61 - rows),
            "the wheel did not scroll"
        );

        // Click on the third visible line, column 3.
        let (x, y) = ((ox + 8 * 3 + 2) as i32, (oy + pitch * 2 + 5) as i32);
        route(&mut desk, &mut router, press(x, y));
        route(&mut desk, &mut router, release(x, y));
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert_eq!(
            notepad.viewport_cursor(),
            Some((2, 3)),
            "the caret did not follow the click"
        );
    }

    #[test]
    fn the_close_glyph_closes_and_focus_falls_to_the_next_card() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let first = desk.launch(DeskApp::Notepad);
        let second = desk.launch(DeskApp::Notepad);
        assert_eq!(desk.focus(), Some(second));

        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == second).unwrap();
        let close = card.close_rect().unwrap();
        let (x, y) = (
            (close.x + close.width / 2) as i32,
            (close.y + close.height / 2) as i32,
        );
        route(&mut desk, &mut router, press(x, y));
        assert_eq!(desk.window_count(), 1);
        assert_eq!(desk.focus(), Some(first));
    }

    #[test]
    fn ctrl_tab_walks_the_stack() {
        let mut desk = Desk::new(1280, 800);
        let a = desk.launch(DeskApp::Notepad);
        let b = desk.launch(DeskApp::Terminal);
        let c = desk.launch(DeskApp::Notepad);
        assert_eq!(desk.focus(), Some(c));
        desk.handle_key(KEY_CTRL_TAB);
        assert_eq!(desk.focus(), Some(a));
        desk.handle_key(KEY_CTRL_TAB);
        assert_eq!(desk.focus(), Some(b));
        desk.handle_key(KEY_CTRL_TAB);
        assert_eq!(desk.focus(), Some(c));
    }

    #[test]
    fn terminal_keys_go_to_the_console_and_its_view_fills_the_card() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Terminal);
        let (request, changed) = desk.handle_key(b'h');
        assert_eq!(request, Some(DeskRequest::Terminal(b'h')));
        assert!(changed);

        let view = TerminalView {
            lines: alloc::vec!["PandaGen Workspace".to_string(), "WS > h".to_string()],
            cursor: Some((1, 6)),
            status: "WS".to_string(),
        };
        let windows = desk.windows("", true, Some(&view));
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(card.frame.title.as_deref(), Some("Terminal"));
        assert_eq!(card.frame.cursor.map(|c| (c.line, c.column)), Some((1, 6)));

        // Ctrl+W closes the console card rather than reaching the console.
        let (request, _) = desk.handle_key(crate::notepad::CTRL_W);
        assert_eq!(request, None);
        assert_eq!(desk.window_count(), 0);
    }

    #[test]
    fn keys_go_to_the_focused_notepad_and_io_comes_back_as_a_request() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        for byte in b"hi" {
            let (request, changed) = desk.handle_key(*byte);
            assert!(request.is_none() && changed);
        }
        let (request, _) = desk.handle_key(crate::notepad::CTRL_S);
        assert_eq!(
            request,
            Some(DeskRequest::ListFiles { id }),
            "the first save asks for a name, and for the names to complete against"
        );
        for byte in b"n.txt" {
            desk.handle_key(*byte);
        }
        let (request, _) = desk.handle_key(b'\n');
        match request {
            Some(DeskRequest::Io { id: got, effect }) => {
                assert_eq!(got, id);
                assert_eq!(
                    effect,
                    NotepadEffect::Save {
                        path: "n.txt".to_string(),
                        content: "hi".to_string()
                    }
                );
                desk.io_done(id, &effect, Ok(None), 0);
            }
            other => panic!("expected a save request, got {other:?}"),
        }
        assert!(!desk.window(id).unwrap().notepad().unwrap().is_dirty());
        let windows = desk.windows("09:41", true, None);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        assert_eq!(bar.frame.title.as_deref(), Some("Notepad"));
    }
}
