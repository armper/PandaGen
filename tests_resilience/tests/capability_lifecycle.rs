//! Capability Lifecycle Integration Tests
//!
//! These tests validate the complete capability lifecycle model:
//! - Grant, delegate, drop semantics
//! - Invalidation on task death
//! - Move-only semantics (no implicit cloning)
//! - Audit trail verification

use core_types::{Cap, CapabilityEvent, ServiceId, TaskId};
use ipc::{MessageEnvelope, MessagePayload, SchemaVersion};
use kernel_api::{KernelApi, TaskDescriptor};
use tests_resilience::{spawn_test_service, test_bootstrap};

#[test]
fn test_capability_lifecycle_grant_and_use() {
    let (mut kernel, _registry) = test_bootstrap();

    // Create a task
    let task = spawn_test_service(&mut kernel, "test_service").unwrap();

    // Grant a capability
    let cap: Cap<()> = Cap::new(100);
    kernel.grant_capability(task, cap).unwrap();

    // Verify audit trail
    let audit = kernel.audit_log();
    assert_eq!(audit.len(), 1);
    assert!(audit.has_event(|e| matches!(e, CapabilityEvent::Granted { 
        cap_id: 100, 
        grantee, 
        .. 
    } if *grantee == task)));
}

#[test]
fn test_capability_invalidation_on_task_termination() {
    let (mut kernel, _registry) = test_bootstrap();

    // Create a task
    let task = spawn_test_service(&mut kernel, "test_service").unwrap();

    // Grant capabilities
    let cap1: Cap<()> = Cap::new(100);
    let cap2: Cap<()> = Cap::new(101);
    kernel.grant_capability(task, cap1).unwrap();
    kernel.grant_capability(task, cap2).unwrap();

    // Verify capabilities are valid
    assert!(kernel.is_capability_valid(100, task));
    assert!(kernel.is_capability_valid(101, task));

    // Terminate the task
    kernel.terminate_task(task);

    // Capabilities should be invalidated
    assert!(!kernel.is_capability_valid(100, task));
    assert!(!kernel.is_capability_valid(101, task));

    // Verify audit trail
    let audit = kernel.audit_log();
    let invalidated_count =
        audit.count_events(|e| matches!(e, CapabilityEvent::Invalidated { .. }));
    assert_eq!(invalidated_count, 2);
}

#[test]
fn test_capability_move_semantics_delegation() {
    let (mut kernel, _registry) = test_bootstrap();

    // Create two tasks
    let task1 = spawn_test_service(&mut kernel, "task1").unwrap();
    let task2 = spawn_test_service(&mut kernel, "task2").unwrap();

    // Grant capability to task1
    let cap: Cap<()> = Cap::new(100);
    kernel.grant_capability(task1, cap).unwrap();

    // Task1 owns the capability
    assert!(kernel.is_capability_valid(100, task1));
    assert!(!kernel.is_capability_valid(100, task2));

    // Delegate from task1 to task2 (move semantics)
    kernel.delegate_capability(100, task1, task2).unwrap();

    // Now task2 owns it and task1 doesn't
    assert!(!kernel.is_capability_valid(100, task1));
    assert!(kernel.is_capability_valid(100, task2));

    // Verify audit trail
    let audit = kernel.audit_log();
    assert!(audit.has_event(|e| matches!(
        e,
        CapabilityEvent::Delegated {
            cap_id: 100,
            from_task,
            to_task,
            ..
        } if *from_task == task1 && *to_task == task2
    )));
}

#[test]
fn test_capability_cannot_be_used_after_delegation() {
    let (mut kernel, _registry) = test_bootstrap();

    // Create two tasks
    let task1 = spawn_test_service(&mut kernel, "task1").unwrap();
    let task2 = spawn_test_service(&mut kernel, "task2").unwrap();

    // Grant capability to task1
    let cap: Cap<()> = Cap::new(100);
    kernel.grant_capability(task1, cap).unwrap();

    // Delegate to task2
    kernel.delegate_capability(100, task1, task2).unwrap();

    // Task1 cannot delegate again (no longer owns it)
    let result = kernel.delegate_capability(100, task1, task2);
    assert!(result.is_err());

    // Task1 cannot drop it either
    let result = kernel.drop_capability(100, task1);
    assert!(result.is_err());
}

#[test]
fn test_delegation_chain_a_to_b_to_c() {
    let (mut kernel, _registry) = test_bootstrap();

    // Create three tasks
    let task_a = spawn_test_service(&mut kernel, "task_a").unwrap();
    let task_b = spawn_test_service(&mut kernel, "task_b").unwrap();
    let task_c = spawn_test_service(&mut kernel, "task_c").unwrap();

    // Grant to A
    let cap: Cap<()> = Cap::new(100);
    kernel.grant_capability(task_a, cap).unwrap();

    // A delegates to B
    kernel.delegate_capability(100, task_a, task_b).unwrap();

    // B delegates to C
    kernel.delegate_capability(100, task_b, task_c).unwrap();

    // Only C owns it now
    assert!(!kernel.is_capability_valid(100, task_a));
    assert!(!kernel.is_capability_valid(100, task_b));
    assert!(kernel.is_capability_valid(100, task_c));

    // Verify audit trail shows full chain
    let audit = kernel.audit_log();
    let events = audit.get_events_for_cap(100);
    assert_eq!(events.len(), 3); // Grant + 2 Delegates
}

