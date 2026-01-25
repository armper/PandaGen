//! Command interface for workspace manager
//!
//! This module provides a minimal command surface for workspace operations.
//! It is NOT a shell - just component orchestration commands.

use crate::{ComponentId, ComponentType, LaunchConfig, WorkspaceError, WorkspaceManager};
use identity::{ExitReason, IdentityKind, TrustDomain};
use serde::{Deserialize, Serialize};

/// Workspace command
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceCommand {
    /// Open a component
    Open {
        component_type: ComponentType,
        args: Vec<String>,
    },
    /// List all components
    List,
    /// Focus a component by ID
    Focus { component_id: ComponentId },
    /// Focus next component
    FocusNext,
    /// Focus previous component
    FocusPrev,
    /// Close a component by ID
    Close { component_id: ComponentId },
    /// Get status of a component
    Status { component_id: ComponentId },
    /// Get currently focused component
    GetFocus,
    /// Settings: List all settings
    SettingsList,
    /// Settings: Set a setting value
    SettingsSet { key: String, value: String },
    /// Settings: Reset a setting to default
    SettingsReset { key: String },
    /// Settings: Save settings to storage
    SettingsSave,
    /// Open file picker
    OpenFilePicker,
    /// Open recent files
    RecentFiles,
}

/// Result of executing a workspace command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandResult {
    /// Component was opened
    Opened {
        component_id: ComponentId,
        name: String,
    },
    /// List of components
    List { components: Vec<ComponentSummary> },
    /// Focus changed
    FocusChanged { component_id: ComponentId },
    /// Component closed
    Closed { component_id: ComponentId },
    /// Component status
    Status { summary: ComponentSummary },
    /// Currently focused component
    FocusInfo { component_id: Option<ComponentId> },
    /// Command succeeded with message
    Success { message: String },
    /// Command failed with error
    Error { message: String },
}

/// Summary of a component for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentSummary {
    pub id: ComponentId,
    pub component_type: ComponentType,
    pub name: String,
    pub state: crate::ComponentState,
    pub focusable: bool,
    pub has_focus: bool,
}

impl WorkspaceManager {
    /// Executes a workspace command with tracking
    pub fn execute_command(&mut self, command: WorkspaceCommand) -> CommandResult {
        // Format command for history tracking
        let command_str = format_command(&command);

        // Execute the command
        let result = self.execute_command_inner(command);

        // Track in recent history
        self.recent_history.add_command(command_str.clone());

        // Update status based on result
        match &result {
            CommandResult::Opened { name, .. } => {
                self.workspace_status
                    .set_last_action(format!("Opened {}", name));
            }
            CommandResult::FocusChanged { .. } => {
                self.workspace_status.set_last_action("Focus changed");
            }
            CommandResult::Closed { .. } => {
                self.workspace_status.set_last_action("Component closed");
            }
            CommandResult::Success { message } => {
                self.workspace_status.set_last_action(message);
            }
            CommandResult::Error { message } => {
                self.recent_history.add_error(message.clone());
            }
            _ => {}
        }

        // Update workspace status
        self.update_workspace_status();

        result
    }

    /// Internal command execution (without tracking)
    fn execute_command_inner(&mut self, command: WorkspaceCommand) -> CommandResult {
        match command {
            WorkspaceCommand::Open {
                component_type,
                args,
            } => self.cmd_open(component_type, args),
            WorkspaceCommand::List => self.cmd_list(),
            WorkspaceCommand::Focus { component_id } => self.cmd_focus(component_id),
            WorkspaceCommand::FocusNext => self.cmd_focus_next(),
            WorkspaceCommand::FocusPrev => self.cmd_focus_prev(),
            WorkspaceCommand::Close { component_id } => self.cmd_close(component_id),
            WorkspaceCommand::Status { component_id } => self.cmd_status(component_id),
            WorkspaceCommand::GetFocus => self.cmd_get_focus(),
            WorkspaceCommand::SettingsList => self.cmd_settings_list(),
            WorkspaceCommand::SettingsSet { key, value } => self.cmd_settings_set(key, value),
            WorkspaceCommand::SettingsReset { key } => self.cmd_settings_reset(key),
            WorkspaceCommand::SettingsSave => self.cmd_settings_save(),
            WorkspaceCommand::OpenFilePicker => self.cmd_open_file_picker(),
            WorkspaceCommand::RecentFiles => self.cmd_recent_files(),
        }
    }

