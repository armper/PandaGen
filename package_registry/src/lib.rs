//! Package registry and reproducible build metadata.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,
    pub source_digest: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryIndex {
    pub packages: Vec<PackageEntry>,
}

impl RegistryIndex {
    pub fn add(&mut self, entry: PackageEntry) {
        self.packages.push(entry);
    }

    pub fn find(&self, name: &str, version: &str) -> Option<&PackageEntry> {
        self.packages
            .iter()
            .find(|entry| entry.name == name && entry.version == version)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildPlan {
    pub name: String,
    pub version: String,
    pub source_digest: String,
    pub toolchain: String,
    pub build_flags: Vec<String>,
}

impl BuildPlan {
    pub fn reproducible_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.name.as_bytes());
        hasher.update(b"@");
        hasher.update(self.version.as_bytes());
        hasher.update(b"|");
        hasher.update(self.source_digest.as_bytes());
        hasher.update(b"|");
        hasher.update(self.toolchain.as_bytes());
        hasher.update(b"|");
        for flag in &self.build_flags {
            hasher.update(flag.as_bytes());
            hasher.update(b";");
        }
        hex::encode(hasher.finalize())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryLock {
    pub packages: Vec<PackageEntry>,
    pub build_hash: String,
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("Package not found: {0}@{1}")]
    NotFound(String, String),
}

pub struct RegistryResolver {
    index: RegistryIndex,
}

impl RegistryResolver {
    pub fn new(index: RegistryIndex) -> Self {
        Self { index }
    }

    pub fn resolve(&self, plan: &BuildPlan) -> Result<RegistryLock, RegistryError> {
        let entry = self
            .index
            .find(&plan.name, &plan.version)
            .ok_or_else(|| RegistryError::NotFound(plan.name.clone(), plan.version.clone()))?;

        let build_hash = plan.reproducible_hash();
        Ok(RegistryLock {
            packages: vec![entry.clone()],
            build_hash,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplyChainReport {
    pub build_hash: String,
    pub sources: BTreeMap<String, String>,
}

impl SupplyChainReport {
    pub fn from_lock(lock: &RegistryLock) -> Self {
        let mut sources = BTreeMap::new();
        for pkg in &lock.packages {
            sources.insert(
                format!("{}@{}", pkg.name, pkg.version),
                pkg.source_digest.clone(),
            );
        }
        Self {
            build_hash: lock.build_hash.clone(),
            sources,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reproducible_hash_deterministic() {
        let plan = BuildPlan {
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            source_digest: "abc".to_string(),
            toolchain: "rust-1.75".to_string(),
            build_flags: vec!["-O".to_string()],
        };
        let h1 = plan.reproducible_hash();
        let h2 = plan.reproducible_hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_reproducible_hash_changes() {
        let mut plan = BuildPlan {
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            source_digest: "abc".to_string(),
            toolchain: "rust-1.75".to_string(),
            build_flags: vec!["-O".to_string()],
        };
        let h1 = plan.reproducible_hash();
        plan.build_flags.push("-C target-cpu=native".to_string());
        let h2 = plan.reproducible_hash();
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_resolve_registry() {
        let mut index = RegistryIndex::default();
        index.add(PackageEntry {
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            source_digest: "abc".to_string(),
        });

        let resolver = RegistryResolver::new(index);
        let plan = BuildPlan {
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            source_digest: "abc".to_string(),
            toolchain: "rust-1.75".to_string(),
            build_flags: vec![],
        };
        let lock = resolver.resolve(&plan).unwrap();
        assert_eq!(lock.packages.len(), 1);
        assert_eq!(lock.packages[0].name, "demo");
    }
}
