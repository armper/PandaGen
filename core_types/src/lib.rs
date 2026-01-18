//! # Core Types
//!
//! This crate defines the fundamental types used throughout PandaGen.
//!
//! ## Philosophy
//!
//! Core types are designed with these principles:
//! - **Explicit over implicit**: Capabilities are typed and cannot be confused.
//! - **Type safety first**: The type system prevents misuse at compile time.
//! - **No ambient authority**: All access requires explicit capability.
//!
//! ## Key Types
//!
//! - [`Cap<T>`]: A strongly-typed capability handle
//! - [`ServiceId`]: Unique identifier for services
//! - [`TaskId`]: Unique identifier for tasks

pub mod capability;
pub mod ids;

pub use capability::{Cap, CapabilityError, CapabilityGrant, CapabilityTransfer};
pub use ids::{ServiceId, TaskId};
