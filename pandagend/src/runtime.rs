//! # Host Runtime
//!
//! The main event loop that ties everything together.

use crate::commands::{HostCommand, HostCommandParser};
use crate::input_script::{InputScript, ScriptedInput};
use fs_view::DirectoryView;
use identity::{ExitReason, IdentityKind, IdentityMetadata, TrustDomain};
use input_types::InputEvent;
use policy::NoOpPolicy;
use services_fs_view::FileSystemViewService;
use services_storage::{JournaledStorage, ObjectId};
use services_workspace_manager::{
    ComponentType, EditorIoContext, LaunchConfig, WorkspaceError, WorkspaceManager,
    WorkspaceRenderSnapshot,
};
use sim_kernel::SimulatedKernel;
use text_renderer_host::TextRenderer;
use thiserror::Error;

/// Host runtime error types
#[derive(Debug, Error)]
pub enum HostRuntimeError {
    #[error("Workspace error: {0}")]
    WorkspaceError(#[from] WorkspaceError),

    #[error("Script error: {0}")]
    ScriptError(String),

    #[error("Command error: {0}")]
    CommandError(String),

    #[error("No running components")]
    NoRunningComponents,
}

/// Host mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostMode {
    /// Simulation mode (deterministic, scripted input)
    Sim,
    /// HAL mode (real keyboard input)
    #[cfg(feature = "hal_mode")]
    Hal,
}

/// Host runtime configuration
#[derive(Debug, Clone)]
pub struct HostRuntimeConfig {
    /// Host mode
    pub mode: HostMode,
    /// Optional input script (for sim mode)
    pub script: Option<String>,
    /// Maximum steps to run (0 = unlimited)
    pub max_steps: usize,
    /// Whether to exit when no components are running
    pub exit_on_idle: bool,
}

impl Default for HostRuntimeConfig {
    fn default() -> Self {
        Self {
            mode: HostMode::Sim,
            script: None,
            max_steps: 0,
            exit_on_idle: false,
        }
    }
}

/// Host runtime state
enum HostState {
    /// Normal operation - routing input to components
    Running,
    /// Host control mode - capturing commands
    HostControl,
    /// Shutting down
    Shutdown,
}

/// Host runtime
pub struct HostRuntime {
    /// Configuration
    config: HostRuntimeConfig,
    /// Simulated kernel
    kernel: SimulatedKernel,
    /// Workspace manager
    workspace: WorkspaceManager,
    /// Text renderer
    renderer: TextRenderer,
    /// Input script (if in sim mode)
    script: Option<InputScript>,
    /// Current state
    state: HostState,
    /// Step counter
    steps: usize,
    /// Host command buffer (for control mode)
    command_buffer: String,
}

impl HostRuntime {
    /// Creates a new host runtime
    pub fn new(config: HostRuntimeConfig) -> Result<Self, HostRuntimeError> {
        // Create kernel with no-op policy (host doesn't enforce, workspace does)
        let mut kernel = SimulatedKernel::new().with_policy_engine(Box::new(NoOpPolicy));
        kernel
            .bootstrap_core_services()
            .map_err(|e| HostRuntimeError::CommandError(e.to_string()))?;

        // Create workspace identity
        let workspace_identity = IdentityMetadata::new(
            IdentityKind::Service,
            TrustDomain::core(),
            "workspace-manager",
            0,
        );

        // Create workspace with policy
        let mut workspace = WorkspaceManager::new(workspace_identity);

        // Provision a default editor I/O context (capability-scoped root)
        let root_id = ObjectId::new();
        let root = DirectoryView::new(root_id);
        let mut fs_view = FileSystemViewService::new();
        fs_view.register_directory(root.clone());
        workspace.set_editor_io_context(EditorIoContext::with_fs_view(
            JournaledStorage::new(),
            fs_view,
            root,
        ));

        // Create renderer
        let renderer = TextRenderer::new();

        // Parse script if provided
        let script = if let Some(script_text) = &config.script {
            Some(
                InputScript::from_text(script_text)
                    .map_err(|e| HostRuntimeError::ScriptError(e.to_string()))?,
            )
        } else {
            None
        };

        Ok(Self {
            config,
            kernel,
            workspace,
            renderer,
            script,
            state: HostState::Running,
            steps: 0,
            command_buffer: String::new(),
        })
    }

