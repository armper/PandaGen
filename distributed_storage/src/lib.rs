//! Distributed storage sync for versioned objects.

use serde::{Deserialize, Serialize};
use services_storage::{ObjectId, VersionId};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(Uuid);

impl DeviceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionedObject {
    pub object_id: ObjectId,
    pub version_id: VersionId,
    pub payload: Vec<u8>,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceLog {
    pub device_id: DeviceId,
    pub entries: Vec<VersionedObject>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncState {
    pub logs: Vec<DeviceLog>,
}

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("Device not found: {0:?}")]
    DeviceNotFound(DeviceId),
}

impl SyncState {
    pub fn add_device(&mut self, device_id: DeviceId) {
        self.logs.push(DeviceLog {
            device_id,
            entries: Vec::new(),
        });
    }

    pub fn append(&mut self, device_id: DeviceId, entry: VersionedObject) -> Result<(), SyncError> {
        let log = self
            .logs
            .iter_mut()
            .find(|log| log.device_id == device_id)
            .ok_or(SyncError::DeviceNotFound(device_id))?;
        log.entries.push(entry);
        Ok(())
    }

    pub fn merge(&self, other: &SyncState) -> SyncState {
        let mut merged: HashMap<DeviceId, Vec<VersionedObject>> = HashMap::new();
        for log in self.logs.iter().chain(other.logs.iter()) {
            merged
                .entry(log.device_id)
                .or_default()
                .extend(log.entries.clone());
        }

        let logs = merged
            .into_iter()
            .map(|(device_id, entries)| DeviceLog { device_id, entries })
            .collect();

        SyncState { logs }
    }

    pub fn compact(&self) -> HashMap<ObjectId, VersionedObject> {
        let mut latest: HashMap<ObjectId, VersionedObject> = HashMap::new();
        for entry in self.all_entries() {
            let replace = match latest.get(&entry.object_id) {
                Some(current) => entry.timestamp_ns >= current.timestamp_ns,
                None => true,
            };
            if replace {
                latest.insert(entry.object_id, entry.clone());
            }
        }
        latest
    }

    pub fn all_entries(&self) -> Vec<VersionedObject> {
        self.logs
            .iter()
            .flat_map(|log| log.entries.clone())
            .collect()
    }

    pub fn version_set(&self) -> HashSet<VersionId> {
        self.logs
            .iter()
            .flat_map(|log| log.entries.iter().map(|e| e.version_id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_merge_and_compact() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();
        let object = ObjectId::new();

        let mut a = SyncState::default();
        a.add_device(device_a);
        a.append(
            device_a,
            VersionedObject {
                object_id: object,
                version_id: VersionId::new(),
                payload: b"v1".to_vec(),
                timestamp_ns: 10,
            },
        )
        .unwrap();

        let mut b = SyncState::default();
        b.add_device(device_b);
        b.append(
            device_b,
            VersionedObject {
                object_id: object,
                version_id: VersionId::new(),
                payload: b"v2".to_vec(),
                timestamp_ns: 20,
            },
        )
        .unwrap();

        let merged = a.merge(&b);
        assert_eq!(merged.all_entries().len(), 2);
        let compacted = merged.compact();
        assert_eq!(compacted.len(), 1);
        assert_eq!(compacted.get(&object).unwrap().payload, b"v2".to_vec());
    }
}
