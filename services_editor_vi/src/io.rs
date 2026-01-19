//! Document I/O operations

use services_storage::{ObjectId, VersionId};
use thiserror::Error;

/// Document I/O error
#[derive(Debug, Error)]
pub enum IoError {
    #[error("Document not found")]
    NotFound,

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Invalid UTF-8 content")]
    InvalidUtf8,
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