    /// Runs the host event loop
    ///
    /// Returns when:
    /// - Quit command received
    /// - Max steps reached (if configured)
    /// - No running components (if exit_on_idle configured)
    /// - Script exhausted (in sim mode)
    pub fn run(&mut self) -> Result<(), HostRuntimeError> {
        loop {
            // Check exit conditions
            if matches!(self.state, HostState::Shutdown) {
                break;
            }

            if self.config.max_steps > 0 && self.steps >= self.config.max_steps {
                break;
            }

            if self.config.exit_on_idle && self.workspace.list_components().is_empty() {
                break;
            }

            // Check if script is exhausted (in sim mode)
            if self.config.mode == HostMode::Sim {
                if let Some(script) = &self.script {
                    if !script.has_more() {
                        // Script exhausted, exit
                        break;
                    }
                }
            }

            // Run one step
            self.step()?;
            self.steps += 1;
        }

        Ok(())
    }

    /// Executes one step of the event loop
    pub fn step(&mut self) -> Result<(), HostRuntimeError> {
        // 1. Input pump
        self.pump_input()?;

        // 2. System step (run kernel/workspace until idle)
        self.kernel.run_until_idle();

        // 3. Render step (if revision changed)
        self.render()?;

        Ok(())
    }

    /// Pumps input from the appropriate source
    fn pump_input(&mut self) -> Result<(), HostRuntimeError> {
        match self.config.mode {
            HostMode::Sim => self.pump_sim_input(),
            #[cfg(feature = "hal_mode")]
            HostMode::Hal => self.pump_hal_input(),
        }
    }

    /// Pumps input from script (sim mode)
    fn pump_sim_input(&mut self) -> Result<(), HostRuntimeError> {
        if let Some(script) = &mut self.script {
            if let Some(scripted_input) = script.next_input() {
                match scripted_input {
                    ScriptedInput::Key(code, modifiers) => {
                        // Check for host control hotkey (Ctrl+Space toggles mode)
                        if code == input_types::KeyCode::Space
                            && modifiers.contains(input_types::Modifiers::CTRL)
                        {
                            self.toggle_host_control();
                            return Ok(());
                        }

                        // Convert to InputEvent
                        if let Some(event) =
                            InputScript::to_input_event(&ScriptedInput::Key(code, modifiers))
                        {
                            self.handle_input_event(event)?;
                        }
                    }
                    ScriptedInput::Wait(millis) => {
                        // Advance simulation time
                        self.kernel
                            .advance_time(kernel_api::Duration::from_millis(millis));
                    }
                }
            }
        }

        Ok(())
    }

    /// Pumps input from HAL (hal mode)
    ///
    /// **NOTE**: HAL mode is currently a stub and not functional.
    /// This is a placeholder for future HAL keyboard integration.
    /// Use sim mode for current functionality.
    #[cfg(feature = "hal_mode")]
    fn pump_hal_input(&mut self) -> Result<(), HostRuntimeError> {
        // TODO: Integrate with services_input_hal_bridge
        // For now, HAL mode is a stub
        Ok(())
    }

    /// Handles an input event
    fn handle_input_event(&mut self, event: InputEvent) -> Result<(), HostRuntimeError> {
        match self.state {
            HostState::Running => {
                // Route to focused component via workspace
                self.workspace.route_input(&event);
                Ok(())
            }
            HostState::HostControl => {
                // Capture for command parsing
                self.handle_host_control_input(event)
            }
            HostState::Shutdown => Ok(()),
        }
    }

    /// Toggles host control mode
    fn toggle_host_control(&mut self) {
        self.state = match self.state {
            HostState::Running => {
                self.command_buffer.clear();
                HostState::HostControl
            }
            HostState::HostControl => HostState::Running,
            HostState::Shutdown => HostState::Shutdown,
        };
    }

    /// Handles input in host control mode
    ///
    /// **NOTE**: This is a simplified implementation for basic command input.
    /// Production implementation should support:
    /// - Unicode characters and composing text
    /// - Input method editors (IME)
    /// - Command history and editing
    /// - Tab completion
    fn handle_host_control_input(&mut self, event: InputEvent) -> Result<(), HostRuntimeError> {
        let InputEvent::Key(key_event) = event;
        match key_event.code {
            input_types::KeyCode::Enter => {
                // Execute command
                let command_text = self.command_buffer.clone();
                self.command_buffer.clear();
                self.state = HostState::Running;
                self.execute_command(&command_text)?;
            }
            input_types::KeyCode::Escape => {
                // Cancel
                self.command_buffer.clear();
                self.state = HostState::Running;
            }
            input_types::KeyCode::Backspace => {
                // Delete last character
                self.command_buffer.pop();
            }
            _ => {
                // Append character (simplified, doesn't handle Shift or other modifiers)
                if let Some(text) = &key_event.text {
                    self.command_buffer.push_str(text);
                } else if let Some(c) = Self::keycode_to_char(key_event.code) {
                    self.command_buffer.push(c);
                }
            }
        }

        Ok(())
    }

