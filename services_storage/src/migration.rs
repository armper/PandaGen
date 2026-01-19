//! Storage object schema migration
//!
//! This module provides the mechanism for migrating storage objects
//! from one schema version to another.

use core_types::{MigrationLineage, ObjectSchemaVersion};
use std::fmt;

/// Error that can occur during migration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    /// The migration path is not supported
    UnsupportedMigration {
        from: ObjectSchemaVersion,
        to: ObjectSchemaVersion,
    },
    /// Migration failed due to invalid data
    InvalidData(String),
    /// Migration requires a version that doesn't exist
    MissingVersion { required: ObjectSchemaVersion },
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrationError::UnsupportedMigration { from, to } => {
                write!(f, "Migration from {} to {} is not supported", from, to)
            }
            MigrationError::InvalidData(msg) => {
                write!(f, "Migration failed: invalid data - {}", msg)
            }
            MigrationError::MissingVersion { required } => {
                write!(
                    f,
                    "Migration requires version {} which is not available",
                    required
                )
            }
        }
    }
}

impl std::error::Error for MigrationError {}

/// Trait for migrating storage object data between schema versions
///
/// Implementations must be:
/// - **Deterministic**: Same input always produces same output
/// - **Pure**: No side effects (no I/O, no global state mutation)
/// - **Testable**: Can be fully tested without kernel or services
pub trait Migrator {
    /// Migrates data from one schema version to another
    ///
    /// Returns the migrated data or an error if migration is not possible.
    ///
    /// # Arguments
    /// * `from_version` - The schema version of the input data
    /// * `to_version` - The target schema version
    /// * `data` - The raw bytes to migrate
    ///
    /// # Returns
    /// The migrated data bytes, or an error if migration failed
    fn migrate(
        &self,
        from_version: ObjectSchemaVersion,
        to_version: ObjectSchemaVersion,
        data: &[u8],
    ) -> Result<Vec<u8>, MigrationError>;

    /// Checks if a migration path is supported
    ///
    /// This allows callers to check before attempting migration.
    fn supports_migration(
        &self,
        from_version: ObjectSchemaVersion,
        to_version: ObjectSchemaVersion,
    ) -> bool;
}

/// A simple migrator that supports sequential version migrations
///
/// This is a reference implementation that migrates through each
/// intermediate version: v1 -> v2 -> v3 -> v4
pub struct SequentialMigrator {
    /// List of migration functions, indexed by "from" version
    /// migrations[0] is v1->v2, migrations[1] is v2->v3, etc.
    #[allow(clippy::type_complexity)]
    migrations: Vec<Box<dyn Fn(&[u8]) -> Result<Vec<u8>, MigrationError>>>,
}

impl SequentialMigrator {
    /// Creates a new sequential migrator
    pub fn new() -> Self {
        Self {
            migrations: Vec::new(),
        }
    }

    /// Adds a migration step from version N to N+1
    ///
    /// Migrations must be added in order: 1->2, then 2->3, then 3->4, etc.
    pub fn add_migration<F>(mut self, migration: F) -> Self
    where
        F: Fn(&[u8]) -> Result<Vec<u8>, MigrationError> + 'static,
    {
        self.migrations.push(Box::new(migration));
        self
    }
}

impl Default for SequentialMigrator {
    fn default() -> Self {
        Self::new()
    }
}

