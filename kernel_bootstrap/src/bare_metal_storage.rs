//! Bare-metal storage integration
//!
//! Provides filesystem access for the kernel_bootstrap environment.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use services_storage::{
    ObjectId, PersistentFilesystem, TransactionError,
};
use hal::RamDisk;

/// Bare-metal filesystem wrapper
pub struct BareMetalFilesystem {
    pub(crate) fs: PersistentFilesystem<RamDisk>,
    root_id: ObjectId,
}

impl BareMetalFilesystem {
    /// Create a new filesystem with an in-memory disk
    pub fn new() -> Result<Self, TransactionError> {
        // Create a 10 MB RAM disk for now
        // TODO: Replace with VirtioBlkDevice when initialization is ready
        let disk = RamDisk::with_capacity_mb(10);
        
        let fs = PersistentFilesystem::format(disk, "system")?;
        let root_id = fs.root_dir_id();
        
        Ok(Self { fs, root_id })
    }
    
    /// Get the root directory ID
    pub fn root_id(&self) -> ObjectId {
        self.root_id
    }
    
    /// Create a file with content
    pub fn create_file(&mut self, name: &str, content: &[u8]) -> Result<ObjectId, TransactionError> {
        let file_id = self.fs.write_file(content)?;
        self.fs.link(name, self.root_id, file_id, services_storage::ObjectKind::Blob, 0)?;
        Ok(file_id)
    }
    
    /// Read a file by name
    pub fn read_file_by_name(&mut self, name: &str) -> Result<Vec<u8>, TransactionError> {
        let dir = self.fs.read_directory(self.root_id)?;
        let entry = dir.get_entry(name)
            .ok_or_else(|| TransactionError::StorageError("File not found".into()))?;
        self.fs.read_file(entry.object_id)
    }
    
    /// Write content to a file (update existing or create new)
    pub fn write_file_by_name(&mut self, name: &str, content: &[u8]) -> Result<ObjectId, TransactionError> {
        // Try to unlink existing file first
        let _ = self.fs.unlink(name, self.root_id, 0);
        
        // Create new file
        self.create_file(name, content)
    }
    
    /// List files in root directory
    pub fn list_files(&mut self) -> Result<Vec<String>, TransactionError> {
        let entries = self.fs.list(self.root_id)?;
        Ok(entries.into_iter().map(|(name, _)| name).collect())
    }
    
    /// Delete a file
    pub fn delete_file(&mut self, name: &str) -> Result<(), TransactionError> {
        self.fs.unlink(name, self.root_id, 0)?;
        Ok(())
    }
    
    /// Read file by object ID
    pub fn read_file(&mut self, object_id: ObjectId) -> Result<Vec<u8>, TransactionError> {
        self.fs.read_file(object_id)
    }
    
    /// Write file by object ID (creates new version)
    pub fn write_file(&mut self, _object_id: ObjectId, content: &[u8]) -> Result<ObjectId, TransactionError> {
        // For now, we need to replace the file entirely
        // In a full implementation, we'd update the version
        let file_id = self.fs.write_file(content)?;
        Ok(file_id)
    }
}

impl Default for BareMetalFilesystem {
    fn default() -> Self {
        Self::new().expect("Failed to create filesystem")
    }
}