    fn cmd_open(&mut self, component_type: ComponentType, args: Vec<String>) -> CommandResult {
        // Determine name from args
        let name = if let Some(first_arg) = args.first() {
            format!("{}-{}", component_type, first_arg)
        } else {
            format!("{}-{}", component_type, self.components.len())
        };

        // Create launch config
        let mut config = LaunchConfig::new(
            component_type,
            name.clone(),
            IdentityKind::Component,
            TrustDomain::user(),
        );

        // Add args as metadata
        for (i, arg) in args.iter().enumerate() {
            config = config.with_metadata(format!("arg{}", i), arg);
        }

        // Launch component
        match self.launch_component(config) {
            Ok(component_id) => {
                // Track file in recent history if it's an editor
                if component_type == ComponentType::Editor {
                    if let Some(filename) = args.first() {
                        self.recent_history.add_file(filename.clone());
                    }
                }

                CommandResult::Opened { component_id, name }
            }
            Err(err) => CommandResult::Error {
                message: format!("Failed to open component: {}", err),
            },
        }
    }

    fn cmd_list(&self) -> CommandResult {
        let focused_id = self.get_focused_component();

        let components: Vec<ComponentSummary> = self
            .list_components()
            .iter()
            .map(|c| ComponentSummary {
                id: c.id,
                component_type: c.component_type,
                name: c.name.clone(),
                state: c.state,
                focusable: c.focusable,
                has_focus: Some(c.id) == focused_id,
            })
            .collect();

        CommandResult::List { components }
    }

    fn cmd_focus(&mut self, component_id: ComponentId) -> CommandResult {
        match self.focus_component(component_id) {
            Ok(()) => CommandResult::FocusChanged { component_id },
            Err(err) => CommandResult::Error {
                message: format!("Failed to focus component: {}", err),
            },
        }
    }

    fn cmd_focus_next(&mut self) -> CommandResult {
        match self.focus_next() {
            Ok(()) => {
                if let Some(component_id) = self.get_focused_component() {
                    CommandResult::FocusChanged { component_id }
                } else {
                    CommandResult::Error {
                        message: "No component focused".to_string(),
                    }
                }
            }
            Err(err) => CommandResult::Error {
                message: format!("Failed to focus next: {}", err),
            },
        }
    }

    fn cmd_focus_prev(&mut self) -> CommandResult {
        match self.focus_previous() {
            Ok(()) => {
                if let Some(component_id) = self.get_focused_component() {
                    CommandResult::FocusChanged { component_id }
                } else {
                    CommandResult::Error {
                        message: "No component focused".to_string(),
                    }
                }
            }
            Err(err) => CommandResult::Error {
                message: format!("Failed to focus previous: {}", err),
            },
        }
    }

    fn cmd_close(&mut self, component_id: ComponentId) -> CommandResult {
        match self.terminate_component(component_id, ExitReason::Normal) {
            Ok(()) => CommandResult::Closed { component_id },
            Err(err) => CommandResult::Error {
                message: format!("Failed to close component: {}", err),
            },
        }
    }

    fn cmd_status(&self, component_id: ComponentId) -> CommandResult {
        match self.get_component(component_id) {
            Some(component) => {
                let focused_id = self.get_focused_component();
                let summary = ComponentSummary {
                    id: component.id,
                    component_type: component.component_type,
                    name: component.name.clone(),
                    state: component.state,
                    focusable: component.focusable,
                    has_focus: Some(component.id) == focused_id,
                };
                CommandResult::Status { summary }
            }
            None => CommandResult::Error {
                message: format!("Component not found: {}", component_id),
            },
        }
    }

    fn cmd_get_focus(&self) -> CommandResult {
        let component_id = self.get_focused_component();
        CommandResult::FocusInfo { component_id }
    }

