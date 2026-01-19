//! # Filesystem View Service
//!
//! This service provides filesystem-like operations over capability-based storage.
//!
//! ## Philosophy
//!
//! - All operations are capability-driven
//! - Service never escalates authority
//! - Linking requires explicit object capability
//! - Removing links does NOT delete objects (immutability preserved)
//!
//! ## Operations
//!
//! - `ls(path)`: List directory contents
//! - `stat(path)`: Get object metadata
//! - `open(path)`: Resolve path and return object capability
//! - `mkdir(path)`: Create a new directory
//! - `link(path, object_cap)`: Create a name -> object link
//! - `unlink(path)`: Remove a name -> object link

pub mod operations;
pub mod service;

pub use operations::{FileSystemOperations, OperationError, StatInfo};
pub use service::FileSystemViewService;
