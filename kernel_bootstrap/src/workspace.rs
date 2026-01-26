//! Minimal workspace manager for bare-metal kernel
//!
//! This provides a workspace-like experience in the bare-metal kernel without
//! requiring the full std-based services_workspace_manager.

#[cfg(not(test))]
extern crate alloc;

use core::fmt::Write;

#[cfg(not(test))]
use alloc::boxed::Box;
#[cfg(not(test))]
use alloc::string::{String, ToString};
#[cfg(test)]
use std::string::{String, ToString};

#[cfg(not(test))]
use alloc::vec;

use crate::serial::SerialPort;
use crate::{ChannelId, CommandRequest, KernelApiV0, KernelContext, KernelMessage, COMMAND_MAX};

use crate::minimal_editor::{EditorMode, MinimalEditor};
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

use services_command_palette::{CommandDescriptor, CommandPalette};

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
    /// Command buffer
    command_buffer: [u8; COMMAND_MAX],
    /// Command length
    command_len: usize,
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
            Box::new(|_| Ok("Available commands: help, editor, quit".to_string())),
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
                "quit",
                "Quit",
                "Exit the workspace",
                vec!["exit".to_string(), "close".to_string(), "q".to_string()],
            ),
            Box::new(|_| Ok("Quitting...".to_string())),
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
            command_buffer: [0; COMMAND_MAX],
            command_len: 0,
            output_lines: [OutputLine::empty(); OUTPUT_MAX_LINES],
            output_head: 0,
            output_count: 0,
            output_seq: 0,
            #[cfg(not(test))]
            filesystem: None,
            palette_overlay: PaletteOverlayState::new(),
            command_palette,
        }
    }

    /// Set the filesystem for this session
    #[cfg(not(test))]
    pub fn set_filesystem(&mut self, fs: BareMetalFilesystem) {
        self.filesystem = Some(fs);
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

        let _ = writeln!(serial, "route_input:");
        let _ = writeln!(serial, "  key={{byte={:#x}}}", byte);
        let _ = writeln!(serial, "  focus_tile={{{:?}}}", focused_tile);

        // 1. Check for global shortcuts BEFORE component routing
        // Ctrl+P (0x10) opens command palette
        if byte == 0x10 && !self.palette_overlay.is_open() {
            let _ = writeln!(serial, "  action=open_palette");

            // Determine current focus target
            let current_focus = if self.active_component == Some(ComponentType::Editor) {
                FocusTarget::Editor
            } else if self.in_command_mode {
                FocusTarget::Cli
            } else {
                FocusTarget::None
            };

            self.palette_overlay.open(current_focus);
            return true;
        }

        // 2. If palette is open, route all input to it
        if self.palette_overlay.is_open() {
            let _ = writeln!(serial, "  action=palette_input");

            // Convert byte to KeyEvent
            if let Some(key_event) = byte_to_key_event(byte) {
                let action = handle_palette_key(
                    &mut self.palette_overlay,
                    &self.command_palette,
                    &key_event,
                );

                match action {
                    PaletteKeyAction::Close => {
                        let _ = writeln!(serial, "  palette_action=close");
                        self.palette_overlay.close();
                        return true;
                    }
                    PaletteKeyAction::Execute(cmd_id) => {
                        let _ = writeln!(serial, "  palette_action=execute cmd={}", cmd_id);

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

                        // Close palette after execution
                        self.palette_overlay.close();
                        return true;
                    }
                    PaletteKeyAction::Consumed => {
                        let _ = writeln!(serial, "  palette_action=consumed");
                        return true;
                    }
                    PaletteKeyAction::None => {
                        let _ = writeln!(serial, "  palette_action=none");
                        // Fall through - shouldn't happen but handle gracefully
                        return false;
                    }
                }
            } else {
                let _ = writeln!(serial, "  palette_action=unknown_byte");
                return true; // Consume unknown bytes when palette is open
            }
        }

        // 3. Check tile focus before delivering to component
        #[cfg(feature = "console_vga")]
        {
            // Editor lives in Top tile. If Bottom is focused, Editor shouldn't get input.
            if self.active_component == Some(ComponentType::Editor) && focused_tile != TileId::Top {
                let _ = writeln!(serial, "  consumed_by=none (focus mismatch)");
                return false;
            }
        }

        // 4. Route to active component (existing logic)
        // If editor is active, route input to it
        #[cfg(not(test))]
        if self.active_component == Some(ComponentType::Editor) {
            if let Some(ref mut editor) = self.editor {
                let _ = writeln!(
                    serial,
                    "  action=process_byte_start cursor={:?}",
                    editor.cursor()
                );
                let should_quit = editor.process_byte(byte);
                let _ = writeln!(
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
                        let line_delta = if new_row > pre_row {
                            new_row - pre_row
                        } else {
                            pre_row - new_row
                        };
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
                        if let Some(mut io) = editor_instance.editor_io.take() {
                            self.filesystem = Some(io.into_filesystem());
                        }
                    }

                    let _ = serial.write_str("\r\nEditor closed\r\n");
                    self.show_prompt(serial);
                }
                return true;
            }
        }

        // Otherwise, handle as command input
        match byte {
            b'\r' | b'\n' => {
                let _ = serial.write_str("\r\n");
                self.execute_command(ctx, serial);
                self.command_len = 0;
                true
            }
            0x08 | 0x7f => {
                // Backspace
                if self.command_len > 0 {
                    self.command_len -= 1;
                    let _ = serial.write_str("\x08 \x08");
                }
                true
            }
            byte if byte >= 0x20 && byte < 0x7F => {
                // Printable character
                if self.command_len < self.command_buffer.len() {
                    self.command_buffer[self.command_len] = byte;
                    self.command_len += 1;
                    let _ = serial.write_byte(byte);
                }
                true
            }
            _ => false,
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

    /// Execute the current command
    fn execute_command(&mut self, ctx: &mut KernelContext, serial: &mut SerialPort) {
        let command_buf = {
            let command = core::str::from_utf8(&self.command_buffer[..self.command_len])
                .unwrap_or("")
                .trim();

            if command.is_empty() {
                let _ = write!(serial, "> ");
                return;
            }

            let mut buffer = [0u8; COMMAND_MAX];
            let bytes = command.as_bytes();
            let len = bytes.len().min(COMMAND_MAX);
            buffer[..len].copy_from_slice(&bytes[..len]);
            (buffer, len)
        };
        let command = core::str::from_utf8(&command_buf.0[..command_buf.1]).unwrap_or("");
        let mut parts = command.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        if cmd.is_empty() {
            let _ = write!(serial, "> ");
            return;
        }

        // Parse command
        self.emit_command_line(serial, command.as_bytes());
        match cmd {
            "help" => {
                self.emit_line(serial, "Workspace Commands:");
                self.emit_line(serial, "help           - Show this help");
                self.emit_line(serial, "open <what>    - Open editor or CLI");
                self.emit_line(serial, "list           - List components");
                self.emit_line(serial, "focus <id>     - Focus component");
                self.emit_line(serial, "quit           - Exit component");
                self.emit_line(serial, "halt           - Halt system");
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

                            // Try to open file from path argument
                            let path = parts.next();
                            let mut editor = MinimalEditor::new(23);

                            let mut open_message: Option<String> = None;
                            let mut open_secondary: Option<String> = None;

                            // If we have a filesystem, create an IO adapter and keep it with the editor
                            if let Some(fs) = self.filesystem.take() {
                                let mut io = BareMetalEditorIo::new(fs);

                                // Try to open the file if a path was provided
                                if let Some(path) = path {
                                    match io.open(path) {
                                        Ok((content, handle)) => {
                                            editor.load_content(&content);
                                            editor.set_editor_io(io, handle);
                                            open_message = Some(alloc::format!(
                                                "Opened: {} [filesystem available]",
                                                path
                                            ));
                                        }
                                        Err(_) => {
                                            // File not found - create new buffer with IO for save-as
                                            let handle = io.new_buffer(Some(path.to_string()));
                                            editor.set_editor_io(io, handle);
                                            open_message =
                                                Some(alloc::format!("File not found: {}", path));
                                            open_secondary = Some(
                                                "Starting with empty buffer [filesystem available]"
                                                    .to_string(),
                                            );
                                        }
                                    }
                                } else {
                                    // No path provided - new buffer with no default path
                                    let handle = io.new_buffer(None);
                                    editor.set_editor_io(io, handle);
                                    open_message =
                                        Some("New buffer [filesystem available]".to_string());
                                }
                            } else {
                                // No filesystem available
                                open_message =
                                    Some("Warning: No filesystem - :w will not work".to_string());
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
                    Some("cli") => {
                        self.active_component = Some(ComponentType::Cli);
                        self.emit_line(serial, "CLI component registered");
                        self.emit_line(
                            serial,
                            "Note: Full CLI requires services_workspace_manager",
                        );
                    }
                    _ => {
                        self.emit_line(serial, "Usage: open editor [path] | open cli");
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
                self.active_component = None;
                // self.editor = None;
                self.emit_line(serial, "Closed component");
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
                                        &alloc::format!("Error: file not found: {}", path),
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
            "write" => {
                #[cfg(not(test))]
                {
                    if let Some(path) = parts.next() {
                        let text = parts.collect::<alloc::vec::Vec<_>>().join(" ");
                        if let Some(ref mut fs) = self.filesystem {
                            match fs.write_file_by_name(path, text.as_bytes()) {
                                Ok(_) => {
                                    self.emit_line(serial, &alloc::format!("Wrote to {}", path));
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

        let _ = write!(serial, "> ");
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
        let _ = write!(serial, "> ");
    }

    /// Get a text snapshot of the current workspace state for display
    /// Returns command buffer text directly without heap allocation
    pub fn get_command_text(&self) -> &[u8] {
        &self.command_buffer[..self.command_len]
    }

    /// Get the cursor column for the current state
    pub fn get_cursor_col(&self) -> usize {
        // ">" is at column 0-1, command starts at column 2
        2 + self.command_len
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

    /// Check if editor is active
    pub fn is_editor_active(&self) -> bool {
        self.active_component == Some(ComponentType::Editor) && self.editor.is_some()
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
    }

    fn push_output_bytes(&mut self, bytes: &[u8]) {
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
        len = append_bytes(&mut buffer, len, b"> ");
        len = append_bytes(&mut buffer, len, cmd);
        let line = core::str::from_utf8(&buffer[..len]).unwrap_or("> ");
        self.emit_line(serial, line);
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

fn append_bytes(buffer: &mut [u8], mut len: usize, bytes: &[u8]) -> usize {
    let space = buffer.len().saturating_sub(len);
    let count = bytes.len().min(space);
    if count > 0 {
        buffer[len..len + count].copy_from_slice(&bytes[..count]);
        len += count;
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full integration tests with WorkspaceSession require kernel context
    // These are simpler unit tests of individual components

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
}
