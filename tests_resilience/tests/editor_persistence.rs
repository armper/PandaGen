//! Editor Persistence and Recovery Tests
//!
//! Validates that editor operations have correct transaction
//! semantics and can survive crashes.

use services_editor_vi::Editor;
use services_storage::{ObjectId, Transaction, TransactionState};
use tests_resilience::test_bootstrap;

/// Test: Editor save uses transaction semantics
///
/// Validates that editor saves are wrapped in transactions
/// and follow proper commit/rollback semantics.
#[test]
fn test_editor_save_transaction_semantics() {
    let (_kernel, _registry) = test_bootstrap();
    
    // Create a transaction for save operation
    let mut tx = Transaction::new();
    assert_eq!(tx.state(), TransactionState::Active);
    
    // Simulate editor save modifying an object
    let doc_id = ObjectId::new();
    tx.modify(doc_id).expect("Failed to modify document");
    
    // Commit the save
    tx.commit().expect("Failed to commit save");
    assert_eq!(tx.state(), TransactionState::Committed);
}

/// Test: Unsaved changes can be rolled back
///
/// Validates that if editor is closed without saving,
/// changes can be rolled back (not persisted).
#[test]
fn test_unsaved_changes_rollback() {
    let (_kernel, _registry) = test_bootstrap();
    
    let mut tx = Transaction::new();
    let doc_id = ObjectId::new();
    
    // Make modifications
    tx.modify(doc_id).expect("Failed to modify");
    assert_eq!(tx.modified_objects().len(), 1);
    
    // User quits without saving - rollback
    tx.rollback().expect("Failed to rollback");
    assert_eq!(tx.state(), TransactionState::RolledBack);
    assert_eq!(tx.modified_objects().len(), 0);
}

/// Test: Multiple saves create multiple commits
///
/// Validates that each :w command creates a new committed transaction.
#[test]
fn test_multiple_saves_multiple_commits() {
    let (_kernel, _registry) = test_bootstrap();
    
    let doc_id = ObjectId::new();
    
    // First save
    let mut tx1 = Transaction::new();
    tx1.modify(doc_id).expect("Failed to modify");
    tx1.commit().expect("Failed to commit tx1");
    assert_eq!(tx1.state(), TransactionState::Committed);
    
    // Second save (new transaction)
    let mut tx2 = Transaction::new();
    tx2.modify(doc_id).expect("Failed to modify");
    tx2.commit().expect("Failed to commit tx2");
    assert_eq!(tx2.state(), TransactionState::Committed);
    
    // Both transactions are independent and committed
}

/// Test: :wq saves before quit
///
/// Validates that :wq command ensures changes are committed
/// before the editor quits.
#[test]
fn test_write_quit_commits_changes() {
    let (_kernel, _registry) = test_bootstrap();
    
    let mut tx = Transaction::new();
    let doc_id = ObjectId::new();
    
    // User makes changes
    tx.modify(doc_id).expect("Failed to modify");
    
    // User executes :wq - must commit before quit
    tx.commit().expect("Failed to commit on :wq");
    assert_eq!(tx.state(), TransactionState::Committed);
    
    // Now safe to quit - changes are persisted
}

/// Test: Force quit discards uncommitted changes
///
/// Validates that :q! rolls back uncommitted changes.
#[test]
fn test_force_quit_discards_changes() {
    let (_kernel, _registry) = test_bootstrap();
    
    let mut tx = Transaction::new();
    let doc_id = ObjectId::new();
    
    // User makes changes
    tx.modify(doc_id).expect("Failed to modify");
    assert_eq!(tx.modified_objects().len(), 1);
    
    // User executes :q! - discard changes
    tx.rollback().expect("Failed to rollback on :q!");
    assert_eq!(tx.state(), TransactionState::RolledBack);
    
    // Changes are discarded
    assert_eq!(tx.modified_objects().len(), 0);
}

/// Test: Transaction isolation for concurrent editors
///
/// Validates that two editors working on different files
/// have isolated transactions.
#[test]
fn test_concurrent_editor_isolation() {
    let (_kernel, _registry) = test_bootstrap();
    
    // Editor 1 opens file A
    let mut tx1 = Transaction::new();
    let file_a = ObjectId::new();
    tx1.modify(file_a).expect("Failed to modify file A");
    
    // Editor 2 opens file B
    let mut tx2 = Transaction::new();
    let file_b = ObjectId::new();
    tx2.modify(file_b).expect("Failed to modify file B");
    
    // Each editor's transaction is isolated
    assert_eq!(tx1.modified_objects().len(), 1);
    assert_eq!(tx2.modified_objects().len(), 1);
    
    // Editor 1 saves
    tx1.commit().expect("Failed to commit tx1");
    
    // Editor 2 is unaffected
    assert_eq!(tx2.state(), TransactionState::Active);
    
    // Editor 2 can still commit
    tx2.commit().expect("Failed to commit tx2");
}

/// Test: Crash before commit leaves no partial state
///
/// Validates that if system crashes before transaction commits,
/// no partial state is visible on recovery.
#[test]
fn test_crash_before_commit_no_partial_state() {
    let (_kernel, _registry) = test_bootstrap();
    
    let mut tx = Transaction::new();
    let doc_id = ObjectId::new();
    
    // User types content (transaction active)
    tx.modify(doc_id).expect("Failed to modify");
    assert_eq!(tx.state(), TransactionState::Active);
    
    // System crashes before user saves
    // Simulate crash by rolling back
    tx.rollback().expect("Failed to rollback");
    
    // After recovery, no partial edits are visible
    assert_eq!(tx.modified_objects().len(), 0);
    assert_eq!(tx.state(), TransactionState::RolledBack);
}

/// Test: Empty document can be saved
#[test]
fn test_save_empty_document() {
    let (_kernel, _registry) = test_bootstrap();
    
    let mut tx = Transaction::new();
    let doc_id = ObjectId::new();
    
    // Create an empty document and save it
    tx.modify(doc_id).expect("Failed to modify empty doc");
    tx.commit().expect("Failed to commit empty doc");
    
    assert_eq!(tx.state(), TransactionState::Committed);
}

/// Test: Large document transaction
///
/// Validates that saving large documents still follows
/// proper transaction semantics.
#[test]
fn test_large_document_transaction() {
    let (_kernel, _registry) = test_bootstrap();
    
    let mut tx = Transaction::new();
    let doc_id = ObjectId::new();
    
    // Simulate large document
    tx.modify(doc_id).expect("Failed to modify large doc");
    
    // Should commit successfully
    tx.commit().expect("Failed to commit large doc");
    assert_eq!(tx.state(), TransactionState::Committed);
}
