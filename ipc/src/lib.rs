//! # Inter-Process Communication (IPC)
//!
//! This crate defines PandaGen's message-passing primitives.
//!
//! ## Philosophy
//!
//! - **Messages, not shared memory**: All communication is explicit message passing
//! - **Typed, not stringly-typed**: Messages have schema versions and types
//! - **Traceable**: Every message has a correlation ID for debugging
//! - **Versionable**: Schema evolution is built-in from day one
//!
//! ## Architecture
//!
//! Messages are the fundamental unit of communication. They contain:
//! - Routing information (destination)
//! - Action/method to invoke
//! - Schema version for backward compatibility
//! - Correlation ID for request/response matching
//! - Typed payload
//!
//! Unlike traditional IPC (pipes, sockets, signals), messages are structured
//! and self-describing.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod channel;
pub mod message;
pub mod typed;

pub use channel::{ChannelEnd, ChannelId};
pub use message::{
    Compatibility, Message, MessageEnvelope, MessageId, MessagePayload, SchemaMismatchError,
    SchemaVersion, VersionPolicy,
};
pub use typed::{
    CommandError, CommandErrorCode, CommandRequest, CommandResponse, CommandStatus,
    COMMAND_REQUEST_ACTION, COMMAND_RESPONSE_ACTION, COMMAND_SCHEMA_VERSION,
};