#[test]
fn test_capability_drop_explicit() {
    let (mut kernel, _registry) = test_bootstrap();

    let task = spawn_test_service(&mut kernel, "test_service").unwrap();

    // Grant capability
    let cap: Cap<()> = Cap::new(100);
    kernel.grant_capability(task, cap).unwrap();

    // Drop capability explicitly
    kernel.drop_capability(100, task).unwrap();

    // Capability is no longer valid
    assert!(!kernel.is_capability_valid(100, task));

    // Verify audit trail
    let audit = kernel.audit_log();
    assert!(audit.has_event(|e| matches!(e, CapabilityEvent::Dropped { cap_id: 100, .. })));
}

#[test]
fn test_cannot_delegate_to_nonexistent_task() {
    let (mut kernel, _registry) = test_bootstrap();

    let task = spawn_test_service(&mut kernel, "test_service").unwrap();

    // Grant capability
    let cap: Cap<()> = Cap::new(100);
    kernel.grant_capability(task, cap).unwrap();

    // Try to delegate to non-existent task
    let fake_task = TaskId::new();
    let result = kernel.delegate_capability(100, task, fake_task);
    assert!(result.is_err());

    // Original task still owns it
    assert!(kernel.is_capability_valid(100, task));
}

#[test]
fn test_cannot_grant_to_nonexistent_task() {
    let mut kernel = sim_kernel::SimulatedKernel::new();

    let fake_task = TaskId::new();
    let cap: Cap<()> = Cap::new(100);

    let result = kernel.grant_capability(fake_task, cap);
    assert!(result.is_err());
}

#[test]
fn test_multiple_capabilities_per_task() {
    let (mut kernel, _registry) = test_bootstrap();

    let task = spawn_test_service(&mut kernel, "test_service").unwrap();

    // Grant multiple capabilities
    for i in 100..110 {
        let cap: Cap<()> = Cap::new(i);
        kernel.grant_capability(task, cap).unwrap();
    }

    // All should be valid
    for i in 100..110 {
        assert!(kernel.is_capability_valid(i, task));
    }

    // Terminate task
    kernel.terminate_task(task);

    // All should be invalid
    for i in 100..110 {
        assert!(!kernel.is_capability_valid(i, task));
    }

    // Audit log should have 10 grants + 10 invalidations
    let audit = kernel.audit_log();
    let grant_count = audit.count_events(|e| matches!(e, CapabilityEvent::Granted { .. }));
    let invalid_count = audit.count_events(|e| matches!(e, CapabilityEvent::Invalidated { .. }));
    assert_eq!(grant_count, 10);
    assert_eq!(invalid_count, 10);
}

#[test]
fn test_capability_audit_trail_chronological() {
    let (mut kernel, _registry) = test_bootstrap();

    let task1 = spawn_test_service(&mut kernel, "task1").unwrap();
    let task2 = spawn_test_service(&mut kernel, "task2").unwrap();

    // Perform operations with time advancing
    let cap: Cap<()> = Cap::new(100);
    kernel.grant_capability(task1, cap).unwrap();

    kernel.advance_time(kernel_api::Duration::from_millis(100));

    kernel.delegate_capability(100, task1, task2).unwrap();

    kernel.advance_time(kernel_api::Duration::from_millis(100));

    kernel.drop_capability(100, task2).unwrap();

    // Verify chronological order
    let audit = kernel.audit_log();
    let events = audit.get_events();
    assert_eq!(events.len(), 3);

    // Timestamps should be increasing
    for i in 1..events.len() {
        assert!(events[i].timestamp >= events[i - 1].timestamp);
    }
}

#[test]
fn test_no_capability_leak_on_message_drop() {
    use sim_kernel::fault_injection::{FaultPlan, MessageFault};

    let plan = FaultPlan::new().with_message_fault(MessageFault::DropNext { count: 1 });

    let mut kernel = sim_kernel::SimulatedKernel::new().with_fault_plan(plan);

    let task = kernel
        .spawn_task(TaskDescriptor::new("task".to_string()))
        .unwrap()
        .task_id;

    // Grant capability
    let cap: Cap<()> = Cap::new(100);
    kernel.grant_capability(task, cap).unwrap();

    // Create a channel and send a message (which will be dropped by fault injection)
    let channel = kernel.create_channel().unwrap();
    let msg = MessageEnvelope::new(
        ServiceId::new(),
        "test".to_string(),
        SchemaVersion::new(1, 0),
        MessagePayload::new(&"data").unwrap(),
    );
    let _ = kernel.send_message(channel, msg);

    // Capability should still be valid (not leaked despite message drop)
    assert!(kernel.is_capability_valid(100, task));

    // Audit should only show the grant
    let audit = kernel.audit_log();
    assert_eq!(audit.len(), 1);
}

#[test]
fn test_crash_restart_caps_not_valid_unless_reissued() {
    let (mut kernel, _registry) = test_bootstrap();

    // Create a service task
    let task1 = spawn_test_service(&mut kernel, "service_v1").unwrap();

    // Grant capability
    let cap: Cap<()> = Cap::new(100);
    kernel.grant_capability(task1, cap).unwrap();
    assert!(kernel.is_capability_valid(100, task1));

    // Simulate crash
    kernel.terminate_task(task1);
    assert!(!kernel.is_capability_valid(100, task1));

    // Restart service (new task)
    let task2 = spawn_test_service(&mut kernel, "service_v2").unwrap();

    // Old capability is not valid for new task
    assert!(!kernel.is_capability_valid(100, task2));

    // Need to explicitly re-grant
    let cap2: Cap<()> = Cap::new(100); // Same ID, but new grant
    kernel.grant_capability(task2, cap2).unwrap();
    assert!(kernel.is_capability_valid(100, task2));
}
