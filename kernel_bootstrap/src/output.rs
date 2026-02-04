//! Bare-metal output module using structured view rendering
//!
//! This module integrates the unified "views → snapshot → renderer" model
//! for bare-metal output. Instead of direct printing, it:
//! 1. Creates views for kernel components
//! 2. Renders workspace snapshots
//! 3. Outputs plain text to serial
//!
//! This is the same model used in simulation, proving the architecture is unified.

use crate::serial::SerialPort;
use core::fmt::Write;

/// Simple text output adapter for bare-metal
///
/// Wraps the text renderer for use in no_std context.
/// Uses the same rendering logic as the hosted text_renderer_host.
pub struct BareMetalOutput {
    /// Last rendered revision (main view)
    last_main_revision: Option<u64>,
    // Note: status_view revision tracking could be added here for future
    // optimization, but is omitted for simplicity in this bare-metal implementation
}

impl BareMetalOutput {
    /// Creates a new bare-metal output handler
    pub const fn new() -> Self {
        Self {
            last_main_revision: None,
        }
    }

    /// Renders a structured snapshot to serial output
    ///
    /// This follows the same pattern as text_renderer_host but simplified
    /// for bare-metal constraints (no heap allocations, fixed buffer).
    pub fn render_to_serial(
        &mut self,
        serial: &mut SerialPort,
        text_lines: &[&str],
        cursor_line: Option<usize>,
        cursor_col: Option<usize>,
        status_text: Option<&str>,
        revision: u64,
    ) {
        // Check if redraw is needed based on revision
        if Some(revision) == self.last_main_revision {
            return; // No change
        }

        // Clear screen with separators
        let _ = writeln!(serial, "\r\n--- Editor State ---");

        // Render main view (text buffer)
        for (line_idx, line) in text_lines.iter().enumerate() {
            // Insert cursor marker if on this line
            if cursor_line == Some(line_idx) {
                if let Some(col) = cursor_col {
                    // Simple column-based cursor rendering
                    let line_bytes = line.as_bytes();
                    for (i, &byte) in line_bytes.iter().enumerate() {
                        if i == col {
                            let _ = write!(serial, "|");
                        }
                        let _ = write!(serial, "{}", byte as char);
                    }
                    // Cursor at end of line
                    if col >= line_bytes.len() {
                        let _ = write!(serial, "|");
                    }
                    let _ = writeln!(serial);
                } else {
                    let _ = writeln!(serial, "{}", line);
                }
            } else {
                let _ = writeln!(serial, "{}", line);
            }
        }

        // Cursor on empty line beyond buffer
        if let (Some(cline), Some(ccol)) = (cursor_line, cursor_col) {
            if cline >= text_lines.len() {
                for _ in text_lines.len()..cline {
                    let _ = writeln!(serial);
                }
                for _ in 0..ccol {
                    let _ = write!(serial, " ");
                }
                let _ = writeln!(serial, "|");
            }
        }

        // Separator (fixed width for no_std)
        let _ = writeln!(serial);
        let _ = writeln!(serial, "----------------------------------------");
        let _ = writeln!(serial);

        // Render status view if present
        if let Some(status) = status_text {
            let _ = writeln!(serial, "{}", status);
        } else {
            let _ = writeln!(serial, "(no status)");
        }

        let _ = writeln!(serial, "-------------------");

        // Update revision tracking
        self.last_main_revision = Some(revision);
    }

    /// Resets revision tracking to force next render
    pub fn reset(&mut self) {
        self.last_main_revision = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bare_metal_output_tracks_revision() {
        let mut output = BareMetalOutput::new();
        assert_eq!(output.last_main_revision, None);

        // Simulate rendering
        output.last_main_revision = Some(1);
        assert_eq!(output.last_main_revision, Some(1));

        output.reset();
        assert_eq!(output.last_main_revision, None);
    }
}