    /// Converts a KeyCode to a character (simplified)
    ///
    /// **NOTE**: This is a minimal implementation that:
    /// - Only maps to lowercase letters
    /// - Does not handle Shift modifier for uppercase
    /// - Does not handle all punctuation
    /// - Should be replaced with proper text input handling in production
    fn keycode_to_char(code: input_types::KeyCode) -> Option<char> {
        use input_types::KeyCode;
        match code {
            KeyCode::A => Some('a'),
            KeyCode::B => Some('b'),
            KeyCode::C => Some('c'),
            KeyCode::D => Some('d'),
            KeyCode::E => Some('e'),
            KeyCode::F => Some('f'),
            KeyCode::G => Some('g'),
            KeyCode::H => Some('h'),
            KeyCode::I => Some('i'),
            KeyCode::J => Some('j'),
            KeyCode::K => Some('k'),
            KeyCode::L => Some('l'),
            KeyCode::M => Some('m'),
            KeyCode::N => Some('n'),
            KeyCode::O => Some('o'),
            KeyCode::P => Some('p'),
            KeyCode::Q => Some('q'),
            KeyCode::R => Some('r'),
            KeyCode::S => Some('s'),
            KeyCode::T => Some('t'),
            KeyCode::U => Some('u'),
            KeyCode::V => Some('v'),
            KeyCode::W => Some('w'),
            KeyCode::X => Some('x'),
            KeyCode::Y => Some('y'),
            KeyCode::Z => Some('z'),
            KeyCode::Num0 => Some('0'),
            KeyCode::Num1 => Some('1'),
            KeyCode::Num2 => Some('2'),
            KeyCode::Num3 => Some('3'),
            KeyCode::Num4 => Some('4'),
            KeyCode::Num5 => Some('5'),
            KeyCode::Num6 => Some('6'),
            KeyCode::Num7 => Some('7'),
            KeyCode::Num8 => Some('8'),
            KeyCode::Num9 => Some('9'),
            KeyCode::Space => Some(' '),
            KeyCode::Semicolon => Some(':'),
            KeyCode::Slash => Some('/'),
            KeyCode::Period => Some('.'),
            KeyCode::Comma => Some(','),
            _ => None,
        }
    }

    /// Executes a host command
    pub fn execute_command(&mut self, command_text: &str) -> Result<(), HostRuntimeError> {
        let command = HostCommandParser::parse(command_text)
            .map_err(|e| HostRuntimeError::CommandError(e.to_string()))?;

        match command {
            HostCommand::OpenEditor { path } => {
                let config = LaunchConfig::new(
                    ComponentType::Editor,
                    format!("editor-{}", path.as_deref().unwrap_or("scratch")),
                    IdentityKind::Component,
                    TrustDomain::user(),
                )
                .with_metadata("path", path.unwrap_or_default());

                self.workspace.launch_component(config)?;
            }
            HostCommand::OpenCli => {
                let config = LaunchConfig::new(
                    ComponentType::Cli,
                    "cli-console",
                    IdentityKind::Component,
                    TrustDomain::user(),
                );

                self.workspace.launch_component(config)?;
            }
            HostCommand::List => {
                // List will be rendered in status (future enhancement)
            }
            HostCommand::Focus { component_id } => {
                self.workspace.focus_component(component_id)?;
            }
            HostCommand::Next => {
                self.workspace.focus_next()?;
            }
            HostCommand::Previous => {
                self.workspace.focus_previous()?;
            }
            HostCommand::Close { component_id } => {
                self.workspace
                    .terminate_component(component_id, ExitReason::Normal)?;
            }
            HostCommand::Quit => {
                self.state = HostState::Shutdown;
            }
        }

        Ok(())
    }

