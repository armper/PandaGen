//! # Workspace Command Registry
//!
//! Shared command registry for command palette and prompt.
//! Single source of truth for all workspace commands.

use crate::command_surface::{
    PaletteDescriptorSpec, HELPER_COMMAND_SPECS, LAUNCH_COMMAND_SPECS, NON_LAUNCH_PALETTE_SPECS,
};
use services_command_palette::{CommandDescriptor, CommandPalette};

/// Builds the default workspace command registry
pub fn build_command_registry() -> CommandPalette {
    let mut palette = CommandPalette::new();

    // Launch and helper command descriptors are generated from shared command-surface specs.
    for spec in LAUNCH_COMMAND_SPECS {
        register_workspace_surface_command(&mut palette, &spec.palette);
    }
    for spec in HELPER_COMMAND_SPECS {
        register_workspace_surface_command(&mut palette, &spec.palette);
    }
    for spec in NON_LAUNCH_PALETTE_SPECS {
        register_workspace_surface_command(&mut palette, spec);
    }

    palette
}

fn register_workspace_surface_command(palette: &mut CommandPalette, spec: &PaletteDescriptorSpec) {
    let mut descriptor = CommandDescriptor::new(
        spec.id,
        spec.name,
        spec.description,
        spec.tags.iter().map(|tag| (*tag).to_string()).collect(),
    )
    .with_category(spec.category);

    if let Some(keybinding) = spec.keybinding {
        descriptor = descriptor.with_keybinding(keybinding);
    }
    if spec.requires_args {
        descriptor = descriptor.requires_args();
    }
    if let Some(prompt_pattern) = spec.prompt_pattern {
        descriptor = descriptor.with_prompt_pattern(prompt_pattern);
    }

    let response = format!("{} command - handled by workspace", spec.name);
    palette.register_command(descriptor, Box::new(move |_args| Ok(response.clone())));
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
    fn test_registry_has_open_custom() {
        let registry = build_command_registry();
        let commands = registry.filter_commands("custom");

        assert!(commands.iter().any(|c| c.id.as_str() == "open_custom"));
    }

    #[test]
    fn test_registry_has_open_cli() {
        let registry = build_command_registry();
        let commands = registry.filter_commands("cli");

        assert!(commands.iter().any(|c| c.id.as_str() == "open_cli"));
    }

    #[test]
    fn test_registry_has_open_pipeline() {
        let registry = build_command_registry();
        let commands = registry.filter_commands("pipeline");

        assert!(commands.iter().any(|c| c.id.as_str() == "open_pipeline"));
    }

    #[test]
    fn test_registry_has_open_file_picker() {
        let registry = build_command_registry();
        let commands = registry.filter_commands("file picker");

        assert!(commands.iter().any(|c| c.id.as_str() == "open_file_picker"));
    }

    #[test]
    fn test_registry_parametric_commands() {
        let registry = build_command_registry();
        let commands = registry.list_commands();

        // Check open_editor prompt mirrors parser syntax: args optional.
        let open_editor = commands
            .iter()
            .find(|c| c.id.as_str() == "open_editor")
            .unwrap();
        assert!(!open_editor.requires_args);
        assert_eq!(open_editor.prompt_pattern, Some("open editor ".to_string()));

        // Check open_cli prompt mirrors parser syntax: args optional.
        let open_cli = commands
            .iter()
            .find(|c| c.id.as_str() == "open_cli")
            .unwrap();
        assert!(!open_cli.requires_args);
        assert_eq!(open_cli.prompt_pattern, Some("open cli ".to_string()));

        // Check open_pipeline prompt mirrors parser syntax: args optional.
        let open_pipeline = commands
            .iter()
            .find(|c| c.id.as_str() == "open_pipeline")
            .unwrap();
        assert!(!open_pipeline.requires_args);
        assert_eq!(
            open_pipeline.prompt_pattern,
            Some("open pipeline ".to_string())
        );

        // Check open_custom is parametric
        let open_custom = commands
            .iter()
            .find(|c| c.id.as_str() == "open_custom")
            .unwrap();
        assert!(open_custom.requires_args);
        assert_eq!(open_custom.prompt_pattern, Some("open custom ".to_string()));

        // Check open_file_picker prompt mirrors parser alias.
        let open_file_picker = commands
            .iter()
            .find(|c| c.id.as_str() == "open_file_picker")
            .unwrap();
        assert!(!open_file_picker.requires_args);
        assert_eq!(
            open_file_picker.prompt_pattern,
            Some("open file".to_string())
        );

        // Check recent prompt mirrors parser syntax.
        let recent = commands.iter().find(|c| c.id.as_str() == "recent").unwrap();
        assert!(!recent.requires_args);
        assert_eq!(recent.prompt_pattern, Some("recent".to_string()));

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

        let open_custom = commands
            .iter()
            .find(|c| c.id.as_str() == "open_custom")
            .unwrap();
        assert_eq!(open_custom.keybinding, Some("Ctrl+Shift+O".to_string()));

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
