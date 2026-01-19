//! Example demonstrating the Filesystem Illusion Service
//!
//! This example shows how to use the filesystem view service to create
//! a directory hierarchy and perform operations on it.

use cli_console::commands::CommandHandler;
use services_storage::ObjectKind;

fn main() {
    println!("=== PandaGen Filesystem Illusion Service Demo ===\n");

    // Create a command handler (simulates a user session)
    let mut handler = CommandHandler::new();

    println!("1. Creating directory structure...");
    handler.mkdir("docs").expect("Failed to create docs");
    handler
        .mkdir("projects")
        .expect("Failed to create projects");
    println!("   ✓ Created: /docs");
    println!("   ✓ Created: /projects\n");

    println!("2. Creating nested directories...");
    // Note: In a real implementation, we'd need to properly register
    // intermediate directories. This is simplified for the demo.
    println!("   (Nested directory creation requires proper registration)\n");

    println!("3. Linking files to the filesystem...");
    let readme_id = services_storage::ObjectId::new();
    let notes_id = services_storage::ObjectId::new();

    handler
        .link("README.md", readme_id, ObjectKind::Blob)
        .expect("Failed to link README");
    handler
        .link("notes.txt", notes_id, ObjectKind::Blob)
        .expect("Failed to link notes");
    println!("   ✓ Linked: /README.md → {}", readme_id);
    println!("   ✓ Linked: /notes.txt → {}\n", notes_id);

    println!("4. Listing root directory...");
    let entries = handler.ls("/").expect("Failed to list root");
    for entry in &entries {
        println!("   - {}", entry);
    }
    println!();

    println!("5. Getting file information...");
    let stat = handler.stat("README.md").expect("Failed to stat file");
    println!("   File: README.md");
    println!("{}", stat);

    println!("6. Opening a file...");
    let cat_output = handler.cat("notes.txt").expect("Failed to cat file");
    println!("   {}\n", cat_output);

    println!("=== Demo Complete ===");
    println!("\nKey Points:");
    println!("✓ No global filesystem - each handler has its own root");
    println!("✓ All operations are capability-driven");
    println!("✓ Paths are views, not authority");
    println!("✓ Storage remains immutable and object-based");
}
