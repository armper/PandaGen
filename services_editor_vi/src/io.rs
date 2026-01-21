//! Document I/O operations

use alloc::string::{String, ToString};
use alloc::format;
use core::fmt;
use fs_view::DirectoryView;
use services_fs_view::{FileSystemOperations, FileSystemViewService};
use services_storage::{
    JournaledStorage, ObjectId, TransactionError, TransactionalStorage, VersionId,
};

/// Document I/O error
#[derive(Debug)]
pub enum IoError {
    NotFound,
    PermissionDenied(String),
    StorageError(String),
    InvalidUtf8,
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IoError::NotFound => write!(f, "Document not found"),
            IoError::PermissionDenied(s) => write!(f, "Permission denied: {}", s),
            IoError::StorageError(s) => write!(f, "Storage error: {}", s),
            IoError::InvalidUtf8 => write!(f, "Invalid UTF-8 content"),
        }
    }
}

/// Document handle
///
/// Represents an open document with its capability.
/// Documents are identified by object IDs and version IDs.
#[derive(Debug, Clone)]
pub struct DocumentHandle {
    /// Object ID of the document
    pub object_id: ObjectId,
    /// Current version ID
    pub version_id: VersionId,
    /// Optional path label (for display only, not authority)
    pub path_label: Option<String>,
    /// Whether we have write permission to the directory
    pub can_update_link: bool,
}

impl DocumentHandle {
    pub fn new(
        object_id: ObjectId,
        version_id: VersionId,
        path_label: Option<String>,
        can_update_link: bool,
    ) -> Self {
        Self {
            object_id,
            version_id,
            path_label,
            can_update_link,
        }
    }
}

/// Options for opening a document
#[derive(Debug, Clone)]
pub struct OpenOptions {
    /// Path to open (for convenience via fs_view)
    pub path: Option<String>,
    /// Direct object capability (preferred)
    pub object_id: Option<ObjectId>,
}

