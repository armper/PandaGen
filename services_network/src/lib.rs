//! Networking service for PandaGen.
//!
//! Provides packet I/O with explicit policy checks and budget enforcement.

use identity::ExecutionId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use thiserror::Error;
use uuid::Uuid;

/// Unique identifier for a network interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetworkInterfaceId(Uuid);

impl NetworkInterfaceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Endpoint (address + port) for packets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub address: String,
    pub port: u16,
}

impl Endpoint {
    pub fn new(address: impl Into<String>, port: u16) -> Self {
        Self {
            address: address.into(),
            port,
        }
    }
}

/// Packet protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacketProtocol {
    Tcp,
    Udp,
    Custom,
}

/// Network packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Packet {
    pub source: Endpoint,
    pub destination: Endpoint,
    pub protocol: PacketProtocol,
    pub payload: Vec<u8>,
}

/// Direction of packet flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PacketDirection {
    Send,
    Receive,
}

/// Operation type for budget accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PacketOperation {
    Send,
    Receive,
}

/// Context used for network policy evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketContext {
    pub execution_id: ExecutionId,
    pub interface_id: NetworkInterfaceId,
    pub direction: PacketDirection,
    pub packet: Packet,
}

/// Policy decision for network operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkDecision {
    Allow,
    Deny { reason: String },
}

/// Network policy trait.
pub trait NetworkPolicy: Send + Sync {
    fn evaluate(&self, context: &PacketContext) -> NetworkDecision;
}

/// Allow-all network policy.
pub struct AllowAllPolicy;

impl NetworkPolicy for AllowAllPolicy {
    fn evaluate(&self, _context: &PacketContext) -> NetworkDecision {
        NetworkDecision::Allow
    }
}

/// Deny-all network policy.
pub struct DenyAllPolicy;

impl NetworkPolicy for DenyAllPolicy {
    fn evaluate(&self, _context: &PacketContext) -> NetworkDecision {
        NetworkDecision::Deny {
            reason: "All network traffic denied".to_string(),
        }
    }
}

/// Budget enforcement abstraction for packet operations.
pub trait PacketBudget {
    fn consume_packet(
        &mut self,
        execution_id: ExecutionId,
        operation: PacketOperation,
    ) -> Result<(), kernel_api::KernelError>;
}

impl PacketBudget for sim_kernel::SimulatedKernel {
    fn consume_packet(
        &mut self,
        execution_id: ExecutionId,
        operation: PacketOperation,
    ) -> Result<(), kernel_api::KernelError> {
        let op = match operation {
            PacketOperation::Send => sim_kernel::resource_audit::PacketOperation::Send,
            PacketOperation::Receive => sim_kernel::resource_audit::PacketOperation::Receive,
        };
        self.try_consume_packet(execution_id, op)
    }
}

/// Network service errors.
#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("Policy denied packet: {0}")]
    PolicyDenied(String),

    #[error("Budget enforcement failed: {0}")]
    BudgetError(String),
}

impl From<kernel_api::KernelError> for NetworkError {
    fn from(error: kernel_api::KernelError) -> Self {
        NetworkError::BudgetError(error.to_string())
    }
}

/// Network service implementing packet IO with policy + budgets.
pub struct NetworkService {
    policy: Box<dyn NetworkPolicy>,
    queues: HashMap<NetworkInterfaceId, VecDeque<Packet>>,
}

impl NetworkService {
    pub fn new(policy: Box<dyn NetworkPolicy>) -> Self {
        Self {
            policy,
            queues: HashMap::new(),
        }
    }

    pub fn send_packet<B: PacketBudget>(
        &mut self,
        budget: &mut B,
        execution_id: ExecutionId,
        interface_id: NetworkInterfaceId,
        packet: Packet,
    ) -> Result<(), NetworkError> {
        let context = PacketContext {
            execution_id,
            interface_id,
            direction: PacketDirection::Send,
            packet: packet.clone(),
        };

        match self.policy.evaluate(&context) {
            NetworkDecision::Allow => {}
            NetworkDecision::Deny { reason } => return Err(NetworkError::PolicyDenied(reason)),
        }

        budget.consume_packet(execution_id, PacketOperation::Send)?;

        self.queues
            .entry(interface_id)
            .or_default()
            .push_back(packet);
        Ok(())
    }

