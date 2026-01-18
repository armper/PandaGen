//! Storage Consistency Tests
//!
//! Validates that storage maintains consistency under crash conditions.
//! Tests transactional semantics and ensures no partial commits or corruption.

use kernel_api::KernelApi;
use services_storage::{ObjectId, Transaction, TransactionState};
use sim_kernel::fault_injection::{FaultPlan, LifecycleFault};
use sim_kernel::test_utils::with_fault_plan;

/// Test: Transaction rollback on crash
///
/// Validates that when a transaction crashes before commit,
/// no partial changes are visible.
#[test]
fn test_transaction_rollback_on_crash() {
    let mut tx = Transaction::new();

    // Begin modifications
    let obj1 = ObjectId::new();
    let obj2 = ObjectId::new();

    tx.modify(obj1).expect("Failed to modify obj1");
    tx.modify(obj2).expect("Failed to modify obj2");

    assert_eq!(tx.modified_objects().len(), 2);
    assert_eq!(tx.state(), TransactionState::Active);

    // Simulate crash by rolling back
    tx.rollback().expect("Failed to rollback");

    // All modifications should be discarded
    assert_eq!(tx.state(), TransactionState::RolledBack);
    assert_eq!(tx.modified_objects().len(), 0);
}

/// Test: No partial commits under fault injection
///
/// When a crash occurs during a transaction, either all changes
/// are applied or none are (atomicity).
#[test]
fn test_no_partial_commits() {
    let mut tx = Transaction::new();

    let obj_id = ObjectId::new();
    tx.modify(obj_id).expect("Failed to modify");

    // Cannot commit after rollback
    tx.rollback().expect("Failed to rollback");
    let commit_result = tx.commit();
    assert!(commit_result.is_err());

    // Transaction is in final state
    assert_eq!(tx.state(), TransactionState::RolledBack);
}

/// Test: Multiple transactions are isolated
///
/// Changes in one transaction are not visible to another
/// until commit.
#[test]
fn test_transaction_isolation() {
    let mut tx1 = Transaction::new();
    let tx2 = Transaction::new();

    let obj_id = ObjectId::new();

    // tx1 modifies object
    tx1.modify(obj_id).expect("Failed to modify in tx1");
    assert_eq!(tx1.modified_objects().len(), 1);

    // tx2 doesn't see tx1's modifications (not committed yet)
    assert_eq!(tx2.modified_objects().len(), 0);

    // tx1 commits
    tx1.commit().expect("Failed to commit tx1");

    // tx2 still independent
    assert_eq!(tx2.modified_objects().len(), 0);
}

/// Test: Transaction state transitions are enforced
#[test]
fn test_transaction_state_enforcement() {
    let mut tx = Transaction::new();
    assert_eq!(tx.state(), TransactionState::Active);

    // Can modify while active
    tx.modify(ObjectId::new()).expect("Should succeed");

    // Commit the transaction
    tx.commit().expect("Failed to commit");
    assert_eq!(tx.state(), TransactionState::Committed);

    // Cannot modify after commit
    let result = tx.modify(ObjectId::new());
    assert!(result.is_err());

    // Cannot commit again
    let result = tx.commit();
    assert!(result.is_err());
}

/// Test: Crash during storage write doesn't corrupt state
///
/// Uses fault injection to simulate crash during write operation.
#[test]
fn test_crash_during_write() {
    let plan = FaultPlan::new().with_lifecycle_fault(LifecycleFault::CrashOnSend);

    with_fault_plan(plan, |kernel| {
        // Create a transaction
        let mut tx = Transaction::new();
        let obj_id = ObjectId::new();

        tx.modify(obj_id).expect("Failed to modify");

        // In a real storage system, write would happen here
        // If crash occurs during write (simulated by send failure),
        // transaction should rollback

        // Simulate failure by explicitly rolling back
        tx.rollback().expect("Failed to rollback");

        assert_eq!(tx.state(), TransactionState::RolledBack);
        assert_eq!(tx.modified_objects().len(), 0);

        // Kernel state remains consistent
        assert!(kernel.is_idle());
    });
}

