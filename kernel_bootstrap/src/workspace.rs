//! Minimal workspace manager for bare-metal kernel
//!
//! This provides a workspace-like experience in the bare-metal kernel without
//! requiring the full std-based services_workspace_manager.

#[cfg(not(test))]
extern crate alloc;

use crate::display_mode::DisplayMode;
use crate::line_edit::{Edit, LineEdit};
use core::fmt::Write;

#[cfg(not(test))]
use alloc::boxed::Box;
#[cfg(not(test))]
use alloc::string::{String, ToString};
#[cfg(test)]
use std::string::{String, ToString};

#[cfg(not(test))]
use alloc::format;
#[cfg(not(test))]
use alloc::vec;
#[cfg(not(test))]
use alloc::vec::Vec;
#[cfg(test)]
use std::format;
#[cfg(test)]
use std::vec::Vec;

use crate::serial::SerialPort;
use crate::{
    ChannelId, CommandRequest, CommandStatus, KernelApiV0, KernelContext, KernelMessage, MessageId,
    COMMAND_MAX,
};

#[cfg(all(debug_assertions, not(test)))]
use crate::minimal_editor::EditorMode;
use crate::minimal_editor::MinimalEditor;
use crate::palette_overlay::{
    handle_palette_key, FocusTarget, PaletteKeyAction, PaletteOverlayState,
};

use input_types::{KeyCode, KeyEvent, KeyState, Modifiers};

#[cfg(not(test))]
use crate::bare_metal_editor_io::BareMetalEditorIo;
#[cfg(not(test))]
use crate::bare_metal_storage::BareMetalFilesystem;

#[cfg(feature = "console_vga")]
use console_vga::{SplitLayout, TileId, TileManager, VGA_HEIGHT, VGA_WIDTH};

use services_command_palette::{CommandDescriptor, CommandId, CommandPalette};
use services_gui_host::{
    GfxSnapshot, HostEvent, HostResponse, HostedComponent, HostedSurface, ListComponent,
    NoticeLevel, ShellNotice,
};

/// The per-key trace of where input went. Off: it wrote three lines to
/// the serial port for every keystroke, and would have written a
/// passphrase's keys there too.
const ROUTE_TRACE: bool = false;

macro_rules! route_trace {
    ($serial:expr, $($arg:tt)*) => {
        if ROUTE_TRACE {
            let _ = writeln!($serial, $($arg)*);
        }
    };
}

/// The words Tab can finish at the prompt (KBD-012).
fn command_words() -> Vec<&'static str> {
    let mut words = vec![
        "help", "open", "list", "focus", "clear", "cls", "display", "pointer", "pipeline", "heap",
        "cpus", "smp", "net", "gfx", "quit", "exit", "halt", "ls", "cat", "write", "boot", "mem",
        "ticks", "editor", "fault", "fetch", "resolve", "random", "threads", "spin", "stop",
        "after", "run", "programs",
    ];
    words.extend(crate::access_shell::WORDS);
    words
}

/// Component type in the workspace
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ComponentType {
    Editor,
    Cli,
    Shell,
}

impl core::fmt::Display for ComponentType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ComponentType::Editor => write!(f, "Editor"),
            ComponentType::Cli => write!(f, "CLI"),
            ComponentType::Shell => write!(f, "Shell"),
        }
    }
}

/// Converts a raw byte to a KeyEvent
///
/// This is a temporary bridge function until the full KeyEvent pipeline is integrated.
/// Handles both uppercase and lowercase ASCII letters, numbers, and basic control keys.
fn byte_to_key_event(byte: u8) -> Option<KeyEvent> {
    let key_code = match byte {
        0x1B => KeyCode::Escape,
        b'\n' | b'\r' => KeyCode::Enter,
        0x08 | 0x7F => KeyCode::Backspace,
        0x09 => KeyCode::Tab,
        b' ' => KeyCode::Space,
        0x10 => KeyCode::P,
        0x80 => KeyCode::Up,
        0x81 => KeyCode::Down,
        // Letters (lowercase)
        b'a' | b'A' => KeyCode::A,
        b'b' | b'B' => KeyCode::B,
        b'c' | b'C' => KeyCode::C,
        b'd' | b'D' => KeyCode::D,
        b'e' | b'E' => KeyCode::E,
        b'f' | b'F' => KeyCode::F,
        b'g' | b'G' => KeyCode::G,
        b'h' | b'H' => KeyCode::H,
        b'i' | b'I' => KeyCode::I,
        b'j' | b'J' => KeyCode::J,
        b'k' | b'K' => KeyCode::K,
        b'l' | b'L' => KeyCode::L,
        b'm' | b'M' => KeyCode::M,
        b'n' | b'N' => KeyCode::N,
        b'o' | b'O' => KeyCode::O,
        b'p' | b'P' => KeyCode::P,
        b'q' | b'Q' => KeyCode::Q,
        b'r' | b'R' => KeyCode::R,
        b's' | b'S' => KeyCode::S,
        b't' | b'T' => KeyCode::T,
        b'u' | b'U' => KeyCode::U,
        b'v' | b'V' => KeyCode::V,
        b'w' | b'W' => KeyCode::W,
        b'x' | b'X' => KeyCode::X,
        b'y' | b'Y' => KeyCode::Y,
        b'z' | b'Z' => KeyCode::Z,
        // Numbers
        b'0' => KeyCode::Num0,
        b'1' => KeyCode::Num1,
        b'2' => KeyCode::Num2,
        b'3' => KeyCode::Num3,
        b'4' => KeyCode::Num4,
        b'5' => KeyCode::Num5,
        b'6' => KeyCode::Num6,
        b'7' => KeyCode::Num7,
        b'8' => KeyCode::Num8,
        b'9' => KeyCode::Num9,
        // Symbols
        b'-' => KeyCode::Minus,
        b'=' => KeyCode::Equal,
        b'[' => KeyCode::LeftBracket,
        b']' => KeyCode::RightBracket,
        b'\\' => KeyCode::Backslash,
        b';' => KeyCode::Semicolon,
        b'\'' => KeyCode::Quote,
        b',' => KeyCode::Comma,
        b'.' => KeyCode::Period,
        b'/' => KeyCode::Slash,
        b'`' => KeyCode::Grave,
        // Unknown/unhandled
        _ => return None,
    };

    Some(KeyEvent::new(
        key_code,
        Modifiers::none(),
        KeyState::Pressed,
    ))
}

/// Workspace session state
pub struct WorkspaceSession {
    /// Active component type
    active_component: Option<ComponentType>,
    /// Editor instance (bare-metal)
    editor: Option<MinimalEditor>,
    /// Tile manager for layout and focus
    #[cfg(feature = "console_vga")]
    tile_manager: TileManager,
    /// Command channel for component communication
    command_channel: ChannelId,
    /// Response channel for replies
    response_channel: ChannelId,
    /// Whether we're in command mode
    in_command_mode: bool,
    /// The input line, for the prompt and the CLI alike (KBD-012).
    line: LineEdit,
    /// Output log (fixed-size ring buffer)
    output_lines: [OutputLine; OUTPUT_MAX_LINES],
    output_head: usize,
    output_count: usize,
    output_seq: u64,
    /// Filesystem storage (optional)
    #[cfg(not(test))]
    filesystem: Option<BareMetalFilesystem>,
    /// Command palette overlay state
    palette_overlay: PaletteOverlayState,
    /// Command palette service
    command_palette: CommandPalette,
    /// CLI mode active
    cli_active: bool,
    /// Whether the CLI first-run hint has been shown
    cli_hint_shown: bool,
    /// Clear screen requested (CLI/workspace)
    clear_requested: bool,
    /// Display mode currently driving the framebuffer (set by the loop).
    display_mode: DisplayMode,
    /// Display mode change requested by the `display` command.
    display_mode_request: Option<DisplayMode>,
    /// Whether a framebuffer exists so graphics mode can be entered.
    graphics_available: bool,
    /// Whether a pointer device was brought up at boot.
    pointer_available: bool,
    /// Last known pointer position in display pixels.
    pointer_position: (i32, i32),
    /// Held pointer buttons as a bit set (bit 0 primary).
    pointer_buttons: u8,
    /// Pointer events observed since boot.
    pointer_events: u64,
    /// Window role under the pointer (graphics mode), if any.
    pointer_over: Option<&'static str>,
    /// Window role owning keyboard focus (graphics mode), if any.
    pointer_focus: Option<&'static str>,
    /// Whether a window currently captures the pointer.
    pointer_captured: bool,
    /// Shell notices, newest first.
    notices: Vec<PendingNotice>,
    /// Path of the document open in the editor, if it has one.
    editor_path: Option<String>,
    /// File picker, when open (GFX-037).
    file_picker: Option<FilePickerState>,
    /// Lines the graphical workspace view is scrolled up from the tail
    /// (GFX-038). Reset whenever new output arrives.
    scrollback_offset: usize,
    /// Pipeline run in progress or last finished (GFX-039).
    pipeline_run: Option<PipelineRun>,
    /// Dedicated reply channel for pipeline stages, so the console task
    /// polling the shared response channel cannot consume stage replies.
    pipeline_channel: Option<ChannelId>,
    /// Custom component hosted in the workspace window (GFX-040).
    hosted: Option<ListComponent>,
    /// Deliberately held allocations from `heap stress`, for testing the
    /// low-memory degradation path (GFX-048).
    stress_blocks: Vec<Vec<u8>>,
    /// Latest graphics telemetry snapshot pushed by the loop (GFX-049).
    gfx_snapshot: Option<GfxSnapshot>,
    /// Set by `gfx reset`; consumed by the loop.
    gfx_reset_requested: bool,
    /// A `fetch` or `resolve` for the loop to hand the network (NET-033).
    net_request: Option<NetRequest>,
    /// A program `run` asked for (PROC-002), for the loop to start.
    program_request: Option<String>,
}

/// `fetch <url>`, `resolve <name> [server]`, or the same after `net`:
/// `None` for any other line, `Some(None)` for one of these said wrongly.
pub fn net_request_of(command: &str) -> Option<Option<NetRequest>> {
    let mut words = command.split_whitespace();
    let mut word = words.next()?;
    if word == "net" {
        word = words.next().filter(|w| *w == "fetch" || *w == "resolve")?;
    }
    let request = match (word, words.next(), words.next(), words.next()) {
        ("fetch", Some(url), None, None) => Some(NetRequest::Fetch(String::from(url))),
        ("resolve", Some(name), server, None) => Some(NetRequest::Resolve {
            name: String::from(name),
            server: server.map(String::from),
        }),
        ("fetch" | "resolve", ..) => None,
        _ => return None,
    };
    Some(request)
}

/// What the Terminal asked of the network; the loop starts it, and the
/// answer comes back as lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetRequest {
    Fetch(String),
    Resolve {
        name: String,
        /// `server[:port]`, when not the resolver DHCP named.
        server: Option<String>,
    },
}

/// Lifecycle of one pipeline stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageState {
    Pending,
    Running { request_id: MessageId, started: u64 },
    Succeeded { ticks: u64, summary: String },
    Failed { ticks: u64, error: String },
}