    /// Renders the current workspace state
    fn render(&mut self) -> Result<(), HostRuntimeError> {
        let snapshot = self.workspace.render_snapshot();

        // Check if redraw is needed
        if self
            .renderer
            .needs_redraw(snapshot.main_view.as_ref(), snapshot.status_view.as_ref())
        {
            let mut output = self
                .renderer
                .render_snapshot(snapshot.main_view.as_ref(), snapshot.status_view.as_ref());

            // Add debug info if present (only in debug builds)
            #[cfg(debug_assertions)]
            if let Some(debug_info) = &snapshot.debug_info {
                output.push('\n');
                output.push_str("╔══════════════ DEBUG INFO ══════════════╗\n");
                output.push_str(&format!(
                    "║ Focused: {:?} ({})\n",
                    debug_info.focused_component_type,
                    debug_info
                        .focused_component_name
                        .as_deref()
                        .unwrap_or("none")
                ));
                output.push_str(&format!(
                    "║ Last Key: {}\n",
                    debug_info
                        .last_key_event
                        .as_deref()
                        .unwrap_or("(no key events yet)")
                ));
                output.push_str(&format!("║ Routed To: {:?}\n", debug_info.last_routed_to));
                output.push_str(&format!(
                    "║ Global Keybinding: {}\n",
                    if debug_info.consumed_by_global {
                        "YES"
                    } else {
                        "NO"
                    }
                ));
                output.push_str("╚════════════════════════════════════════╝\n");
            }

            // Host can print (it's a host, not a component)
            print!("{}", output);
        }

        Ok(())
    }

    /// Returns the current workspace snapshot (for testing)
    pub fn snapshot(&self) -> WorkspaceRenderSnapshot {
        self.workspace.render_snapshot()
    }

    /// Returns the step count
    pub fn step_count(&self) -> usize {
        self.steps
    }

    /// Returns a reference to the workspace (for testing)
    pub fn workspace(&self) -> &WorkspaceManager {
        &self.workspace
    }

    /// Returns a mutable reference to the workspace (for testing)
    pub fn workspace_mut(&mut self) -> &mut WorkspaceManager {
        &mut self.workspace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_creation() {
        let config = HostRuntimeConfig::default();
        let runtime = HostRuntime::new(config).unwrap();
        assert_eq!(runtime.step_count(), 0);
    }

    #[test]
    fn test_runtime_with_script() {
        let script = r#"
            i
            "Hi"
            Escape
        "#;

        let config = HostRuntimeConfig {
            mode: HostMode::Sim,
            script: Some(script.to_string()),
            max_steps: 10,
            exit_on_idle: false,
        };

        let runtime = HostRuntime::new(config);
        assert!(runtime.is_ok());
    }

    #[test]
    fn test_runtime_step() {
        let config = HostRuntimeConfig {
            mode: HostMode::Sim,
            script: Some("i".to_string()),
            max_steps: 1,
            exit_on_idle: false,
        };

        let mut runtime = HostRuntime::new(config).unwrap();
        let result = runtime.run();
        assert!(result.is_ok());
        assert_eq!(runtime.step_count(), 1);
    }

    #[test]
    fn test_runtime_max_steps() {
        let config = HostRuntimeConfig {
            mode: HostMode::Sim,
            script: Some("i\na\nb\nc".to_string()),
            max_steps: 2,
            exit_on_idle: false,
        };

        let mut runtime = HostRuntime::new(config).unwrap();
        runtime.run().unwrap();
        assert_eq!(runtime.step_count(), 2);
    }

    #[test]
    fn test_command_open_editor() {
        let config = HostRuntimeConfig::default();
        let mut runtime = HostRuntime::new(config).unwrap();

        runtime.execute_command("open editor").unwrap();
        assert_eq!(runtime.workspace().list_components().len(), 1);
    }

    #[test]
    fn test_command_quit() {
        let config = HostRuntimeConfig::default();
        let mut runtime = HostRuntime::new(config).unwrap();

        runtime.execute_command("quit").unwrap();
        assert!(matches!(runtime.state, HostState::Shutdown));
    }

    #[test]
    fn test_host_control_toggle() {
        let config = HostRuntimeConfig::default();
        let mut runtime = HostRuntime::new(config).unwrap();

        assert!(matches!(runtime.state, HostState::Running));
        runtime.toggle_host_control();
        assert!(matches!(runtime.state, HostState::HostControl));
        runtime.toggle_host_control();
        assert!(matches!(runtime.state, HostState::Running));
    }
}
