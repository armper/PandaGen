//! Minimal workspace manager for bare-metal kernel
//!
//! This provides a workspace-like experience in the bare-metal kernel without
//! requiring the full std-based services_workspace_manager.

use core::fmt::Write;

use crate::serial::SerialPort;
use crate::{KernelContext, KernelMessage, CommandRequest, ChannelId, KernelApiV0, COMMAND_MAX};

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
}

impl WorkspaceSession {
    pub fn new(command_channel: ChannelId, response_channel: ChannelId) -> Self {
        Self {
            active_component: None,
            command_channel,
            response_channel,
            in_command_mode: true,
            command_buffer: [0; COMMAND_MAX],
            command_len: 0,
        }
    }

    /// Process a single byte of input
    pub fn process_input(
        &mut self,
        byte: u8,
        ctx: &mut KernelContext,
        serial: &mut SerialPort,
    ) -> bool {
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

    /// Execute the current command
    fn execute_command(&mut self, ctx: &mut KernelContext, serial: &mut SerialPort) {
        let command = core::str::from_utf8(&self.command_buffer[..self.command_len])
            .unwrap_or("")
            .trim();

        if command.is_empty() {
            let _ = write!(serial, "> ");
            return;
        }

        // Parse command
        let mut parts = command.split_whitespace();
        let cmd = parts.next().unwrap_or("");

        match cmd {
            "help" => {
                let _ = writeln!(
                    serial,
                    "Workspace Commands:\r\n\
                     help           - Show this help\r\n\
                     open <what>    - Open editor or CLI\r\n\
                     list           - List components\r\n\
                     focus <id>     - Focus component\r\n\
                     quit           - Exit component\r\n\
                     halt           - Halt system\r\n\
                     \r\n\
                     System Commands:\r\n\
                     boot           - Show boot info\r\n\
                     mem            - Show memory info\r\n\
                     ticks          - Show system ticks"
                );
            }
            "open" => {
                let what = parts.next();
                match what {
                    Some("editor") => {
                        self.active_component = Some(ComponentType::Editor);
                        let _ = writeln!(serial, "Opened editor (not yet implemented)");
                    }
                    Some("cli") => {
                        self.active_component = Some(ComponentType::Cli);
                        let _ = writeln!(serial, "Opened CLI (not yet implemented)");
                    }
                    _ => {
                        let _ = writeln!(serial, "Usage: open <editor|cli>");
                    }
                }
            }
            "list" => {
                let _ = writeln!(serial, "Active components:");
                if let Some(comp) = self.active_component {
                    let _ = writeln!(serial, "  - {:?}", comp);
                } else {
                    let _ = writeln!(serial, "  (none)");
                }
            }
            "focus" => {
                let _ = writeln!(serial, "Focus switching not yet implemented");
            }
            "quit" => {
                self.active_component = None;
                let _ = writeln!(serial, "Closed component");
            }
            "halt" => {
                let _ = writeln!(serial, "Halting system...");
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
                let _ = writeln!(serial, "Unknown command: {}. Type 'help' for help.", cmd);
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

        if let Some(request) = CommandRequest::from_bytes(&command_bytes[..len], request_id, self.response_channel) {
            if ctx.send(self.command_channel, KernelMessage::CommandRequest(request)).is_ok() {
                // Wait for response (simplified synchronous handling)
                // In a real implementation, this would be async
                let _ = writeln!(serial, "(Delegated to command service)");
            } else {
                let _ = writeln!(serial, "Error: command queue full");
            }
        }
    }

    /// Show the initial prompt
    pub fn show_prompt(&self, serial: &mut SerialPort) {
        let _ = write!(serial, "> ");
    }
}
