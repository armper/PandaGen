//! Path resolution logic
//!
//! This module handles parsing and resolving paths in the filesystem view.

use thiserror::Error;

/// Errors that can occur during path resolution
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PathError {
    /// Path is empty or invalid
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// Path component not found during traversal
    #[error("Not found: {0}")]
    NotFound(String),

    /// Attempted to traverse through a non-directory object
    #[error("Not a directory: {0}")]
    NotADirectory(String),

    /// Capability error during resolution
    #[error("Access denied: {0}")]
    AccessDenied(String),
}

/// Path resolver
///
/// Handles splitting paths into components and validating syntax.
pub struct PathResolver;

impl PathResolver {
    /// Splits a path into components
    ///
    /// # Examples
    ///
    /// ```
    /// use fs_view::PathResolver;
    ///
    /// let components = PathResolver::split_path("docs/notes/todo.txt").unwrap();
    /// assert_eq!(components, vec!["docs", "notes", "todo.txt"]);
    ///
    /// let components = PathResolver::split_path("todo.txt").unwrap();
    /// assert_eq!(components, vec!["todo.txt"]);
    /// ```
    pub fn split_path(path: &str) -> Result<Vec<&str>, PathError> {
        // Remove leading/trailing slashes
        let path = path.trim_matches('/');

        // Empty path after trimming
        if path.is_empty() {
            return Err(PathError::InvalidPath("Empty path".to_string()));
        }

        // Split by '/' and validate components
        let components: Vec<&str> = path.split('/').collect();

        // Validate each component
        for component in &components {
            if component.is_empty() {
                return Err(PathError::InvalidPath(
                    "Path contains empty component".to_string(),
                ));
            }
            if *component == "." || *component == ".." {
                return Err(PathError::InvalidPath(
                    "Relative path components (. or ..) are not supported".to_string(),
                ));
            }
        }

        Ok(components)
    }

    /// Validates a single path component name
    ///
    /// Returns true if the name is valid for a directory entry.
    pub fn is_valid_name(name: &str) -> bool {
        !name.is_empty()
            && name != "."
            && name != ".."
            && !name.contains('/')
            && !name.contains('\0')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_simple_path() {
        let result = PathResolver::split_path("todo.txt").unwrap();
        assert_eq!(result, vec!["todo.txt"]);
    }

    #[test]
    fn test_split_nested_path() {
        let result = PathResolver::split_path("docs/notes/todo.txt").unwrap();
        assert_eq!(result, vec!["docs", "notes", "todo.txt"]);
    }

    #[test]
    fn test_split_path_with_leading_slash() {
        let result = PathResolver::split_path("/docs/notes.txt").unwrap();
        assert_eq!(result, vec!["docs", "notes.txt"]);
    }

    #[test]
    fn test_split_path_with_trailing_slash() {
        let result = PathResolver::split_path("docs/").unwrap();
        assert_eq!(result, vec!["docs"]);
    }

    #[test]
    fn test_empty_path() {
        let result = PathResolver::split_path("");
        assert!(result.is_err());
        assert!(matches!(result, Err(PathError::InvalidPath(_))));
    }

    #[test]
    fn test_only_slashes() {
        let result = PathResolver::split_path("///");
        assert!(result.is_err());
        assert!(matches!(result, Err(PathError::InvalidPath(_))));
    }

    #[test]
    fn test_double_slash() {
        let result = PathResolver::split_path("docs//notes.txt");
        assert!(result.is_err());
        assert!(matches!(result, Err(PathError::InvalidPath(_))));
    }

    #[test]
    fn test_dot_component() {
        let result = PathResolver::split_path("docs/./notes.txt");
        assert!(result.is_err());
        assert!(matches!(result, Err(PathError::InvalidPath(_))));
    }

    #[test]
    fn test_dotdot_component() {
        let result = PathResolver::split_path("docs/../notes.txt");
        assert!(result.is_err());
        assert!(matches!(result, Err(PathError::InvalidPath(_))));
    }

    #[test]
    fn test_is_valid_name() {
        assert!(PathResolver::is_valid_name("todo.txt"));
        assert!(PathResolver::is_valid_name("my-file"));
        assert!(PathResolver::is_valid_name("file_123"));

        assert!(!PathResolver::is_valid_name(""));
        assert!(!PathResolver::is_valid_name("."));
        assert!(!PathResolver::is_valid_name(".."));
        assert!(!PathResolver::is_valid_name("has/slash"));
        assert!(!PathResolver::is_valid_name("has\0null"));
    }
}