    fn cmd_settings_list(&self) -> CommandResult {
        // Get all default settings
        let defaults = self.settings_registry.list_defaults();
        let user_overrides = self
            .settings_registry
            .list_user_overrides(&self.current_user);

        let mut message = String::from("Settings:\n");

        for key in &defaults {
            let value = self.settings_registry.get(&self.current_user, key);
            let is_override = user_overrides.contains(key);
            let marker = if is_override { "*" } else { " " };

            if let Some(v) = value {
                message.push_str(&format!("{}  {} = {}\n", marker, key, v));
            }
        }

        if !user_overrides.is_empty() {
            message.push_str("\n* = user override\n");
        }

        CommandResult::Success { message }
    }

    fn cmd_settings_set(&mut self, key: String, value_str: String) -> CommandResult {
        use services_settings::{SettingKey, SettingValue};

        // Get the default to determine type
        let setting_key = SettingKey::new(&key);
        let default_value = self.settings_registry.get_default(&setting_key);

        let value = match default_value {
            Some(SettingValue::Boolean(_)) => {
                // Parse as boolean
                match value_str.to_lowercase().as_str() {
                    "true" | "yes" | "1" | "on" => SettingValue::Boolean(true),
                    "false" | "no" | "0" | "off" => SettingValue::Boolean(false),
                    _ => {
                        return CommandResult::Error {
                            message: format!("Invalid boolean value: {}", value_str),
                        }
                    }
                }
            }
            Some(SettingValue::Integer(_)) => {
                // Parse as integer
                match value_str.parse::<i64>() {
                    Ok(i) => SettingValue::Integer(i),
                    Err(_) => {
                        return CommandResult::Error {
                            message: format!("Invalid integer value: {}", value_str),
                        }
                    }
                }
            }
            Some(SettingValue::Float(_)) => {
                // Parse as float
                match value_str.parse::<f64>() {
                    Ok(f) => SettingValue::Float(f),
                    Err(_) => {
                        return CommandResult::Error {
                            message: format!("Invalid float value: {}", value_str),
                        }
                    }
                }
            }
            Some(SettingValue::String(_)) => {
                // Use as string
                SettingValue::String(value_str)
            }
            Some(SettingValue::StringList(_)) => {
                // Parse as comma-separated list
                let items: Vec<String> =
                    value_str.split(',').map(|s| s.trim().to_string()).collect();
                SettingValue::StringList(items)
            }
            None => {
                return CommandResult::Error {
                    message: format!("Unknown setting: {}", key),
                }
            }
        };

        // Set the value and apply
        self.set_setting(key.clone(), value.clone());

        CommandResult::Success {
            message: format!("Set {} = {}", key, value),
        }
    }

    fn cmd_settings_reset(&mut self, key: String) -> CommandResult {
        if self.reset_setting(&key) {
            // Get the default value for display
            let default_value = self.get_setting(&key);
            let value_str = match default_value {
                Some(val) => format!("{}", val),
                None => "none".to_string(),
            };
            CommandResult::Success {
                message: format!("Reset {} to default: {}", key, value_str),
            }
        } else {
            CommandResult::Error {
                message: format!("Setting not found or not overridden: {}", key),
            }
        }
    }

    fn cmd_settings_save(&mut self) -> CommandResult {
        match self.save_settings() {
            Ok(()) => CommandResult::Success {
                message: "Settings saved successfully".to_string(),
            },
            Err(err) => CommandResult::Error {
                message: format!("Failed to save settings: {}", err),
            },
        }
    }

    fn cmd_open_file_picker(&mut self) -> CommandResult {
        // Launch file picker component
        // TODO: Implement actual file picker launching with proper root directory
        CommandResult::Success {
            message: "File picker opened (not yet implemented)".to_string(),
        }
    }

    fn cmd_recent_files(&mut self) -> CommandResult {
        // Show recent files
        let recent_files = self.recent_history.get_recent_files();
        if recent_files.is_empty() {
            CommandResult::Success {
                message: "No recent files".to_string(),
            }
        } else {
            let files_list = recent_files.join("\n");
            CommandResult::Success {
                message: format!("Recent files:\n{}", files_list),
            }
        }
    }
}

