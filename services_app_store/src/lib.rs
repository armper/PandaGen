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
    /// The source this listing is for. The storefront used to plan an
    /// install with an *empty* digest and let the registry hand back
    /// whatever it had pinned, so nothing anywhere stated what was being
    /// installed -- the lookup was by name and version alone, which is a
    /// lookup and not a verification.
    pub source_digest: String,
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

/// A ceiling on what a listing may ask for.
///
/// Two defects lived here, and they are A1's two defects exactly, in a crate
/// A1's fix never touched.
///
/// It capped `cpu_ticks` and nothing else, while `ResourceBudget` has six
/// fields -- so a listing asking for `u64::MAX` memory units, messages,
/// packets, storage operations or pipeline stages was approved by a policy
/// whose whole purpose is to cap what a listing may ask for.
///
/// And it skipped any field the listing left `None`, which in
/// `ResourceBudget` means *unlimited* -- so declaring no CPU budget at all
/// walked straight past a CPU cap. A field-by-field validator forgets the
/// `None`-means-unlimited fields first, which is the note A1 left behind.
///
/// A cap left `None` here means "this store does not cap that resource",
/// and a listing that asks for unlimited is refused by any cap that is set.
#[derive(Debug, Clone, Default)]
pub struct BudgetCapPolicy {
    pub max_cpu_ticks: Option<u64>,
    pub max_memory_units: Option<u64>,
    pub max_message_count: Option<u64>,
    pub max_packet_count: Option<u64>,
    pub max_storage_ops: Option<u64>,
    pub max_pipeline_stages: Option<u64>,
}

impl BudgetCapPolicy {
    /// A policy that caps every resource at `limit`.
    pub fn capped_at(limit: u64) -> Self {
        Self {
            max_cpu_ticks: Some(limit),
            max_memory_units: Some(limit),
            max_message_count: Some(limit),
            max_packet_count: Some(limit),
            max_storage_ops: Some(limit),
            max_pipeline_stages: Some(limit),
        }
    }

    /// One resource against one cap.
    ///
    /// `requested` of `None` is a listing asking for *unlimited*, which no
    /// cap can accommodate.
    fn check(name: &str, cap: Option<u64>, requested: Option<u64>) -> Result<(), StoreError> {
        let Some(cap) = cap else {
            return Ok(());
        };
        match requested {
            None => Err(StoreError::PolicyDenied(format!(
                "{name} budget is unlimited, which exceeds the cap of {cap}"
            ))),
            Some(requested) if requested > cap => Err(StoreError::PolicyDenied(format!(
                "{name} budget {requested} exceeds cap {cap}"
            ))),
            Some(_) => Ok(()),
        }
    }
}

impl StorePolicy for BudgetCapPolicy {
    fn allow(&self, listing: &AppListing) -> Result<(), StoreError> {
        let budget = &listing.default_budget;
        Self::check("cpu", self.max_cpu_ticks, budget.cpu_ticks.map(|v| v.0))?;
        Self::check(
            "memory",
            self.max_memory_units,
            budget.memory_units.map(|v| v.0),
        )?;
        Self::check(
            "message",
            self.max_message_count,
            budget.message_count.map(|v| v.0),
        )?;
        Self::check(
            "packet",
            self.max_packet_count,
            budget.packet_count.map(|v| v.0),
        )?;
        Self::check(
            "storage",
            self.max_storage_ops,
            budget.storage_ops.map(|v| v.0),
        )?;
        Self::check(
            "pipeline stage",
            self.max_pipeline_stages,
            budget.pipeline_stages.map(|v| v.0),
        )?;
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
            source_digest: listing.source_digest.clone(),
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
        index
            .add(PackageEntry {
                name: "demo".to_string(),
                version: "0.1.0".to_string(),
                source_digest: "abc".to_string(),
            })
            .unwrap();

        let mut store = AppStorefront::new(index, Box::new(AllowAllPolicy));
        store.add_listing(AppListing {
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            description: "Demo app".to_string(),
            source_digest: "abc".to_string(),
            default_budget: ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(10)),
        });

        let plan = store.plan_install("demo", "0.1.0").unwrap();
        assert_eq!(plan.package.name, "demo");
    }

    #[test]
    fn test_storefront_policy_denied() {
        let mut index = RegistryIndex::default();
        index
            .add(PackageEntry {
                name: "heavy".to_string(),
                version: "0.1.0".to_string(),
                source_digest: "abc".to_string(),
            })
            .unwrap();

        let mut store = AppStorefront::new(
            index,
            Box::new(BudgetCapPolicy {
                max_cpu_ticks: Some(5),
                ..Default::default()
            }),
        );
        store.add_listing(AppListing {
            name: "heavy".to_string(),
            version: "0.1.0".to_string(),
            description: "Heavy app".to_string(),
            source_digest: "abc".to_string(),
            default_budget: ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(10)),
        });

        let result = store.plan_install("heavy", "0.1.0");
        assert!(matches!(result, Err(StoreError::PolicyDenied(_))));
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;
    use resources::CpuTicks;

    #[test]
    fn a_listing_whose_source_is_not_the_pinned_one_cannot_be_installed() {
        // The storefront planned an install with an empty source digest and
        // let the registry hand back whatever it had pinned, so nothing
        // anywhere stated what was being installed: the lookup was by name
        // and version, which is a lookup and not a verification.
        let mut index = RegistryIndex::default();
        index
            .add(PackageEntry {
                name: "demo".to_string(),
                version: "0.1.0".to_string(),
                source_digest: "GENUINE".to_string(),
            })
            .unwrap();

        let mut store = AppStorefront::new(index, Box::new(AllowAllPolicy));
        store.add_listing(AppListing {
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            description: "Demo app".to_string(),
            source_digest: "ATTACKER".to_string(),
            default_budget: ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(10)),
        });

        assert!(
            store.plan_install("demo", "0.1.0").is_err(),
            "a listing for source the registry does not pin was installable"
        );
    }
}
