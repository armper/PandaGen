//! App storefront host for curated components.

use package_registry::{PackageEntry, RegistryIndex, RegistryResolver};
use resources::ResourceBudget;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppListing {
    pub name: String,
    pub version: String,
    pub description: String,
    pub default_budget: ResourceBudget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPlan {
    pub package: PackageEntry,
    pub budget: ResourceBudget,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("Listing not found: {0}@{1}")]
    ListingNotFound(String, String),

    #[error("Policy denied install: {0}")]
    PolicyDenied(String),

    #[error("Registry error: {0}")]
    Registry(String),
}

pub trait StorePolicy: Send + Sync {
    fn allow(&self, listing: &AppListing) -> Result<(), StoreError>;
}

pub struct AllowAllPolicy;

impl StorePolicy for AllowAllPolicy {
    fn allow(&self, _listing: &AppListing) -> Result<(), StoreError> {
        Ok(())
    }
}

pub struct BudgetCapPolicy {
    pub max_cpu_ticks: Option<u64>,
}

impl StorePolicy for BudgetCapPolicy {
    fn allow(&self, listing: &AppListing) -> Result<(), StoreError> {
        if let (Some(max), Some(cpu)) = (self.max_cpu_ticks, listing.default_budget.cpu_ticks) {
            if cpu.0 > max {
                return Err(StoreError::PolicyDenied(format!(
                    "cpu budget {} exceeds cap {}",
                    cpu.0, max
                )));
            }
        }
        Ok(())
    }
}

pub struct AppStorefront {
    listings: Vec<AppListing>,
    registry: RegistryResolver,
    policy: Box<dyn StorePolicy>,
}

impl AppStorefront {
    pub fn new(index: RegistryIndex, policy: Box<dyn StorePolicy>) -> Self {
        Self {
            listings: Vec::new(),
            registry: RegistryResolver::new(index),
            policy,
        }
    }

    pub fn add_listing(&mut self, listing: AppListing) {
        self.listings.push(listing);
    }

    pub fn list(&self) -> &[AppListing] {
        &self.listings
    }

    pub fn plan_install(&self, name: &str, version: &str) -> Result<InstallPlan, StoreError> {
        let listing = self
            .listings
            .iter()
            .find(|item| item.name == name && item.version == version)
            .ok_or_else(|| StoreError::ListingNotFound(name.to_string(), version.to_string()))?;

        self.policy.allow(listing)?;

        let plan = package_registry::BuildPlan {
            name: listing.name.clone(),
            version: listing.version.clone(),
            source_digest: "".to_string(),
            toolchain: "registry".to_string(),
            build_flags: vec![],
        };

        let lock = self
            .registry
            .resolve(&plan)
            .map_err(|err| StoreError::Registry(err.to_string()))?;

        Ok(InstallPlan {
            package: lock.packages[0].clone(),
            budget: listing.default_budget,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resources::CpuTicks;

    #[test]
    fn test_storefront_plan_install() {
        let mut index = RegistryIndex::default();
        index.add(PackageEntry {
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            source_digest: "abc".to_string(),
        });

        let mut store = AppStorefront::new(index, Box::new(AllowAllPolicy));
        store.add_listing(AppListing {
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            description: "Demo app".to_string(),
            default_budget: ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(10)),
        });

        let plan = store.plan_install("demo", "0.1.0").unwrap();
        assert_eq!(plan.package.name, "demo");
    }

    #[test]
    fn test_storefront_policy_denied() {
        let mut index = RegistryIndex::default();
        index.add(PackageEntry {
            name: "heavy".to_string(),
            version: "0.1.0".to_string(),
            source_digest: "abc".to_string(),
        });

        let mut store = AppStorefront::new(index, Box::new(BudgetCapPolicy { max_cpu_ticks: Some(5) }));
        store.add_listing(AppListing {
            name: "heavy".to_string(),
            version: "0.1.0".to_string(),
            description: "Heavy app".to_string(),
            default_budget: ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(10)),
        });

        let result = store.plan_install("heavy", "0.1.0");
        assert!(matches!(result, Err(StoreError::PolicyDenied(_))));
    }
}
