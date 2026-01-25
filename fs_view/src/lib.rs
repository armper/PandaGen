//! # Filesystem View
//!
//! This crate provides a filesystem-like view over capability-based storage objects.
//!
//! ## Philosophy
//!
//! - **Paths are views, not authority**: Path resolution never grants access you don't have
//! - **Naming is a service, not a primitive**: Directory hierarchy is built from Map objects
//! - **Storage remains object-based**: No global filesystem, no kernel-level paths
//! - **Humans get convenience, system keeps truth**: Familiar UX without compromising design
//!
//! ## Design
//!
//! - A directory is a `Map` object mapping names to `ObjectCapability`
//! - There is NO global root; a "root" is simply a Map capability given to you
//! - Path resolution walks Map objects step-by-step
//! - Resolution consumes no authority beyond what is already held

pub mod directory;
pub mod path;

pub use directory::{DirectoryEntry, DirectoryView};
pub use path::{PathError, PathResolver};

use services_storage::ObjectId;

/// Trait for resolving subdirectories
///
/// This trait abstracts away the mechanism for loading directory contents,
/// allowing components to work with different storage backends while
/// remaining testable and avoiding circular dependencies.
pub trait DirectoryResolver {
    /// Resolves a directory by its ObjectId, returning its view
    ///
    /// Returns None if the directory cannot be accessed or doesn't exist.
    fn resolve_directory(&self, id: &ObjectId) -> Option<DirectoryView>;
}
