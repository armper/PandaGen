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

pub mod block_storage;
pub mod failing_device;
pub mod journaled_storage;
pub mod migration;
pub mod object;
pub mod persistent_fs;
pub mod transaction;

pub use block_storage::{BlockStorage, BlockStorageError, StorageRecoveryReport};
pub use failing_device::{FailingBlockDevice, FailurePolicy};
pub use journaled_storage::{
    JournaledStorage, StorageBudget, StorageOperation, StorageService, StorageServiceError,
};
pub use migration::{create_lineage, MigrationError, Migrator, SequentialMigrator};
pub use object::{Object, ObjectId, ObjectKind, VersionId};
pub use persistent_fs::{DirectoryMetadata, PersistentDirectory, PersistentFilesystem};
pub use transaction::{
    Transaction, TransactionError, TransactionId, TransactionState, TransactionalStorage,
};
