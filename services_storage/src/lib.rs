//! # Storage Service
//!
//! This crate defines PandaGen's storage model.
//!
//! ## Philosophy
//!
//! **Storage is rethought from first principles.**
//!
//! Traditional filesystems (hierarchical paths, inodes, etc.) are not goals.
//! Instead, we provide:
//! - Versioned objects (not files)
//! - Transactional operations (not filesystem calls)
//! - Explicit types (Blob, Log, Map)
//! - Schema evolution with migration support
//!
//! ## Design
//!
//! - **ObjectKind**: Different object types for different use cases
//! - **ObjectId**: Unique identifier (not paths)
//! - **VersionId**: Every object version is immutable and addressable
//! - **Transactions**: Atomic operations with rollback
//! - **Schema Evolution**: Objects have schema identity and version
//! - **Migration**: Deterministic, testable data transformations

pub mod migration;
pub mod object;
pub mod journaled_storage;
pub mod transaction;

pub use migration::{create_lineage, MigrationError, Migrator, SequentialMigrator};
pub use journaled_storage::{
    JournaledStorage, StorageBudget, StorageOperation, StorageService, StorageServiceError,
};
pub use object::{Object, ObjectId, ObjectKind, VersionId};
pub use transaction::{
    Transaction, TransactionError, TransactionId, TransactionState, TransactionalStorage,
};