/// Parses a command string into a WorkspaceCommand
///
/// Examples:
/// - "open editor notes.txt" -> Open { component_type: Editor, args: ["notes.txt"] }
/// - "list" -> List
/// - "focus comp:abc" -> Focus { component_id: ... }
/// - "close comp:abc" -> Close { component_id: ... }
pub fn parse_command(input: &str) -> Result<WorkspaceCommand, WorkspaceError> {
    let parts: Vec<&str> = input.split_whitespace().collect();

    if parts.is_empty() {
        return Err(WorkspaceError::InvalidCommand("Empty command".to_string()));
    }

    match parts[0] {
        "open" => {
            if parts.len() < 2 {
                return Err(WorkspaceError::InvalidCommand(
                    "Usage: open <component_type> [args...]".to_string(),
                ));
            }

            let component_type = match parts[1] {
                "editor" => ComponentType::Editor,
                "cli" => ComponentType::Cli,
                "pipeline" => ComponentType::PipelineExecutor,
                other => {
                    return Err(WorkspaceError::InvalidCommand(format!(
                        "Unknown component type: {}",
                        other
                    )))
                }
            };

            let args = parts[2..].iter().map(|s| s.to_string()).collect();

            Ok(WorkspaceCommand::Open {
                component_type,
                args,
            })
        }
        "list" => Ok(WorkspaceCommand::List),
        "focus" => parse_component_id_command(&parts, "focus", |id| WorkspaceCommand::Focus {
            component_id: id,
        }),
        "next" => Ok(WorkspaceCommand::FocusNext),
        "prev" | "previous" => Ok(WorkspaceCommand::FocusPrev),
        "close" => parse_component_id_command(&parts, "close", |id| WorkspaceCommand::Close {
            component_id: id,
        }),
        "status" => parse_component_id_command(&parts, "status", |id| WorkspaceCommand::Status {
            component_id: id,
        }),
        "settings" => {
            if parts.len() < 2 {
                return Err(WorkspaceError::InvalidCommand(
                    "Usage: settings <list|set|reset|save>".to_string(),
                ));
            }

            match parts[1] {
                "list" => Ok(WorkspaceCommand::SettingsList),
                "set" => {
                    if parts.len() < 4 {
                        return Err(WorkspaceError::InvalidCommand(
                            "Usage: settings set <key> <value>".to_string(),
                        ));
                    }
                    // Join remaining parts to support values with spaces (e.g., "hello world")
                    Ok(WorkspaceCommand::SettingsSet {
                        key: parts[2].to_string(),
                        value: parts[3..].join(" "),
                    })
                }
                "reset" => {
                    if parts.len() < 3 {
                        return Err(WorkspaceError::InvalidCommand(
                            "Usage: settings reset <key>".to_string(),
                        ));
                    }
                    Ok(WorkspaceCommand::SettingsReset {
                        key: parts[2].to_string(),
                    })
                }
                "save" => Ok(WorkspaceCommand::SettingsSave),
                other => Err(WorkspaceError::InvalidCommand(format!(
                    "Unknown settings command: {}",
                    other
                ))),
            }
        }
        unknown => Err(WorkspaceError::InvalidCommand(format!(
            "Unknown command: {}",
            unknown
        ))),
    }
}

/// Helper function to parse commands that take a component ID
fn parse_component_id_command<F>(
    parts: &[&str],
    command_name: &str,
    constructor: F,
) -> Result<WorkspaceCommand, WorkspaceError>
where
    F: FnOnce(ComponentId) -> WorkspaceCommand,
{
    if parts.len() < 2 {
        return Err(WorkspaceError::InvalidCommand(format!(
            "Usage: {} <component_id>",
            command_name
        )));
    }

    let id_str = parts[1];
    if !id_str.starts_with("comp:") {
        return Err(WorkspaceError::InvalidCommand(
            "Component ID must start with 'comp:'".to_string(),
        ));
    }

    let uuid_str = &id_str[5..];
    let uuid = uuid::Uuid::parse_str(uuid_str)
        .map_err(|_| WorkspaceError::InvalidCommand(format!("Invalid UUID: {}", uuid_str)))?;

    Ok(constructor(ComponentId::from_uuid(uuid)))
}

