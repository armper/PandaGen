//! CLI Commands for Filesystem View
//!
//! This module implements command-line interface commands for the filesystem view service.

use fs_view::DirectoryView;
use hal::BlockDevice;
use services_fs_view::{FileSystemOperations, FileSystemViewService};
use services_storage::{
    JournaledStorage, ObjectId, ObjectKind, PersistentFilesystem, TransactionError,
    TransactionalStorage,
};

/// CLI Command handler with persistent storage backend
pub struct PersistentCommandHandler<D: BlockDevice> {
    /// Persistent filesystem
    pub fs: PersistentFilesystem<D>,
    /// Current working directory
    pub current_dir: ObjectId,
}

impl<D: BlockDevice> PersistentCommandHandler<D> {
    /// Creates a new command handler with a formatted filesystem
    pub fn new(device: D, owner: impl Into<String>) -> Result<Self, TransactionError> {
        let fs = PersistentFilesystem::format(device, owner)?;
        let current_dir = fs.root_dir_id();
        Ok(Self { fs, current_dir })
    }

    /// Opens an existing filesystem
    pub fn open(device: D, root_dir_id: ObjectId) -> Result<Self, TransactionError> {
        let fs = PersistentFilesystem::open(device, root_dir_id)?;
        let current_dir = fs.root_dir_id();
        Ok(Self { fs, current_dir })
    }

    /// Lists directory contents
    pub fn ls(&mut self, path: &str) -> Result<Vec<String>, String> {
        let dir_id = self.resolve_path(path)?;
        let entries = self
            .fs
            .list(dir_id)
            .map_err(|e| format!("Failed to list directory '{}': {}", path, e))?;

        let names: Vec<String> = entries.iter().map(|(name, _)| name.clone()).collect();
        Ok(names)
    }

    /// Reads file contents
    pub fn cat(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let file_id = self.resolve_path(path)?;
        self.fs
            .read_file(file_id)
            .map_err(|e| format!("Failed to read file '{}': {}", path, e))
    }

    /// Creates a directory
    pub fn mkdir(&mut self, name: &str) -> Result<String, String> {
        if name.is_empty() {
            return Err("Directory name cannot be empty".to_string());
        }
        if name.contains('/') || name.contains('\\') {
            return Err("Directory name cannot contain path separators".to_string());
        }

        let timestamp = get_timestamp();
        let dir_id = self
            .fs
            .mkdir(name, self.current_dir, "user", timestamp)
            .map_err(|e| format!("Failed to create directory '{}': {}", name, e))?;

        Ok(format!("Created directory: {} (id: {})", name, dir_id))
    }

    /// Writes a file
    pub fn write_file(&mut self, name: &str, content: &[u8]) -> Result<String, String> {
        if name.is_empty() {
            return Err("File name cannot be empty".to_string());
        }
        if name.contains('/') || name.contains('\\') {
            return Err("File name cannot contain path separators".to_string());
        }

        let timestamp = get_timestamp();
        let file_id = self
            .fs
            .write_file(content)
            .map_err(|e| format!("Failed to write file '{}': {}", name, e))?;

        self.fs
            .link(name, self.current_dir, file_id, ObjectKind::Blob, timestamp)
            .map_err(|e| format!("Failed to link file '{}': {}", name, e))?;

        Ok(format!(
            "Wrote file: {} ({} bytes, id: {})",
            name,
            content.len(),
            file_id
        ))
    }

    /// Removes a file or directory entry
    pub fn rm(&mut self, name: &str) -> Result<String, String> {
        if name.is_empty() {
            return Err("File name cannot be empty".to_string());
        }

        let timestamp = get_timestamp();
        let removed = self
            .fs
            .unlink(name, self.current_dir, timestamp)
            .map_err(|e| format!("Failed to remove '{}': {}", name, e))?;

        match removed {
            Some(entry) => Ok(format!("Removed: {} (id: {})", name, entry.object_id)),
            None => Err(format!("Not found: '{}'", name)),
        }
    }

    /// Resolves a path to an ObjectId (simplified - just returns current dir or parses name)
    fn resolve_path(&mut self, path: &str) -> Result<ObjectId, String> {
        let path = path.trim();
        if path.is_empty() || path == "/" || path == "." {
            return Ok(self.current_dir);
        }

        // Simple case: just a name in current directory
        let entries = self
            .fs
            .list(self.current_dir)
            .map_err(|e| format!("Failed to read current directory: {}", e))?;

        for (name, entry) in entries {
            if name == path {
                return Ok(entry.object_id);
            }
        }

        Err(format!(
            "Path not found: '{}' (current directory only supported)",
            path
        ))
    }
}

