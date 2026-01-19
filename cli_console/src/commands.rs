//! CLI Commands for Filesystem View
//!
//! This module implements command-line interface commands for the filesystem view service.

use fs_view::DirectoryView;
use services_fs_view::{FileSystemOperations, FileSystemViewService};
use services_storage::{ObjectId, ObjectKind};

/// CLI Command handler
pub struct CommandHandler {
    /// Filesystem view service
    pub fs_service: FileSystemViewService,
    /// Current root directory
    pub root: DirectoryView,
}

impl CommandHandler {
    /// Creates a new command handler with an empty root directory
    pub fn new() -> Self {
        let root_id = ObjectId::new();
        let root = DirectoryView::new(root_id);
        let mut fs_service = FileSystemViewService::new();

        // Register the root directory with the service
        fs_service.register_directory(root.clone());

        Self { fs_service, root }
    }

    /// Lists directory contents
    ///
    /// Example: `pg ls docs/`
    pub fn ls(&self, path: &str) -> Result<Vec<String>, String> {
        let entries = self
            .fs_service
            .ls(&self.root, path)
            .map_err(|e| format!("ls failed: {}", e))?;

        let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        Ok(names)
    }

    /// Reads file contents (stub - returns object ID)
    ///
    /// Example: `pg cat docs/notes.txt`
    pub fn cat(&self, path: &str) -> Result<String, String> {
        let obj_id = self
            .fs_service
            .open(&self.root, path)
            .map_err(|e| format!("cat failed: {}", e))?;

        // In a real implementation, this would read the object contents
        Ok(format!("Object ID: {}", obj_id))
    }

    /// Creates a directory
    ///
    /// Example: `pg mkdir docs/projects`
    pub fn mkdir(&mut self, path: &str) -> Result<String, String> {
        let dir_id = self
            .fs_service
            .mkdir(&mut self.root, path)
            .map_err(|e| format!("mkdir failed: {}", e))?;

        Ok(format!("Created directory: {}", dir_id))
    }

    /// Links an object to a path
    ///
    /// Example: `pg link docs/todo.txt <object_id>`
    pub fn link(
        &mut self,
        path: &str,
        object_id: ObjectId,
        kind: ObjectKind,
    ) -> Result<String, String> {
        self.fs_service
            .link(&mut self.root, path, object_id, kind)
            .map_err(|e| format!("link failed: {}", e))?;

        Ok(format!("Linked {} to {}", object_id, path))
    }

    /// Displays object information
    ///
    /// Example: `pg stat docs/notes.txt`
    pub fn stat(&self, path: &str) -> Result<String, String> {
        let stat_info = self
            .fs_service
            .stat(&self.root, path)
            .map_err(|e| format!("stat failed: {}", e))?;

        let mut output = format!("Object ID: {}\n", stat_info.id);
        output.push_str(&format!("Kind: {}\n", stat_info.kind));
        if let Some(size) = stat_info.size {
            output.push_str(&format!("Size: {} bytes\n", size));
        }
        if let Some(count) = stat_info.entry_count {
            output.push_str(&format!("Entries: {}\n", count));
        }

        Ok(output)
    }
}

impl Default for CommandHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_handler_creation() {
        let handler = CommandHandler::new();
        assert!(handler.fs_service.get_directory(&handler.root.id).is_some());
    }

    #[test]
    fn test_mkdir_command() {
        let mut handler = CommandHandler::new();
        let result = handler.mkdir("docs");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Created directory"));
    }

    #[test]
    fn test_link_and_cat_command() {
        let mut handler = CommandHandler::new();
        let obj_id = ObjectId::new();

        let link_result = handler.link("file.txt", obj_id, ObjectKind::Blob);
        assert!(link_result.is_ok());

        let cat_result = handler.cat("file.txt");
        assert!(cat_result.is_ok());
        assert!(cat_result.unwrap().contains(&obj_id.to_string()));
    }

    #[test]
    fn test_ls_command() {
        let mut handler = CommandHandler::new();
        handler.mkdir("docs").unwrap();
        handler.mkdir("projects").unwrap();

        let result = handler.ls("/");
        assert!(result.is_ok());
        let entries = result.unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&"docs".to_string()));
        assert!(entries.contains(&"projects".to_string()));
    }

    #[test]
    fn test_stat_command() {
        let mut handler = CommandHandler::new();
        let obj_id = ObjectId::new();
        handler.link("file.txt", obj_id, ObjectKind::Blob).unwrap();

        let result = handler.stat("file.txt");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Object ID"));
    }

    #[test]
    fn test_nested_mkdir_and_ls() {
        let mut handler = CommandHandler::new();
        handler.mkdir("docs").unwrap();

        // Register docs directory so we can create subdirectories
        let docs_entry = handler.root.get_entry("docs").unwrap();
        let docs_dir = DirectoryView::new(docs_entry.object_id);
        handler.fs_service.register_directory(docs_dir);

        handler.mkdir("docs/notes").unwrap();

        let result = handler.ls("docs");
        assert!(result.is_ok());
        let entries = result.unwrap();
        assert!(entries.contains(&"notes".to_string()));
    }

    #[test]
    fn test_cat_nonexistent_file() {
        let handler = CommandHandler::new();
        let result = handler.cat("nonexistent.txt");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cat failed"));
    }
}
