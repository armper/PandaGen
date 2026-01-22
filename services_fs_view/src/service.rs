//! Filesystem View Service implementation
//!
//! This module provides the actual service that implements filesystem operations.

use crate::operations::{FileSystemOperations, OperationError, StatInfo};
use fs_view::{DirectoryEntry, DirectoryView, PathResolver};
use services_storage::{ObjectId, ObjectKind};
use std::collections::HashMap;

/// The Filesystem View Service
///
/// Maintains a view of the directory hierarchy and provides operations
/// to manipulate it.
#[derive(Debug, Clone)]
pub struct FileSystemViewService {
    /// All directories in the system, indexed by ObjectId
    directories: HashMap<ObjectId, DirectoryView>,
}

impl FileSystemViewService {
    /// Creates a new filesystem view service
    pub fn new() -> Self {
        Self {
            directories: HashMap::new(),
        }
    }

    /// Registers a directory with the service
    ///
    /// This allows the service to traverse into this directory.
    pub fn register_directory(&mut self, dir: DirectoryView) {
        self.directories.insert(dir.id, dir);
    }

    /// Gets a directory by ID
    pub fn get_directory(&self, id: &ObjectId) -> Option<&DirectoryView> {
        self.directories.get(id)
    }

    /// Gets a mutable directory by ID
    pub fn get_directory_mut(&mut self, id: &ObjectId) -> Option<&mut DirectoryView> {
        self.directories.get_mut(id)
    }

    /// Resolves a path and returns the final directory and entry name
    fn resolve_parent<'a>(
        &'a self,
        root: &'a DirectoryView,
        path: &str,
    ) -> Result<(&'a DirectoryView, String), OperationError> {
        let components = PathResolver::split_path(path)?;

        if components.is_empty() {
            return Err(OperationError::PathError(fs_view::PathError::InvalidPath(
                "Empty path".to_string(),
            )));
        }

        if components.len() == 1 {
            return Ok((root, components[0].to_string()));
        }

        let mut current_dir = root;
        for component in &components[..components.len() - 1] {
            let entry = current_dir
                .get_entry(component)
                .ok_or_else(|| OperationError::NotFound(component.to_string()))?;

            if entry.kind != ObjectKind::Map {
                return Err(OperationError::NotADirectory(component.to_string()));
            }

            current_dir = self
                .directories
                .get(&entry.object_id)
                .ok_or_else(|| OperationError::NotFound(component.to_string()))?;
        }

        let final_component = components.last().unwrap().to_string();
        Ok((current_dir, final_component))
    }

    /// Resolves a full path to a directory entry
    fn resolve_path<'a>(
        &'a self,
        root: &'a DirectoryView,
        path: &str,
    ) -> Result<&'a DirectoryEntry, OperationError> {
        let (parent_dir, name) = self.resolve_parent(root, path)?;
        parent_dir
            .get_entry(&name)
            .ok_or(OperationError::NotFound(name))
    }
}

impl Default for FileSystemViewService {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystemOperations for FileSystemViewService {
    fn ls(&self, root: &DirectoryView, path: &str) -> Result<Vec<DirectoryEntry>, OperationError> {
        // Special case: if path is empty or just "/", list root
        let trimmed_path = path.trim_matches('/');
        if trimmed_path.is_empty() {
            return Ok(root.list_entries().into_iter().cloned().collect());
        }

        let entry = self.resolve_path(root, path)?;

        if entry.kind != ObjectKind::Map {
            return Err(OperationError::NotADirectory(path.to_string()));
        }

        let dir = self
            .directories
            .get(&entry.object_id)
            .ok_or_else(|| OperationError::NotFound(path.to_string()))?;

        Ok(dir.list_entries().into_iter().cloned().collect())
    }

    fn stat(&self, root: &DirectoryView, path: &str) -> Result<StatInfo, OperationError> {
        // Special case: if path is empty or just "/", return root stat
        let trimmed_path = path.trim_matches('/');
        if trimmed_path.is_empty() {
            return Ok(StatInfo {
                id: root.id,
                kind: ObjectKind::Map,
                size: None,
                entry_count: Some(root.count()),
            });
        }

        let entry = self.resolve_path(root, path)?;

        let (size, entry_count) = if entry.kind == ObjectKind::Map {
            let dir = self.directories.get(&entry.object_id);
            (None, dir.map(|d| d.count()))
        } else {
            // Note: Size is None for non-directory objects since we don't store
            // actual blob data in this view service. A real implementation would
            // query the storage service for actual object sizes.
            (None, None)
        };

        Ok(StatInfo {
            id: entry.object_id,
            kind: entry.kind,
            size,
            entry_count,
        })
    }

    fn open(&self, root: &DirectoryView, path: &str) -> Result<ObjectId, OperationError> {
        // Special case: if path is empty or just "/", return root ID
        let trimmed_path = path.trim_matches('/');
        if trimmed_path.is_empty() {
            return Ok(root.id);
        }

        let entry = self.resolve_path(root, path)?;
        Ok(entry.object_id)
    }

    fn mkdir(&mut self, root: &mut DirectoryView, path: &str) -> Result<ObjectId, OperationError> {
        let (parent_dir, name) = self.resolve_parent(root, path)?;

        // Check if already exists
        if parent_dir.get_entry(&name).is_some() {
            return Err(OperationError::AlreadyExists(name));
        }

        // Create new directory
        let new_dir_id = ObjectId::new();
        let new_dir = DirectoryView::new(new_dir_id);

        // Add entry to parent
        let parent_id = parent_dir.id;
        let entry = DirectoryEntry::new(name.clone(), new_dir_id, ObjectKind::Map);

        // We need to get mutable parent from the service's directories
        // But we also need to handle the case where parent is root
        if parent_id == root.id {
            root.add_entry(entry);
        } else {
            let parent_mut = self
                .directories
                .get_mut(&parent_id)
                .ok_or_else(|| OperationError::NotFound("parent".to_string()))?;
            parent_mut.add_entry(entry);
        }

        // Register the new directory
        self.directories.insert(new_dir_id, new_dir);

        Ok(new_dir_id)
    }

