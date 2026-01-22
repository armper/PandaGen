//! Persistent Filesystem Backend
//!
//! This module integrates fs_view with block-backed storage to provide
//! persistent filesystem capabilities.

use crate::{
    BlockStorage, ObjectId, ObjectKind, TransactionError, TransactionalStorage, VersionId,
};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hal::BlockDevice;
use serde::{Deserialize, Serialize};

/// Directory entry - compatible with fs_view::DirectoryEntry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryEntry {
    /// Entry name
    pub name: String,
    /// Object ID this entry points to
    pub object_id: ObjectId,
    /// Object kind (Blob, Map, Log)
    pub kind: ObjectKind,
}

impl DirectoryEntry {
    /// Create a new directory entry
    pub fn new(name: String, object_id: ObjectId, kind: ObjectKind) -> Self {
        Self {
            name,
            object_id,
            kind,
        }
    }
}

/// Persistent directory storage backed by blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentDirectory {
    /// Directory entries (name → object_id mapping)
    pub entries: BTreeMap<String, DirectoryEntry>,
    /// Parent directory (if any)
    pub parent: Option<ObjectId>,
    /// Metadata
    pub metadata: DirectoryMetadata,
}

/// Directory metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryMetadata {
    /// Creation timestamp (nanoseconds since epoch)
    pub created_at: u64,
    /// Last modification timestamp
    pub modified_at: u64,
    /// Owner identity
    pub owner: String,
}

impl PersistentDirectory {
    /// Create a new empty directory
    pub fn new(owner: impl Into<String>, timestamp: u64) -> Self {
        Self {
            entries: BTreeMap::new(),
            parent: None,
            metadata: DirectoryMetadata {
                created_at: timestamp,
                modified_at: timestamp,
                owner: owner.into(),
            },
        }
    }

    /// Add an entry to the directory
    pub fn add_entry(&mut self, name: String, entry: DirectoryEntry, timestamp: u64) {
        self.entries.insert(name, entry);
        self.metadata.modified_at = timestamp;
    }

    /// Remove an entry from the directory
    pub fn remove_entry(&mut self, name: &str, timestamp: u64) -> Option<DirectoryEntry> {
        let entry = self.entries.remove(name);
        if entry.is_some() {
            self.metadata.modified_at = timestamp;
        }
        entry
    }

    /// Get an entry by name
    pub fn get_entry(&self, name: &str) -> Option<&DirectoryEntry> {
        self.entries.get(name)
    }