/// One stage: a command routed through the kernel command service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRun {
    pub command: String,
    pub state: StageState,
}

/// A sequential pipeline of kernel commands with a per-stage trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineRun {
    pub stages: Vec<StageRun>,
    pub started: u64,
    pub finished: Option<u64>,
}

impl PipelineRun {
    pub fn running_index(&self) -> Option<usize> {
        self.stages
            .iter()
            .position(|s| matches!(s.state, StageState::Running { .. }))
    }

    pub fn done_count(&self) -> usize {
        self.stages
            .iter()
            .filter(|s| {
                matches!(
                    s.state,
                    StageState::Succeeded { .. } | StageState::Failed { .. }
                )
            })
            .count()
    }

    pub fn failed(&self) -> bool {
        self.stages
            .iter()
            .any(|s| matches!(s.state, StageState::Failed { .. }))
    }

    pub fn is_finished(&self) -> bool {
        self.finished.is_some()
    }
}

/// File picker state: a flat listing of the root with a selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePickerState {
    pub entries: Vec<String>,
    pub selection: usize,
}

/// Maximum notices retained for the shell.
const MAX_SHELL_NOTICES: usize = 4;

/// A notice plus the tick it was first shown (set by the loop).
#[derive(Debug, Clone)]
struct PendingNotice {
    notice: ShellNotice,
    shown_at: Option<u64>,
}

