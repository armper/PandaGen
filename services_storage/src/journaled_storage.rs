//! Journaled storage backend with crash-consistent recovery.

use crate::{
    ObjectId, Transaction, TransactionError, TransactionId, TransactionalStorage, VersionId,
};
use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use identity::ExecutionId;
use kernel_api::KernelError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
struct VersionEntry {
    version_id: VersionId,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalEntry {
    Write {
        tx_id: TransactionId,
        object_id: ObjectId,
        version_id: VersionId,
        data: Vec<u8>,
    },
    Commit {
        tx_id: TransactionId,
    },
}

#[derive(Debug, Clone)]
struct PendingWrite {
    object_id: ObjectId,
    version_id: VersionId,
    data: Vec<u8>,
}

/// In-memory journaled storage backend.
#[derive(Debug, Clone)]
pub struct JournaledStorage {
    objects: BTreeMap<ObjectId, Vec<VersionEntry>>,
    journal: Vec<JournalEntry>,
    pending: BTreeMap<TransactionId, Vec<PendingWrite>>,
}

impl JournaledStorage {
    pub fn new() -> Self {
        Self {
            objects: BTreeMap::new(),
            journal: Vec::new(),
            pending: BTreeMap::new(),
        }
    }

    /// Returns the journal entries (for testing).
    pub fn journal_entries(&self) -> &[JournalEntry] {
        &self.journal
    }

    /// Returns a clone of the journal entries.
    ///
    /// This is intended for deterministic tests and snapshotting.
    pub fn journal_clone(&self) -> Vec<JournalEntry> {
        self.journal.clone()
    }

    /// Reconstructs storage state from a journal snapshot.
    ///
    /// This simulates a reboot where journal entries are persisted externally.
    pub fn from_journal(entries: Vec<JournalEntry>) -> Self {
        let mut storage = Self {
            objects: BTreeMap::new(),
            journal: entries,
            pending: BTreeMap::new(),
        };
        storage.recover();
        storage
    }

    /// Reads the latest data for an object (including pending writes).
    pub fn read_data(
        &self,
        tx: &Transaction,
        object_id: ObjectId,
    ) -> Result<Vec<u8>, TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }

        if let Some(pending) = self.pending.get(&tx.id()) {
            if let Some(entry) = pending.iter().rev().find(|p| p.object_id == object_id) {
                return Ok(entry.data.clone());
            }
        }

        let versions = self
            .objects
            .get(&object_id)
            .ok_or_else(|| TransactionError::ObjectNotFound(object_id.to_string()))?;
        versions
            .last()
            .map(|entry| entry.data.clone())
            .ok_or_else(|| TransactionError::ObjectNotFound(object_id.to_string()))
    }

    /// Recovers committed transactions from the journal.
    pub fn recover(&mut self) {
        let mut committed = BTreeSet::new();
        let mut writes: BTreeMap<TransactionId, Vec<PendingWrite>> = BTreeMap::new();

        for entry in &self.journal {
            match entry {
                JournalEntry::Write {
                    tx_id,
                    object_id,
                    version_id,
                    data,
                } => {
                    writes.entry(*tx_id).or_default().push(PendingWrite {
                        object_id: *object_id,
                        version_id: *version_id,
                        data: data.clone(),
                    });
                }
                JournalEntry::Commit { tx_id } => {
                    committed.insert(*tx_id);
                }
            }
        }

        for tx_id in committed {
            if let Some(pending) = writes.remove(&tx_id) {
                for write in pending {
                    self.objects
                        .entry(write.object_id)
                        .or_default()
                        .push(VersionEntry {
                            version_id: write.version_id,
                            data: write.data,
                        });
                }
            }
        }
    }
}

impl Default for JournaledStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionalStorage for JournaledStorage {
    fn begin_transaction(&mut self) -> Result<Transaction, TransactionError> {
        Ok(Transaction::new())
    }

    fn read(&self, tx: &Transaction, object_id: ObjectId) -> Result<VersionId, TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }

        if let Some(pending) = self.pending.get(&tx.id()) {
            if let Some(entry) = pending.iter().rev().find(|p| p.object_id == object_id) {
                return Ok(entry.version_id);
            }
        }

        let versions = self
            .objects
            .get(&object_id)
            .ok_or_else(|| TransactionError::ObjectNotFound(object_id.to_string()))?;
        versions
            .last()
            .map(|entry| entry.version_id)
            .ok_or_else(|| TransactionError::ObjectNotFound(object_id.to_string()))
    }

    fn write(
        &mut self,
        tx: &mut Transaction,
        object_id: ObjectId,
        data: &[u8],
    ) -> Result<VersionId, TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }

        let version_id = VersionId::new();
        let pending_write = PendingWrite {
            object_id,
            version_id,
            data: data.to_vec(),
        };

        self.pending
            .entry(tx.id())
            .or_default()
            .push(pending_write.clone());
        self.journal.push(JournalEntry::Write {
            tx_id: tx.id(),
            object_id,
            version_id,
            data: pending_write.data,
        });

        tx.modify(object_id)?;
        Ok(version_id)
    }

    fn commit(&mut self, tx: &mut Transaction) -> Result<(), TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }

        if let Some(pending) = self.pending.remove(&tx.id()) {
            for write in pending {
                self.objects
                    .entry(write.object_id)
                    .or_default()
                    .push(VersionEntry {
                        version_id: write.version_id,
                        data: write.data,
                    });
            }
        }

        self.journal.push(JournalEntry::Commit { tx_id: tx.id() });
        tx.commit()?;
        Ok(())
    }

    fn rollback(&mut self, tx: &mut Transaction) -> Result<(), TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }

        self.pending.remove(&tx.id());
        tx.rollback()?;
        Ok(())
    }
}