impl Migrator for SequentialMigrator {
    fn migrate(
        &self,
        from_version: ObjectSchemaVersion,
        to_version: ObjectSchemaVersion,
        data: &[u8],
    ) -> Result<Vec<u8>, MigrationError> {
        if from_version == to_version {
            // No migration needed
            return Ok(data.to_vec());
        }

        if from_version > to_version {
            // Downgrade not supported
            return Err(MigrationError::UnsupportedMigration {
                from: from_version,
                to: to_version,
            });
        }

        // Convert version numbers to array indices
        // Note: Versions are 1-based (v1, v2, v3...) but array is 0-indexed
        // So v1->v2 migration is at migrations[0], v2->v3 at migrations[1], etc.
        let from_idx = from_version.as_u32() as usize;
        let to_idx = to_version.as_u32() as usize;

        // Sanity check for potential overflow (defensive programming)
        // In practice, version numbers should never be this large
        const MAX_REASONABLE_VERSION: u32 = 1_000_000;
        if from_version.as_u32() > MAX_REASONABLE_VERSION
            || to_version.as_u32() > MAX_REASONABLE_VERSION
        {
            return Err(MigrationError::UnsupportedMigration {
                from: from_version,
                to: to_version,
            });
        }

        // Check bounds
        // from_idx must be at least 1 (versions start at 1, not 0)
        // to_idx can be at most migrations.len() + 1 (we can migrate up to the next version)
        const MIN_VERSION_IDX: usize = 1; // Versions are 1-indexed
        if from_idx < MIN_VERSION_IDX || to_idx > self.migrations.len() + 1 {
            return Err(MigrationError::UnsupportedMigration {
                from: from_version,
                to: to_version,
            });
        }

        // Apply migrations sequentially
        // Invariant: from_idx >= 1 (checked above), so from_idx - 1 >= 0
        let mut current_data = data.to_vec();
        for i in (from_idx - 1)..(to_idx - 1) {
            if i >= self.migrations.len() {
                return Err(MigrationError::MissingVersion {
                    required: ObjectSchemaVersion::new((i + 2) as u32),
                });
            }
            current_data = self.migrations[i](&current_data)?;
        }

        Ok(current_data)
    }

    fn supports_migration(
        &self,
        from_version: ObjectSchemaVersion,
        to_version: ObjectSchemaVersion,
    ) -> bool {
        if from_version == to_version {
            return true;
        }

        if from_version > to_version {
            return false; // No downgrade
        }

        let from_idx = from_version.as_u32() as usize;
        let to_idx = to_version.as_u32() as usize;

        // Check if we have all migrations
        from_idx > 0 && to_idx <= self.migrations.len() + 1
    }
}

