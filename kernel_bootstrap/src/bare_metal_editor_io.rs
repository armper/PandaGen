//! Bare-metal Editor I/O implementation
//!
//! Provides file I/O for the minimal editor using the bare-metal filesystem.

extern crate alloc;

use crate::bare_metal_storage::BareMetalFilesystem;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use services_storage::ObjectId;

/// The schema Notepad writes (GFX-057).
pub const TEXT_SCHEMA: &str = "text/plain";

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
    /// Seconds since the epoch, from the RTC, stamped on what this writes;
    /// 0 when the kernel has no date.
    now: u64,
}

impl BareMetalEditorIo {
    pub fn new(fs: BareMetalFilesystem) -> Self {
        Self { fs, now: 0 }
    }

    /// `new`, with the time writes are stamped with (GFX-056).
    pub fn with_clock(fs: BareMetalFilesystem, now: u64) -> Self {
        Self { fs, now }
    }

    /// The root directory with sizes and times.
    pub fn list_entries(&mut self) -> Result<Vec<crate::desk::FileEntry>, EditorIoError> {
        Ok(self.fs.list_entries()?)
    }

    /// Remove `name`.
    pub fn delete(&mut self, name: &str) -> Result<(), EditorIoError> {
        let known = self.fs.list_files()?.iter().any(|n| n == name);
        if !known {
            return Err(EditorIoError::NotFound);
        }
        Ok(self.fs.delete_file_at(name, self.now)?)
    }

    /// Give `from` the name `to`. The filesystem has no rename, so this is
    /// a copy under the new name and then the old name's removal -- in that
    /// order, so a failure part-way leaves the file under at least one name.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), EditorIoError> {
        if from == to || to.is_empty() {
            return Ok(());
        }
        let content = self.fs.read_file_by_name(from)?;
        self.fs.create_file_at(to, &content, self.now)?;
        self.fs.delete_file_at(from, self.now)?;
        Ok(())
    }

    /// Create `name` empty, unless it exists.
    pub fn create_empty(&mut self, name: &str) -> Result<(), EditorIoError> {
        if self.fs.list_files()?.iter().any(|n| n == name) {
            return Err(EditorIoError::StorageError(alloc::format!("{name} exists")));
        }
        self.fs
            .write_named(name, b"", self.now, Some(TEXT_SCHEMA))?;
        Ok(())
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
        // Notepad writes text; the entry says so, and the name need not.
        let object_id =
            self.fs
                .write_named(path, content.as_bytes(), self.now, Some(TEXT_SCHEMA))?;
        let handle = DocumentHandle::new(Some(object_id), Some(path.to_string()));
        Ok((alloc::format!("Saved as {}", path), handle))
    }

    /// Add and remove tags on `name` (GFX-057).
    pub fn set_tags(
        &mut self,
        name: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<(), EditorIoError> {
        let changed = self.fs.update_entry(name, self.now, |entry| {
            entry.tags.retain(|t| !remove.contains(t));
            for tag in add {
                if !entry.tags.contains(tag) {
                    entry.tags.push(tag.clone());
                }
            }
            entry.tags.sort();
        })?;
        if changed {
            Ok(())
        } else {
            Err(EditorIoError::NotFound)
        }
    }

    /// Put `name` in the bin, or take it out.
    pub fn set_trashed(&mut self, name: &str, trashed: bool) -> Result<(), EditorIoError> {
        let changed = self
            .fs
            .update_entry(name, self.now, |entry| entry.trashed = trashed)?;
        if changed {
            Ok(())
        } else {
            Err(EditorIoError::NotFound)
        }
    }

    /// An earlier content of `name`; 0 is the newest kept.
    pub fn read_version(&mut self, name: &str, index: usize) -> Result<String, EditorIoError> {
        let bytes = self.fs.read_version(name, index)?;
        String::from_utf8(bytes).map_err(|_| EditorIoError::InvalidUtf8)
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
