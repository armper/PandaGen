//! Distributed storage sync for versioned objects.

pub mod consensus;

pub use consensus::{
    AppendEntriesRequest, AppendEntriesResponse, ConsensusCluster, ConsensusError, ConsensusNode,
    ConsensusNodeId, LogEntry, NodeState, VoteRequest, VoteResponse,
};

use serde::{Deserialize, Serialize};
use services_storage::{ObjectId, VersionId};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use uuid::Uuid;

#[cfg(target_os = "none")]
fn new_uuid() -> Uuid {
    use core::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let hi = COUNTER.fetch_add(1, Ordering::Relaxed);
    let lo = COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&hi.to_le_bytes());
    bytes[8..].copy_from_slice(&lo.to_le_bytes());

    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    Uuid::from_bytes(bytes)
}

#[cfg(not(target_os = "none"))]
fn new_uuid() -> Uuid {
    Uuid::new_v4()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(Uuid);

impl Default for DeviceId {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceId {
    pub fn new() -> Self {
        Self(new_uuid())
    }

    /// The inner UUID, so callers can order devices deterministically.
    pub fn as_uuid(&self) -> Uuid {
        self.0
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

    /// Union of two sync states.
    ///
    /// This concatenated both sides' entries with no dedup, so re-syncing
    /// with a peer you had already merged duplicated every shared entry and
    /// a device's log grew multiplicatively with the number of sync rounds.
    /// A merge is a union: merging the same peer twice must be the same as
    /// merging it once.
    pub fn merge(&self, other: &SyncState) -> SyncState {
        let mut merged: HashMap<DeviceId, Vec<VersionedObject>> = HashMap::new();
        for log in self.logs.iter().chain(other.logs.iter()) {
            let entries = merged.entry(log.device_id).or_default();
            for entry in &log.entries {
                if !entries
                    .iter()
                    .any(|seen: &VersionedObject| seen.version_id == entry.version_id)
                {
                    entries.push(entry.clone());
                }
            }
        }

        // Sorted, so two nodes with the same inputs produce the same state
        // rather than whatever order a HashMap happened to iterate in.
        let mut logs: Vec<DeviceLog> = merged
            .into_iter()
            .map(|(device_id, mut entries)| {
                entries.sort_by(|a, b| {
                    a.timestamp_ns
                        .cmp(&b.timestamp_ns)
                        .then_with(|| a.version_id.as_uuid().cmp(&b.version_id.as_uuid()))
                });
                DeviceLog { device_id, entries }
            })
            .collect();
        logs.sort_by(|a, b| a.device_id.as_uuid().cmp(&b.device_id.as_uuid()));

        SyncState { logs }
    }

    /// The latest version of each object.
    ///
    /// The tie-break used to be `>=` on the timestamp alone, over entries in
    /// whatever order a HashMap iterated, so two devices writing the same
    /// object at the same nanosecond left the surviving payload up to the
    /// hasher: two nodes with identical inputs converged on *different*
    /// contents for the same object, which is precisely the property the
    /// word "consistency" is supposed to name. The version id breaks the tie
    /// the same way everywhere.
    pub fn compact(&self) -> HashMap<ObjectId, VersionedObject> {
        let mut latest: HashMap<ObjectId, VersionedObject> = HashMap::new();
        for entry in self.all_entries() {
            let replace = match latest.get(&entry.object_id) {
                Some(current) => {
                    (entry.timestamp_ns, entry.version_id.as_uuid())
                        > (current.timestamp_ns, current.version_id.as_uuid())
                }
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

#[cfg(test)]
mod consistency_tests {
    use super::*;

    fn entry(object: ObjectId, payload: &[u8], timestamp_ns: u64) -> VersionedObject {
        VersionedObject {
            object_id: object,
            version_id: VersionId::new(),
            payload: payload.to_vec(),
            timestamp_ns,
        }
    }

    #[test]
    fn merging_the_same_peer_twice_is_the_same_as_merging_it_once() {
        // `merge` concatenated both sides' entries with no dedup, so
        // re-syncing with a peer you had already merged duplicated every
        // shared entry: a device's log grew multiplicatively with the number
        // of sync rounds.
        let device = DeviceId::new();
        let object = ObjectId::new();

        let mut a = SyncState::default();
        a.add_device(device);
        a.append(device, entry(object, b"AAA", 10)).unwrap();

        let mut b = SyncState::default();
        b.add_device(device);
        b.append(device, entry(object, b"BBB", 20)).unwrap();

        let once = a.merge(&b);
        assert_eq!(once.all_entries().len(), 2);
        let twice = once.merge(&b);
        assert_eq!(
            twice.all_entries().len(),
            2,
            "merging a peer we have already merged duplicated its entries"
        );
    }

    #[test]
    fn two_nodes_with_the_same_inputs_agree_on_the_contents() {
        // The tie-break was `>=` on the timestamp alone, over entries in
        // whatever order a HashMap iterated. Two devices writing the same
        // object at the same nanosecond left the surviving payload up to the
        // hasher -- two nodes with identical inputs converged on *different*
        // contents for the same object.
        let first = DeviceId::new();
        let second = DeviceId::new();
        let object = ObjectId::new();
        let left = entry(object, b"AAA", 1_000);
        let right = entry(object, b"BBB", 1_000);

        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            let mut a = SyncState::default();
            a.add_device(first);
            a.append(first, left.clone()).unwrap();

            let mut b = SyncState::default();
            b.add_device(second);
            b.append(second, right.clone()).unwrap();

            let merged = a.merge(&b);
            let winner = merged.compact().remove(&object).unwrap();
            seen.insert(winner.payload);
        }
        assert_eq!(
            seen.len(),
            1,
            "the same inputs produced more than one answer: {seen:?}"
        );
    }

    #[test]
    fn a_log_entry_with_index_zero_does_not_panic() {
        // `index` comes off the wire and `(index - 1) as usize` had no
        // guard: a debug build panicked, and a release build wrapped to
        // usize::MAX and pushed the entry at the wrong slot, desynchronising
        // every later index for the life of the node.
        use crate::consensus::{AppendEntriesRequest, ConsensusNode, ConsensusNodeId, LogEntry};
        let mut node = ConsensusNode::new(ConsensusNodeId::new());
        let response = node.handle_append_entries(AppendEntriesRequest {
            term: 1,
            leader_id: ConsensusNodeId::new(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![LogEntry {
                term: 1,
                index: 0,
                payload: b"x".to_vec(),
                timestamp_ns: 1,
            }],
            leader_commit: 0,
        });
        let _ = response;
    }
}
