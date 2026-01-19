//! Directory view and entry types
//!
//! This module defines how directories are represented in the filesystem view.

use services_storage::{ObjectId, ObjectKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Marker type for object capabilities in the filesystem view
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectCapability;

/// A single entry in a directory
///
/// Maps a name to an object capability.
#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    /// Name of this entry
    pub name: String,
    /// Object ID of the target
    pub object_id: ObjectId,
    /// Kind of object
    pub kind: ObjectKind,
}

impl DirectoryEntry {
    /// Creates a new directory entry
    pub fn new(name: String, object_id: ObjectId, kind: ObjectKind) -> Self {
        Self {
            name,
            object_id,
            kind,
        }
    }
}

/// A directory view
///
/// Represents a directory as a map from names to object capabilities.
/// This is a conceptual representation; in reality, directories are
/// stored as Map objects in the storage service.
#[derive(Debug, Clone)]
pub struct DirectoryView {
    /// The object ID of this directory
    pub id: ObjectId,
    /// Entries in this directory (name -> entry)
    entries: HashMap<String, DirectoryEntry>,
}

impl DirectoryView {
    /// Creates a new empty directory view
    pub fn new(id: ObjectId) -> Self {
        Self {
            id,
            entries: HashMap::new(),
        }
    }

    /// Adds an entry to the directory
    ///
    /// Returns true if the entry was added, false if it already exists.
    pub fn add_entry(&mut self, entry: DirectoryEntry) -> bool {
        if self.entries.contains_key(&entry.name) {
            return false;
        }
        self.entries.insert(entry.name.clone(), entry);
        true
    }

    /// Removes an entry from the directory
    ///
    /// Returns the removed entry if it existed.
    pub fn remove_entry(&mut self, name: &str) -> Option<DirectoryEntry> {
        self.entries.remove(name)
    }

    /// Gets an entry by name
    pub fn get_entry(&self, name: &str) -> Option<&DirectoryEntry> {
        self.entries.get(name)
    }

    /// Lists all entries in the directory
    pub fn list_entries(&self) -> Vec<&DirectoryEntry> {
        self.entries.values().collect()
    }

    /// Counts the number of entries
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directory_entry_creation() {
        let obj_id = ObjectId::new();
        let entry = DirectoryEntry::new("test.txt".to_string(), obj_id, ObjectKind::Blob);
        
        assert_eq!(entry.name, "test.txt");
        assert_eq!(entry.object_id, obj_id);
        assert_eq!(entry.kind, ObjectKind::Blob);
    }

    #[test]
    fn test_directory_view_creation() {
        let dir_id = ObjectId::new();
        let dir = DirectoryView::new(dir_id);
        
        assert_eq!(dir.id, dir_id);
        assert_eq!(dir.count(), 0);
    }

    #[test]
    fn test_add_entry() {
        let dir_id = ObjectId::new();
        let mut dir = DirectoryView::new(dir_id);
        
        let obj_id = ObjectId::new();
        let entry = DirectoryEntry::new("file.txt".to_string(), obj_id, ObjectKind::Blob);
        
        assert!(dir.add_entry(entry));
        assert_eq!(dir.count(), 1);
    }

    #[test]
    fn test_add_duplicate_entry() {
        let dir_id = ObjectId::new();
        let mut dir = DirectoryView::new(dir_id);
        
        let obj_id = ObjectId::new();
        let entry1 = DirectoryEntry::new("file.txt".to_string(), obj_id, ObjectKind::Blob);
        let entry2 = DirectoryEntry::new("file.txt".to_string(), obj_id, ObjectKind::Blob);
        
        assert!(dir.add_entry(entry1));
        assert!(!dir.add_entry(entry2)); // Duplicate should fail
        assert_eq!(dir.count(), 1);
    }

    #[test]
    fn test_remove_entry() {
        let dir_id = ObjectId::new();
        let mut dir = DirectoryView::new(dir_id);
        
        let obj_id = ObjectId::new();
        let entry = DirectoryEntry::new("file.txt".to_string(), obj_id, ObjectKind::Blob);
        dir.add_entry(entry);
        
        let removed = dir.remove_entry("file.txt");
        assert!(removed.is_some());
        assert_eq!(dir.count(), 0);
    }

    #[test]
    fn test_remove_nonexistent_entry() {
        let dir_id = ObjectId::new();
        let mut dir = DirectoryView::new(dir_id);
        
        let removed = dir.remove_entry("nonexistent.txt");
        assert!(removed.is_none());
    }

    #[test]
    fn test_get_entry() {
        let dir_id = ObjectId::new();
        let mut dir = DirectoryView::new(dir_id);
        
        let obj_id = ObjectId::new();
        let entry = DirectoryEntry::new("file.txt".to_string(), obj_id, ObjectKind::Blob);
        dir.add_entry(entry);
        
        let retrieved = dir.get_entry("file.txt");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "file.txt");
    }

    #[test]
    fn test_list_entries() {
        let dir_id = ObjectId::new();
        let mut dir = DirectoryView::new(dir_id);
        
        let obj1 = ObjectId::new();
        let obj2 = ObjectId::new();
        
        dir.add_entry(DirectoryEntry::new("file1.txt".to_string(), obj1, ObjectKind::Blob));
        dir.add_entry(DirectoryEntry::new("file2.txt".to_string(), obj2, ObjectKind::Blob));
        
        let entries = dir.list_entries();
        assert_eq!(entries.len(), 2);
    }
}