    /// List all entries
    pub fn list_entries(&self) -> Vec<(String, DirectoryEntry)> {
        self.entries
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

/// Persistent filesystem operations
pub struct PersistentFilesystem<D: BlockDevice> {
    storage: BlockStorage<D>,
    root_dir_id: ObjectId,
}

impl<D: BlockDevice> PersistentFilesystem<D> {
    /// Create a new filesystem with an empty root directory
    pub fn format(device: D, owner: impl Into<String>) -> Result<Self, TransactionError> {
        let mut storage = BlockStorage::format(device)
            .map_err(|e| TransactionError::StorageError(format!("format failed: {:?}", e)))?;

        // Create root directory
        let root_dir = PersistentDirectory::new(owner, 0);
        let root_json = serde_json::to_vec(&root_dir)
            .map_err(|e| TransactionError::StorageError(format!("serialize failed: {:?}", e)))?;

        // Write root directory to storage
        let root_dir_id = ObjectId::new();
        let mut tx = storage.begin_transaction()?;
        storage.write(&mut tx, root_dir_id, &root_json)?;
        storage.commit(&mut tx)?;

        Ok(Self {
            storage,
            root_dir_id,
        })
    }

    /// Open existing filesystem
    pub fn open(device: D, root_dir_id: ObjectId) -> Result<Self, TransactionError> {
        let storage = BlockStorage::open(device)
            .map_err(|e| TransactionError::StorageError(format!("open failed: {:?}", e)))?;

        Ok(Self {
            storage,
            root_dir_id,
        })
    }

    /// Get the root directory ID
    pub fn root_dir_id(&self) -> ObjectId {
        self.root_dir_id
    }

    /// Read a directory by object ID
    pub fn read_directory(
        &mut self,
        dir_id: ObjectId,
    ) -> Result<PersistentDirectory, TransactionError> {
        let tx = self.storage.begin_transaction()?;
        let version_id = self.storage.read(&tx, dir_id)?;

        let data = self
            .storage
            .read_object_data(dir_id, version_id)
            .map_err(|e| TransactionError::StorageError(format!("read data failed: {:?}", e)))?;

        let dir: PersistentDirectory = serde_json::from_slice(&data)
            .map_err(|e| TransactionError::StorageError(format!("deserialize failed: {:?}", e)))?;

        Ok(dir)
    }

    /// Write a directory to storage
    pub fn write_directory(
        &mut self,
        dir_id: ObjectId,
        dir: &PersistentDirectory,
    ) -> Result<VersionId, TransactionError> {
        let dir_json = serde_json::to_vec(dir)
            .map_err(|e| TransactionError::StorageError(format!("serialize failed: {:?}", e)))?;

        let mut tx = self.storage.begin_transaction()?;
        let version_id = self.storage.write(&mut tx, dir_id, &dir_json)?;
        self.storage.commit(&mut tx)?;

        Ok(version_id)
    }

    /// Create a new directory
    pub fn mkdir(
        &mut self,
        name: impl Into<String>,
        parent_dir_id: ObjectId,
        owner: impl Into<String>,
        timestamp: u64,
    ) -> Result<ObjectId, TransactionError> {
        // Create new directory
        let new_dir_id = ObjectId::new();
        let mut new_dir = PersistentDirectory::new(owner, timestamp);
        new_dir.parent = Some(parent_dir_id);

        // Write new directory
        self.write_directory(new_dir_id, &new_dir)?;

        // Add entry to parent directory
        let mut parent = self.read_directory(parent_dir_id)?;
        let entry = DirectoryEntry::new(name.into(), new_dir_id, ObjectKind::Map);
        parent.add_entry(entry.name.clone(), entry, timestamp);
        self.write_directory(parent_dir_id, &parent)?;

        Ok(new_dir_id)
    }

    /// Link an object into a directory
    pub fn link(
        &mut self,
        name: impl Into<String>,
        dir_id: ObjectId,
        object_id: ObjectId,
        kind: ObjectKind,
        timestamp: u64,
    ) -> Result<(), TransactionError> {
        let mut dir = self.read_directory(dir_id)?;
        let entry = DirectoryEntry::new(name.into(), object_id, kind);
        dir.add_entry(entry.name.clone(), entry, timestamp);
        self.write_directory(dir_id, &dir)?;
        Ok(())
    }

    /// Unlink an entry from a directory
    pub fn unlink(
        &mut self,
        name: &str,
        dir_id: ObjectId,
        timestamp: u64,
    ) -> Result<Option<DirectoryEntry>, TransactionError> {
        let mut dir = self.read_directory(dir_id)?;
        let entry = dir.remove_entry(name, timestamp);
        self.write_directory(dir_id, &dir)?;
        Ok(entry)
    }

    /// List directory contents
    pub fn list(
        &mut self,
        dir_id: ObjectId,
    ) -> Result<Vec<(String, DirectoryEntry)>, TransactionError> {
        let dir = self.read_directory(dir_id)?;
        Ok(dir.list_entries())
    }

    /// Write file content (as a Blob object)
    pub fn write_file(&mut self, content: &[u8]) -> Result<ObjectId, TransactionError> {
        let file_id = ObjectId::new();
        let mut tx = self.storage.begin_transaction()?;
        self.storage.write(&mut tx, file_id, content)?;
        self.storage.commit(&mut tx)?;
        Ok(file_id)
    }

    /// Read file content
    pub fn read_file(&mut self, file_id: ObjectId) -> Result<Vec<u8>, TransactionError> {
        let tx = self.storage.begin_transaction()?;
        let version_id = self.storage.read(&tx, file_id)?;
        let data = self
            .storage
            .read_object_data(file_id, version_id)
            .map_err(|e| TransactionError::StorageError(format!("read data failed: {:?}", e)))?;
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use hal::RamDisk;

    #[test]
    fn test_link_then_read() {
        let disk = RamDisk::with_capacity_mb(10);
        let mut fs = PersistentFilesystem::format(disk, "root").unwrap();

        let root_id = fs.root_dir_id();
        let file_id = fs.write_file(b"test").unwrap();

        // Link the file
        fs.link("test.txt", root_id, file_id, ObjectKind::Blob, 1000)
            .unwrap();

        // Try to read the directory
        let root_dir = fs.read_directory(root_id).unwrap();
        assert_eq!(root_dir.entries.len(), 1);
        assert!(root_dir.entries.contains_key("test.txt"));
    }

    #[test]
    fn test_read_after_write() {
        let disk = RamDisk::with_capacity_mb(10);
        let mut fs = PersistentFilesystem::format(disk, "root").unwrap();

        let root_id = fs.root_dir_id();

        // Try to read the root directory that was just created
        let root_dir = fs.read_directory(root_id).unwrap();
        assert_eq!(root_dir.entries.len(), 0);
    }

    #[test]
    fn test_format_and_root() {
        let disk = RamDisk::with_capacity_mb(10);
        let fs = PersistentFilesystem::format(disk, "root").unwrap();

        let root = fs.root_dir_id();
        assert!(root.to_string().len() > 0);
    }

    #[test]
    fn test_mkdir() {
        let disk = RamDisk::with_capacity_mb(10);
        let mut fs = PersistentFilesystem::format(disk, "root").unwrap();

        let root_id = fs.root_dir_id();
        let usr_id = fs.mkdir("usr", root_id, "root", 1000).unwrap();

        // Check that usr is in root
        let entries = fs.list(root_id).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "usr");
        assert_eq!(entries[0].1.object_id, usr_id);
    }

    #[test]
    fn test_write_read_file() {
        let disk = RamDisk::with_capacity_mb(10);
        let mut fs = PersistentFilesystem::format(disk, "root").unwrap();

        let content = b"Hello, PandaGen!";
        let file_id = fs.write_file(content).unwrap();

        let read_content = fs.read_file(file_id).unwrap();
        assert_eq!(read_content, content);
    }

    #[test]
    fn test_link_and_list() {
        let disk = RamDisk::with_capacity_mb(10);
        let mut fs = PersistentFilesystem::format(disk, "root").unwrap();

        let root_id = fs.root_dir_id();
        let content = b"test file";
        let file_id = fs.write_file(content).unwrap();

        fs.link("test.txt", root_id, file_id, ObjectKind::Blob, 2000)
            .unwrap();

        let entries = fs.list(root_id).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "test.txt");
        assert_eq!(entries[0].1.object_id, file_id);
    }

    #[test]
    fn test_unlink() {
        let disk = RamDisk::with_capacity_mb(10);
        let mut fs = PersistentFilesystem::format(disk, "root").unwrap();

        let root_id = fs.root_dir_id();
        let file_id = fs.write_file(b"data").unwrap();
        fs.link("file.txt", root_id, file_id, ObjectKind::Blob, 3000)
            .unwrap();

        let removed = fs.unlink("file.txt", root_id, 4000).unwrap();
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name, "file.txt");

        let entries = fs.list(root_id).unwrap();
        assert_eq!(entries.len(), 0);
    }
}
