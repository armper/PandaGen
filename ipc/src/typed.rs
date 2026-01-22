//! Typed IPC messages for commands and responses.
//!
//! This module provides a stable, versioned schema for command dispatch
//! and structured responses with explicit error details.

use crate::{MessageEnvelope, MessageId, MessagePayload, SchemaVersion};
use alloc::string::{String, ToString};
use core_types::ServiceId;
use serde::{Deserialize, Serialize};

/// Command message schema version (v1.0).
pub const COMMAND_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// Envelope action for command requests.
pub const COMMAND_REQUEST_ACTION: &str = "console.command.request";

/// Envelope action for command responses.
pub const COMMAND_RESPONSE_ACTION: &str = "console.command.response";

/// Typed command request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandRequest {
    /// Schema version for this payload.
    pub version: SchemaVersion,
    /// Command text (structured parsing happens in services).
    pub command: String,
}

impl CommandRequest {
    /// Creates a new command request using the current schema version.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            version: COMMAND_SCHEMA_VERSION,
            command: command.into(),
        }
    }

    /// Wraps this request in a message envelope.
    pub fn into_envelope(
        self,
        destination: ServiceId,
    ) -> Result<MessageEnvelope, serde_json::Error> {
        let payload = MessagePayload::new(&self)?;
        Ok(MessageEnvelope::new(
            destination,
            COMMAND_REQUEST_ACTION.to_string(),
            self.version,
            payload,
        ))
    }
}

/// Command response status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CommandStatus {
    Ok,
    Error(CommandError),
}

/// Structured command response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandResponse {
    /// Schema version for this payload.
    pub version: SchemaVersion,
    /// Status of the command.
    pub status: CommandStatus,
    /// Optional output payload (human-readable).
    pub output: Option<String>,
}

impl CommandResponse {
    /// Creates a successful response with optional output.
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            version: COMMAND_SCHEMA_VERSION,
            status: CommandStatus::Ok,
            output: Some(output.into()),
        }
    }

    /// Creates a response without output.
    pub fn ok_empty() -> Self {
        Self {
            version: COMMAND_SCHEMA_VERSION,
            status: CommandStatus::Ok,
            output: None,
        }
    }

    /// Creates an error response.
    pub fn error(error: CommandError) -> Self {
        Self {
            version: COMMAND_SCHEMA_VERSION,
            status: CommandStatus::Error(error),
            output: None,
        }
    }

    /// Wraps this response in a message envelope correlated to a request.
    pub fn into_envelope(
        self,
        destination: ServiceId,
        correlation: MessageId,
    ) -> Result<MessageEnvelope, serde_json::Error> {
        let payload = MessagePayload::new(&self)?;
        Ok(MessageEnvelope::new(
            destination,
            COMMAND_RESPONSE_ACTION.to_string(),
            self.version,
            payload,
        )
        .with_correlation(correlation))
    }
}

/// Command error codes for structured failures.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CommandErrorCode {
    InvalidCommand,
    InvalidArguments,
    ServiceUnavailable,
    Unauthorized,
    Internal,
}

/// Structured command error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandError {
    pub code: CommandErrorCode,
    pub message: String,
    pub details: Option<String>,
}

impl CommandError {
    pub fn new(code: CommandErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_request_envelope_roundtrip() {
        let service = ServiceId::new();
        let request = CommandRequest::new("help");
        let envelope = request.into_envelope(service).unwrap();

        assert_eq!(envelope.destination, service);
        assert_eq!(envelope.action, COMMAND_REQUEST_ACTION);
        assert_eq!(envelope.schema_version, COMMAND_SCHEMA_VERSION);

        let decoded: CommandRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(decoded.command, "help");
    }

    #[test]
    fn test_command_response_envelope_roundtrip() {
        let service = ServiceId::new();
        let correlation = MessageId::new();
        let response = CommandResponse::ok("ok");
        let envelope = response.into_envelope(service, correlation).unwrap();

        assert_eq!(envelope.destination, service);
        assert_eq!(envelope.action, COMMAND_RESPONSE_ACTION);
        assert_eq!(envelope.correlation_id, Some(correlation));

        let decoded: CommandResponse = envelope.payload.deserialize().unwrap();
        assert!(matches!(decoded.status, CommandStatus::Ok));
        assert_eq!(decoded.output, Some("ok".to_string()));
    }

    #[test]
    fn test_command_error_structure() {
        let error = CommandError::new(CommandErrorCode::InvalidCommand, "bad")
            .with_details("unknown command");
        assert_eq!(error.code, CommandErrorCode::InvalidCommand);
        assert_eq!(error.message, "bad");
        assert_eq!(error.details, Some("unknown command".to_string()));
    }
}
