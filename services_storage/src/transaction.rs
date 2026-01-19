//! Transaction support for storage operations

use crate::{ObjectId, VersionId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Unique identifier for a transaction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionId(Uuid);

impl TransactionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for TransactionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during transactions
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransactionError {
    /// Transaction conflict (concurrent modification)
    #[error("Transaction conflict on object {0}")]
    Conflict(String),

    /// Object not found
    #[error("Object not found: {0}")]
    ObjectNotFound(String),

    /// Transaction already committed or rolled back
    #[error("Transaction already finalized")]
    AlreadyFinalized,

    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

/// Transaction state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    /// Transaction is active
    Active,
    /// Transaction has been committed
    Committed,
    /// Transaction has been rolled back
    RolledBack,
}

/// A transaction for atomic storage operations
///
/// Unlike traditional filesystem operations (which are often non-atomic),
/// all storage modifications happen within transactions.
///
/// ## Example
///
/// ```
/// use services_storage::{Transaction, TransactionState};
///
/// let mut tx = Transaction::new();
/// assert_eq!(tx.state(), TransactionState::Active);
///
/// // Perform operations...
///
/// tx.commit().unwrap();
/// assert_eq!(tx.state(), TransactionState::Committed);
/// ```
pub struct Transaction {
    /// Transaction identifier
    id: TransactionId,
    /// Transaction state
    state: TransactionState,
    /// Objects modified in this transaction
    modified: Vec<ObjectId>,
}

impl Transaction {
    /// Creates a new transaction
    pub fn new() -> Self {
        Self {
            id: TransactionId::new(),
            state: TransactionState::Active,
            modified: Vec::new(),
        }
    }

    /// Returns the transaction ID
    pub fn id(&self) -> TransactionId {
        self.id
    }

    /// Returns the current state
    pub fn state(&self) -> TransactionState {
        self.state
    }

    /// Records an object modification
    pub fn modify(&mut self, object_id: ObjectId) -> Result<(), TransactionError> {
        if self.state != TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }
        self.modified.push(object_id);
        Ok(())
    }

    /// Returns the list of modified objects
    pub fn modified_objects(&self) -> &[ObjectId] {
        &self.modified
    }

    /// Commits the transaction
    ///
    /// Makes all modifications permanent.
    pub fn commit(&mut self) -> Result<(), TransactionError> {
        if self.state != TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }
        // In a real system, this would persist changes
        self.state = TransactionState::Committed;
        Ok(())
    }

    /// Rolls back the transaction
    ///
    /// Discards all modifications.
    pub fn rollback(&mut self) -> Result<(), TransactionError> {
        if self.state != TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }
        // In a real system, this would discard changes
        self.state = TransactionState::RolledBack;
        self.modified.clear();
        Ok(())
    }
}

impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for storage backends that support transactions
pub trait TransactionalStorage {
    /// Begins a new transaction
    fn begin_transaction(&mut self) -> Result<Transaction, TransactionError>;

    /// Reads an object within a transaction
    fn read(&self, tx: &Transaction, object_id: ObjectId) -> Result<VersionId, TransactionError>;

    /// Writes an object within a transaction
    fn write(
        &mut self,
        tx: &mut Transaction,
        object_id: ObjectId,
        data: &[u8],
    ) -> Result<VersionId, TransactionError>;

    /// Commits a transaction
    fn commit(&mut self, tx: &mut Transaction) -> Result<(), TransactionError>;

    /// Rolls back a transaction
    fn rollback(&mut self, tx: &mut Transaction) -> Result<(), TransactionError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_transaction_creation() {
        let tx = Transaction::new();
        assert_eq!(tx.state(), TransactionState::Active);
        assert_eq!(tx.modified_objects().len(), 0);
        assert_ne!(tx.id().as_uuid(), Uuid::nil());
    }

    #[test]
    fn test_transaction_modify() {
        let mut tx = Transaction::new();
        let obj_id = ObjectId::new();

        tx.modify(obj_id).unwrap();
        assert_eq!(tx.modified_objects().len(), 1);
        assert_eq!(tx.modified_objects()[0], obj_id);
    }

    #[test]
    fn test_transaction_commit() {
        let mut tx = Transaction::new();
        let obj_id = ObjectId::new();

        tx.modify(obj_id).unwrap();
        tx.commit().unwrap();

        assert_eq!(tx.state(), TransactionState::Committed);
        assert_eq!(tx.modified_objects().len(), 1);
    }

    #[test]
    fn test_transaction_rollback() {
        let mut tx = Transaction::new();
        let obj_id = ObjectId::new();

        tx.modify(obj_id).unwrap();
        tx.rollback().unwrap();

        assert_eq!(tx.state(), TransactionState::RolledBack);
        assert_eq!(tx.modified_objects().len(), 0);
    }

    #[test]
    fn test_transaction_double_commit() {
        let mut tx = Transaction::new();
        tx.commit().unwrap();
        let result = tx.commit();
        assert_eq!(result, Err(TransactionError::AlreadyFinalized));
    }

    #[test]
    fn test_transaction_double_rollback() {
        let mut tx = Transaction::new();
        tx.rollback().unwrap();
        let result = tx.rollback();
        assert_eq!(result, Err(TransactionError::AlreadyFinalized));
    }

    #[test]
    fn test_modify_after_commit() {
        let mut tx = Transaction::new();
        tx.commit().unwrap();
        let result = tx.modify(ObjectId::new());
        assert_eq!(result, Err(TransactionError::AlreadyFinalized));
    }
}