/// Creates a migration lineage record
pub fn create_lineage(
    from: ObjectSchemaVersion,
    to: ObjectSchemaVersion,
    timestamp: Option<u64>,
) -> MigrationLineage {
    let mut lineage = MigrationLineage::new(from, to);
    if let Some(ts) = timestamp {
        lineage = lineage.with_timestamp(ts);
    }
    lineage
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test migrations that transform JSON strings
    fn migrate_v1_to_v2(data: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let s =
            std::str::from_utf8(data).map_err(|e| MigrationError::InvalidData(e.to_string()))?;
        Ok(format!("{}_v2", s).into_bytes())
    }

    fn migrate_v2_to_v3(data: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let s =
            std::str::from_utf8(data).map_err(|e| MigrationError::InvalidData(e.to_string()))?;
        Ok(format!("{}_v3", s).into_bytes())
    }

    fn migrate_v3_to_v4(data: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let s =
            std::str::from_utf8(data).map_err(|e| MigrationError::InvalidData(e.to_string()))?;
        Ok(format!("{}_v4", s).into_bytes())
    }

    #[test]
    fn test_no_migration_needed() {
        let migrator = SequentialMigrator::new();
        let data = b"test_data";

        let result = migrator
            .migrate(
                ObjectSchemaVersion::new(1),
                ObjectSchemaVersion::new(1),
                data,
            )
            .unwrap();

        assert_eq!(result, data);
    }

    #[test]
    fn test_single_step_migration() {
        let migrator = SequentialMigrator::new().add_migration(migrate_v1_to_v2);

        let data = b"original";
        let result = migrator
            .migrate(
                ObjectSchemaVersion::new(1),
                ObjectSchemaVersion::new(2),
                data,
            )
            .unwrap();

        assert_eq!(result, b"original_v2");
    }

    #[test]
    fn test_multi_step_migration() {
        let migrator = SequentialMigrator::new()
            .add_migration(migrate_v1_to_v2)
            .add_migration(migrate_v2_to_v3)
            .add_migration(migrate_v3_to_v4);

        let data = b"start";
        let result = migrator
            .migrate(
                ObjectSchemaVersion::new(1),
                ObjectSchemaVersion::new(4),
                data,
            )
            .unwrap();

        assert_eq!(result, b"start_v2_v3_v4");
    }

    #[test]
    fn test_skip_intermediate_versions() {
        let migrator = SequentialMigrator::new()
            .add_migration(migrate_v1_to_v2)
            .add_migration(migrate_v2_to_v3);

        let data = b"start";

        // Migrate from v1 to v3 (skipping v2 representation, but applying both functions)
        let result = migrator
            .migrate(
                ObjectSchemaVersion::new(1),
                ObjectSchemaVersion::new(3),
                data,
            )
            .unwrap();

        assert_eq!(result, b"start_v2_v3");
    }

    #[test]
    fn test_unsupported_downgrade() {
        let migrator = SequentialMigrator::new().add_migration(migrate_v1_to_v2);

        let result = migrator.migrate(
            ObjectSchemaVersion::new(2),
            ObjectSchemaVersion::new(1),
            b"data",
        );

        assert!(matches!(
            result,
            Err(MigrationError::UnsupportedMigration { .. })
        ));
    }

    #[test]
    fn test_unsupported_too_new_version() {
        let migrator = SequentialMigrator::new().add_migration(migrate_v1_to_v2);

        let result = migrator.migrate(
            ObjectSchemaVersion::new(1),
            ObjectSchemaVersion::new(5),
            b"data",
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_supports_migration() {
        let migrator = SequentialMigrator::new()
            .add_migration(migrate_v1_to_v2)
            .add_migration(migrate_v2_to_v3);

        // Same version is supported
        assert!(
            migrator.supports_migration(ObjectSchemaVersion::new(1), ObjectSchemaVersion::new(1))
        );

        // Forward migrations are supported
        assert!(
            migrator.supports_migration(ObjectSchemaVersion::new(1), ObjectSchemaVersion::new(2))
        );
        assert!(
            migrator.supports_migration(ObjectSchemaVersion::new(1), ObjectSchemaVersion::new(3))
        );
        assert!(
            migrator.supports_migration(ObjectSchemaVersion::new(2), ObjectSchemaVersion::new(3))
        );

        // Downgrades are not supported
        assert!(
            !migrator.supports_migration(ObjectSchemaVersion::new(2), ObjectSchemaVersion::new(1))
        );

        // Too new versions are not supported
        assert!(
            !migrator.supports_migration(ObjectSchemaVersion::new(1), ObjectSchemaVersion::new(5))
        );
    }

    #[test]
    fn test_migration_error_display() {
        let err1 = MigrationError::UnsupportedMigration {
            from: ObjectSchemaVersion::new(2),
            to: ObjectSchemaVersion::new(1),
        };
        assert!(format!("{}", err1).contains("not supported"));

        let err2 = MigrationError::InvalidData("bad utf-8".to_string());
        assert!(format!("{}", err2).contains("invalid data"));

        let err3 = MigrationError::MissingVersion {
            required: ObjectSchemaVersion::new(5),
        };
        assert!(format!("{}", err3).contains("not available"));
    }

    #[test]
    fn test_create_lineage() {
        let lineage = create_lineage(
            ObjectSchemaVersion::new(1),
            ObjectSchemaVersion::new(3),
            Some(1234567890),
        );

        assert_eq!(lineage.from_version, ObjectSchemaVersion::new(1));
        assert_eq!(lineage.to_version, ObjectSchemaVersion::new(3));
        assert_eq!(lineage.migrated_at, Some(1234567890));
    }

    #[test]
    fn test_migration_preserves_version_immutability() {
        // This test verifies that migration creates a NEW version,
        // not modifying the old one
        let migrator = SequentialMigrator::new().add_migration(migrate_v1_to_v2);

        let original_data = b"original";
        let migrated_data = migrator
            .migrate(
                ObjectSchemaVersion::new(1),
                ObjectSchemaVersion::new(2),
                original_data,
            )
            .unwrap();

        // Original data is unchanged (version immutability)
        assert_eq!(original_data, b"original");
        // Migrated data is different
        assert_eq!(migrated_data, b"original_v2");
        // They are not the same
        assert_ne!(original_data, migrated_data.as_slice());
    }
}
