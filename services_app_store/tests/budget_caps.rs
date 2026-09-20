//! A cap on what a listing may ask for has to cap what it asks for.

use resources::{
    CpuTicks, MemoryUnits, MessageCount, PacketCount, PipelineStages, ResourceBudget, StorageOps,
};
use services_app_store::{AppListing, BudgetCapPolicy, StorePolicy};

fn listing(budget: ResourceBudget) -> AppListing {
    AppListing {
        name: "app".to_string(),
        version: "1.0.0".to_string(),
        description: "an app".to_string(),
        source_digest: "sha256:0".to_string(),
        default_budget: budget,
    }
}

/// A1's first defect, in a crate A1's fix never touched: the policy skipped
/// any field the listing left `None`, and in `ResourceBudget` `None` means
/// *unlimited*. Declaring no CPU budget at all walked past a CPU cap.
#[test]
fn a_listing_asking_for_unlimited_does_not_slip_past_a_cap() {
    let policy = BudgetCapPolicy {
        max_cpu_ticks: Some(5),
        ..Default::default()
    };
    assert!(
        policy.allow(&listing(ResourceBudget::unlimited())).is_err(),
        "a listing declaring an unlimited cpu budget passed a policy whose \
         whole job is to cap cpu at 5"
    );
    assert!(
        policy
            .allow(&listing(
                ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(3))
            ))
            .is_ok(),
        "a modest listing must still be approved"
    );
}

/// A1's second defect: the policy capped one of the six fields, so every
/// other resource was unbounded.
///
/// Each case starts from a budget that is modest in *every* field and then
/// makes one field greedy, so a refusal can only come from the field the
/// case names. The first version of this test used
/// `ResourceBudget::unlimited()` and passed against the single-field policy,
/// because leaving cpu unlimited tripped the other rule.
#[test]
fn every_resource_is_capped_not_just_the_first_one() {
    fn modest() -> ResourceBudget {
        ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1))
            .with_memory_units(MemoryUnits::new(1))
            .with_message_count(MessageCount::new(1))
            .with_packet_count(PacketCount::new(1))
            .with_storage_ops(StorageOps::new(1))
            .with_pipeline_stages(PipelineStages::new(1))
    }

    let policy = BudgetCapPolicy::capped_at(5);
    assert!(
        policy.allow(&listing(modest())).is_ok(),
        "a modest listing must be approved, or this test proves nothing"
    );

    let greedy = [
        (
            "memory",
            modest().with_memory_units(MemoryUnits::new(u64::MAX)),
        ),
        (
            "message",
            modest().with_message_count(MessageCount::new(u64::MAX)),
        ),
        (
            "packet",
            modest().with_packet_count(PacketCount::new(u64::MAX)),
        ),
        (
            "storage",
            modest().with_storage_ops(StorageOps::new(u64::MAX)),
        ),
        (
            "pipeline stages",
            modest().with_pipeline_stages(PipelineStages::new(u64::MAX)),
        ),
    ];
    for (name, budget) in greedy {
        assert!(
            policy.allow(&listing(budget)).is_err(),
            "a listing asking for u64::MAX {name} was approved by a policy \
             capping every resource at 5"
        );
    }
}

/// A cap left unset means "this store does not cap that resource", which
/// must still be true.
#[test]
fn an_unset_cap_constrains_nothing() {
    let policy = BudgetCapPolicy::default();
    assert!(policy.allow(&listing(ResourceBudget::unlimited())).is_ok());
    assert!(policy
        .allow(&listing(
            ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(u64::MAX))
        ))
        .is_ok());
}
