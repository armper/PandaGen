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
#[cfg(feature = "hal_mode")]
use std::collections::VecDeque;
#[cfg(feature = "hal_mode")]
use std::sync::{Arc, Mutex};
use text_renderer_host::TextRenderer;
use thiserror::Error;

#[cfg(feature = "hal_mode")]
use core_types::TaskId;
#[cfg(feature = "hal_mode")]
use hal::{HalKeyEvent, KeyboardDevice};
#[cfg(feature = "hal_mode")]
use identity::ExecutionId;
#[cfg(feature = "hal_mode")]
use ipc::ChannelId;
#[cfg(feature = "hal_mode")]
use services_input::{InputService, INPUT_EVENT_ACTION};
#[cfg(feature = "hal_mode")]
use services_input_hal_bridge::{BridgeError, InputHalBridge, PollResult};

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

    #[cfg(feature = "hal_mode")]
    #[error("HAL input error: {0}")]
    HalInputError(String),
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

#[cfg(feature = "hal_mode")]
#[derive(Clone)]
struct SharedQueueKeyboard {
    queue: Arc<Mutex<VecDeque<HalKeyEvent>>>,
}

#[cfg(feature = "hal_mode")]
impl SharedQueueKeyboard {
    fn new(queue: Arc<Mutex<VecDeque<HalKeyEvent>>>) -> Self {
        Self { queue }
    }
}

#[cfg(feature = "hal_mode")]
impl KeyboardDevice for SharedQueueKeyboard {
    fn poll_event(&mut self) -> Option<HalKeyEvent> {
        match self.queue.lock() {
            Ok(mut guard) => guard.pop_front(),
            Err(_) => None,
        }
    }
}

#[cfg(feature = "hal_mode")]
struct HalInputContext {
    bridge: InputHalBridge,
    input_service: InputService,
    input_channel: ChannelId,
    keyboard_queue: Arc<Mutex<VecDeque<HalKeyEvent>>>,
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
    /// HAL input integration (only used in hal mode)
    #[cfg(feature = "hal_mode")]
    hal_input: Option<HalInputContext>,
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

        #[cfg(feature = "hal_mode")]
        let hal_mode_enabled = config.mode == HostMode::Hal;

        #[cfg(feature = "hal_mode")]
        let hal_input = if hal_mode_enabled {
            Some(Self::build_hal_input_context(&mut kernel)?)
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
            #[cfg(feature = "hal_mode")]
            hal_input,
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
    #[cfg(feature = "hal_mode")]
    fn pump_hal_input(&mut self) -> Result<(), HostRuntimeError> {
        let mut hal = self.hal_input.take().ok_or_else(|| {
            HostRuntimeError::HalInputError("HAL mode active without HAL input context".to_string())
        })?;

        let poll_result = {
            hal.bridge
                .poll(&hal.input_service, &mut self.kernel)
                .map_err(Self::map_hal_bridge_error)?
        };

        if poll_result == PollResult::EventDelivered {
            let envelope = kernel_api::KernelApiV0::recv(&mut self.kernel, hal.input_channel)
                .map_err(|e| HostRuntimeError::HalInputError(e.to_string()))?;

            if envelope.action != INPUT_EVENT_ACTION {
                self.hal_input = Some(hal);
                return Err(HostRuntimeError::HalInputError(format!(
                    "unexpected HAL input action: {}",
                    envelope.action
                )));
            }

            let event: InputEvent = envelope.payload.deserialize().map_err(|e| {
                HostRuntimeError::HalInputError(format!(
                    "failed to deserialize HAL input event: {}",
                    e
                ))
            })?;

            // Keep host-control hotkey behavior consistent with sim mode.
            if let InputEvent::Key(ref key) = event {
                if key.code == input_types::KeyCode::Space
                    && key.modifiers.contains(input_types::Modifiers::CTRL)
                    && key.state == input_types::KeyState::Pressed
                {
                    self.toggle_host_control();
                    self.hal_input = Some(hal);
                    return Ok(());
                }
            }

            self.handle_input_event(event)?;
        }

        self.hal_input = Some(hal);
        Ok(())
    }

    #[cfg(feature = "hal_mode")]
    fn build_hal_input_context(
        kernel: &mut SimulatedKernel,
    ) -> Result<HalInputContext, HostRuntimeError> {
        let input_channel = kernel_api::KernelApiV0::create_channel(kernel)
            .map_err(|e| HostRuntimeError::HalInputError(e.to_string()))?;
        let source_task = TaskId::new();
        let mut input_service = InputService::new();
        let subscription = input_service
            .subscribe_keyboard(source_task, input_channel)
            .map_err(|e| HostRuntimeError::HalInputError(e.to_string()))?;

        let keyboard_queue = Arc::new(Mutex::new(VecDeque::new()));
        let keyboard: Box<dyn KeyboardDevice> =
            Box::new(SharedQueueKeyboard::new(keyboard_queue.clone()));

        let bridge = InputHalBridge::new(ExecutionId::new(), source_task, subscription, keyboard);

        Ok(HalInputContext {
            bridge,
            input_service,
            input_channel,
            keyboard_queue,
        })
    }

    #[cfg(feature = "hal_mode")]
    fn map_hal_bridge_error(err: BridgeError) -> HostRuntimeError {
        HostRuntimeError::HalInputError(err.to_string())
    }

    #[cfg(feature = "hal_mode")]
    pub fn inject_hal_event(&mut self, event: HalKeyEvent) -> Result<(), HostRuntimeError> {
        let hal = self.hal_input.as_mut().ok_or_else(|| {
            HostRuntimeError::HalInputError("HAL input context not initialized".to_string())
        })?;
        let mut queue = hal.keyboard_queue.lock().map_err(|_| {
            HostRuntimeError::HalInputError("HAL keyboard queue lock poisoned".to_string())
        })?;
        queue.push_back(event);
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
    #[cfg(feature = "hal_mode")]
    use hal::HalKeyEvent;

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

    #[cfg(feature = "hal_mode")]
    #[test]
    fn test_hal_mode_ctrl_space_toggles_host_control() {
        let config = HostRuntimeConfig {
            mode: HostMode::Hal,
            script: None,
            max_steps: 0,
            exit_on_idle: false,
        };

        let mut runtime = HostRuntime::new(config).unwrap();
        assert!(matches!(runtime.state, HostState::Running));

        // Ctrl down, then Space down => Ctrl+Space hotkey
        runtime
            .inject_hal_event(HalKeyEvent::new(0x1D, true))
            .unwrap();
        runtime.step().unwrap();
        runtime
            .inject_hal_event(HalKeyEvent::new(0x39, true))
            .unwrap();
        runtime.step().unwrap();

        assert!(matches!(runtime.state, HostState::HostControl));
    }

    #[cfg(feature = "hal_mode")]
    #[test]
    fn test_hal_mode_routes_input_to_workspace() {
        let config = HostRuntimeConfig {
            mode: HostMode::Hal,
            script: None,
            max_steps: 0,
            exit_on_idle: false,
        };

        let mut runtime = HostRuntime::new(config).unwrap();
        runtime.execute_command("open editor").unwrap();

        // 'i' key press
        runtime
            .inject_hal_event(HalKeyEvent::new(0x17, true))
            .unwrap();
        runtime.step().unwrap();

        let snapshot = runtime.snapshot();
        let debug = snapshot.debug_info.expect("expected debug info");
        assert!(
            debug.last_key_event.is_some(),
            "HAL key event should be routed into workspace input pipeline"
        );
    }
}
