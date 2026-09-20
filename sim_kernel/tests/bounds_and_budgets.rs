//! Two guards that a large enough number walks straight past, and audit logs
//! with no ceiling.

use kernel_api::{KernelApi, TaskDescriptor};
use resources::{CpuTicks, ResourceBudget};
use sim_kernel::SimulatedKernel;

/// The finding: `current_usage + amount > limit.0`, where `amount` comes
/// straight off the public API. In a debug build the addition panics; in the
/// optimized build the kernel ships it *wraps*, the comparison is false, and
/// the request is granted. The guard is defeated by asking for more.
#[test]
fn a_cpu_budget_cannot_be_walked_past_by_asking_for_more() {
    let mut kernel = SimulatedKernel::new();
    let handle = kernel
        .spawn_task(TaskDescriptor::new("greedy".to_string()))
        .unwrap();
    let execution = kernel.get_task_identity(handle.task_id).unwrap();
    if let Some(identity) = kernel.get_identity_mut(execution) {
        identity.budget = Some(ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(100)));
    }

    kernel
        .try_consume_cpu_ticks(execution, 10)
        .expect("a request inside the budget must be served");

    assert!(
        kernel.try_consume_cpu_ticks(execution, u64::MAX).is_err(),
        "a request for u64::MAX ticks was granted against a 100-tick budget"
    );
}

/// The section size is a `u64` read from the image, so `offset + size`
/// wrapped: a panic in debug, and in release a length check that passes
/// before the slice panics.
#[test]
fn a_hostile_executable_is_refused_rather_than_panicking() {
    let mut kernel = SimulatedKernel::new();
    // magic, version, entry point, section count, then one section:
    // type, size, permissions. 36 bytes, the last eight of them a lie.
    let mut image = Vec::new();
    image.extend_from_slice(&sim_kernel::executable::PEX_MAGIC.to_le_bytes());
    image.extend_from_slice(&1u32.to_le_bytes()); // version
    image.extend_from_slice(&0x1000u64.to_le_bytes()); // entry point, non-zero
    image.extend_from_slice(&1u32.to_le_bytes()); // one section
    image.extend_from_slice(&1u32.to_le_bytes()); // section type: text
    image.extend_from_slice(&u64::MAX.to_le_bytes()); // size
    image.extend_from_slice(&0u32.to_le_bytes()); // permissions
    assert_eq!(image.len(), 36);

    // Not merely "an error": the *right* error. Asserting `is_err()` alone
    // would have passed on a bad magic number, which is how the first two
    // versions of this test passed without ever reaching the arithmetic.
    let refusal = kernel
        .load_executable("hostile".to_string(), &image)
        .expect_err("a section claiming u64::MAX bytes was accepted");
    assert!(
        format!("{refusal:?}").contains("FileTooShort"),
        "refused for the wrong reason: {refusal:?}"
    );

    // And a well-formed image still loads, so the check has not become a
    // refusal of everything.
    let mut good = image[..24].to_vec();
    good.extend_from_slice(&4096u64.to_le_bytes());
    good.extend_from_slice(&0u32.to_le_bytes());
    good.extend_from_slice(&[0u8; 4096]);
    kernel
        .load_executable("good".to_string(), &good)
        .expect("a well-formed image must still load");
}

/// R6-12 bounded the scheduler's audit log. Six siblings in this crate were
/// left unbounded, on paths that run once per tick and once per syscall.
#[test]
fn the_kernels_resource_audit_log_is_bounded() {
    let mut kernel = SimulatedKernel::new();
    let handle = kernel
        .spawn_task(TaskDescriptor::new("busy".to_string()))
        .unwrap();
    let execution = kernel.get_task_identity(handle.task_id).unwrap();
    // A budget, or `try_consume_cpu_ticks` returns before recording anything
    // and the test passes whatever the log does. Generous enough that the
    // identity is not cancelled part-way.
    if let Some(identity) = kernel.get_identity_mut(execution) {
        identity.budget = Some(ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(u64::MAX)));
    }

    for _ in 0..200_000 {
        kernel
            .try_consume_cpu_ticks(execution, 1)
            .expect("the budget is effectively unlimited");
    }
    assert!(
        kernel.resource_audit().len() > 0,
        "nothing was recorded, so this test cannot see the bound"
    );
    let len = kernel.resource_audit().len();
    assert!(len <= 8192, "the resource audit log grew to {len} entries");
}

/// Ordinary budgeted work still succeeds, so the saturation has not turned
/// the guard into a refusal of everything.
#[test]
fn an_ordinary_budget_still_serves_ordinary_requests() {
    let mut kernel = SimulatedKernel::new();
    let handle = kernel
        .spawn_task(TaskDescriptor::new("modest".to_string()))
        .unwrap();
    let execution = kernel.get_task_identity(handle.task_id).unwrap();
    if let Some(identity) = kernel.get_identity_mut(execution) {
        identity.budget = Some(ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(100)));
    }
    for _ in 0..10 {
        kernel
            .try_consume_cpu_ticks(execution, 10)
            .expect("ten lots of ten against a hundred");
    }
}

/// `PolicyDecision::Allow { derived }` is "allow, but only this much
/// authority", and `DerivedAuthority`'s own doc says it "must always be a
/// subset of or equal to the original". Four of the five decision points in
/// the tree discarded it; only `services_pipeline_executor` applied it.
#[cfg(test)]
mod derived_authority {
    use identity::{IdentityKind, TrustDomain};
    use kernel_api::{KernelApi, TaskDescriptor};
    use policy::{
        CapabilitySet, DerivedAuthority, PolicyContext, PolicyDecision, PolicyEngine, PolicyEvent,
    };
    use sim_kernel::SimulatedKernel;

    /// Answers Allow, with an empty derived authority.
    struct AllowNothing;

    impl PolicyEngine for AllowNothing {
        fn name(&self) -> &str {
            "allow-nothing"
        }
        fn evaluate(&self, _event: PolicyEvent, _context: &PolicyContext) -> PolicyDecision {
            PolicyDecision::Allow {
                derived: Some(DerivedAuthority::new(CapabilitySet::from_capabilities(
                    vec![],
                ))),
            }
        }
    }

    #[test]
    fn a_derived_authority_that_excludes_the_capability_refuses_the_delegation() {
        let mut kernel = SimulatedKernel::new().with_policy_engine(Box::new(AllowNothing));

        let (from, _from_exec) = kernel
            .spawn_task_with_identity(
                TaskDescriptor::new("from".to_string()),
                IdentityKind::Service,
                TrustDomain::core(),
                None,
                None,
            )
            .unwrap();
        let (to, _to_exec) = kernel
            .spawn_task_with_identity(
                TaskDescriptor::new("to".to_string()),
                IdentityKind::Service,
                TrustDomain::core(),
                None,
                None,
            )
            .unwrap();

        let cap: core_types::Cap<()> = core_types::Cap::new(77);
        kernel.grant_capability(from.task_id, cap).unwrap();
        let refused = kernel.delegate_capability(77, from.task_id, to.task_id);
        assert!(
            refused.is_err(),
            "the policy answered Allow with an empty derived authority and the \
             capability was delegated anyway"
        );
        assert!(
            !kernel.is_capability_valid(77, to.task_id),
            "the target holds a capability the derived authority excluded"
        );
    }
}