/// Storage budget enforcement trait.
pub trait StorageBudget {
    fn consume_storage_op(
        &mut self,
        execution_id: ExecutionId,
        operation: StorageOperation,
    ) -> Result<(), KernelError>;
}

#[cfg(not(target_os = "none"))]
impl StorageBudget for sim_kernel::SimulatedKernel {
    fn consume_storage_op(
        &mut self,
        execution_id: ExecutionId,
        operation: StorageOperation,
    ) -> Result<(), KernelError> {
        let op = match operation {
            StorageOperation::Read => sim_kernel::resource_audit::StorageOperation::Read,
            StorageOperation::Write => sim_kernel::resource_audit::StorageOperation::Write,
            StorageOperation::Commit => sim_kernel::resource_audit::StorageOperation::Commit,
        };
        self.try_consume_storage_op(execution_id, op)
    }
}

/// Storage operations for budgeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageOperation {
    Read,
    Write,
    Commit,
}

/// Errors from storage service.
#[derive(Debug)]
pub enum StorageServiceError {
    Transaction(String),
    Budget(String),
}

impl core::fmt::Display for StorageServiceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StorageServiceError::Transaction(msg) => write!(f, "Transaction error: {}", msg),
            StorageServiceError::Budget(msg) => write!(f, "Budget error: {}", msg),
        }
    }
}

impl From<TransactionError> for StorageServiceError {
    fn from(error: TransactionError) -> Self {
        use alloc::format;
        StorageServiceError::Transaction(format!("{}", error))
    }
}

impl From<KernelError> for StorageServiceError {
    fn from(error: KernelError) -> Self {
        use alloc::format;
        StorageServiceError::Budget(format!("{:?}", error))
    }
}

/// Budgeted storage service wrapper.
pub struct StorageService<B: StorageBudget> {
    storage: JournaledStorage,
    budget: B,
}

impl<B: StorageBudget> StorageService<B> {
    pub fn new(storage: JournaledStorage, budget: B) -> Self {
        Self { storage, budget }
    }

    pub fn begin_transaction(&mut self) -> Result<Transaction, StorageServiceError> {
        Ok(self.storage.begin_transaction()?)
    }

    pub fn read(
        &mut self,
        execution_id: ExecutionId,
        tx: &Transaction,
        object_id: ObjectId,
    ) -> Result<VersionId, StorageServiceError> {
        self.budget
            .consume_storage_op(execution_id, StorageOperation::Read)?;
        Ok(self.storage.read(tx, object_id)?)
    }

    pub fn write(
        &mut self,
        execution_id: ExecutionId,
        tx: &mut Transaction,
        object_id: ObjectId,
        data: &[u8],
    ) -> Result<VersionId, StorageServiceError> {
        self.budget
            .consume_storage_op(execution_id, StorageOperation::Write)?;
        Ok(self.storage.write(tx, object_id, data)?)
    }

    pub fn commit(
        &mut self,
        execution_id: ExecutionId,
        tx: &mut Transaction,
    ) -> Result<(), StorageServiceError> {
        self.budget
            .consume_storage_op(execution_id, StorageOperation::Commit)?;
        Ok(self.storage.commit(tx)?)
    }

    pub fn rollback(&mut self, tx: &mut Transaction) -> Result<(), StorageServiceError> {
        Ok(self.storage.rollback(tx)?)
    }

    pub fn recover(&mut self) {
        self.storage.recover();
    }

    pub fn storage(&self) -> &JournaledStorage {
        &self.storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockBudget {
        remaining: i64,
    }

    impl StorageBudget for MockBudget {
        fn consume_storage_op(
            &mut self,
            _execution_id: ExecutionId,
            _operation: StorageOperation,
        ) -> Result<(), KernelError> {
            self.remaining -= 1;
            if self.remaining < 0 {
                return Err(KernelError::ResourceBudgetExhausted {
                    resource_type: "StorageOps".to_string(),
                    limit: 0,
                    usage: 0,
                    identity: "exec:test".to_string(),
                    operation: "storage".to_string(),
                });
            }
            Ok(())
        }
    }

    #[test]
    fn test_journal_recovery_committed_only() {
        let mut storage = JournaledStorage::new();
        let mut tx = storage.begin_transaction().unwrap();
        let object = ObjectId::new();
        let version = storage.write(&mut tx, object, b"data").unwrap();
        storage.commit(&mut tx).unwrap();

        let mut tx2 = storage.begin_transaction().unwrap();
        let object2 = ObjectId::new();
        storage.write(&mut tx2, object2, b"temp").unwrap();
        // No commit for tx2

        let mut recovered = JournaledStorage::new();
        recovered.journal = storage.journal.clone();
        recovered.recover();

        let read_tx = recovered.begin_transaction().unwrap();
        let recovered_version = recovered.read(&read_tx, object).unwrap();
        assert_eq!(recovered_version, version);
        let recovered_data = recovered.read_data(&read_tx, object).unwrap();
        assert_eq!(recovered_data, b"data".to_vec());
        assert!(recovered.read(&read_tx, object2).is_err());
    }

    #[test]
    fn test_storage_service_budget_enforcement() {
        let storage = JournaledStorage::new();
        let budget = MockBudget { remaining: 1 };
        let mut service = StorageService::new(storage, budget);

        let exec_id = ExecutionId::new();
        let mut tx = service.begin_transaction().unwrap();
        let object = ObjectId::new();

        service.write(exec_id, &mut tx, object, b"data").unwrap();
        let result = service.write(exec_id, &mut tx, object, b"data2");
        assert!(matches!(result, Err(StorageServiceError::Budget(_))));
    }
}
