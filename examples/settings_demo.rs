//! Example: Using the Settings System
//!
//! This example demonstrates the settings persistence and live apply functionality.

use identity::{IdentityKind, IdentityMetadata, TrustDomain};
use services_workspace_manager::{WorkspaceManager, commands::{WorkspaceCommand, CommandResult}};

fn main() {
    println!("=== PandaGen Settings System Demo ===\n");

    // Create a workspace with settings
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "demo-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity.clone());

    // Show default settings
    println!("1. Listing default settings:");
    let result = workspace.execute_command(WorkspaceCommand::SettingsList);
    if let CommandResult::Success { message } = result {
        println!("{}", message);
    }

    // Change some settings
    println!("\n2. Changing settings:");
    
    println!("   Setting editor.tab_size to 2...");
    let result = workspace.execute_command(WorkspaceCommand::SettingsSet {
        key: "editor.tab_size".to_string(),
        value: "2".to_string(),
    });
    print_result(&result);

    println!("   Setting ui.theme to 'dark'...");
    let result = workspace.execute_command(WorkspaceCommand::SettingsSet {
        key: "ui.theme".to_string(),
        value: "dark".to_string(),
    });
    print_result(&result);

    println!("   Setting editor.line_numbers to false...");
    let result = workspace.execute_command(WorkspaceCommand::SettingsSet {
        key: "editor.line_numbers".to_string(),
        value: "false".to_string(),
    });
    print_result(&result);

    // Show updated settings
    println!("\n3. Listing updated settings (note the * markers for overrides):");
    let result = workspace.execute_command(WorkspaceCommand::SettingsList);
    if let CommandResult::Success { message } = result {
        println!("{}", message);
    }

    // Save settings
    println!("\n4. Saving settings:");
    let result = workspace.execute_command(WorkspaceCommand::SettingsSave);
    print_result(&result);

    // Demonstrate persistence round-trip
    println!("\n5. Simulating persistence round-trip:");
    
    // Export current settings
    let overrides = workspace.settings_registry().export_overrides();
    let data = services_settings::persistence::SettingsOverridesData::from_overrides(&overrides);
    let bytes = services_settings::persistence::serialize_overrides(&data).unwrap();
    println!("   Serialized {} bytes", bytes.len());

    // Create new workspace (simulates reboot)
    let mut new_workspace = WorkspaceManager::new(workspace_identity);
    println!("   Created fresh workspace (simulates reboot)");

    // Restore settings
    let loaded_data = services_settings::persistence::deserialize_overrides(&bytes).unwrap();
    let loaded_overrides = loaded_data.to_overrides();
    new_workspace.settings_registry_mut().import_overrides(loaded_overrides);
    println!("   Restored settings from serialized data");

    // Verify settings persisted
    println!("\n6. Verifying restored settings:");
    assert_eq!(
        new_workspace.get_setting("editor.tab_size").unwrap().as_integer(),
        Some(2)
    );
    println!("   ✓ editor.tab_size = 2");

    assert_eq!(
        new_workspace.get_setting("ui.theme").unwrap().as_string(),
        Some("dark")
    );
    println!("   ✓ ui.theme = dark");

    assert_eq!(
        new_workspace.get_setting("editor.line_numbers").unwrap().as_boolean(),
        Some(false)
    );
    println!("   ✓ editor.line_numbers = false");

    // Reset a setting
    println!("\n7. Resetting editor.tab_size to default:");
    let result = new_workspace.execute_command(WorkspaceCommand::SettingsReset {
        key: "editor.tab_size".to_string(),
    });
    print_result(&result);

    assert_eq!(
        new_workspace.get_setting("editor.tab_size").unwrap().as_integer(),
        Some(4)
    );
    println!("   ✓ Verified: editor.tab_size = 4 (default)");

    // Error handling demo
    println!("\n8. Error handling demonstration:");
    
    println!("   Trying to set unknown setting...");
    let result = new_workspace.execute_command(WorkspaceCommand::SettingsSet {
        key: "unknown.setting".to_string(),
        value: "value".to_string(),
    });
    print_result(&result);

    println!("   Trying to set integer with invalid value...");
    let result = new_workspace.execute_command(WorkspaceCommand::SettingsSet {
        key: "editor.tab_size".to_string(),
        value: "not_a_number".to_string(),
    });
    print_result(&result);

    println!("\n=== Demo Complete! ===");
    println!("\nKey Features Demonstrated:");
    println!("  ✓ Type-safe settings (Boolean, Integer, String)");
    println!("  ✓ Live apply (changes take effect immediately)");
    println!("  ✓ Persistence (settings survive serialization round-trip)");
    println!("  ✓ Reset to defaults");
    println!("  ✓ Error handling (type validation, unknown keys)");
}

fn print_result(result: &CommandResult) {
    match result {
        CommandResult::Success { message } => {
            println!("   ✓ {}", message);
        }
        CommandResult::Error { message } => {
            println!("   ✗ Error: {}", message);
        }
        _ => {
            println!("   Unexpected result");
        }
    }
}
