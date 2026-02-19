//! # Workspace Command Registry
//!
//! Shared command registry for command palette and prompt.
//! Single source of truth for all workspace commands.

use services_command_palette::{CommandDescriptor, CommandPalette};

/// Builds the default workspace command registry
pub fn build_command_registry() -> CommandPalette {
    let mut palette = CommandPalette::new();

    // Open Editor - parametric command
    palette.register_command(
        CommandDescriptor::new(
            "open_editor",
            "Open Editor",
            "Open a file in the text editor",
            vec!["editor".to_string(), "open".to_string(), "file".to_string()],
        )
        .with_category("Workspace")
        .with_keybinding("Ctrl+O")
        .requires_args()
        .with_prompt_pattern("open editor "),
        Box::new(|_args| Ok("Editor command - handled by workspace".to_string())),
    );

    // List Components
    palette.register_command(
        CommandDescriptor::new(
            "list",
            "List Components",
            "List all active components",
            vec!["list".to_string(), "components".to_string()],
        )
        .with_category("Workspace")
        .with_keybinding("Ctrl+L"),
        Box::new(|_args| Ok("List command - handled by workspace".to_string())),
    );

    // Next Component
    palette.register_command(
        CommandDescriptor::new(
            "focus_next",
            "Next Component",
            "Focus the next component",
            vec!["next".to_string(), "focus".to_string()],
        )
        .with_category("Workspace")
        .with_keybinding("Alt+Tab"),
        Box::new(|_args| Ok("Next command - handled by workspace".to_string())),
    );

    // Previous Component
    palette.register_command(
        CommandDescriptor::new(
            "focus_prev",
            "Previous Component",
            "Focus the previous component",
            vec![
                "prev".to_string(),
                "previous".to_string(),
                "focus".to_string(),
            ],
        )
        .with_category("Workspace")
        .with_keybinding("Alt+Shift+Tab"),
        Box::new(|_args| Ok("Previous command - handled by workspace".to_string())),
    );

    // Close Component - parametric
    palette.register_command(
        CommandDescriptor::new(
            "close",
            "Close Component",
            "Close a component by ID",
            vec!["close".to_string(), "kill".to_string()],
        )
        .with_category("Workspace")
        .requires_args()
        .with_prompt_pattern("close "),
        Box::new(|_args| Ok("Close command - handled by workspace".to_string())),
    );

    // Recent Files
    palette.register_command(
        CommandDescriptor::new(
            "recent",
            "Recent Files",
            "Show recently opened files",
            vec![
                "recent".to_string(),
                "history".to_string(),
                "files".to_string(),
            ],
        )
        .with_category("Workspace"),
        Box::new(|_args| Ok("Recent command - handled by workspace".to_string())),
    );

    // Help - Overview
    palette.register_command(
        CommandDescriptor::new(
            "help",
            "Help",
            "Show help overview",
            vec!["help".to_string(), "?".to_string()],
        )
        .with_category("Workspace")
        .with_keybinding("?"),
        Box::new(|_args| Ok("Help command - handled by workspace".to_string())),
    );

    // Help - Workspace
    palette.register_command(
        CommandDescriptor::new(
            "help_workspace",
            "Help: Workspace",
            "Show workspace commands help",
            vec!["help".to_string(), "workspace".to_string()],
        )
        .with_category("Workspace")
        .with_prompt_pattern("help workspace"),
        Box::new(|_args| Ok("Help workspace - handled by workspace".to_string())),
    );

    // Help - Editor
    palette.register_command(
        CommandDescriptor::new(
            "help_editor",
            "Help: Editor",
            "Show editor commands help",
            vec!["help".to_string(), "editor".to_string()],
        )
        .with_category("Editor")
        .with_prompt_pattern("help editor"),
        Box::new(|_args| Ok("Help editor - handled by workspace".to_string())),
    );

    // Help - Keys
    palette.register_command(
        CommandDescriptor::new(
            "help_keys",
            "Help: Keyboard Shortcuts",
            "Show keyboard shortcuts reference",
            vec![
                "help".to_string(),
                "keys".to_string(),
                "keyboard".to_string(),
                "shortcuts".to_string(),
            ],
        )
        .with_category("Workspace")
        .with_prompt_pattern("help keys"),
        Box::new(|_args| Ok("Help keys - handled by workspace".to_string())),
    );

    // Help - System
    palette.register_command(
        CommandDescriptor::new(
            "help_system",
            "Help: System",
            "Show system commands help",
            vec!["help".to_string(), "system".to_string()],
        )
        .with_category("System")
        .with_prompt_pattern("help system"),
        Box::new(|_args| Ok("Help system - handled by workspace".to_string())),
    );

    // Save Current File
    palette.register_command(
        CommandDescriptor::new(
            "save",
            "Save File",
            "Save the current file",
            vec!["save".to_string(), "write".to_string()],
        )
        .with_category("Editor")
        .with_keybinding("Ctrl+S"),
        Box::new(|_args| Ok("Save command - handled by editor".to_string())),
    );

    // Quit Workspace
    palette.register_command(
        CommandDescriptor::new(
            "quit",
            "Quit Workspace",
            "Exit the workspace",
            vec!["quit".to_string(), "exit".to_string()],
        )
        .with_category("System")
        .with_keybinding("Ctrl+Q"),
        Box::new(|_args| Ok("Quit command - handled by workspace".to_string())),
    );

    // Boot Profile - Show
    palette.register_command(
        CommandDescriptor::new(
            "boot_profile_show",
            "Boot Profile: Show",
            "Show current boot profile configuration",
            vec![
                "boot".to_string(),
                "profile".to_string(),
                "show".to_string(),
            ],
        )
        .with_category("System")
        .with_prompt_pattern("boot profile show"),
        Box::new(|_args| Ok("Boot profile show - handled by workspace".to_string())),
    );

    // Boot Profile - Set (parametric)
    palette.register_command(
        CommandDescriptor::new(
            "boot_profile_set",
            "Boot Profile: Set",
            "Set boot profile (workspace/editor/kiosk)",
            vec!["boot".to_string(), "profile".to_string(), "set".to_string()],
        )
        .with_category("System")
        .requires_args()
        .with_prompt_pattern("boot profile set "),
        Box::new(|_args| Ok("Boot profile set - handled by workspace".to_string())),
    );

    // Boot Profile - Save
    palette.register_command(
        CommandDescriptor::new(
            "boot_profile_save",
            "Boot Profile: Save",
            "Persist current boot profile configuration",
            vec![
                "boot".to_string(),
                "profile".to_string(),
                "save".to_string(),
            ],
        )
        .with_category("System")
        .with_prompt_pattern("boot profile save"),
        Box::new(|_args| Ok("Boot profile save - handled by workspace".to_string())),
    );

    palette
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_commands() {
        let registry = build_command_registry();
        let commands = registry.list_commands();

        assert!(!commands.is_empty());
    }

    #[test]
    fn test_registry_has_open_editor() {
        let registry = build_command_registry();
        let commands = registry.filter_commands("editor");

        assert!(commands.iter().any(|c| c.id.as_str() == "open_editor"));
    }

    #[test]
    fn test_registry_parametric_commands() {
        let registry = build_command_registry();
        let commands = registry.list_commands();

        // Check open_editor is parametric
        let open_editor = commands
            .iter()
            .find(|c| c.id.as_str() == "open_editor")
            .unwrap();
        assert!(open_editor.requires_args);
        assert_eq!(open_editor.prompt_pattern, Some("open editor ".to_string()));

        // Check close is parametric
        let close = commands.iter().find(|c| c.id.as_str() == "close").unwrap();
        assert!(close.requires_args);
        assert_eq!(close.prompt_pattern, Some("close ".to_string()));
    }

    #[test]
    fn test_registry_has_categories() {
        let registry = build_command_registry();
        let commands = registry.list_commands();

        assert!(commands
            .iter()
            .any(|c| c.category == Some("Workspace".to_string())));
        assert!(commands
            .iter()
            .any(|c| c.category == Some("Editor".to_string())));
        assert!(commands
            .iter()
            .any(|c| c.category == Some("System".to_string())));
    }

    #[test]
    fn test_registry_has_keybindings() {
        let registry = build_command_registry();
        let commands = registry.list_commands();

        // Check some key commands have keybindings
        let open = commands
            .iter()
            .find(|c| c.id.as_str() == "open_editor")
            .unwrap();
        assert_eq!(open.keybinding, Some("Ctrl+O".to_string()));

        let save = commands.iter().find(|c| c.id.as_str() == "save").unwrap();
        assert_eq!(save.keybinding, Some("Ctrl+S".to_string()));
    }

    #[test]
    fn test_registry_filter_by_category() {
        let registry = build_command_registry();
        let commands = registry.filter_commands("workspace");

        // Should return workspace-related commands
        assert!(commands.len() > 0);
    }

    #[test]
    fn test_registry_help_commands() {
        let registry = build_command_registry();
        let commands = registry.filter_commands("help");

        // Should have multiple help commands
        assert!(commands.len() >= 5);
    }

    #[test]
    fn test_registry_has_boot_profile_commands() {
        let registry = build_command_registry();
        let commands = registry.filter_commands("boot profile");
        assert!(commands
            .iter()
            .any(|c| c.id.as_str() == "boot_profile_show"));
        assert!(commands.iter().any(|c| c.id.as_str() == "boot_profile_set"));
        assert!(commands
            .iter()
            .any(|c| c.id.as_str() == "boot_profile_save"));
    }
}
