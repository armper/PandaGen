//! # Logger Service
//!
//! This crate implements structured logging.
//!
//! ## Philosophy
//!
//! Logging is explicit and structured, not text-based or printf-style.

use core_types::TaskId;

/// Log level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// Debug information
    Debug,
    /// Informational messages
    Info,
    /// Warnings
    Warn,
    /// Errors
    Error,
}

/// A structured log entry
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Log level
    pub level: LogLevel,
    /// Source task (if known)
    pub source: Option<TaskId>,
    /// Log message
    pub message: String,
    /// Structured fields
    pub fields: Vec<(String, String)>,
}

impl LogEntry {
    /// Creates a new log entry
    pub fn new(level: LogLevel, message: String) -> Self {
        Self {
            level,
            source: None,
            message,
            fields: Vec::new(),
        }
    }

    /// Sets the source task
    pub fn with_source(mut self, source: TaskId) -> Self {
        self.source = Some(source);
        self
    }

    /// Adds a field to the log entry
    pub fn with_field(mut self, key: String, value: String) -> Self {
        self.fields.push((key, value));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
    }

    #[test]
    fn test_log_entry_creation() {
        let entry = LogEntry::new(LogLevel::Info, "test message".to_string());
        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.message, "test message");
        assert!(entry.source.is_none());
        assert!(entry.fields.is_empty());
    }

    #[test]
    fn test_log_entry_with_source() {
        let task_id = TaskId::new();
        let entry = LogEntry::new(LogLevel::Info, "test".to_string()).with_source(task_id);
        assert_eq!(entry.source, Some(task_id));
    }

    #[test]
    fn test_log_entry_with_fields() {
        let entry = LogEntry::new(LogLevel::Info, "test".to_string())
            .with_field("key1".to_string(), "value1".to_string())
            .with_field("key2".to_string(), "value2".to_string());

        assert_eq!(entry.fields.len(), 2);
        assert_eq!(entry.fields[0].0, "key1");
        assert_eq!(entry.fields[1].1, "value2");
    }
}