    fn link(
        &mut self,
        root: &mut DirectoryView,
        path: &str,
        object_id: ObjectId,
        kind: ObjectKind,
    ) -> Result<(), OperationError> {
        let (parent_dir, name) = self.resolve_parent(root, path)?;

        // Check if already exists
        if parent_dir.get_entry(&name).is_some() {
            return Err(OperationError::AlreadyExists(name));
        }

        // Create entry
        let entry = DirectoryEntry::new(name.clone(), object_id, kind);
        let parent_id = parent_dir.id;

        // Add to parent
        if parent_id == root.id {
            root.add_entry(entry);
        } else {
            let parent_mut = self
                .directories
                .get_mut(&parent_id)
                .ok_or_else(|| OperationError::NotFound("parent".to_string()))?;
            parent_mut.add_entry(entry);
        }

        Ok(())
    }

    fn unlink(&mut self, root: &mut DirectoryView, path: &str) -> Result<(), OperationError> {
        let (parent_dir, name) = self.resolve_parent(root, path)?;

        // Check if exists
        if parent_dir.get_entry(&name).is_none() {
            return Err(OperationError::NotFound(name));
        }

        let parent_id = parent_dir.id;

        // Remove from parent
        if parent_id == root.id {
            root.remove_entry(&name);
        } else {
            let parent_mut = self
                .directories
                .get_mut(&parent_id)
                .ok_or_else(|| OperationError::NotFound("parent".to_string()))?;
            parent_mut.remove_entry(&name);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_creation() {
        let service = FileSystemViewService::new();
        assert_eq!(service.directories.len(), 0);
    }

    #[test]
    fn test_register_directory() {
        let mut service = FileSystemViewService::new();
        let dir_id = ObjectId::new();
        let dir = DirectoryView::new(dir_id);

        service.register_directory(dir);
        assert!(service.get_directory(&dir_id).is_some());
    }

    #[test]
    fn test_mkdir_in_root() {
        let mut service = FileSystemViewService::new();
        let root_id = ObjectId::new();
        let mut root = DirectoryView::new(root_id);

        let new_dir_id = service.mkdir(&mut root, "docs").unwrap();
        assert!(root.get_entry("docs").is_some());
        assert!(service.get_directory(&new_dir_id).is_some());
    }

    #[test]
    fn test_mkdir_already_exists() {
        let mut service = FileSystemViewService::new();
        let root_id = ObjectId::new();
        let mut root = DirectoryView::new(root_id);

        service.mkdir(&mut root, "docs").unwrap();
        let result = service.mkdir(&mut root, "docs");
        assert!(result.is_err());
        assert!(matches!(result, Err(OperationError::AlreadyExists(_))));
    }

    #[test]
    fn test_link_and_open() {
        let mut service = FileSystemViewService::new();
        let root_id = ObjectId::new();
        let mut root = DirectoryView::new(root_id);

        let obj_id = ObjectId::new();
        service
            .link(&mut root, "file.txt", obj_id, ObjectKind::Blob)
            .unwrap();

        let opened_id = service.open(&root, "file.txt").unwrap();
        assert_eq!(opened_id, obj_id);
    }

    #[test]
    fn test_unlink() {
        let mut service = FileSystemViewService::new();
        let root_id = ObjectId::new();
        let mut root = DirectoryView::new(root_id);

        let obj_id = ObjectId::new();
        service
            .link(&mut root, "file.txt", obj_id, ObjectKind::Blob)
            .unwrap();

        assert!(root.get_entry("file.txt").is_some());

        service.unlink(&mut root, "file.txt").unwrap();
        assert!(root.get_entry("file.txt").is_none());
    }

    #[test]
    fn test_stat() {
        let mut service = FileSystemViewService::new();
        let root_id = ObjectId::new();
        let mut root = DirectoryView::new(root_id);

        let obj_id = ObjectId::new();
        service
            .link(&mut root, "file.txt", obj_id, ObjectKind::Blob)
            .unwrap();

        let stat = service.stat(&root, "file.txt").unwrap();
        assert_eq!(stat.id, obj_id);
        assert_eq!(stat.kind, ObjectKind::Blob);
    }

    #[test]
    fn test_ls_directory() {
        let mut service = FileSystemViewService::new();
        let root_id = ObjectId::new();
        let mut root = DirectoryView::new(root_id);

        let dir_id = service.mkdir(&mut root, "docs").unwrap();

        // Add files to the directory
        let docs_dir = service.get_directory_mut(&dir_id).unwrap();
        docs_dir.add_entry(DirectoryEntry::new(
            "file1.txt".to_string(),
            ObjectId::new(),
            ObjectKind::Blob,
        ));
        docs_dir.add_entry(DirectoryEntry::new(
            "file2.txt".to_string(),
            ObjectId::new(),
            ObjectKind::Blob,
        ));

        let entries = service.ls(&root, "docs").unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_capability_safety_cannot_traverse_without_cap() {
        let service = FileSystemViewService::new();
        let root_id = ObjectId::new();
        let root = DirectoryView::new(root_id);

        // Try to open a path that doesn't exist in root
        let result = service.open(&root, "secret/data.txt");
        assert!(result.is_err());
        assert!(matches!(result, Err(OperationError::NotFound(_))));
    }
}