/// Get current timestamp (nanoseconds since epoch)
fn get_timestamp() -> u64 {
    // In a real system, this would get actual time
    // For now, use a simple counter
    static mut COUNTER: u64 = 1000;
    unsafe {
        COUNTER += 1;
        COUNTER
    }
}

/// CLI Command handler
pub struct CommandHandler {
    /// Filesystem view service
    pub fs_service: FileSystemViewService,
    /// Current root directory
    pub root: DirectoryView,
    /// Storage backend for object content reads/writes
    storage: JournaledStorage,
}

impl CommandHandler {
    /// Creates a new command handler with an empty root directory
    pub fn new() -> Self {
        let root_id = ObjectId::new();
        let root = DirectoryView::new(root_id);
        let mut fs_service = FileSystemViewService::new();

        // Register the root directory with the service
        fs_service.register_directory(root.clone());

        Self {
            fs_service,
            root,
            storage: JournaledStorage::new(),
        }
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

    /// Reads file contents
    ///
    /// Example: `pg cat docs/notes.txt`
    pub fn cat(&mut self, path: &str) -> Result<String, String> {
        let obj_id = self
            .fs_service
            .open(&self.root, path)
            .map_err(|e| format!("cat failed: {}", e))?;

        let mut tx = self
            .storage
            .begin_transaction()
            .map_err(|e| format!("cat failed: {}", e))?;
        let bytes = self
            .storage
            .read_data(&tx, obj_id)
            .map_err(|e| format!("cat failed: {}", e))?;
        let _ = self.storage.rollback(&mut tx);

        Ok(String::from_utf8_lossy(&bytes).into_owned())
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
    use hal::RamDisk;

    #[test]
    fn test_persistent_handler_creation() {
        let disk = RamDisk::with_capacity_mb(10);
        let handler = PersistentCommandHandler::new(disk, "test_user");
        assert!(handler.is_ok());
    }

    #[test]
    fn test_persistent_mkdir_and_ls() {
        let disk = RamDisk::with_capacity_mb(10);
        let mut handler = PersistentCommandHandler::new(disk, "test_user").unwrap();

        let result = handler.mkdir("docs");
        assert!(result.is_ok());

        let ls_result = handler.ls("/");
        assert!(ls_result.is_ok());
        let entries = ls_result.unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains(&"docs".to_string()));
    }

    #[test]
    fn test_persistent_write_and_cat() {
        let disk = RamDisk::with_capacity_mb(10);
        let mut handler = PersistentCommandHandler::new(disk, "test_user").unwrap();

        let content = b"Hello, persistent storage!";
        let write_result = handler.write_file("test.txt", content);
        assert!(write_result.is_ok());

        let cat_result = handler.cat("test.txt");
        assert!(cat_result.is_ok());
        assert_eq!(cat_result.unwrap(), content);
    }

    #[test]
    fn test_persistent_rm() {
        let disk = RamDisk::with_capacity_mb(10);
        let mut handler = PersistentCommandHandler::new(disk, "test_user").unwrap();

        handler.write_file("file.txt", b"data").unwrap();

        let rm_result = handler.rm("file.txt");
        assert!(rm_result.is_ok());

        let ls_result = handler.ls("/");
        assert!(ls_result.is_ok());
        assert_eq!(ls_result.unwrap().len(), 0);
    }

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
        let content = b"hello from storage";

        // Seed storage with actual object content for the linked object.
        let mut tx = handler.storage.begin_transaction().unwrap();
        handler.storage.write(&mut tx, obj_id, content).unwrap();
        handler.storage.commit(&mut tx).unwrap();

        let link_result = handler.link("file.txt", obj_id, ObjectKind::Blob);
        assert!(link_result.is_ok());

        let cat_result = handler.cat("file.txt");
        assert!(cat_result.is_ok());
        assert_eq!(cat_result.unwrap(), "hello from storage");
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
        let mut handler = CommandHandler::new();
        let result = handler.cat("nonexistent.txt");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cat failed"));
    }
}