impl OpenOptions {
    pub fn new() -> Self {
        Self {
            path: None,
            object_id: None,
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_object(mut self, object_id: ObjectId) -> Self {
        self.object_id = Some(object_id);
        self
    }
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Save result
///
/// Contains the new version capability created by the save operation.
#[derive(Debug, Clone)]
pub struct SaveResult {
    /// New version ID created
    pub new_version_id: VersionId,
    /// Whether the directory link was updated
    pub link_updated: bool,
    /// Status message
    pub message: String,
}

/// Open result containing document content and handle.
#[derive(Debug, Clone)]
pub struct OpenResult {
    pub content: String,
    pub handle: DocumentHandle,
}

/// Editor I/O abstraction.
pub trait EditorIo {
    fn open(&mut self, options: OpenOptions) -> Result<OpenResult, IoError>;
    fn save(&mut self, handle: &DocumentHandle, content: &str) -> Result<SaveResult, IoError>;
    /// Save to a new path (Save As)
    fn save_as(&mut self, path: &str, content: &str) -> Result<SaveResult, IoError>;
}

/// Storage-backed editor I/O using JournaledStorage and optional fs_view.
pub struct StorageEditorIo {
    storage: JournaledStorage,
    fs_view: Option<FileSystemViewService>,
    root: Option<DirectoryView>,
}

impl StorageEditorIo {
    pub fn new(storage: JournaledStorage) -> Self {
        Self {
            storage,
            fs_view: None,
            root: None,
        }
    }

    pub fn with_fs_view(
        storage: JournaledStorage,
        fs_view: FileSystemViewService,
        root: DirectoryView,
    ) -> Self {
        Self {
            storage,
            fs_view: Some(fs_view),
            root: Some(root),
        }
    }

    pub fn storage(&self) -> &JournaledStorage {
        &self.storage
    }

    fn map_tx_error(err: TransactionError) -> IoError {
        match err {
            TransactionError::ObjectNotFound(_) => IoError::NotFound,
            other => IoError::StorageError(other.to_string()),
        }
    }
}

impl EditorIo for StorageEditorIo {
    fn open(&mut self, options: OpenOptions) -> Result<OpenResult, IoError> {
        let object_id = if let Some(object_id) = options.object_id {
            object_id
        } else if let Some(path) = options.path.clone() {
            let fs = self
                .fs_view
                .as_ref()
                .ok_or_else(|| IoError::PermissionDenied("No fs_view available".to_string()))?;
            let root = self
                .root
                .as_ref()
                .ok_or_else(|| IoError::PermissionDenied("No root directory".to_string()))?;
            match fs.open(root, &path) {
                Ok(id) => id,
                Err(services_fs_view::OperationError::NotFound(_)) => {
                    return Err(IoError::NotFound)
                }
                Err(services_fs_view::OperationError::AccessDenied(reason)) => {
                    return Err(IoError::PermissionDenied(reason))
                }
                Err(err) => return Err(IoError::StorageError(err.to_string())),
            }
        } else {
            return Err(IoError::NotFound);
        };

        let mut tx = self
            .storage
            .begin_transaction()
            .map_err(|err| IoError::StorageError(err.to_string()))?;

        let version_id = self
            .storage
            .read(&tx, object_id)
            .map_err(Self::map_tx_error)?;
        let data = self
            .storage
            .read_data(&tx, object_id)
            .map_err(Self::map_tx_error)?;
        let _ = self.storage.rollback(&mut tx);

        let content = String::from_utf8(data).map_err(|_| IoError::InvalidUtf8)?;
        let handle = DocumentHandle::new(
            object_id,
            version_id,
            options.path.clone(),
            self.fs_view.is_some(),
        );

        Ok(OpenResult { content, handle })
    }

    fn save(&mut self, handle: &DocumentHandle, content: &str) -> Result<SaveResult, IoError> {
        let mut tx = self
            .storage
            .begin_transaction()
            .map_err(|err| IoError::StorageError(err.to_string()))?;

        let new_version_id = self
            .storage
            .write(&mut tx, handle.object_id, content.as_bytes())
            .map_err(Self::map_tx_error)?;
        self.storage.commit(&mut tx).map_err(Self::map_tx_error)?;

        Ok(SaveResult::new(new_version_id, false, "Saved successfully"))
    }

    fn save_as(&mut self, path: &str, content: &str) -> Result<SaveResult, IoError> {
        let fs = self
            .fs_view
            .as_mut()
            .ok_or_else(|| IoError::PermissionDenied("No fs_view available".to_string()))?;
        let root = self
            .root
            .as_mut()
            .ok_or_else(|| IoError::PermissionDenied("No root directory".to_string()))?;

        // Create new object
        let mut tx = self
            .storage
            .begin_transaction()
            .map_err(|err| IoError::StorageError(err.to_string()))?;

        let object_id = ObjectId::new();
        let version_id = self
            .storage
            .write(&mut tx, object_id, content.as_bytes())
            .map_err(Self::map_tx_error)?;
        self.storage.commit(&mut tx).map_err(Self::map_tx_error)?;

        // Link to filesystem (simplified - assumes path is just a name in current dir)
        // In a full implementation, this would parse the path and create directories as needed
        let name = path.split('/').last().unwrap_or(path);
        if let Err(err) = fs.link(root, name, object_id, services_storage::ObjectKind::Blob) {
            return Err(IoError::StorageError(format!(
                "Failed to link file: {}",
                err
            )));
        }

        Ok(SaveResult::new(
            version_id,
            true,
            format!("Saved as: {}", path),
        ))
    }
}

impl SaveResult {
    pub fn new(new_version_id: VersionId, link_updated: bool, message: impl Into<String>) -> Self {
        Self {
            new_version_id,
            link_updated,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_handle() {
        let obj_id = ObjectId::new();
        let ver_id = VersionId::new();

        let handle = DocumentHandle::new(obj_id, ver_id, Some("/test/file.txt".to_string()), true);

        assert_eq!(handle.object_id, obj_id);
        assert_eq!(handle.version_id, ver_id);
        assert_eq!(handle.path_label, Some("/test/file.txt".to_string()));
        assert!(handle.can_update_link);
    }

    #[test]
    fn test_open_options() {
        let opts = OpenOptions::new().with_path("/test/file.txt");

        assert_eq!(opts.path, Some("/test/file.txt".to_string()));
        assert!(opts.object_id.is_none());
    }

    #[test]
    fn test_open_options_with_object() {
        let obj_id = ObjectId::new();
        let opts = OpenOptions::new().with_object(obj_id);

        assert!(opts.path.is_none());
        assert_eq!(opts.object_id, Some(obj_id));
    }

    #[test]
    fn test_save_result() {
        let ver_id = VersionId::new();
        let result = SaveResult::new(ver_id, true, "Saved successfully");

        assert_eq!(result.new_version_id, ver_id);
        assert!(result.link_updated);
        assert_eq!(result.message, "Saved successfully");
    }
}
