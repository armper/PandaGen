//! Networking service for PandaGen.
//!
//! Provides packet I/O with explicit policy checks and budget enforcement.

use identity::ExecutionId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
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

/// Unique identifier for a network interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetworkInterfaceId(Uuid);

impl Default for NetworkInterfaceId {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkInterfaceId {
    pub fn new() -> Self {
        Self(new_uuid())
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

/// Advanced protocol families supported by PandaGen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvancedProtocol {
    ReliableDatagram,
    StreamMux,
    SecureChannel,
}

/// Protocol framing errors.
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("unsupported protocol: {0:?}")]
    UnsupportedProtocol(AdvancedProtocol),

    #[error("invalid protocol frame: {0}")]
    InvalidFrame(String),

    #[error("protocol validation failed: {0}")]
    ValidationFailed(String),
}

/// Header for advanced protocol frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolHeader {
    pub protocol: AdvancedProtocol,
    pub session_id: u64,
    pub sequence: u64,
}

/// Advanced protocol frame with deterministic encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolFrame {
    pub header: ProtocolHeader,
    pub payload: Vec<u8>,
}

impl ProtocolFrame {
    const MAGIC: [u8; 4] = *b"PGNP";
    const VERSION: u8 = 1;
    const HEADER_LEN: usize = 4 + 1 + 1 + 2 + 8 + 8 + 4;

    pub fn new(protocol: AdvancedProtocol, session_id: u64, sequence: u64, payload: Vec<u8>) -> Self {
        Self {
            header: ProtocolHeader {
                protocol,
                session_id,
                sequence,
            },
            payload,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::HEADER_LEN + self.payload.len());
        out.extend_from_slice(&Self::MAGIC);
        out.push(Self::VERSION);
        out.push(self.header.protocol as u8);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&self.header.session_id.to_le_bytes());
        out.extend_from_slice(&self.header.sequence.to_le_bytes());
        out.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() < Self::HEADER_LEN {
            return Err(ProtocolError::InvalidFrame("frame too short".to_string()));
        }

        if bytes[0..4] != Self::MAGIC {
            return Err(ProtocolError::InvalidFrame("bad magic".to_string()));
        }

        if bytes[4] != Self::VERSION {
            return Err(ProtocolError::InvalidFrame("unsupported version".to_string()));
        }

        let protocol = match bytes[5] {
            0 => AdvancedProtocol::ReliableDatagram,
            1 => AdvancedProtocol::StreamMux,
            2 => AdvancedProtocol::SecureChannel,
            other => {
                return Err(ProtocolError::InvalidFrame(format!(
                    "unknown protocol tag {other}"
                )))
            }
        };

        let session_id = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let sequence = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
        let payload_len = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize;

        let expected_len = Self::HEADER_LEN + payload_len;
        if bytes.len() != expected_len {
            return Err(ProtocolError::InvalidFrame(
                "payload length mismatch".to_string(),
            ));
        }

        let payload = bytes[28..].to_vec();

        Ok(Self {
            header: ProtocolHeader {
                protocol,
                session_id,
                sequence,
            },
            payload,
        })
    }
}

/// Protocol-specific validation hook.
pub trait ProtocolHandler: Send + Sync {
    fn protocol(&self) -> AdvancedProtocol;
    fn validate(&self, frame: &ProtocolFrame) -> Result<(), ProtocolError>;
}

struct ReliableDatagramHandler;
struct StreamMuxHandler;
struct SecureChannelHandler;

impl ProtocolHandler for ReliableDatagramHandler {
    fn protocol(&self) -> AdvancedProtocol {
        AdvancedProtocol::ReliableDatagram
    }

    fn validate(&self, frame: &ProtocolFrame) -> Result<(), ProtocolError> {
        if frame.payload.len() > 1200 {
            return Err(ProtocolError::ValidationFailed(
                "reliable datagram payload exceeds 1200 bytes".to_string(),
            ));
        }
        Ok(())
    }
}

impl ProtocolHandler for StreamMuxHandler {
    fn protocol(&self) -> AdvancedProtocol {
        AdvancedProtocol::StreamMux
    }

    fn validate(&self, frame: &ProtocolFrame) -> Result<(), ProtocolError> {
        if frame.header.session_id == 0 {
            return Err(ProtocolError::ValidationFailed(
                "stream mux session id must be non-zero".to_string(),
            ));
        }
        Ok(())
    }
}

impl ProtocolHandler for SecureChannelHandler {
    fn protocol(&self) -> AdvancedProtocol {
        AdvancedProtocol::SecureChannel
    }

    fn validate(&self, frame: &ProtocolFrame) -> Result<(), ProtocolError> {
        if frame.payload.len() < 16 {
            return Err(ProtocolError::ValidationFailed(
                "secure channel payload must be at least 16 bytes".to_string(),
            ));
        }
        Ok(())
    }
}

/// Registry of advanced protocol handlers.
pub struct ProtocolRegistry {
    handlers: HashMap<AdvancedProtocol, Box<dyn ProtocolHandler>>,
}

impl ProtocolRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(ReliableDatagramHandler));
        registry.register(Box::new(StreamMuxHandler));
        registry.register(Box::new(SecureChannelHandler));
        registry
    }

    pub fn register(&mut self, handler: Box<dyn ProtocolHandler>) {
        self.handlers.insert(handler.protocol(), handler);
    }

    pub fn validate(&self, frame: &ProtocolFrame) -> Result<(), ProtocolError> {
        let handler = self
            .handlers
            .get(&frame.header.protocol)
            .ok_or(ProtocolError::UnsupportedProtocol(frame.header.protocol))?;
        handler.validate(frame)
    }

    pub fn encode_frame(&self, frame: &ProtocolFrame) -> Result<Vec<u8>, ProtocolError> {
        self.validate(frame)?;
        Ok(frame.encode())
    }

    pub fn decode_frame(&self, bytes: &[u8]) -> Result<ProtocolFrame, ProtocolError> {
        let frame = ProtocolFrame::decode(bytes)?;
        self.validate(&frame)?;
        Ok(frame)
    }
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

    #[test]
    fn test_protocol_frame_roundtrip() {
        let frame = ProtocolFrame::new(
            AdvancedProtocol::ReliableDatagram,
            42,
            7,
            vec![1, 2, 3, 4],
        );
        let bytes = frame.encode();
        let decoded = ProtocolFrame::decode(&bytes).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn test_protocol_registry_validation() {
        let registry = ProtocolRegistry::with_defaults();

        let too_large = ProtocolFrame::new(
            AdvancedProtocol::ReliableDatagram,
            1,
            1,
            vec![0u8; 1201],
        );
        assert!(matches!(
            registry.encode_frame(&too_large),
            Err(ProtocolError::ValidationFailed(_))
        ));

        let missing_session = ProtocolFrame::new(
            AdvancedProtocol::StreamMux,
            0,
            1,
            vec![9, 9],
        );
        assert!(matches!(
            registry.encode_frame(&missing_session),
            Err(ProtocolError::ValidationFailed(_))
        ));

        let secure = ProtocolFrame::new(
            AdvancedProtocol::SecureChannel,
            9,
            9,
            vec![1u8; 16],
        );
        let encoded = registry.encode_frame(&secure).unwrap();
        let decoded = registry.decode_frame(&encoded).unwrap();
        assert_eq!(decoded, secure);
    }
}
