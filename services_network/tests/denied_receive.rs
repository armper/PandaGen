//! A refusal must not also be a loss.

use identity::ExecutionId;
use services_network::*;

struct NoBudget;

impl PacketBudget for NoBudget {
    fn consume_packet(
        &mut self,
        _execution_id: ExecutionId,
        _operation: PacketOperation,
    ) -> Result<(), kernel_api::KernelError> {
        Ok(())
    }
}

struct SendOnlyPolicy;

impl NetworkPolicy for SendOnlyPolicy {
    fn evaluate(&self, context: &PacketContext) -> NetworkDecision {
        match context.direction {
            PacketDirection::Send => NetworkDecision::Allow,
            PacketDirection::Receive => NetworkDecision::Deny {
                reason: "receive denied".to_string(),
            },
        }
    }
}

struct AllowAll;

impl NetworkPolicy for AllowAll {
    fn evaluate(&self, _context: &PacketContext) -> NetworkDecision {
        NetworkDecision::Allow
    }
}

/// A budget that allows the send and refuses the receive.
struct SendOnlyBudget;

impl PacketBudget for SendOnlyBudget {
    fn consume_packet(
        &mut self,
        _execution_id: ExecutionId,
        operation: PacketOperation,
    ) -> Result<(), kernel_api::KernelError> {
        match operation {
            PacketOperation::Send => Ok(()),
            PacketOperation::Receive => Err(kernel_api::KernelError::ResourceBudgetExhausted {
                resource_type: "PacketCount".to_string(),
                limit: 0,
                usage: 1,
                identity: "test".to_string(),
                operation: "receive".to_string(),
            }),
        }
    }
}

fn packet(n: u8) -> Packet {
    Packet {
        source: Endpoint::new("10.0.0.1", 1000),
        destination: Endpoint::new("10.0.0.2", 2000),
        protocol: PacketProtocol::Udp,
        payload: vec![n],
    }
}

/// The finding: `receive_packet` popped the packet before evaluating the
/// policy and before charging the budget, so a refusal returned `Err` with
/// the packet already gone -- undeliverable for ever. A caller that hits its
/// ceiling does not get "try again later", it gets a missing packet.
/// `send_packet` three functions up has the order right.
#[test]
fn a_refused_receive_leaves_the_packet_where_it_was() {
    fn check<B: PacketBudget>(name: &str, mut service: NetworkService, budget: &mut B) {
        let execution = ExecutionId::new();
        let interface = NetworkInterfaceId::new();
        service
            .send_packet(budget, execution, interface, packet(1))
            .unwrap();
        assert_eq!(service.queued_packets(interface), 1, "{name}: setup");

        let refused = service.receive_packet(budget, execution, interface);
        assert!(refused.is_err(), "{name}: the receive must be refused");
        assert_eq!(
            service.queued_packets(interface),
            1,
            "{name}: the refused packet was dequeued and destroyed"
        );
    }

    check(
        "policy denial",
        NetworkService::new(Box::new(SendOnlyPolicy)),
        &mut NoBudget,
    );
    check(
        "budget exhaustion",
        NetworkService::new(Box::new(AllowAll)),
        &mut SendOnlyBudget,
    );
}

/// `NetworkInterfaceId` is a caller-supplied parameter with no registration
/// behind it, so any id at all created a new queue and neither the depth nor
/// the number of queues had a ceiling -- while every packet was accepted
/// with `Ok(())` and went nowhere.
#[test]
fn the_queues_have_a_ceiling() {
    let mut service = NetworkService::new(Box::new(AllowAll));
    let mut budget = NoBudget;
    let execution = ExecutionId::new();

    let interface = NetworkInterfaceId::new();
    let mut refused = false;
    for n in 0..(NetworkService::MAX_QUEUED_PACKETS * 2) {
        if service
            .send_packet(&mut budget, execution, interface, packet(n as u8))
            .is_err()
        {
            refused = true;
            break;
        }
    }
    assert!(
        refused,
        "one interface accepted an unbounded number of packets"
    );
    assert!(service.queued_packets(interface) <= NetworkService::MAX_QUEUED_PACKETS);

    let mut service = NetworkService::new(Box::new(AllowAll));
    let mut refused = false;
    for n in 0..(NetworkService::MAX_INTERFACES * 2) {
        if service
            .send_packet(
                &mut budget,
                execution,
                NetworkInterfaceId::new(),
                packet(n as u8),
            )
            .is_err()
        {
            refused = true;
            break;
        }
    }
    assert!(
        refused,
        "a caller minting a fresh interface id per send grew the map for ever"
    );
}

/// A ceiling you cannot come back from is worse than the leak it replaced.
///
/// The first version of the interface ceiling never removed a drained
/// queue, so sixty-four ephemeral ids -- each used once and fully read --
/// refused every later interface for the life of the process, with zero
/// packets queued anywhere.
#[test]
fn a_drained_interface_gives_its_slot_back() {
    let mut service = NetworkService::new(Box::new(AllowAll));
    let mut budget = NoBudget;
    let execution = ExecutionId::new();

    for n in 0..(NetworkService::MAX_INTERFACES * 4) {
        let interface = NetworkInterfaceId::new();
        service
            .send_packet(&mut budget, execution, interface, packet(n as u8))
            .unwrap_or_else(|err| {
                panic!("interface {n} refused with {err:?} though every earlier one was drained")
            });
        service
            .receive_packet(&mut budget, execution, interface)
            .unwrap()
            .expect("the packet just sent");
    }
}

/// A send the budget refuses must not consume an interface slot either.
#[test]
fn a_refused_send_does_not_claim_an_interface() {
    struct NeverAllows;
    impl PacketBudget for NeverAllows {
        fn consume_packet(
            &mut self,
            _execution_id: ExecutionId,
            _operation: PacketOperation,
        ) -> Result<(), kernel_api::KernelError> {
            Err(kernel_api::KernelError::ResourceBudgetExhausted {
                resource_type: "PacketCount".to_string(),
                limit: 0,
                usage: 1,
                identity: "test".to_string(),
                operation: "send".to_string(),
            })
        }
    }

    let mut service = NetworkService::new(Box::new(AllowAll));
    let execution = ExecutionId::new();
    for n in 0..(NetworkService::MAX_INTERFACES * 4) {
        let _ = service.send_packet(
            &mut NeverAllows,
            execution,
            NetworkInterfaceId::new(),
            packet(n as u8),
        );
    }

    // Nothing was ever queued, so nothing should have been claimed.
    let mut budget = NoBudget;
    service
        .send_packet(&mut budget, execution, NetworkInterfaceId::new(), packet(0))
        .expect("refused sends claimed the interface table");
}