impl WorkspaceSession {
    pub fn new(command_channel: ChannelId, response_channel: ChannelId) -> Self {
        let mut command_palette = CommandPalette::new();

        // Register example commands
        command_palette.register_command(
            CommandDescriptor::new(
                "help",
                "Show Help",
                "Display available commands",
                vec!["help".to_string(), "commands".to_string()],
            ),
            Box::new(|_| Ok("Available commands: help, open editor, quit".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "open_editor",
                "Open Editor",
                "Open the text editor",
                vec!["editor".to_string(), "edit".to_string(), "vim".to_string()],
            ),
            Box::new(|_| Ok("Opening editor...".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "open_about",
                "About PandaGen",
                "Show system facts in a hosted component",
                vec!["about".to_string(), "info".to_string()],
            )
            .with_category("Workspace"),
            Box::new(|_| Ok("Opening About...".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "open_file",
                "Open File",
                "Pick a file to open in the editor",
                vec!["open".to_string(), "file".to_string(), "picker".to_string()],
            )
            .with_category("Workspace"),
            Box::new(|_| Ok("Opening file picker...".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "clear",
                "Clear Screen",
                "Clear workspace output",
                vec!["clear".to_string(), "cls".to_string()],
            )
            .with_category("Workspace"),
            Box::new(|_| Ok("Clearing screen...".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "open_cli",
                "Switch to CLI",
                "Switch to the CLI component",
                vec![
                    "cli".to_string(),
                    "console".to_string(),
                    "switch".to_string(),
                ],
            )
            .with_category("Workspace"),
            Box::new(|_| Ok("Switching to CLI...".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "list",
                "List Components",
                "List active components",
                vec!["list".to_string(), "components".to_string()],
            )
            .with_category("Workspace"),
            Box::new(|_| Ok("Listing components...".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "focus",
                "Focus Component",
                "Focus the next component",
                vec!["focus".to_string(), "cycle".to_string()],
            )
            .with_category("Workspace")
            .requires_args()
            .with_prompt_pattern("focus "),
            Box::new(|_| Ok("Focus command ready".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "quit",
                "Quit",
                "Exit the workspace",
                vec!["exit".to_string(), "close".to_string(), "q".to_string()],
            )
            .with_category("Workspace"),
            Box::new(|_| Ok("Quitting...".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "halt",
                "Halt System",
                "Halt the system",
                vec!["halt".to_string(), "shutdown".to_string()],
            )
            .with_category("System"),
            Box::new(|_| Ok("Halting...".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "boot",
                "Show Boot Info",
                "Display boot information",
                vec!["boot".to_string(), "info".to_string()],
            )
            .with_category("System"),
            Box::new(|_| Ok("Boot info".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "mem",
                "Show Memory Info",
                "Display memory information",
                vec!["mem".to_string(), "memory".to_string()],
            )
            .with_category("System"),
            Box::new(|_| Ok("Memory info".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "cpus",
                "Show CPUs",
                "Display online CPUs and LAPIC ids",
                vec!["cpus".to_string(), "smp".to_string()],
            )
            .with_category("System"),
            Box::new(|_| Ok("CPU info".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "ticks",
                "Show System Ticks",
                "Display system tick count",
                vec!["ticks".to_string(), "time".to_string()],
            )
            .with_category("System"),
            Box::new(|_| Ok("System ticks".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "ls",
                "List Files",
                "List filesystem entries",
                vec!["ls".to_string(), "files".to_string()],
            )
            .with_category("File"),
            Box::new(|_| Ok("Listing files...".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "cat",
                "Show File Contents",
                "Display a file's contents",
                vec!["cat".to_string(), "read".to_string()],
            )
            .with_category("File")
            .requires_args()
            .with_prompt_pattern("cat "),
            Box::new(|_| Ok("Cat command ready".to_string())),
        );

        command_palette.register_command(
            CommandDescriptor::new(
                "write",
                "Write File",
                "Create or update a file",
                vec!["write".to_string(), "save".to_string()],
            )
            .with_category("File")
            .requires_args()
            .with_prompt_pattern("write "),
            Box::new(|_| Ok("Write command ready".to_string())),
        );

        Self {
            active_component: None,
            editor: None,
            #[cfg(feature = "console_vga")]
            tile_manager: TileManager::new(
                VGA_WIDTH,
                VGA_HEIGHT,
                SplitLayout::horizontal(VGA_HEIGHT - 5),
            ), // Editor gets most space
            command_channel,
            response_channel,
            in_command_mode: true,
            line: LineEdit::new(),
            output_lines: [OutputLine::empty(); OUTPUT_MAX_LINES],
            output_head: 0,
            output_count: 0,
            output_seq: 0,
            #[cfg(not(test))]
            filesystem: None,
            palette_overlay: PaletteOverlayState::new(),
            command_palette,
            cli_active: false,
            cli_hint_shown: false,
            clear_requested: false,
            display_mode: DisplayMode::DEFAULT,
            display_mode_request: None,
            graphics_available: false,
            pointer_available: false,
            pointer_position: (0, 0),
            pointer_buttons: 0,
            pointer_events: 0,
            pointer_over: None,
            pointer_focus: None,
            pointer_captured: false,
            notices: Vec::new(),
            editor_path: None,
            file_picker: None,
            scrollback_offset: 0,
            pipeline_run: None,
            pipeline_channel: None,
            hosted: None,
            stress_blocks: Vec::new(),
            gfx_snapshot: None,
            gfx_reset_requested: false,
            net_request: None,
            program_request: None,
        }
    }

    /// Set the filesystem for this session
    #[cfg(not(test))]
    pub fn set_filesystem(&mut self, fs: BareMetalFilesystem) {
        self.filesystem = Some(fs);
    }

    /// Borrow the filesystem out of the session, for the desk's apps.
    ///
    /// There is one filesystem and it is owned by whoever is writing; the
    /// editor takes it the same way. Give it back with `set_filesystem`.
    #[cfg(not(test))]
    pub fn take_filesystem(&mut self) -> Option<BareMetalFilesystem> {
        self.filesystem.take()
    }

    /// Activate or deactivate CLI mode
    fn set_cli_active(&mut self, active: bool, serial: &mut SerialPort) {
        self.cli_active = active;
        if active {
            self.reset_cli_buffer();
            self.emit_line(serial, "CLI mode: type commands, `exit` to leave");

            // Show first-run hint
            if !self.cli_hint_shown {
                self.emit_line(serial, "Tip: Ctrl+P opens Commands.");
                self.cli_hint_shown = true;
            }
        } else {
            if self.active_component == Some(ComponentType::Cli) {
                self.active_component = None;
            }
            self.emit_line(serial, "Returned to workspace");
        }
    }

    /// Reset the CLI input buffer
    fn reset_cli_buffer(&mut self) {
        self.line.clear();
    }

    /// Process a single byte of input
    pub fn process_input(
        &mut self,
        byte: u8,
        ctx: &mut KernelContext,
        serial: &mut SerialPort,
    ) -> bool {
        let _pre_editor_row = self.editor.as_ref().map(|editor| editor.cursor().row);
        let _pre_editor_col = self.editor.as_ref().map(|editor| editor.cursor().col);
        #[cfg(feature = "console_vga")]
        let focused_tile = self.tile_manager.focused_tile();
        #[cfg(not(feature = "console_vga"))]
        let focused_tile = "Unavailable";

        route_trace!(serial, "route_input:");
        route_trace!(serial, "  key={{byte={:#x}}}", byte);
        route_trace!(serial, "  focus_tile={{{:?}}}", focused_tile);

        // 1. Check for global shortcuts BEFORE component routing
        // Ctrl+P (0x10) opens command palette
        if byte == 0x10 && !self.palette_overlay.is_open() {
            route_trace!(serial, "  action=open_palette");

            // Determine current focus target
            let current_focus = if self.active_component == Some(ComponentType::Editor) {
                FocusTarget::Editor
            } else if self.in_command_mode {
                FocusTarget::Cli
            } else {
                FocusTarget::None
            };

            self.palette_overlay
                .open_with_context(current_focus, self.cli_active);
            self.palette_overlay
                .update_query(&self.command_palette, String::new());
            return true;
        }

        // 2. If palette is open, route all input to it
        if self.palette_overlay.is_open() {
            route_trace!(serial, "  action=palette_input");

            // Convert byte to KeyEvent
            if let Some(key_event) = byte_to_key_event(byte) {
                let action = handle_palette_key(
                    &mut self.palette_overlay,
                    &self.command_palette,
                    &key_event,
                );

                match action {
                    PaletteKeyAction::Close => {
                        route_trace!(serial, "  palette_action=close");
                        self.palette_overlay.close();
                        return true;
                    }
                    PaletteKeyAction::Execute(cmd_id) => {
                        return self.run_palette_command(cmd_id, ctx, serial);
                    }
                    PaletteKeyAction::Consumed => {
                        route_trace!(serial, "  palette_action=consumed");
                        return true;
                    }
                    PaletteKeyAction::None => {
                        route_trace!(serial, "  palette_action=none");
                        // Fall through - shouldn't happen but handle gracefully
                        return false;
                    }
                }
            } else {
                route_trace!(serial, "  palette_action=unknown_byte");
                return true; // Consume unknown bytes when palette is open
            }
        }

        // 2b. If the file picker is open, it owns the keyboard.
        if self.file_picker.is_some() {
            route_trace!(serial, "  action=picker_input");
            match byte {
                0x80 | b'k' => self.picker_move(-1),
                0x81 | b'j' => self.picker_move(1),
                b'\n' | b'\r' => {
                    self.picker_open_selection(serial);
                }
                0x1b | b'q' => {
                    self.file_picker = None;
                    self.emit_line(serial, "File picker closed");
                }
                _ => {}
            }
            return true;
        }

        // 2c. A hosted custom component owns the keyboard while open.
        if self.hosted.is_some() {
            route_trace!(serial, "  action=hosted_input");
            self.host_event(HostEvent::Key(byte), serial);
            return true;
        }

        // 3. Check tile focus before delivering to component
        #[cfg(feature = "console_vga")]
        {
            // Editor lives in Top tile. If Bottom is focused, Editor shouldn't get input.
            if self.active_component == Some(ComponentType::Editor) && focused_tile != TileId::Top {
                route_trace!(serial, "  consumed_by=none (focus mismatch)");
                return false;
            }
        }

        // 4. Route to active component (existing logic)
        // If editor is active, route input to it
        #[cfg(not(test))]
        if self.active_component == Some(ComponentType::Editor) {
            if let Some(ref mut editor) = self.editor {
                route_trace!(
                    serial,
                    "  action=process_byte_start cursor={:?}",
                    editor.cursor()
                );
                let should_quit = editor.process_byte(byte);
                route_trace!(
                    serial,
                    "  action=process_byte_end cursor={:?} dirty={}",
                    editor.cursor(),
                    editor.is_dirty()
                );

                #[cfg(debug_assertions)]
                {
                    if let (Some(pre_row), Some(pre_col)) = (_pre_editor_row, _pre_editor_col) {
                        let new_row = editor.cursor().row;
                        let new_col = editor.cursor().col;
                        let line_delta = new_row.abs_diff(pre_row);
                        let cursor_moved = new_row != pre_row || new_col != pre_col;
                        let is_editing =
                            matches!(editor.mode(), EditorMode::Insert | EditorMode::Command);
                        let is_normal_typing = matches!(
                            byte,
                            b'a'..=b'z'
                                | b'A'..=b'Z'
                                | b'0'..=b'9'
                                | b' '
                                | b'.'
                                | b','
                                | b';'
                                | b':'
                                | b'\''
                                | b'"'
                                | b'-'
                                | b'_'
                                | b'!'
                                | b'?'
                        );
                        if is_normal_typing && is_editing && cursor_moved {
                            debug_assert!(
                                line_delta <= 1,
                                "typing must dirty at most one line in a render pass"
                            );
                        }
                    }
                }

                if should_quit {
                    self.active_component = None;

                    // Extract filesystem from editor if it had one
                    if let Some(mut editor_instance) = self.editor.take() {
                        if let Some(io) = editor_instance.editor_io.take() {
                            self.filesystem = Some(io.into_filesystem());
                        }
                    }

                    let _ = serial.write_str("\r\nEditor closed\r\n");
                    self.show_prompt(serial);
                }
                return true;
            }
        }

        // 5. The prompt, or the CLI: the line editor (KBD-012).
        route_trace!(serial, "  action=line_input");
        let words = command_words();
        match self.line.key(byte, &words) {
            Edit::Unhandled => {
                // Escape leaves the CLI.
                if byte == 0x1B && self.cli_active {
                    let _ = serial.write_str("\r\n");
                    self.set_cli_active(false, serial);
                    self.show_prompt(serial);
                    return true;
                }
                false
            }
            Edit::Nothing => true,
            Edit::Appended(byte) => {
                // A passphrase echoes as a star.
                let shown = self.line.shown();
                let _ = serial.write_byte(shown.as_bytes().last().copied().unwrap_or(byte));
                true
            }
            Edit::Redraw => {
                let _ = write!(
                    serial,
                    "\r\x1b[K{}{}",
                    self.prompt_prefix(),
                    self.line.shown()
                );
                true
            }
            Edit::Choices(words) => {
                let _ = serial.write_str("\r\n");
                let mut listed = String::new();
                for word in &words {
                    listed.push_str(word);
                    listed.push_str("  ");
                }
                self.emit_line(serial, listed.trim_end());
                let _ = write!(serial, "{}{}", self.prompt_prefix(), self.line.shown());
                true
            }
            Edit::Submit => {
                let _ = serial.write_str("\r\n");
                let command = self.line.take();
                let command = command.trim();
                self.line.remember(command);
                if command.is_empty() {
                    self.show_prompt(serial);
                    return true;
                }
                let was_cli = self.cli_active;
                self.run_command_line(command, ctx, serial);
                // Leaving the CLI said so itself.
                if !was_cli || self.cli_active {
                    self.show_prompt(serial);
                }
                true
            }
        }
    }

    // TODO: Re-enable when dependencies are no_std
    // /// Process input for the editor
    // fn process_editor_input(
    //     &mut self,
    //     editor: &mut Editor,
    //     byte: u8,
    //     serial: &mut SerialPort,
    // ) -> bool {
    //     // Convert byte to KeyEvent
    //     let key_event = match byte {
    //         b'\r' | b'\n' => KeyEvent::pressed(KeyCode::Enter, Modifiers::none()),
    //         0x08 | 0x7f => KeyEvent::pressed(KeyCode::Backspace, Modifiers::none()),
    //         0x1b => KeyEvent::pressed(KeyCode::Escape, Modifiers::none()),
    //         byte if byte >= 0x20 && byte < 0x7F => {
    //             KeyEvent::pressed(KeyCode::Char(byte as char), Modifiers::none())
    //         }
    //         _ => return false,
    //     };
    //
    //     // Process input through editor
    //     let result = editor.process_input(InputEvent::Key(key_event));
    //
    //     // Check if editor wants to quit
    //     match result {
    //         Ok(EditorAction::Quit) => {
    //             self.active_component = None;
    //             self.editor = None;
    //             self.emit_line(serial, "\r\nEditor closed");
    //             let _ = write!(serial, "> ");
    //         }
    //         Ok(EditorAction::Continue) => {
    //             // Render editor state to serial
    //             self.render_editor_to_serial(editor, serial);
    //         }
    //         Err(e) => {
    //             use alloc::format;
    //             self.emit_line(serial, &format!("\r\nEditor error: {}", e));
    //         }
    //     }
    //
    //     true
    // }
    //
    // /// Render editor to serial port
    // fn render_editor_to_serial(&self, editor: &Editor, serial: &mut SerialPort) {
    //     // Clear screen and render editor
    //     let _ = serial.write_str("\x1b[2J\x1b[H"); // ANSI clear screen + home
    //     let render = editor.render();
    //     let _ = serial.write_str(&render);
    // }

    /// Run what is on the line, as if Enter had been pressed.
    fn execute_command(&mut self, ctx: &mut KernelContext, serial: &mut SerialPort) {
        let command = self.line.take();
        let command = command.trim();
        if command.is_empty() {
            self.show_prompt(serial);
            return;
        }
        self.line.remember(command);
        self.run_command_line(command, ctx, serial);
        self.show_prompt(serial);
    }

    /// Run a command line (shared between normal prompt and CLI)
    fn run_command_line(
        &mut self,
        command: &str,
        ctx: &mut KernelContext,
        serial: &mut SerialPort,
    ) {
        let mut parts = command.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        if cmd.is_empty() {
            return;
        }

        if cmd == "clear" || cmd == "cls" {
            self.request_clear();
            return;
        }

        if cmd == "display" {
            self.emit_command_line(serial, command.as_bytes());
            self.run_display_command(serial, parts.next());
            return;
        }

        if cmd == "pointer" {
            self.emit_command_line(serial, command.as_bytes());
            self.run_pointer_command(serial);
            return;
        }

        if cmd == "gfx" {
            self.emit_command_line(serial, command.as_bytes());
            match parts.next() {
                Some("reset") => {
                    self.gfx_reset_requested = true;
                    self.emit_line(serial, "gfx: counters reset");
                }
                None | Some("stats") => match self.gfx_snapshot {
                    Some(snapshot) => {
                        for line in snapshot.lines() {
                            self.emit_line(serial, &line);
                        }
                    }
                    None => self.emit_line(serial, "gfx: no telemetry yet"),
                },
                Some(_) => self.emit_line(serial, "Usage: gfx [stats | reset]"),
            }
            return;
        }

        // Asking the network (NET-033): the loop starts it, and the
        // answer arrives as lines while the prompt stays free.
        if let Some(request) = net_request_of(command) {
            self.emit_command_line(serial, command.as_bytes());
            match request {
                Some(request) => self.net_request = Some(request),
                None => self.emit_line(
                    serial,
                    "Usage: fetch http://host[:port]/path | resolve <name> [server[:port]]",
                ),
            }
            return;
        }

        if cmd == "net" {
            self.emit_command_line(serial, command.as_bytes());
            self.delegate_to_command_service(ctx, serial, command);
            return;
        }

        // Threads (PROC-001): the table, a busy one to watch, and asking
        // one to stop.
        // Programs (PROC-002): `run` starts one in ring 3; `programs`
        // lists what there is to run.
        if cmd == "run" || cmd == "programs" {
            self.emit_command_line(serial, command.as_bytes());
            match (cmd, parts.next()) {
                ("run", Some(name)) => self.program_request = Some(String::from(name)),
                ("run", None) => self.emit_line(serial, "Usage: run <program> (see `programs`)"),
                _ =>
                {
                    #[cfg(not(test))]
                    for (name, what) in crate::programs::CATALOG {
                        self.emit_line(serial, &format!("  {name:<8} {what}"));
                    }
                }
            }
            return;
        }

        if matches!(cmd, "threads" | "spin" | "stop" | "after") {
            self.emit_command_line(serial, command.as_bytes());
            let arg = parts.next().and_then(|n| n.parse::<u64>().ok());
            #[cfg(all(not(test), target_os = "none"))]
            match (cmd, arg) {
                ("threads", _) => {
                    for line in crate::threads::listing() {
                        self.emit_line(serial, &line);
                    }
                }
                ("spin", seconds) => {
                    let seconds = seconds.unwrap_or(5).clamp(1, 600);
                    match crate::threads::start_spin(seconds) {
                        Ok(id) => self.emit_line(
                            serial,
                            &format!("spin: thread {id} counting primes for {seconds} s"),
                        ),
                        Err(why) => self.emit_line(serial, why),
                    }
                }
                ("after", Some(seconds)) => {
                    let words: Vec<&str> = parts.collect();
                    let seconds = seconds.clamp(1, 86_400);
                    let words = if words.is_empty() {
                        String::from("time's up")
                    } else {
                        words.join(" ")
                    };
                    match crate::threads::start_after(seconds, words) {
                        Ok(id) => self.emit_line(
                            serial,
                            &format!("after: thread {id} asleep for {seconds} s"),
                        ),
                        Err(why) => self.emit_line(serial, why),
                    }
                }
                ("stop", Some(id)) if crate::threads::ask_stop(id as u32) => {
                    self.emit_line(serial, &format!("stop: asked thread {id} to stop"))
                }
                ("stop", Some(id)) => self.emit_line(
                    serial,
                    &format!("stop: no thread {id} (the desk, 0, runs on)"),
                ),
                _ => self.emit_line(
                    serial,
                    "Usage: threads | spin [seconds] | after <seconds> [words] | stop <id>",
                ),
            }
            #[cfg(not(all(not(test), target_os = "none")))]
            {
                let _ = arg;
                self.emit_line(serial, "threads: need the machine");
            }
            return;
        }

        // The machine's generator (SEC-030): bytes, and how it has done.
        if cmd == "random" {
            self.emit_command_line(serial, command.as_bytes());
            let n = parts
                .next()
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(32)
                .clamp(1, 64);
            #[cfg(all(not(test), target_os = "none"))]
            {
                let mut bytes = [0u8; 64];
                crate::random::fill(&mut bytes[..n]);
                let hex: String = bytes[..n].iter().map(|b| format!("{b:02x}")).collect();
                self.emit_line(serial, &hex);
                if let Some((generated, reseeds, hardware)) = crate::random::stats() {
                    self.emit_line(
                        serial,
                        &format!(
                            "random: {generated} bytes given, reseeded {reseeds} times, seeded {}",
                            if hardware {
                                "with the CPU's generator"
                            } else {
                                "from timing alone"
                            }
                        ),
                    );
                }
            }
            #[cfg(not(all(not(test), target_os = "none")))]
            self.emit_line(serial, &format!("random: {n} bytes need the machine"));
            return;
        }

        if cmd == "smp" {
            self.emit_command_line(serial, command.as_bytes());
            self.delegate_to_command_service(ctx, serial, command);
            return;
        }

        if cmd == "fault" {
            // Deliberate CPU exception (see the kernel command service).
            self.emit_command_line(serial, command.as_bytes());
            self.delegate_to_command_service(ctx, serial, command);
            return;
        }

        if cmd == "heap" {
            self.emit_command_line(serial, command.as_bytes());
            let sub = parts.next();
            let arg = parts.next();
            self.run_heap_command(serial, sub, arg);
            return;
        }

        if cmd == "pipeline" {
            self.emit_command_line(serial, command.as_bytes());
            let sub = parts.next();
            let rest = parts.next();
            self.run_pipeline_command(serial, sub, rest);
            return;
        }

        // Parse command
        self.emit_command_line(serial, command.as_bytes());

        // Handle special CLI exit commands
        if self.cli_active && (cmd == "exit" || cmd == "quit") {
            self.set_cli_active(false, serial);
            return;
        }

        match cmd {
            "help" => {
                self.emit_line(serial, "Workspace Commands:");
                self.emit_line(serial, "help           - Show this help");
                for line in crate::access_shell::help() {
                    self.emit_line(serial, &line);
                }
                self.emit_line(serial, "open <what>    - Open editor, CLI, or file picker");
                self.emit_line(serial, "editor [path]  - Open the text editor");
                self.emit_line(serial, "list           - List components");
                self.emit_line(serial, "focus <id>     - Focus component");
                self.emit_line(serial, "clear | cls    - Clear the screen");
                self.emit_line(serial, "display <mode> - Switch text | graphics display");
                self.emit_line(serial, "pointer        - Show pointer position and buttons");
                self.emit_line(
                    serial,
                    "pipeline run a,b,c | status | clear - Run kernel commands as stages",
                );
                self.emit_line(
                    serial,
                    "heap stress <KiB> | release - Hold or free test allocations",
                );
                self.emit_line(serial, "cpus           - Show online CPUs");
                self.emit_line(serial, "smp run <n>    - Run n jobs on the other CPUs");
                self.emit_line(serial, "smp present [on|off] - Spread presents across CPUs");
                self.emit_line(
                    serial,
                    "net [status|ping <ip>|udp <ip> <port> <text>] - Network",
                );
                self.emit_line(
                    serial,
                    "fetch http://host[:port]/path - Get a page over HTTP",
                );
                self.emit_line(
                    serial,
                    "resolve <name> [server[:port]] - Look a name up (DNS)",
                );
                self.emit_line(
                    serial,
                    "random [bytes]  - Random bytes from the machine's generator",
                );
                self.emit_line(
                    serial,
                    "threads        - The machine's threads, and their CPU",
                );
                self.emit_line(
                    serial,
                    "spin [secs]    - A thread that counts primes flat out",
                );
                self.emit_line(serial, "after <secs> [words] - The words, that much later");
                self.emit_line(serial, "stop <id>      - Ask a thread to stop");
                self.emit_line(serial, "run <program>  - Start a program in ring 3");
                self.emit_line(serial, "programs       - The programs there are to run");
                self.emit_line(serial, "gfx [stats|reset] - Show display path telemetry");
                self.emit_line(serial, "quit           - Exit component");
                self.emit_line(serial, "halt           - Halt system");
                self.emit_line(serial, "");
                self.emit_line(
                    serial,
                    "Keys: Up/Down history, Left/Right/Home/End move, Tab finishes, Ctrl+U clears",
                );
                self.emit_line(serial, "");
                self.emit_line(serial, "File Commands:");
                self.emit_line(serial, "ls             - List files");
                self.emit_line(serial, "cat <path>     - Show file contents");
                self.emit_line(serial, "write <path> <text> - Create/update file");
                self.emit_line(serial, "");
                self.emit_line(serial, "System Commands:");
                self.emit_line(serial, "boot           - Show boot info");
                self.emit_line(serial, "mem            - Show memory info");
                self.emit_line(serial, "ticks          - Show system ticks");
            }
            "open" => {
                let what = parts.next();
                match what {
                    Some("editor") => {
                        let path = parts.next();
                        self.open_editor(serial, path);
                    }
                    Some("cli") => {
                        self.active_component = Some(ComponentType::Cli);
                        self.set_cli_active(true, serial);
                    }
                    Some("file") | Some("picker") | Some("files") => {
                        self.open_file_picker(serial);
                    }
                    Some("about") => {
                        self.open_about(serial);
                    }
                    _ => {
                        self.emit_line(serial, "Usage: open editor [path] | open cli | open file");
                    }
                }
            }
            "list" => {
                self.emit_line(serial, "Active components:");
                if let Some(comp) = self.active_component {
                    match comp {
                        ComponentType::Editor => self.emit_line(serial, "  - Editor"),
                        ComponentType::Cli => self.emit_line(serial, "  - Cli"),
                        ComponentType::Shell => self.emit_line(serial, "  - Shell"),
                    }
                } else {
                    self.emit_line(serial, "  (none)");
                }
            }
            "focus" => {
                #[cfg(feature = "console_vga")]
                {
                    self.tile_manager.focus_next();
                    let focused = self.tile_manager.focused_tile();
                    let msg = match focused {
                        TileId::Top => "Focused: Top",
                        TileId::Bottom => "Focused: Bottom",
                    };
                    self.emit_line(serial, msg);
                }
                #[cfg(not(feature = "console_vga"))]
                self.emit_line(serial, "Focus switching unavailable (no console_vga)");
            }
            "quit" => {
                if !self.cli_active {
                    if self.active_component.is_some() {
                        self.active_component = None;
                        // self.editor = None;
                        self.emit_line(serial, "Closed component");
                    } else {
                        self.emit_line(serial, "No active component (use 'halt' to stop)");
                    }
                }
            }
            "editor" => {
                let path = parts.next();
                self.open_editor(serial, path);
            }
            "halt" => {
                self.emit_line(serial, "Halting system...");
                #[cfg(not(test))]
                crate::halt_loop();
            }
            "boot" => {
                // Delegate to existing boot command
                self.delegate_to_command_service(ctx, serial, "boot");
            }
            "mem" => {
                // Delegate to existing mem command
                self.delegate_to_command_service(ctx, serial, "mem");
            }
            "cpus" => {
                self.delegate_to_command_service(ctx, serial, "cpus");
            }
            "ticks" => {
                // Delegate to existing ticks command
                self.delegate_to_command_service(ctx, serial, "ticks");
            }
            "ls" => {
                #[cfg(not(test))]
                {
                    if let Some(ref mut fs) = self.filesystem {
                        match fs.list_files() {
                            Ok(files) => {
                                if files.is_empty() {
                                    self.emit_line(serial, "(no files)");
                                } else {
                                    for file in files {
                                        self.emit_line(serial, &file);
                                    }
                                }
                            }
                            Err(_) => {
                                self.emit_line(serial, "Error: failed to list files");
                            }
                        }
                    } else {
                        self.emit_line(serial, "Error: filesystem not initialized");
                    }
                }
                #[cfg(test)]
                self.emit_line(serial, "ls: not available in test mode");
            }
            "cat" => {
                #[cfg(not(test))]
                {
                    if let Some(path) = parts.next() {
                        if let Some(ref mut fs) = self.filesystem {
                            match fs.read_file_by_name(path) {
                                Ok(content) => match core::str::from_utf8(&content) {
                                    Ok(text) => {
                                        for line in text.lines() {
                                            self.emit_line(serial, line);
                                        }
                                    }
                                    Err(_) => {
                                        self.emit_line(
                                            serial,
                                            "Error: file contains invalid UTF-8",
                                        );
                                    }
                                },
                                Err(_) => {
                                    self.emit_line(
                                        serial,
                                        &format!("Error: file not found: {}", path),
                                    );
                                }
                            }
                        } else {
                            self.emit_line(serial, "Error: filesystem not initialized");
                        }
                    } else {
                        self.emit_line(serial, "Usage: cat <path>");
                    }
                }
                #[cfg(test)]
                self.emit_line(serial, "cat: not available in test mode");
            }
            // Who may do what (FS-005): the filesystem's guard answers.
            word if crate::access_shell::handles(word) => {
                #[cfg(not(test))]
                {
                    if let Some(ref mut fs) = self.filesystem {
                        let now = fs.clock();
                        let lines = crate::access_shell::run(fs, command, now, fresh_salt());
                        for line in lines {
                            self.emit_line(serial, &line);
                        }
                    } else {
                        self.emit_line(serial, "Error: filesystem not initialized");
                    }
                }
                #[cfg(test)]
                {
                    let _ = word;
                    self.emit_line(serial, "access: not available in test mode");
                }
            }
            "write" => {
                #[cfg(not(test))]
                {
                    if let Some(path) = parts.next() {
                        let text = parts.collect::<alloc::vec::Vec<_>>().join(" ");
                        if let Some(ref mut fs) = self.filesystem {
                            match fs.write_file_by_name(path, text.as_bytes()) {
                                Ok(_) => {
                                    self.emit_line(serial, &format!("Wrote to {}", path));
                                }
                                Err(_) => {
                                    self.emit_line(serial, "Error: failed to write file");
                                }
                            }
                        } else {
                            self.emit_line(serial, "Error: filesystem not initialized");
                        }
                    } else {
                        self.emit_line(serial, "Usage: write <path> <text>");
                    }
                }
                #[cfg(test)]
                self.emit_line(serial, "write: not available in test mode");
            }
            _ => {
                self.emit_unknown_command(serial, cmd);
            }
        }
    }

    fn open_editor(&mut self, serial: &mut SerialPort, _path: Option<&str>) {
        self.editor_path = _path.map(|p| p.to_string());
        #[cfg(not(test))]
        {
            // Recover filesystem from any stale editor instance
            if self.filesystem.is_none() {
                if let Some(mut stale_editor) = self.editor.take() {
                    if let Some(io) = stale_editor.editor_io.take() {
                        self.filesystem = Some(io.into_filesystem());
                    }
                }
            }

            let mut editor = MinimalEditor::new(23);

            let open_message: Option<String>;
            let mut open_secondary: Option<String> = None;

            // If we have a filesystem, create an IO adapter and keep it with the editor
            if let Some(fs) = self.filesystem.take() {
                let mut io = BareMetalEditorIo::new(fs);

                // Try to open the file if a path was provided
                if let Some(path) = _path {
                    match io.open(path) {
                        Ok((content, handle)) => {
                            editor.load_content(&content);
                            editor.set_editor_io(io, handle);
                            open_message =
                                Some(alloc::format!("Opened: {} [filesystem available]", path));
                        }
                        Err(_) => {
                            // File not found - create new buffer with IO for save-as
                            let handle = io.new_buffer(Some(path.to_string()));
                            editor.set_editor_io(io, handle);
                            open_message = Some(alloc::format!("File not found: {}", path));
                            open_secondary = Some(
                                "Starting with empty buffer [filesystem available]".to_string(),
                            );
                        }
                    }
                } else {
                    // No path provided - new buffer with no default path
                    let handle = io.new_buffer(None);
                    editor.set_editor_io(io, handle);
                    open_message = Some("New buffer [filesystem available]".to_string());
                }
            } else {
                // No filesystem available
                open_message = Some("Warning: No filesystem - :w will not work".to_string());
            }

            if let Some(message) = open_message {
                self.emit_line(serial, &message);
            }
            if let Some(message) = open_secondary {
                self.emit_line(serial, &message);
            }

            self.editor = Some(editor);
            self.active_component = Some(ComponentType::Editor);
            self.emit_line(
                serial,
                "Keys: i=insert, Esc=normal, h/j/k/l=move, :q=quit, :w=save",
            );
        }
        #[cfg(test)]
        {
            let editor = MinimalEditor::new(23);
            self.editor = Some(editor);
            self.active_component = Some(ComponentType::Editor);
            self.emit_line(serial, "Editor opened (test mode)");
        }
    }

    /// Delegate command to the existing command service
    fn delegate_to_command_service(
        &mut self,
        ctx: &mut KernelContext,
        serial: &mut SerialPort,
        command: &str,
    ) {
        let request_id = ctx.next_message_id();

        let mut command_bytes = [0u8; COMMAND_MAX];
        let cmd_bytes = command.as_bytes();
        let len = cmd_bytes.len().min(COMMAND_MAX);
        command_bytes[..len].copy_from_slice(&cmd_bytes[..len]);

        if let Some(request) =
            CommandRequest::from_bytes(&command_bytes[..len], request_id, self.response_channel)
        {
            if ctx
                .send(self.command_channel, KernelMessage::CommandRequest(request))
                .is_ok()
            {
                // Wait for response (simplified synchronous handling)
                // In a real implementation, this would be async
                self.emit_line(serial, "(Delegated to command service)");
            } else {
                self.emit_line(serial, "Error: command queue full");
            }
        }
    }

    /// Show the initial prompt
    pub fn show_prompt(&self, serial: &mut SerialPort) {
        let _ = write!(serial, "{}", self.prompt_prefix());
    }

    /// Get a text snapshot of the current workspace state for display
    /// Returns command buffer text directly without heap allocation
    ///
    /// As shown: a passphrase being typed is stars (KBD-012).
    pub fn get_command_text(&self) -> String {
        self.line.shown()
    }

    fn set_command_text(&mut self, text: &str) {
        self.line.set(text);
    }

    /// Get the cursor column for the current state
    pub fn get_cursor_col(&self) -> usize {
        self.prompt_prefix().len() + self.line.cursor()
    }

    pub fn prompt_prefix(&self) -> &'static str {
        if self.cli_active {
            "CLI> "
        } else {
            "WS > "
        }
    }

    fn prompt_prefix_bytes(&self) -> &'static [u8] {
        if self.cli_active {
            b"CLI> "
        } else {
            b"WS > "
        }
    }

    pub fn output_line_count(&self) -> usize {
        self.output_count
    }

    /// Monotonic sequence number for output lines
    pub fn output_sequence(&self) -> u64 {
        self.output_seq
    }

    pub fn output_line(&self, index: usize) -> Option<&OutputLine> {
        if index >= self.output_count {
            return None;
        }

        let start = if self.output_head >= self.output_count {
            self.output_head - self.output_count
        } else {
            OUTPUT_MAX_LINES + self.output_head - self.output_count
        };
        let idx = (start + index) % OUTPUT_MAX_LINES;
        Some(&self.output_lines[idx])
    }

    pub fn append_output_text(&mut self, text: &str) {
        for line in text.split('\n') {
            let line = line.trim_end_matches('\r');
            self.push_output_bytes(line.as_bytes());
        }
    }

    /// Record which display mode the loop is currently running.
    pub fn set_display_mode(&mut self, mode: DisplayMode) {
        self.display_mode = mode;
    }

    pub fn display_mode(&self) -> DisplayMode {
        self.display_mode
    }

    /// Whether the loop has a framebuffer that can host the graphics desktop.
    pub fn set_graphics_available(&mut self, available: bool) {
        self.graphics_available = available;
    }

    /// Record whether a pointer device exists.
    pub fn set_pointer_available(&mut self, available: bool) {
        self.pointer_available = available;
    }

    /// Record the latest pointer state (called once per pointer event).
    pub fn set_pointer_state(&mut self, x: i32, y: i32, buttons: u8) {
        self.pointer_position = (x, y);
        self.pointer_buttons = buttons;
        self.pointer_events = self.pointer_events.wrapping_add(1);
    }

    /// Record the desktop router's view of the pointer (graphics mode).
    pub fn set_pointer_routing(
        &mut self,
        over: Option<&'static str>,
        focus: Option<&'static str>,
        captured: bool,
    ) {
        self.pointer_over = over;
        self.pointer_focus = focus;
        self.pointer_captured = captured;
    }

    pub fn is_pointer_available(&self) -> bool {
        self.pointer_available
    }

    pub fn pointer_position(&self) -> (i32, i32) {
        self.pointer_position
    }

    pub fn pointer_buttons(&self) -> u8 {
        self.pointer_buttons
    }

    fn run_pointer_command(&mut self, serial: &mut SerialPort) {
        if !self.pointer_available {
            self.emit_line(serial, "Pointer: no device");
            return;
        }
        let mut buffer = [0u8; OUTPUT_LINE_MAX];
        let mut cursor = 0usize;
        cursor = append_bytes(&mut buffer, cursor, b"Pointer: x=");
        cursor = append_i32(&mut buffer, cursor, self.pointer_position.0);
        cursor = append_bytes(&mut buffer, cursor, b" y=");
        cursor = append_i32(&mut buffer, cursor, self.pointer_position.1);
        cursor = append_bytes(&mut buffer, cursor, b" buttons=");
        for bit in 0..3 {
            let down = self.pointer_buttons & (1 << bit) != 0;
            cursor = append_bytes(&mut buffer, cursor, if down { b"1" } else { b"0" });
        }
        cursor = append_bytes(&mut buffer, cursor, b" events=");
        cursor = append_i32(
            &mut buffer,
            cursor,
            i32::try_from(self.pointer_events.saturating_sub(1)).unwrap_or(i32::MAX),
        );
        let line = core::str::from_utf8(&buffer[..cursor]).unwrap_or("Pointer: ?");
        self.emit_line(serial, line);

        let mut buffer = [0u8; OUTPUT_LINE_MAX];
        let mut cursor = 0usize;
        cursor = append_bytes(&mut buffer, cursor, b"Routing: over=");
        cursor = append_bytes(
            &mut buffer,
            cursor,
            self.pointer_over.unwrap_or("desktop").as_bytes(),
        );
        cursor = append_bytes(&mut buffer, cursor, b" focus=");
        cursor = append_bytes(
            &mut buffer,
            cursor,
            self.pointer_focus.unwrap_or("none").as_bytes(),
        );
        cursor = append_bytes(&mut buffer, cursor, b" capture=");
        cursor = append_bytes(
            &mut buffer,
            cursor,
            if self.pointer_captured { b"1" } else { b"0" },
        );
        let line = core::str::from_utf8(&buffer[..cursor]).unwrap_or("Routing: ?");
        self.emit_line(serial, line);
    }

    /// Take the program `run` asked for.
    pub fn take_program_request(&mut self) -> Option<String> {
        self.program_request.take()
    }

    /// Take the `fetch` or `resolve` the Terminal asked for.
    pub fn take_net_request(&mut self) -> Option<NetRequest> {
        self.net_request.take()
    }

    /// A line a thread left (PROC-001), into the transcript.
    pub fn emit_thread_line(&mut self, serial: &mut SerialPort, line: &str) {
        self.emit_line(serial, line);
    }

    /// A line of the network's answer, into the transcript.
    pub fn emit_net_line(&mut self, serial: &mut SerialPort, line: &str) {
        self.emit_line(serial, line);
    }

    /// Take a pending display-mode switch requested by the user.
    pub fn consume_display_mode_request(&mut self) -> Option<DisplayMode> {
        self.display_mode_request.take()
    }

    /// Ask for a display switch from outside the command line -- the desk's
    /// palette does, for "Switch to the text console".
    pub fn request_display_mode(&mut self, mode: DisplayMode) {
        self.display_mode_request = Some(mode);
    }

    fn run_display_command(&mut self, serial: &mut SerialPort, arg: Option<&str>) {
        match arg {
            None | Some("status") => {
                let line = if self.graphics_available {
                    match self.display_mode {
                        DisplayMode::TextConsole => "Display: text (graphics available)",
                        DisplayMode::GraphicsDesktop => "Display: graphics (text available)",
                        DisplayMode::Desk => "Display: desk (text available)",
                    }
                } else {
                    "Display: text (no framebuffer; graphics unavailable)"
                };
                self.emit_line(serial, line);
            }
            Some(name) => match DisplayMode::parse(name) {
                Some(DisplayMode::GraphicsDesktop) | Some(DisplayMode::Desk)
                    if !self.graphics_available =>
                {
                    self.emit_line(
                        serial,
                        "Graphics display needs a framebuffer; staying in text mode.",
                    );
                }
                Some(mode) if mode == self.display_mode => {
                    self.emit_line(serial, "Display mode unchanged.");
                }
                Some(mode) => {
                    self.display_mode_request = Some(mode);
                    let text = match mode {
                        DisplayMode::TextConsole => "Switching display to text.",
                        DisplayMode::GraphicsDesktop => "Switching display to graphics.",
                        DisplayMode::Desk => "Switching display to desk.",
                    };
                    self.emit_line(serial, text);
                    self.push_notice(NoticeLevel::Info, text);
                }
                None => {
                    self.emit_line(serial, "Usage: display [text | graphics | desk | status]");
                }
            },
        }
    }

    /// Execute a command chosen from the palette or the shell launcher.
    ///
    /// Returns true when the workspace state changed (it always does: at
    /// minimum the palette closes or output is appended).
    pub fn run_palette_command(
        &mut self,
        cmd_id: CommandId,
        ctx: &mut KernelContext,
        serial: &mut SerialPort,
    ) -> bool {
        let _ = writeln!(serial, "  palette_action=execute cmd={}", cmd_id);

        match cmd_id.as_str() {
            "open_editor" => {
                self.open_editor(serial, None);
                self.palette_overlay.close();
                return true;
            }
            "open_cli" => {
                self.active_component = Some(ComponentType::Cli);
                self.set_cli_active(true, serial);
                self.palette_overlay.close();
                return true;
            }
            "open_file" => {
                self.palette_overlay.close();
                self.open_file_picker(serial);
                return true;
            }
            "open_about" => {
                self.palette_overlay.close();
                self.open_about(serial);
                return true;
            }
            "quit" => {
                self.active_component = None;
                self.emit_line(serial, "Closed component");
                self.palette_overlay.close();
                return true;
            }
            _ => {}
        }

        let prompt_pattern = self
            .command_palette
            .get_command(&cmd_id)
            .and_then(|descriptor| descriptor.prompt_pattern.clone());
        if let Some(pattern) = prompt_pattern {
            self.set_command_text(&pattern);
            self.palette_overlay.close();
            return true;
        }

        // Execute command
        let result = self.command_palette.execute_command(&cmd_id, &[]);
        match result {
            Ok(msg) => {
                let _ = writeln!(serial, "  palette_result=success msg={}", msg);
                self.append_output_text(&msg);
            }
            Err(err) => {
                let _ = writeln!(serial, "  palette_result=error err={}", err);
                self.append_output_text(&err);
            }
        }

        match cmd_id.as_str() {
            "help" | "list" | "halt" | "boot" | "mem" | "cpus" | "ticks" | "ls" | "clear" => {
                self.set_command_text(cmd_id.as_str());
                self.execute_command(ctx, serial);
            }
            _ => {}
        }

        // Close palette after execution
        self.palette_overlay.close();
        true
    }

    /// Store the latest telemetry snapshot (called by the loop each frame).
    pub fn set_gfx_snapshot(&mut self, snapshot: GfxSnapshot) {
        self.gfx_snapshot = Some(snapshot);
    }

    pub fn consume_gfx_reset(&mut self) -> bool {
        core::mem::take(&mut self.gfx_reset_requested)
    }

    fn run_heap_command(&mut self, serial: &mut SerialPort, sub: Option<&str>, arg: Option<&str>) {
        match sub {
            Some("stress") => {
                let kib: usize = arg.and_then(|a| a.parse().ok()).unwrap_or(0);
                if kib == 0 {
                    self.emit_line(serial, "Usage: heap stress <KiB>");
                    return;
                }
                // Allocate in 1 MiB pieces so a refusal is granular.
                let mut held = 0usize;
                let mut remaining = kib * 1024;
                while remaining > 0 {
                    let piece = remaining.min(1024 * 1024);
                    let mut block = Vec::new();
                    if block.try_reserve_exact(piece).is_err() {
                        break;
                    }
                    block.resize(piece, 0xA5);
                    self.stress_blocks.push(block);
                    held += piece;
                    remaining -= piece;
                }
                let line = format!("heap stress: holding {} KiB more", held / 1024);
                self.emit_line(serial, &line);
            }
            Some("release") => {
                let total: usize = self.stress_blocks.iter().map(Vec::len).sum();
                self.stress_blocks.clear();
                let line = format!("heap stress: released {} KiB", total / 1024);
                self.emit_line(serial, &line);
            }
            _ => self.emit_line(serial, "Usage: heap stress <KiB> | heap release"),
        }
    }

    fn run_pipeline_command(
        &mut self,
        serial: &mut SerialPort,
        sub: Option<&str>,
        rest: Option<&str>,
    ) {
        match sub {
            Some("run") => {
                let Some(spec) = rest else {
                    self.emit_line(serial, "Usage: pipeline run <cmd>[,<cmd>...]");
                    return;
                };
                if self
                    .pipeline_run
                    .as_ref()
                    .is_some_and(|run| !run.is_finished())
                {
                    self.emit_line(serial, "A pipeline is already running.");
                    return;
                }
                let stages: Vec<StageRun> = spec
                    .split(',')
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .map(|c| StageRun {
                        command: c.to_string(),
                        state: StageState::Pending,
                    })
                    .collect();
                if stages.is_empty() {
                    self.emit_line(serial, "Usage: pipeline run <cmd>[,<cmd>...]");
                    return;
                }
                if self.pipeline_channel.is_none() {
                    self.emit_line(serial, "Pipeline runtime unavailable (no reply channel).");
                    return;
                }
                let count = stages.len();
                self.pipeline_run = Some(PipelineRun {
                    stages,
                    started: 0,
                    finished: None,
                });
                let mut line = String::from("Pipeline started: ");
                line.push_str(&format!("{} stage(s)", count));
                self.emit_line(serial, &line);
            }
            None | Some("status") => match &self.pipeline_run {
                None => self.emit_line(serial, "No pipeline run."),
                Some(run) => {
                    let lines = pipeline_trace_lines(run);
                    let header = format!(
                        "Pipeline: {}/{} stages done{}",
                        run.done_count(),
                        run.stages.len(),
                        if run.failed() { " (failed)" } else { "" }
                    );
                    self.emit_line(serial, &header);
                    for line in lines {
                        self.emit_line(serial, &line);
                    }
                }
            },
            Some("clear") => {
                self.pipeline_run = None;
                self.emit_line(serial, "Pipeline cleared.");
            }
            Some(_) => self.emit_line(
                serial,
                "Usage: pipeline run <cmd>[,<cmd>...] | status | clear",
            ),
        }
    }

    /// Install the reply channel pipeline stages use.
    pub fn set_pipeline_channel(&mut self, channel: ChannelId) {
        self.pipeline_channel = Some(channel);
    }

    pub fn pipeline_run(&self) -> Option<&PipelineRun> {
        self.pipeline_run.as_ref()
    }

    /// Advance the pipeline: submit the next pending stage to the kernel
    /// command service when nothing is running, and consume the response of
    /// the running stage. Returns true when the trace changed.
    pub fn pipeline_poll(
        &mut self,
        ctx: &mut KernelContext,
        serial: &mut SerialPort,
        now: u64,
    ) -> bool {
        let Some(reply_channel) = self.pipeline_channel else {
            return false;
        };
        let Some(run) = self.pipeline_run.as_mut() else {
            return false;
        };
        if run.is_finished() {
            return false;
        }
        if run.started == 0 {
            run.started = now;
        }
        let mut changed = false;

        // Consume responses for the running stage.
        if let Some(index) = run.running_index() {
            let StageState::Running {
                request_id,
                started,
            } = run.stages[index].state.clone()
            else {
                unreachable!()
            };
            while let Some(message) = ctx.try_recv(reply_channel) {
                let KernelMessage::CommandResponse(response) = message else {
                    continue;
                };
                if response.correlation_id != request_id {
                    continue;
                }
                let ticks = now.saturating_sub(started);
                let state = match &response.status {
                    CommandStatus::Ok => StageState::Succeeded {
                        ticks,
                        summary: response
                            .output_str()
                            .and_then(|o| o.lines().next())
                            .unwrap_or("")
                            .to_string(),
                    },
                    CommandStatus::Error(err) => StageState::Failed {
                        ticks,
                        error: err.as_str().unwrap_or("error").to_string(),
                    },
                };
                let ok = matches!(state, StageState::Succeeded { .. });
                let _ = writeln!(
                    serial,
                    "pipeline: stage {} {} ({} ticks)",
                    index + 1,
                    if ok { "ok" } else { "failed" },
                    ticks
                );
                run.stages[index].state = state;
                changed = true;
                if !ok {
                    run.finished = Some(now);
                }
                break;
            }
        }

        // Submit the next pending stage.
        if run.running_index().is_none() && !run.is_finished() {
            match run
                .stages
                .iter()
                .position(|s| matches!(s.state, StageState::Pending))
            {
                Some(index) => {
                    let request_id = ctx.next_message_id();
                    let command = run.stages[index].command.clone();
                    let bytes = command.as_bytes();
                    let len = bytes.len().min(COMMAND_MAX);
                    match CommandRequest::from_bytes(&bytes[..len], request_id, reply_channel) {
                        Some(request)
                            if ctx
                                .send(self.command_channel, KernelMessage::CommandRequest(request))
                                .is_ok() =>
                        {
                            run.stages[index].state = StageState::Running {
                                request_id,
                                started: now,
                            };
                        }
                        _ => {
                            run.stages[index].state = StageState::Failed {
                                ticks: 0,
                                error: "could not submit".to_string(),
                            };
                            run.finished = Some(now);
                        }
                    }
                    changed = true;
                }
                None => {
                    run.finished = Some(now);
                    changed = true;
                    let total = now.saturating_sub(run.started);
                    let _ = writeln!(serial, "pipeline: finished in {} ticks", total);
                    let summary = format!(
                        "Pipeline finished: {} stage(s) in {} ticks",
                        run.stages.len(),
                        total
                    );
                    self.push_output_bytes(summary.as_bytes());
                    self.push_notice(NoticeLevel::Success, &summary);
                }
            }
        } else if changed && run.is_finished() && run.failed() {
            let text = format!("Pipeline failed at stage {}", run.done_count());
            self.push_output_bytes(text.as_bytes());
            self.push_notice(NoticeLevel::Error, &text);
        }
        changed
    }

    /// Open the About component: the reference custom hosted app.
    pub fn open_about(&mut self, serial: &mut SerialPort) {
        if self.is_editor_active() {
            self.emit_line(serial, "Close the editor first (:q).");
            return;
        }
        self.file_picker = None;
        let entries = vec![
            "PandaGen: a clean-slate OS runtime".to_string(),
            "No POSIX, no paths, capabilities everywhere".to_string(),
            format!("Display: {} mode", self.display_mode.label()),
            format!(
                "Pointer: {}",
                if self.pointer_available {
                    "PS/2 mouse"
                } else {
                    "none"
                }
            ),
            "Enter: copy line to output    Esc: close".to_string(),
        ];
        self.hosted = Some(
            ListComponent::new("About PandaGen", entries)
                .with_status("About: Up/Down or hover, Enter activates, Esc closes"),
        );
        self.emit_line(serial, "About opened");
    }

    pub fn has_hosted(&self) -> bool {
        self.hosted.is_some()
    }

    /// Surface of the hosted component for `rows` content rows.
    pub fn hosted_surface(&self, rows: usize) -> Option<HostedSurface> {
        self.hosted.as_ref().map(|c| c.surface(rows))
    }

    /// Deliver an event to the hosted component. Returns true when a redraw
    /// is needed (state changed or the component closed).
    pub fn host_event(&mut self, event: HostEvent, serial: &mut SerialPort) -> bool {
        let Some(component) = self.hosted.as_mut() else {
            return false;
        };
        match component.handle(event) {
            HostResponse::Ignored => false,
            HostResponse::Redraw => {
                if let Some(index) = component.take_activated() {
                    let line = component.entries.get(index).cloned().unwrap_or_default();
                    let _ = writeln!(serial, "hosted: activated {}", index);
                    self.push_output_bytes(line.as_bytes());
                }
                true
            }
            HostResponse::Close => {
                self.hosted = None;
                self.emit_line(serial, "About closed");
                true
            }
        }
    }

    /// Open the file picker over the root listing.
    pub fn open_file_picker(&mut self, serial: &mut SerialPort) {
        if self.is_editor_active() {
            self.emit_line(serial, "Close the editor first (:q) to pick a file.");
            return;
        }
        #[cfg(not(test))]
        let entries = match self.filesystem.as_mut() {
            Some(fs) => match fs.list_files() {
                Ok(files) => files,
                Err(_) => {
                    self.emit_line(serial, "Failed to list files");
                    return;
                }
            },
            None => {
                self.emit_line(serial, "No filesystem available");
                return;
            }
        };
        #[cfg(test)]
        let entries: Vec<String> = Vec::new();
        if entries.is_empty() {
            self.emit_line(serial, "(no files)");
            return;
        }
        self.emit_line(serial, "Files (Up/Down select, Enter open, Esc close):");
        for entry in &entries {
            let mut line = String::from("  ");
            line.push_str(entry);
            self.emit_line(serial, &line);
        }
        self.file_picker = Some(FilePickerState {
            entries,
            selection: 0,
        });
    }

    pub fn is_file_picker_open(&self) -> bool {
        self.file_picker.is_some()
    }

    pub fn file_picker(&self) -> Option<&FilePickerState> {
        self.file_picker.as_ref()
    }

    fn picker_move(&mut self, delta: i32) {
        if let Some(picker) = self.file_picker.as_mut() {
            let last = picker.entries.len().saturating_sub(1);
            picker.selection = if delta < 0 {
                picker
                    .selection
                    .saturating_sub(delta.unsigned_abs() as usize)
            } else {
                picker.selection.saturating_add(delta as usize).min(last)
            };
        }
    }

    /// Pointer hover over a picker entry. Returns true if the selection moved.
    pub fn picker_hover(&mut self, index: usize) -> bool {
        match self.file_picker.as_mut() {
            Some(picker) if index < picker.entries.len() => {
                let changed = picker.selection != index;
                picker.selection = index;
                changed
            }
            _ => false,
        }
    }

    /// Wheel over the picker: positive notches move up.
    pub fn picker_scroll(&mut self, notches: i32) -> bool {
        if self.file_picker.is_none() || notches == 0 {
            return false;
        }
        self.picker_move(-notches);
        true
    }

    /// Open the selected entry in the editor and close the picker.
    pub fn picker_open_selection(&mut self, serial: &mut SerialPort) -> bool {
        let Some(picker) = self.file_picker.take() else {
            return false;
        };
        let Some(name) = picker.entries.get(picker.selection).cloned() else {
            return false;
        };
        let _ = writeln!(serial, "picker: open {}", name);
        self.active_component = Some(ComponentType::Editor);
        self.open_editor(serial, Some(&name));
        true
    }

    /// Click on a picker entry: select and open it.
    pub fn picker_click(&mut self, index: usize, serial: &mut SerialPort) -> bool {
        // Clicking the blank space below the list used to clamp onto the last
        // entry and open it. A click that is not on an entry is not a click.
        let on_an_entry = self
            .file_picker
            .as_ref()
            .is_some_and(|picker| index < picker.entries.len());
        if !on_an_entry {
            return false;
        }
        self.picker_hover(index);
        self.picker_open_selection(serial)
    }

    /// Resize the editor viewport to `rows` (graphics window height). Returns
    /// true when the editor exists and its viewport changed.
    pub fn set_editor_viewport_rows(&mut self, rows: usize) -> bool {
        match self.editor.as_mut() {
            Some(editor) if editor.viewport_rows() != rows.max(1) => {
                editor.set_viewport_rows(rows);
                true
            }
            _ => false,
        }
    }

    /// Display name of the editor document.
    pub fn editor_title(&self) -> String {
        match &self.editor_path {
            Some(path) => path.clone(),
            None => "new buffer".to_string(),
        }
    }

    /// Pointer hover over a palette result: move the selection there.
    /// Returns true when the selection changed.
    pub fn palette_hover_result(&mut self, index: usize) -> bool {
        if !self.palette_overlay.is_open() {
            return false;
        }
        let before = self.palette_overlay.selection_index();
        self.palette_overlay.set_selection(index);
        before != self.palette_overlay.selection_index()
    }

    /// Wheel over the palette: positive notches move the selection up.
    pub fn palette_scroll(&mut self, notches: i32) -> bool {
        if !self.palette_overlay.is_open() || notches == 0 {
            return false;
        }
        for _ in 0..notches.unsigned_abs() {
            if notches > 0 {
                self.palette_overlay.move_selection_up();
            } else {
                self.palette_overlay.move_selection_down();
            }
        }
        true
    }

    /// Click on a palette result: select it and run it.
    pub fn palette_click_result(
        &mut self,
        index: usize,
        ctx: &mut KernelContext,
        serial: &mut SerialPort,
    ) -> bool {
        if !self.palette_overlay.is_open() {
            return false;
        }
        // A click must land on a result, not near one.
        if index >= self.palette_overlay.displayed_results().len() {
            return false;
        }
        self.palette_overlay.set_selection(index);
        let Some(cmd_id) = self.palette_overlay.selected_command().cloned() else {
            return false;
        };
        let _ = writeln!(serial, "palette click: index={} cmd={}", index, cmd_id);
        self.run_palette_command(cmd_id, ctx, serial)
    }

    /// Click outside the palette dismisses it.
    pub fn palette_dismiss(&mut self) -> bool {
        if self.palette_overlay.is_open() {
            self.palette_overlay.close();
            true
        } else {
            false
        }
    }

    /// Launcher entries: enabled commands that need no arguments, sorted by
    /// name so the strip is stable across frames. The flag marks the entry
    /// for the component currently open.
    pub fn launcher_items(&self) -> Vec<(CommandId, String, bool)> {
        let mut items: Vec<(CommandId, String, bool)> = self
            .command_palette
            .list_commands()
            .into_iter()
            .filter(|d| d.enabled && !d.requires_args && d.prompt_pattern.is_none())
            .map(|d| {
                let active = match d.id.as_str() {
                    "open_editor" => self.is_editor_active(),
                    "open_cli" => self.cli_active,
                    _ => false,
                };
                (d.id.clone(), d.name.clone(), active)
            })
            .collect();
        items.sort_by(|a, b| a.1.cmp(&b.1));
        items
    }

    /// Run the launcher entry shown on `line` (display order).
    pub fn activate_launcher_line(
        &mut self,
        line: usize,
        ctx: &mut KernelContext,
        serial: &mut SerialPort,
    ) -> bool {
        let items = self.launcher_items();
        match items.get(line) {
            Some((id, _, _)) => {
                let id = id.clone();
                let _ = writeln!(serial, "launcher: line={} cmd={}", line, id);
                self.run_palette_command(id, ctx, serial)
            }
            None => false,
        }
    }

    /// Queue a shell notice (newest first, oldest dropped past capacity).
    pub fn push_notice(&mut self, level: NoticeLevel, text: &str) {
        let mut line = String::new();
        line.push_str(text);
        self.notices.insert(
            0,
            PendingNotice {
                notice: ShellNotice::new(level, line),
                shown_at: None,
            },
        );
        self.notices.truncate(MAX_SHELL_NOTICES);
    }

    /// Stamp unstamped notices with `now`, drop those older than `ttl`
    /// ticks, and return the tick at which the next one expires.
    pub fn stamp_and_expire_notices(&mut self, now: u64, ttl: u64) -> Option<u64> {
        for pending in &mut self.notices {
            if pending.shown_at.is_none() {
                pending.shown_at = Some(now);
            }
        }
        self.notices
            .retain(|p| p.shown_at.is_none_or(|t| now.saturating_sub(t) < ttl));
        self.notices
            .iter()
            .filter_map(|p| p.shown_at.map(|t| t.saturating_add(ttl)))
            .min()
    }

    /// Current notices, newest first.
    pub fn notices(&self) -> Vec<ShellNotice> {
        self.notices.iter().map(|p| p.notice.clone()).collect()
    }

    pub fn consume_clear_request(&mut self) -> bool {
        if self.clear_requested {
            self.clear_requested = false;
            true
        } else {
            false
        }
    }

    /// Clear the console's transcript -- also when someone signs in or out
    /// (FS-006), so the next person does not read the last one's.
    pub fn request_clear(&mut self) {
        self.clear_requested = true;
        self.output_head = 0;
        self.output_count = 0;
        self.output_seq = self.output_seq.wrapping_add(1);
    }

    /// Check if editor is active
    pub fn is_editor_active(&self) -> bool {
        self.active_component == Some(ComponentType::Editor) && self.editor.is_some()
    }

    /// Whether the command palette is taking input.
    pub fn palette_is_open(&self) -> bool {
        self.palette_overlay.is_open()
    }

    /// Get reference to the editor
    pub fn editor(&self) -> Option<&MinimalEditor> {
        self.editor.as_ref()
    }

    /// Check if the command palette is open
    pub fn is_palette_open(&self) -> bool {
        self.palette_overlay.is_open()
    }

    /// Get reference to the palette overlay state (for rendering)
    pub fn palette_overlay(&self) -> &PaletteOverlayState {
        &self.palette_overlay
    }

    fn emit_line(&mut self, serial: &mut SerialPort, text: &str) {
        let _ = writeln!(serial, "{}", text);
        self.append_output_text(text);
    }

    fn emit_unknown_command(&mut self, serial: &mut SerialPort, cmd: &str) {
        let mut buffer = [0u8; OUTPUT_LINE_MAX];
        let mut len = 0usize;

        let prefix = b"Unknown command: ";
        len = append_bytes(&mut buffer, len, prefix);
        len = append_bytes(&mut buffer, len, cmd.as_bytes());
        len = append_bytes(&mut buffer, len, b". Type 'help' for help.");

        let line = core::str::from_utf8(&buffer[..len]).unwrap_or("Unknown command.");
        self.emit_line(serial, line);
        self.push_notice(NoticeLevel::Warning, line);
    }

    /// Scroll the graphical view of the scrollback: positive notches move
    /// toward older lines. `visible` is how many lines fit on screen.
    pub fn scroll_view(&mut self, notches: i32, visible: usize) -> bool {
        let total = self.output_line_count();
        let max = total.saturating_sub(visible);
        let before = self.scrollback_offset;
        self.scrollback_offset = if notches > 0 {
            self.scrollback_offset
                .saturating_add(notches as usize)
                .min(max)
        } else {
            self.scrollback_offset
                .saturating_sub(notches.unsigned_abs() as usize)
        };
        before != self.scrollback_offset
    }

    pub fn scrollback_offset(&self) -> usize {
        self.scrollback_offset
    }

    fn push_output_bytes(&mut self, bytes: &[u8]) {
        // New output snaps the view back to the live tail.
        self.scrollback_offset = 0;
        if bytes.is_empty() {
            self.push_output_line(&[]);
            return;
        }

        let mut offset = 0usize;
        while offset < bytes.len() {
            let remaining = bytes.len() - offset;
            let chunk_len = remaining.min(OUTPUT_LINE_MAX);
            let chunk = &bytes[offset..offset + chunk_len];
            self.push_output_line(chunk);
            offset += chunk_len;
        }
    }

    fn push_output_line(&mut self, bytes: &[u8]) {
        let idx = self.output_head;
        self.output_lines[idx].set_from_bytes(bytes);
        self.output_head = (self.output_head + 1) % OUTPUT_MAX_LINES;
        if self.output_count < OUTPUT_MAX_LINES {
            self.output_count += 1;
        }
        self.output_seq = self.output_seq.wrapping_add(1);
    }

    fn emit_command_line(&mut self, serial: &mut SerialPort, cmd: &[u8]) {
        let mut buffer = [0u8; OUTPUT_LINE_MAX];
        let mut len = 0usize;
        len = append_bytes(&mut buffer, len, self.prompt_prefix_bytes());
        let masked = crate::line_edit::mask(core::str::from_utf8(cmd).unwrap_or(""));
        len = append_bytes(&mut buffer, len, masked.as_bytes());
        let line = core::str::from_utf8(&buffer[..len]).unwrap_or("WS > ");
        self.emit_line(serial, line);
    }

    /// Check if CLI is active
    pub fn is_cli_active(&self) -> bool {
        self.cli_active
    }

    /// Get the current mode indicator string
    pub fn mode_indicator(&self) -> &str {
        if self.cli_active {
            "[CLI Mode] Ctrl+P: Commands"
        } else {
            "[Workspace] Ctrl+P: Commands"
        }
    }

    /// Get the status line hint for the workspace footer
    pub fn status_line(&self) -> &str {
        if self.cli_active {
            "CLI: Enter run | Esc exit | Ctrl+P Commands"
        } else {
            "WS: Ctrl+P Commands | open editor | open cli | help"
        }
    }
}

const OUTPUT_MAX_LINES: usize = 64;
const OUTPUT_LINE_MAX: usize = 80;

#[derive(Copy, Clone)]
pub struct OutputLine {
    len: usize,
    bytes: [u8; OUTPUT_LINE_MAX],
}

impl OutputLine {
    const fn empty() -> Self {
        Self {
            len: 0,
            bytes: [0; OUTPUT_LINE_MAX],
        }
    }

    fn set_from_bytes(&mut self, bytes: &[u8]) {
        let len = bytes.len().min(OUTPUT_LINE_MAX);
        self.bytes[..len].copy_from_slice(&bytes[..len]);
        self.len = len;
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// One text line per stage: `[state] command  detail`.
pub fn pipeline_trace_lines(run: &PipelineRun) -> Vec<String> {
    run.stages
        .iter()
        .map(|stage| match &stage.state {
            StageState::Pending => format!("[ .. ] {}", stage.command),
            StageState::Running { .. } => format!("[ >> ] {}", stage.command),
            StageState::Succeeded { ticks, summary } => {
                format!("[ ok ] {}  {} ticks  {}", stage.command, ticks, summary)
            }
            StageState::Failed { ticks, error } => {
                format!("[FAIL] {}  {} ticks  {}", stage.command, ticks, error)
            }
        })
        .collect()
}

/// Append a signed decimal without allocating.
fn append_i32(buffer: &mut [u8], len: usize, value: i32) -> usize {
    let mut digits = [0u8; 11];
    let mut count = 0usize;
    let mut magnitude = value.unsigned_abs();
    if magnitude == 0 {
        digits[0] = b'0';
        count = 1;
    }
    while magnitude > 0 {
        digits[count] = b'0' + (magnitude % 10) as u8;
        magnitude /= 10;
        count += 1;
    }
    let mut len = if value < 0 {
        append_bytes(buffer, len, b"-")
    } else {
        len
    };
    while count > 0 {
        count -= 1;
        len = append_bytes(buffer, len, &digits[count..count + 1]);
    }
    len
}

fn append_bytes(buffer: &mut [u8], mut len: usize, bytes: &[u8]) -> usize {
    let space = buffer.len().saturating_sub(len);
    let count = bytes.len().min(space);
    if count > 0 {
        buffer[len..len + count].copy_from_slice(&bytes[..count]);
        len += count;
    }
    len
}

/// Sixteen bytes for a passphrase's salt (FS-005): the time-stamp counter
/// and a count of salts made, hashed. Not a cryptographic generator, but a
/// salt only has to be unlikely to repeat.
#[cfg(not(test))]
pub(crate) fn fresh_salt() -> [u8; 16] {
    // From the machine's generator (SEC-030).
    #[cfg(all(not(test), target_os = "none"))]
    {
        let mut salt = [0u8; 16];
        crate::random::fill(&mut salt);
        salt
    }
    #[cfg(not(all(not(test), target_os = "none")))]
    fresh_salt_from_the_clock()
}

#[cfg(not(all(not(test), target_os = "none")))]
fn fresh_salt_from_the_clock() -> [u8; 16] {
    use core::sync::atomic::{AtomicU64, Ordering};
    static MADE: AtomicU64 = AtomicU64::new(0);
    let n = MADE.fetch_add(1, Ordering::Relaxed);
    // SAFETY: RDTSC reads a counter; it has no side effects.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    let mut seed = [0u8; 16];
    seed[..8].copy_from_slice(&tsc.to_le_bytes());
    seed[8..].copy_from_slice(&n.to_le_bytes());
    let digest = remote_ipc::sha256::digest(&seed);
    let mut salt = [0u8; 16];
    salt.copy_from_slice(&digest[..16]);
    salt
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full integration tests with WorkspaceSession require kernel context
    // These are simpler unit tests of individual components

    #[test]
    fn test_fetch_and_resolve_lines_become_requests() {
        assert_eq!(
            net_request_of("fetch example.com/a"),
            Some(Some(NetRequest::Fetch("example.com/a".into())))
        );
        assert_eq!(
            net_request_of("net fetch http://10.0.2.2:18080/"),
            Some(Some(NetRequest::Fetch("http://10.0.2.2:18080/".into())))
        );
        assert_eq!(
            net_request_of("resolve panda.test 10.0.2.2:15353"),
            Some(Some(NetRequest::Resolve {
                name: "panda.test".into(),
                server: Some("10.0.2.2:15353".into())
            }))
        );
        assert_eq!(net_request_of("fetch"), Some(None), "usage");
        assert_eq!(net_request_of("fetch a b"), Some(None), "usage");
        assert_eq!(
            net_request_of("net ping 10.0.2.2"),
            None,
            "the command service's"
        );
        assert_eq!(net_request_of("ls"), None);
    }

    #[test]
    fn test_output_line_creation() {
        let line = OutputLine::empty();
        assert_eq!(line.as_bytes().len(), 0);
    }

    #[test]
    fn test_output_line_set_from_bytes() {
        let mut line = OutputLine::empty();
        line.set_from_bytes(b"Hello");
        assert_eq!(line.as_bytes(), b"Hello");
    }

    #[test]
    fn test_output_line_truncation() {
        let mut line = OutputLine::empty();
        let long_text = [b'x'; OUTPUT_LINE_MAX + 10];
        line.set_from_bytes(&long_text);
        assert_eq!(line.as_bytes().len(), OUTPUT_LINE_MAX);
    }

    #[test]
    fn test_append_bytes_function() {
        let mut buffer = [0u8; 10];
        let len = append_bytes(&mut buffer, 0, b"Hello");
        assert_eq!(len, 5);
        assert_eq!(&buffer[..5], b"Hello");

        let len = append_bytes(&mut buffer, len, b" World");
        assert_eq!(len, 10);
        assert_eq!(&buffer[..10], b"Hello Worl");
    }

    #[test]
    fn test_component_type_display() {
        let editor = ComponentType::Editor;
        let cli = ComponentType::Cli;

        assert_eq!(format!("{}", editor), "Editor");
        assert_eq!(format!("{}", cli), "CLI");
    }

    #[test]
    fn test_cli_state_initialization() {
        let session = WorkspaceSession::new(ChannelId(0), ChannelId(1));
        assert!(!session.is_cli_active());
        assert!(session.line.is_empty());
        assert_eq!(session.line.cursor(), 0);
    }

    #[test]
    fn test_cli_buffer_management() {
        let mut serial = crate::serial::SerialPort::new(0x3F8);

        let mut session = WorkspaceSession::new(ChannelId(0), ChannelId(1));

        // Activate CLI
        session.set_cli_active(true, &mut serial);
        assert!(session.is_cli_active());
        assert!(session.line.is_empty());

        // Reset buffer
        session.line.set("x");
        session.reset_cli_buffer();
        assert!(session.line.is_empty());
        assert_eq!(session.line.cursor(), 0);
    }

    #[test]
    fn test_cli_prompt_display() {
        let mut serial = crate::serial::SerialPort::new(0x3F8);

        let mut session = WorkspaceSession::new(ChannelId(0), ChannelId(1));

        // Normal prompt
        session.show_prompt(&mut serial);
        // Can't directly check output in test mode without accessing SerialPort internals

        // CLI prompt
        session.set_cli_active(true, &mut serial);
        session.show_prompt(&mut serial);
        // Visual inspection shows this works correctly
    }

    #[test]
    fn test_get_command_text_cli_vs_normal() {
        let mut session = WorkspaceSession::new(ChannelId(0), ChannelId(1));

        // Set normal command buffer
        session.set_command_text("hello");
        assert_eq!(session.get_command_text(), "hello");

        // Activate CLI and set CLI buffer
        let mut serial = crate::serial::SerialPort::new(0x3F8);
        session.set_cli_active(true, &mut serial);
        assert_eq!(
            session.get_command_text(),
            "",
            "entering the CLI clears the line"
        );
        session.set_command_text("world");
        assert_eq!(session.get_command_text(), "world");
    }

    #[test]
    fn test_get_cursor_col_cli_vs_normal() {
        let mut session = WorkspaceSession::new(ChannelId(0), ChannelId(1));

        // Normal mode: cursor at end of command
        session.set_command_text("hello");
        assert_eq!(session.get_cursor_col(), "WS > ".len() + 5);

        // CLI mode: cursor position tracked separately
        let mut serial = crate::serial::SerialPort::new(0x3F8);
        session.set_cli_active(true, &mut serial);
        session.set_command_text("0123456789");
        for _ in 0..3 {
            session.line.key(crate::notepad::KEY_LEFT, &[]);
        }
        assert_eq!(session.get_cursor_col(), "CLI> ".len() + 7);
    }

    #[test]
    fn test_status_line_changes_with_cli() {
        let mut session = WorkspaceSession::new(ChannelId(0), ChannelId(1));
        assert!(session.status_line().starts_with("WS:"));

        let mut serial = crate::serial::SerialPort::new(0x3F8);
        session.set_cli_active(true, &mut serial);
        assert!(session.status_line().starts_with("CLI:"));
    }
}
