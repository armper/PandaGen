//! Deterministic consensus primitives (Raft-style) for distributed storage.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
pub struct ConsensusNodeId(Uuid);

impl ConsensusNodeId {
    pub fn new() -> Self {
        Self(new_uuid())
    }
}

impl Default for ConsensusNodeId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeState {
    Follower,
    Candidate,
    Leader,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub term: u64,
    pub index: u64,
    pub payload: Vec<u8>,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteRequest {
    pub term: u64,
    pub candidate_id: ConsensusNodeId,
    pub last_log_index: u64,
    pub last_log_term: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteResponse {
    pub term: u64,
    pub vote_granted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppendEntriesRequest {
    pub term: u64,
    pub leader_id: ConsensusNodeId,
    pub prev_log_index: u64,
    pub prev_log_term: u64,
    pub entries: Vec<LogEntry>,
    pub leader_commit: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppendEntriesResponse {
    pub term: u64,
    pub success: bool,
    pub match_index: u64,
}

#[derive(Debug, Error)]
pub enum ConsensusError {
    #[error("node not found: {0:?}")]
    NodeNotFound(ConsensusNodeId),

    #[error("leader required")]
    LeaderRequired,

    #[error("quorum not reached")]
    QuorumNotReached,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusNode {
    pub id: ConsensusNodeId,
    pub state: NodeState,
    pub current_term: u64,
    pub voted_for: Option<ConsensusNodeId>,
    pub log: Vec<LogEntry>,
    pub commit_index: u64,
    pub last_applied: u64,
}

impl ConsensusNode {
    pub fn new(id: ConsensusNodeId) -> Self {
        Self {
            id,
            state: NodeState::Follower,
            current_term: 0,
            voted_for: None,
            log: Vec::new(),
            commit_index: 0,
            last_applied: 0,
        }
    }

    pub fn last_log_index(&self) -> u64 {
        self.log.last().map(|entry| entry.index).unwrap_or(0)
    }

    pub fn last_log_term(&self) -> u64 {
        self.log.last().map(|entry| entry.term).unwrap_or(0)
    }

    pub fn become_candidate(&mut self) -> VoteRequest {
        self.state = NodeState::Candidate;
        self.current_term += 1;
        self.voted_for = Some(self.id);
        VoteRequest {
            term: self.current_term,
            candidate_id: self.id,
            last_log_index: self.last_log_index(),
            last_log_term: self.last_log_term(),
        }
    }

    pub fn become_leader(&mut self) {
        self.state = NodeState::Leader;
    }

    pub fn handle_request_vote(&mut self, request: VoteRequest) -> VoteResponse {
        if request.term < self.current_term {
            return VoteResponse {
                term: self.current_term,
                vote_granted: false,
            };
        }

        if request.term > self.current_term {
            self.current_term = request.term;
            self.voted_for = None;
            self.state = NodeState::Follower;
        }

        let up_to_date = request.last_log_term > self.last_log_term()
            || (request.last_log_term == self.last_log_term()
                && request.last_log_index >= self.last_log_index());

        let can_vote = self.voted_for.is_none() || self.voted_for == Some(request.candidate_id);
        let vote_granted = can_vote && up_to_date;

        if vote_granted {
            self.voted_for = Some(request.candidate_id);
        }

        VoteResponse {
            term: self.current_term,
            vote_granted,
        }
    }

    pub fn handle_append_entries(&mut self, request: AppendEntriesRequest) -> AppendEntriesResponse {
        if request.term < self.current_term {
            return AppendEntriesResponse {
                term: self.current_term,
                success: false,
                match_index: self.last_log_index(),
            };
        }

        if request.term > self.current_term {
            self.current_term = request.term;
            self.voted_for = None;
        }

        self.state = NodeState::Follower;

        if request.prev_log_index > 0 {
            let prev = self
                .log
                .get((request.prev_log_index - 1) as usize)
                .cloned();
            if prev.is_none() || prev.unwrap().term != request.prev_log_term {
                return AppendEntriesResponse {
                    term: self.current_term,
                    success: false,
                    match_index: self.last_log_index(),
                };
            }
        }

        for entry in request.entries.into_iter() {
            if let Some(existing) = self.log.get((entry.index - 1) as usize) {
                if existing.term != entry.term {
                    self.log.truncate((entry.index - 1) as usize);
                    self.log.push(entry);
                }
            } else {
                self.log.push(entry);
            }
        }

        if request.leader_commit > self.commit_index {
            self.commit_index = self
                .last_log_index()
                .min(request.leader_commit);
        }

        AppendEntriesResponse {
            term: self.current_term,
            success: true,
            match_index: self.last_log_index(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ConsensusCluster {
    nodes: HashMap<ConsensusNodeId, ConsensusNode>,
}

impl ConsensusCluster {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, node: ConsensusNode) {
        self.nodes.insert(node.id, node);
    }

    pub fn node(&self, id: ConsensusNodeId) -> Option<&ConsensusNode> {
        self.nodes.get(&id)
    }

    pub fn node_mut(&mut self, id: ConsensusNodeId) -> Option<&mut ConsensusNode> {
        self.nodes.get_mut(&id)
    }

    pub fn elect_leader(&mut self, candidate_id: ConsensusNodeId) -> Result<(), ConsensusError> {
        let request = {
            let candidate = self
                .nodes
                .get_mut(&candidate_id)
                .ok_or(ConsensusError::NodeNotFound(candidate_id))?;
            candidate.become_candidate()
        };

        let mut votes = 1usize;
        for (node_id, node) in self.nodes.iter_mut() {
            if *node_id == candidate_id {
                continue;
            }
            let response = node.handle_request_vote(request);
            if response.vote_granted {
                votes += 1;
            }
        }

        let quorum = self.nodes.len() / 2 + 1;
        if votes < quorum {
            return Err(ConsensusError::QuorumNotReached);
        }

        if let Some(candidate) = self.nodes.get_mut(&candidate_id) {
            candidate.become_leader();
        }

        Ok(())
    }

    pub fn replicate_entry(
        &mut self,
        leader_id: ConsensusNodeId,
        payload: Vec<u8>,
        timestamp_ns: u64,
    ) -> Result<LogEntry, ConsensusError> {
        let (term, index, prev_index, prev_term) = {
            let leader = self
                .nodes
                .get(&leader_id)
                .ok_or(ConsensusError::NodeNotFound(leader_id))?;
            if leader.state != NodeState::Leader {
                return Err(ConsensusError::LeaderRequired);
            }
            let prev_index = leader.last_log_index();
            let prev_term = leader.last_log_term();
            (leader.current_term, prev_index + 1, prev_index, prev_term)
        };

        let entry = LogEntry {
            term,
            index,
            payload,
            timestamp_ns,
        };

        let request = AppendEntriesRequest {
            term,
            leader_id,
            prev_log_index: prev_index,
            prev_log_term: prev_term,
            entries: vec![entry.clone()],
            leader_commit: index,
        };

        let mut successes = 0usize;
        for node in self.nodes.values_mut() {
            let response = node.handle_append_entries(request.clone());
            if response.success {
                successes += 1;
            }
        }

        let quorum = self.nodes.len() / 2 + 1;
        if successes < quorum {
            return Err(ConsensusError::QuorumNotReached);
        }

        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_election_and_replication() {
        let mut cluster = ConsensusCluster::new();
        let node_a = ConsensusNode::new(ConsensusNodeId::new());
        let node_b = ConsensusNode::new(ConsensusNodeId::new());
        let node_c = ConsensusNode::new(ConsensusNodeId::new());

        let leader_id = node_a.id;
        cluster.add_node(node_a);
        cluster.add_node(node_b);
        cluster.add_node(node_c);

        cluster.elect_leader(leader_id).unwrap();
        assert_eq!(cluster.node(leader_id).unwrap().state, NodeState::Leader);

        let entry = cluster
            .replicate_entry(leader_id, b"alpha".to_vec(), 10)
            .unwrap();

        for node in cluster.nodes.values() {
            assert_eq!(node.log.len(), 1);
            assert_eq!(node.log[0], entry);
            assert_eq!(node.commit_index, 1);
        }
    }

    #[test]
    fn test_vote_up_to_date_rule() {
        let mut follower = ConsensusNode::new(ConsensusNodeId::new());
        follower.log.push(LogEntry {
            term: 2,
            index: 1,
            payload: vec![1],
            timestamp_ns: 5,
        });

        let request = VoteRequest {
            term: 2,
            candidate_id: ConsensusNodeId::new(),
            last_log_index: 0,
            last_log_term: 0,
        };

        let response = follower.handle_request_vote(request);
        assert!(!response.vote_granted);
    }
}
