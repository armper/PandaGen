//! Filesystem operations
//!
//! This module defines the operations provided by the filesystem view service.

use fs_view::{DirectoryEntry, DirectoryView, PathError};
use services_storage::{ObjectId, ObjectKind};
use thiserror::Error;

/// Errors that can occur during filesystem operations
#[derive(Debug, Error)]
pub enum OperationError {
    /// Path resolution error
    #[error("Path error: {0}")]
    PathError(#[from] PathError),

    /// Object not found
    #[error("Object not found: {0}")]
    NotFound(String),

    /// Already exists
    #[error("Already exists: {0}")]
    AlreadyExists(String),

    /// Not a directory
    #[error("Not a directory: {0}")]
    NotADirectory(String),

    /// Access denied
    #[error("Access denied: {0}")]
    AccessDenied(String),

    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

/// Metadata information about an object
#[derive(Debug, Clone)]
pub struct StatInfo {
    /// Object ID
    pub id: ObjectId,
    /// Object kind
    pub kind: ObjectKind,
    /// Size (if applicable)
    pub size: Option<usize>,
    /// Entry count (for directories)
    pub entry_count: Option<usize>,
}

/// Filesystem operations trait
///
/// This trait defines the operations that can be performed on the filesystem view.
pub trait FileSystemOperations {
    /// List directory contents
    ///
    /// Returns a list of entries in the directory at the given path.
    fn ls(&self, root: &DirectoryView, path: &str) -> Result<Vec<DirectoryEntry>, OperationError>;

    /// Get object metadata
    ///
    /// Returns metadata about the object at the given path.
    fn stat(&self, root: &DirectoryView, path: &str) -> Result<StatInfo, OperationError>;

    /// Open an object by path
    ///
    /// Resolves the path and returns the object ID.
    fn open(&self, root: &DirectoryView, path: &str) -> Result<ObjectId, OperationError>;

    /// Create a directory
    ///
    /// Creates a new directory at the given path.
    fn mkdir(&mut self, root: &mut DirectoryView, path: &str) -> Result<ObjectId, OperationError>;

    /// Link an object to a path
    ///
    /// Creates a name -> object link at the given path.
    fn link(
        &mut self,
        root: &mut DirectoryView,
        path: &str,
        object_id: ObjectId,
        kind: ObjectKind,
    ) -> Result<(), OperationError>;

    /// Unlink a path
    ///
    /// Removes the name -> object link at the given path.
    /// Note: This does NOT delete the object itself.
    fn unlink(&mut self, root: &mut DirectoryView, path: &str) -> Result<(), OperationError>;
}

// Helper functions for testing path resolution
#[cfg(test)]
mod test_helpers {
    use super::*;
    use fs_view::PathResolver;

    /// Resolves a path within a directory tree (test helper)
    pub fn resolve_parent<'a>(
        root: &'a DirectoryView,
        path: &str,
        directories: &'a std::collections::HashMap<ObjectId, DirectoryView>,
    ) -> Result<(&'a DirectoryView, String), OperationError> {
        let components = PathResolver::split_path(path)?;

        if components.is_empty() {
            return Err(OperationError::PathError(PathError::InvalidPath(
                "Empty path".to_string(),
            )));
        }

        // If single component, parent is root
        if components.len() == 1 {
            return Ok((root, components[0].to_string()));
        }

        // Traverse to parent directory
        let mut current_dir = root;
        for component in &components[..components.len() - 1] {
            let entry = current_dir
                .get_entry(component)
                .ok_or_else(|| OperationError::NotFound(component.to_string()))?;

            // Must be a directory to traverse
            if entry.kind != ObjectKind::Map {
                return Err(OperationError::NotADirectory(component.to_string()));
            }

            // Get the next directory
            current_dir = directories
                .get(&entry.object_id)
                .ok_or_else(|| OperationError::NotFound(component.to_string()))?;
        }

        let final_component = components.last().unwrap().to_string();
        Ok((current_dir, final_component))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_helpers::*;

    #[test]
    fn test_stat_info_creation() {
        let obj_id = ObjectId::new();
        let stat = StatInfo {
            id: obj_id,
            kind: ObjectKind::Blob,
            size: Some(1024),
            entry_count: None,
        };

        assert_eq!(stat.id, obj_id);
        assert_eq!(stat.kind, ObjectKind::Blob);
        assert_eq!(stat.size, Some(1024));
        assert_eq!(stat.entry_count, None);
    }

    #[test]
    fn test_resolve_parent_single_component() {
        let root_id = ObjectId::new();
        let root = DirectoryView::new(root_id);
        let dirs = std::collections::HashMap::new();

        let result = resolve_parent(&root, "file.txt", &dirs);
        assert!(result.is_ok());
        let (parent, name) = result.unwrap();
        assert_eq!(parent.id, root_id);
        assert_eq!(name, "file.txt");
    }

    #[test]
    fn test_resolve_parent_nested_path() {
        let root_id = ObjectId::new();
        let mut root = DirectoryView::new(root_id);

        let dir_id = ObjectId::new();
        root.add_entry(DirectoryEntry::new(
            "docs".to_string(),
            dir_id,
            ObjectKind::Map,
        ));

        let mut docs_dir = DirectoryView::new(dir_id);
        let subdir_id = ObjectId::new();
        docs_dir.add_entry(DirectoryEntry::new(
            "notes".to_string(),
            subdir_id,
            ObjectKind::Map,
        ));

        let mut dirs = std::collections::HashMap::new();
        dirs.insert(dir_id, docs_dir);
        dirs.insert(subdir_id, DirectoryView::new(subdir_id));

        let result = resolve_parent(&root, "docs/notes/file.txt", &dirs);
        assert!(result.is_ok());
        let (parent, name) = result.unwrap();
        assert_eq!(parent.id, subdir_id);
        assert_eq!(name, "file.txt");
    }

    #[test]
    fn test_resolve_parent_not_found() {
        let root_id = ObjectId::new();
        let root = DirectoryView::new(root_id);
        let dirs = std::collections::HashMap::new();

        let result = resolve_parent(&root, "nonexistent/file.txt", &dirs);
        assert!(result.is_err());
        assert!(matches!(result, Err(OperationError::NotFound(_))));
    }
}
