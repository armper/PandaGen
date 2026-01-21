//! Minimal workspace manager for bare-metal kernel
//!
//! This provides a workspace-like experience in the bare-metal kernel without
//! requiring the full std-based services_workspace_manager.

use core::fmt::Write;

use crate::serial::SerialPort;
use crate::{ChannelId, CommandRequest, KernelApiV0, KernelContext, KernelMessage, COMMAND_MAX};

use crate::minimal_editor::MinimalEditor;

/// Component type in the workspace
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ComponentType {
    Editor,
    Cli,
    Shell,
}

/// Workspace session state
pub struct WorkspaceSession {
    /// Active component type
    active_component: Option<ComponentType>,
    /// Editor instance (bare-metal)
    editor: Option<MinimalEditor>,
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
}

impl WorkspaceSession {
    pub fn new(command_channel: ChannelId, response_channel: ChannelId) -> Self {
        Self {
            active_component: None,
            editor: None,
            command_channel,
            response_channel,
            in_command_mode: true,
            command_buffer: [0; COMMAND_MAX],
            command_len: 0,
            output_lines: [OutputLine::empty(); OUTPUT_MAX_LINES],
            output_head: 0,
            output_count: 0,
            output_seq: 0,
        }
    }

    /// Process a single byte of input
    pub fn process_input(
        &mut self,
        byte: u8,
        ctx: &mut KernelContext,
        serial: &mut SerialPort,
    ) -> bool {
        // If editor is active, route input to it
        #[cfg(not(test))]
        if self.active_component == Some(ComponentType::Editor) {
            if let Some(ref mut editor) = self.editor {
                let should_quit = editor.process_byte(byte);
                if should_quit {
                    self.active_component = None;
                    self.editor = None;
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
                self.emit_line(serial, "System Commands:");
                self.emit_line(serial, "boot           - Show boot info");
                self.emit_line(serial, "mem            - Show memory info");
                self.emit_line(serial, "ticks          - Show system ticks");
            }
            "open" => {
                let what = parts.next();
                match what {
                    Some("editor") => {
                        // Create editor with viewport size (e.g., 23 rows for VGA 80x25 minus status line)
                        let editor = MinimalEditor::new(23);
                        self.editor = Some(editor);
                        self.active_component = Some(ComponentType::Editor);
                        self.emit_line(serial, "Editor opened");
                        self.emit_line(serial, "Keys: i=insert, Esc=normal, h/j/k/l=move, :q=quit, :q!=force");
                        self.emit_line(serial, "Note: Filesystem unavailable (in-memory editing only)");
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
                        self.emit_line(serial, "Usage: open <editor|cli>");
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
                self.emit_line(serial, "Focus switching not yet implemented");
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