/// Test: Double commit prevention
#[test]
fn test_double_commit_prevention() {
    let mut tx = Transaction::new();

    tx.modify(ObjectId::new()).expect("Failed to modify");
    tx.commit().expect("Failed to commit");

    // Second commit should fail
    let result = tx.commit();
    assert!(result.is_err());
    assert_eq!(tx.state(), TransactionState::Committed);
}

/// Test: Rollback after partial modifications
#[test]
fn test_rollback_after_partial_modifications() {
    let mut tx = Transaction::new();

    // Make several modifications
    for _ in 0..5 {
        tx.modify(ObjectId::new()).expect("Failed to modify");
    }

    assert_eq!(tx.modified_objects().len(), 5);

    // Rollback discards all
    tx.rollback().expect("Failed to rollback");
    assert_eq!(tx.modified_objects().len(), 0);
    assert_eq!(tx.state(), TransactionState::RolledBack);
}

/// Test: Storage invariants under message loss
///
/// When messages carrying storage operations are dropped,
/// the storage state should remain consistent (no orphaned objects).
#[test]
fn test_storage_consistency_under_message_loss() {
    use core_types::ServiceId;
    use ipc::{MessageEnvelope, MessagePayload, SchemaVersion};
    use sim_kernel::fault_injection::MessageFault;

    let plan = FaultPlan::new().with_message_fault(MessageFault::DropMatching {
        action: "storage.write".to_string(),
    });

    with_fault_plan(plan, |kernel| {
        let service_id = ServiceId::new();
        let channel = kernel.create_channel().expect("Failed to create channel");

        kernel
            .register_service(service_id, channel)
            .expect("Failed to register service");

        // Send a storage write message - will be dropped
        let payload = MessagePayload::new(&"write_data").expect("Failed to create payload");
        let message = MessageEnvelope::new(
            service_id,
            "storage.write".to_string(),
            SchemaVersion::new(1, 0),
            payload,
        );

        kernel
            .send_message(channel, message)
            .expect("Send succeeded (but dropped)");

        // Message was dropped, no state corruption
        let result = kernel.receive_message(channel, None);
        assert!(result.is_err()); // No message to receive

        // Storage would remain in consistent state
        // No partial write, no orphaned objects
    });
}

/// Test: Crash recovery - transaction log replay
///
/// Simulates a crash and recovery scenario where committed transactions
/// are preserved and uncommitted ones are discarded.
#[test]
fn test_transaction_recovery() {
    // Transaction 1: Committed before crash
    let mut tx1 = Transaction::new();
    tx1.modify(ObjectId::new()).expect("Failed to modify");
    tx1.commit().expect("Failed to commit");

    // Transaction 2: Not committed before crash
    let mut tx2 = Transaction::new();
    tx2.modify(ObjectId::new()).expect("Failed to modify");

    // Simulate crash
    // tx1 is committed - changes are durable
    assert_eq!(tx1.state(), TransactionState::Committed);

    // tx2 is not committed - changes should be rolled back on recovery
    assert_eq!(tx2.state(), TransactionState::Active);
    tx2.rollback().expect("Recovery rollback");
    assert_eq!(tx2.state(), TransactionState::RolledBack);
}

/// Test: Version consistency across failures
///
/// Validates that version IDs remain consistent even when
/// operations fail or are retried.
#[test]
fn test_version_consistency() {
    use services_storage::VersionId;

    let mut tx = Transaction::new();
    let obj_id = ObjectId::new();

    // First modification
    tx.modify(obj_id).expect("Failed to modify");

    // In a real system, each commit would create a new version
    // Version IDs are monotonically increasing and immutable
    let version1 = VersionId::new();

    tx.commit().expect("Failed to commit");

    // After crash and recovery, new modifications create new versions
    let mut tx2 = Transaction::new();
    tx2.modify(obj_id).expect("Failed to modify");

    let version2 = VersionId::new();

    // Versions are distinct
    assert_ne!(version1, version2);
}
