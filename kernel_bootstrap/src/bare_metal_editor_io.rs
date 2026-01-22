//! Bare-metal Editor I/O implementation
//!
//! Provides file I/O for the minimal editor using the bare-metal filesystem.

extern crate alloc;

use crate::bare_metal_storage::BareMetalFilesystem;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use services_storage::ObjectId;

/// Editor I/O error
#[derive(Debug)]
pub enum EditorIoError {
    NotFound,
    StorageError(String),
    InvalidUtf8,
}

impl From<services_storage::TransactionError> for EditorIoError {
    fn from(err: services_storage::TransactionError) -> Self {
        match err {
            services_storage::TransactionError::ObjectNotFound(_) => EditorIoError::NotFound,
            other => EditorIoError::StorageError(alloc::format!("{:?}", other)),
        }
    }
}

/// Document handle for bare-metal editor
#[derive(Debug, Clone)]
pub struct DocumentHandle {
    pub object_id: Option<ObjectId>,
    pub path: Option<String>,
}

impl DocumentHandle {
    pub fn new(object_id: Option<ObjectId>, path: Option<String>) -> Self {
        Self { object_id, path }
    }
}

/// Bare-metal editor I/O implementation
pub struct BareMetalEditorIo {
    fs: BareMetalFilesystem,
}

impl BareMetalEditorIo {
    pub fn new(fs: BareMetalFilesystem) -> Self {
        Self { fs }
    }

    /// Extract the filesystem (for returning to workspace)
    pub fn into_filesystem(self) -> BareMetalFilesystem {
        self.fs
    }

    /// Open a file by path
    pub fn open(&mut self, path: &str) -> Result<(String, DocumentHandle), EditorIoError> {
        let content = self.fs.read_file_by_name(path)?;
        let content_str = core::str::from_utf8(&content)
            .map_err(|_| EditorIoError::InvalidUtf8)?
            .to_string();

        // Get the object ID for this file
        let dir = self.fs.fs.read_directory(self.fs.root_id())?;
        let entry = dir.get_entry(path).ok_or(EditorIoError::NotFound)?;

        let handle = DocumentHandle::new(Some(entry.object_id), Some(path.to_string()));
        Ok((content_str, handle))
    }

    /// Save content to the current file
    pub fn save(
        &mut self,
        handle: &DocumentHandle,
        content: &str,
    ) -> Result<String, EditorIoError> {
        if let Some(ref path) = handle.path {
            self.fs.write_file_by_name(path, content.as_bytes())?;
            Ok(alloc::format!("Saved to {}", path))
        } else {
            Err(EditorIoError::StorageError("No path specified".to_string()))
        }
    }

    /// Save content to a new path (save-as)
    pub fn save_as(
        &mut self,
        path: &str,
        content: &str,
    ) -> Result<(String, DocumentHandle), EditorIoError> {
        let object_id = self.fs.write_file_by_name(path, content.as_bytes())?;
        let handle = DocumentHandle::new(Some(object_id), Some(path.to_string()));
        Ok((alloc::format!("Saved as {}", path), handle))
    }

    /// Create a new empty file
    pub fn new_buffer(&self, path: Option<String>) -> DocumentHandle {
        DocumentHandle::new(None, path)
    }

    /// List available files
    pub fn list_files(&mut self) -> Result<Vec<String>, EditorIoError> {
        Ok(self.fs.list_files()?)
    }
}
