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
//! - [`ObjectSchemaId`]: Identifier for storage object schema types
//! - [`ObjectSchemaVersion`]: Version number for storage object schemas

pub mod capability;
pub mod ids;
pub mod memory;
pub mod service_ids;
pub mod storage_schema;

pub use capability::{
    Cap, CapabilityError, CapabilityEvent, CapabilityGrant, CapabilityInvalidReason,
    CapabilityMetadata, CapabilityStatus, CapabilityTransfer,
};
pub use ids::{ServiceId, TaskId};
pub use service_ids::{
    command_service_id, console_service_id, input_service_id, timer_service_id,
};
pub use memory::{
    AddressSpace, AddressSpaceCap, AddressSpaceId, MemoryAccessType, MemoryBacking, MemoryError,
    MemoryPerms, MemoryRegion, MemoryRegionCap, MemoryRegionId,
};
pub use storage_schema::{MigrationLineage, ObjectSchemaId, ObjectSchemaVersion};