    pub fn receive_packet<B: PacketBudget>(
        &mut self,
        budget: &mut B,
        execution_id: ExecutionId,
        interface_id: NetworkInterfaceId,
    ) -> Result<Option<Packet>, NetworkError> {
        let packet = match self.queues.get_mut(&interface_id) {
            Some(queue) => queue.pop_front(),
            None => None,
        };

        let packet = match packet {
            Some(packet) => packet,
            None => return Ok(None),
        };

        let context = PacketContext {
            execution_id,
            interface_id,
            direction: PacketDirection::Receive,
            packet: packet.clone(),
        };

        match self.policy.evaluate(&context) {
            NetworkDecision::Allow => {}
            NetworkDecision::Deny { reason } => return Err(NetworkError::PolicyDenied(reason)),
        }

        budget.consume_packet(execution_id, PacketOperation::Receive)?;

        Ok(Some(packet))
    }

    pub fn queued_packets(&self, interface_id: NetworkInterfaceId) -> usize {
        self.queues.get(&interface_id).map(|q| q.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resources::PacketCount;

    #[derive(Default)]
    struct MockBudget {
        limit: Option<PacketCount>,
        used: PacketCount,
    }

    impl PacketBudget for MockBudget {
        fn consume_packet(
            &mut self,
            _execution_id: ExecutionId,
            _operation: PacketOperation,
        ) -> Result<(), kernel_api::KernelError> {
            let next = self.used.0 + 1;
            if let Some(limit) = self.limit {
                if next > limit.0 {
                    return Err(kernel_api::KernelError::ResourceBudgetExhausted {
                        resource_type: "PacketCount".to_string(),
                        limit: limit.0,
                        usage: self.used.0,
                        identity: "exec:test".to_string(),
                        operation: "packet".to_string(),
                    });
                }
            }
            self.used = PacketCount::new(next);
            Ok(())
        }
    }

    #[test]
    fn test_network_service_send_receive() {
        let mut service = NetworkService::new(Box::new(AllowAllPolicy));
        let mut budget = MockBudget::default();
        let exec_id = ExecutionId::new();
        let iface = NetworkInterfaceId::new();

        let packet = Packet {
            source: Endpoint::new("10.0.0.1", 1000),
            destination: Endpoint::new("10.0.0.2", 2000),
            protocol: PacketProtocol::Udp,
            payload: vec![1, 2, 3],
        };

        service
            .send_packet(&mut budget, exec_id, iface, packet.clone())
            .unwrap();
        assert_eq!(service.queued_packets(iface), 1);

        let received = service
            .receive_packet(&mut budget, exec_id, iface)
            .unwrap()
            .unwrap();
        assert_eq!(received, packet);
    }

    #[test]
    fn test_network_service_budget_exhausted() {
        let mut service = NetworkService::new(Box::new(AllowAllPolicy));
        let mut budget = MockBudget {
            limit: Some(PacketCount::new(1)),
            used: PacketCount::zero(),
        };
        let exec_id = ExecutionId::new();
        let iface = NetworkInterfaceId::new();

        let packet = Packet {
            source: Endpoint::new("10.0.0.1", 1000),
            destination: Endpoint::new("10.0.0.2", 2000),
            protocol: PacketProtocol::Udp,
            payload: vec![1],
        };

        service
            .send_packet(&mut budget, exec_id, iface, packet.clone())
            .unwrap();
        let result = service.send_packet(&mut budget, exec_id, iface, packet);
        assert!(matches!(result, Err(NetworkError::BudgetError(_))));
    }

    #[test]
    fn test_network_policy_denies() {
        let mut service = NetworkService::new(Box::new(DenyAllPolicy));
        let mut budget = MockBudget::default();
        let exec_id = ExecutionId::new();
        let iface = NetworkInterfaceId::new();

        let packet = Packet {
            source: Endpoint::new("10.0.0.1", 1000),
            destination: Endpoint::new("10.0.0.2", 2000),
            protocol: PacketProtocol::Tcp,
            payload: vec![9],
        };

        let result = service.send_packet(&mut budget, exec_id, iface, packet);
        assert!(matches!(result, Err(NetworkError::PolicyDenied(_))));
    }
}