/// Formats a WorkspaceCommand as a string for display
fn format_command(command: &WorkspaceCommand) -> String {
    match command {
        WorkspaceCommand::Open {
            component_type,
            args,
        } => {
            if args.is_empty() {
                format!("open {}", component_type)
            } else {
                format!("open {} {}", component_type, args.join(" "))
            }
        }
        WorkspaceCommand::List => "list".to_string(),
        WorkspaceCommand::Focus { component_id } => format!("focus {}", component_id),
        WorkspaceCommand::FocusNext => "next".to_string(),
        WorkspaceCommand::FocusPrev => "prev".to_string(),
        WorkspaceCommand::Close { component_id } => format!("close {}", component_id),
        WorkspaceCommand::Status { component_id } => format!("status {}", component_id),
        WorkspaceCommand::GetFocus => "get_focus".to_string(),
        WorkspaceCommand::SettingsList => "settings list".to_string(),
        WorkspaceCommand::SettingsSet { key, value } => format!("settings set {} {}", key, value),
        WorkspaceCommand::SettingsReset { key } => format!("settings reset {}", key),
        WorkspaceCommand::SettingsSave => "settings save".to_string(),
        WorkspaceCommand::OpenFilePicker => "open file".to_string(),
        WorkspaceCommand::RecentFiles => "recent files".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IdentityMetadata;

    fn create_test_workspace() -> WorkspaceManager {
        let workspace_identity = IdentityMetadata::new(
            IdentityKind::Service,
            TrustDomain::core(),
            "test-workspace",
            0,
        );
        WorkspaceManager::new(workspace_identity)
    }

    #[test]
    fn test_parse_open_command() {
        let cmd = parse_command("open editor notes.txt").unwrap();
        match cmd {
            WorkspaceCommand::Open {
                component_type,
                args,
            } => {
                assert_eq!(component_type, ComponentType::Editor);
                assert_eq!(args, vec!["notes.txt"]);
            }
            _ => panic!("Expected Open command"),
        }
    }

    #[test]
    fn test_parse_list_command() {
        let cmd = parse_command("list").unwrap();
        assert_eq!(cmd, WorkspaceCommand::List);
    }

    #[test]
    fn test_parse_next_command() {
        let cmd = parse_command("next").unwrap();
        assert_eq!(cmd, WorkspaceCommand::FocusNext);
    }

    #[test]
    fn test_parse_prev_command() {
        let cmd = parse_command("prev").unwrap();
        assert_eq!(cmd, WorkspaceCommand::FocusPrev);
    }

    #[test]
    fn test_parse_invalid_command() {
        let result = parse_command("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_list_empty() {
        let mut workspace = create_test_workspace();
        let result = workspace.execute_command(WorkspaceCommand::List);

        match result {
            CommandResult::List { components } => {
                assert_eq!(components.len(), 0);
            }
            _ => panic!("Expected List result"),
        }
    }

    #[test]
    fn test_execute_open() {
        let mut workspace = create_test_workspace();
        let result = workspace.execute_command(WorkspaceCommand::Open {
            component_type: ComponentType::Editor,
            args: vec!["test.txt".to_string()],
        });

        match result {
            CommandResult::Opened { component_id, name } => {
                assert!(name.contains("Editor"));
                assert!(workspace.get_component(component_id).is_some());
            }
            _ => panic!("Expected Opened result"),
        }
    }

    #[test]
    fn test_execute_list_with_components() {
        let mut workspace = create_test_workspace();

        workspace.execute_command(WorkspaceCommand::Open {
            component_type: ComponentType::Editor,
            args: vec![],
        });
        workspace.execute_command(WorkspaceCommand::Open {
            component_type: ComponentType::Cli,
            args: vec![],
        });

        let result = workspace.execute_command(WorkspaceCommand::List);

        match result {
            CommandResult::List { components } => {
                assert_eq!(components.len(), 2);
            }
            _ => panic!("Expected List result"),
        }
    }

    #[test]
    fn test_execute_close() {
        let mut workspace = create_test_workspace();

        let open_result = workspace.execute_command(WorkspaceCommand::Open {
            component_type: ComponentType::Editor,
            args: vec![],
        });

        let component_id = match open_result {
            CommandResult::Opened { component_id, .. } => component_id,
            _ => panic!("Expected Opened result"),
        };

        let result = workspace.execute_command(WorkspaceCommand::Close { component_id });

        match result {
            CommandResult::Closed { component_id: id } => {
                assert_eq!(id, component_id);
            }
            _ => panic!("Expected Closed result"),
        }
    }

    #[test]
    fn test_execute_focus_next() {
        let mut workspace = create_test_workspace();

        workspace.execute_command(WorkspaceCommand::Open {
            component_type: ComponentType::Editor,
            args: vec![],
        });
        workspace.execute_command(WorkspaceCommand::Open {
            component_type: ComponentType::Cli,
            args: vec![],
        });

        let result = workspace.execute_command(WorkspaceCommand::FocusNext);

        match result {
            CommandResult::FocusChanged { .. } => {
                // Success
            }
            _ => panic!("Expected FocusChanged result"),
        }
    }

    #[test]
    fn test_execute_get_focus() {
        let mut workspace = create_test_workspace();

        let open_result = workspace.execute_command(WorkspaceCommand::Open {
            component_type: ComponentType::Editor,
            args: vec![],
        });

        let component_id = match open_result {
            CommandResult::Opened { component_id, .. } => component_id,
            _ => panic!("Expected Opened result"),
        };

        let result = workspace.execute_command(WorkspaceCommand::GetFocus);

        match result {
            CommandResult::FocusInfo {
                component_id: focus_id,
            } => {
                assert_eq!(focus_id, Some(component_id));
            }
            _ => panic!("Expected FocusInfo result"),
        }
    }

    #[test]
    fn test_parse_settings_list_command() {
        let cmd = parse_command("settings list").unwrap();
        assert_eq!(cmd, WorkspaceCommand::SettingsList);
    }

    #[test]
    fn test_parse_settings_set_command() {
        let cmd = parse_command("settings set editor.tab_size 2").unwrap();
        match cmd {
            WorkspaceCommand::SettingsSet { key, value } => {
                assert_eq!(key, "editor.tab_size");
                assert_eq!(value, "2");
            }
            _ => panic!("Expected SettingsSet command"),
        }
    }

    #[test]
    fn test_parse_settings_reset_command() {
        let cmd = parse_command("settings reset editor.tab_size").unwrap();
        match cmd {
            WorkspaceCommand::SettingsReset { key } => {
                assert_eq!(key, "editor.tab_size");
            }
            _ => panic!("Expected SettingsReset command"),
        }
    }

    #[test]
    fn test_parse_settings_save_command() {
        let cmd = parse_command("settings save").unwrap();
        assert_eq!(cmd, WorkspaceCommand::SettingsSave);
    }

    #[test]
    fn test_execute_settings_list() {
        let mut workspace = create_test_workspace();
        let result = workspace.execute_command(WorkspaceCommand::SettingsList);

        match result {
            CommandResult::Success { message } => {
                assert!(message.contains("Settings:"));
                assert!(message.contains("editor.tab_size"));
            }
            _ => panic!("Expected Success result"),
        }
    }

    #[test]
    fn test_execute_settings_set_integer() {
        let mut workspace = create_test_workspace();
        let result = workspace.execute_command(WorkspaceCommand::SettingsSet {
            key: "editor.tab_size".to_string(),
            value: "2".to_string(),
        });

        match result {
            CommandResult::Success { message } => {
                assert!(message.contains("Set editor.tab_size"));
            }
            _ => panic!("Expected Success result"),
        }

        // Verify setting was changed
        let value = workspace.get_setting("editor.tab_size");
        assert_eq!(value.unwrap().as_integer(), Some(2));
    }

    #[test]
    fn test_execute_settings_set_boolean() {
        let mut workspace = create_test_workspace();
        let result = workspace.execute_command(WorkspaceCommand::SettingsSet {
            key: "editor.line_numbers".to_string(),
            value: "false".to_string(),
        });

        match result {
            CommandResult::Success { .. } => {}
            _ => panic!("Expected Success result"),
        }

        // Verify setting was changed
        let value = workspace.get_setting("editor.line_numbers");
        assert_eq!(value.unwrap().as_boolean(), Some(false));
    }

    #[test]
    fn test_execute_settings_set_string() {
        let mut workspace = create_test_workspace();
        let result = workspace.execute_command(WorkspaceCommand::SettingsSet {
            key: "ui.theme".to_string(),
            value: "dark".to_string(),
        });

        match result {
            CommandResult::Success { .. } => {}
            _ => panic!("Expected Success result"),
        }

        // Verify setting was changed
        let value = workspace.get_setting("ui.theme");
        assert_eq!(value.unwrap().as_string(), Some("dark"));
    }

    #[test]
    fn test_execute_settings_reset() {
        let mut workspace = create_test_workspace();

        // First set a value
        workspace.execute_command(WorkspaceCommand::SettingsSet {
            key: "editor.tab_size".to_string(),
            value: "2".to_string(),
        });

        // Then reset it
        let result = workspace.execute_command(WorkspaceCommand::SettingsReset {
            key: "editor.tab_size".to_string(),
        });

        match result {
            CommandResult::Success { .. } => {}
            _ => panic!("Expected Success result"),
        }

        // Verify it's back to default
        let value = workspace.get_setting("editor.tab_size");
        assert_eq!(value.unwrap().as_integer(), Some(4)); // Default is 4
    }

    #[test]
    fn test_execute_settings_save() {
        let mut workspace = create_test_workspace();
        let result = workspace.execute_command(WorkspaceCommand::SettingsSave);

        match result {
            CommandResult::Success { message } => {
                assert!(message.contains("Settings saved"));
            }
            _ => panic!("Expected Success result"),
        }
    }

    #[test]
    fn test_execute_settings_set_invalid_type() {
        let mut workspace = create_test_workspace();
        let result = workspace.execute_command(WorkspaceCommand::SettingsSet {
            key: "editor.tab_size".to_string(),
            value: "not_a_number".to_string(),
        });

        match result {
            CommandResult::Error { message } => {
                assert!(message.contains("Invalid integer value"));
            }
            _ => panic!("Expected Error result"),
        }
    }

    #[test]
    fn test_execute_settings_set_unknown_key() {
        let mut workspace = create_test_workspace();
        let result = workspace.execute_command(WorkspaceCommand::SettingsSet {
            key: "unknown.setting".to_string(),
            value: "value".to_string(),
        });

        match result {
            CommandResult::Error { message } => {
                assert!(message.contains("Unknown setting"));
            }
            _ => panic!("Expected Error result"),
        }
    }

    #[test]
    fn test_settings_persistence_roundtrip() {
        use services_settings::persistence::{
            deserialize_overrides, serialize_overrides, SettingsOverridesData,
        };

        let mut workspace = create_test_workspace();

        // Set some settings
        workspace.set_setting(
            "editor.tab_size",
            services_settings::SettingValue::Integer(2),
        );
        workspace.set_setting(
            "ui.theme",
            services_settings::SettingValue::String("dark".to_string()),
        );

        // Export and serialize
        let overrides = workspace.settings_registry().export_overrides();
        let data = SettingsOverridesData::from_overrides(&overrides);
        let bytes = serialize_overrides(&data).unwrap();

        // Deserialize and import
        let loaded_data = deserialize_overrides(&bytes).unwrap();
        let loaded_overrides = loaded_data.to_overrides();

        // Create new workspace and import
        let mut new_workspace = create_test_workspace();
        new_workspace
            .settings_registry_mut()
            .import_overrides(loaded_overrides);

        // Verify settings persisted
        assert_eq!(
            new_workspace
                .get_setting("editor.tab_size")
                .unwrap()
                .as_integer(),
            Some(2)
        );
        assert_eq!(
            new_workspace.get_setting("ui.theme").unwrap().as_string(),
            Some("dark")
        );
    }
}
