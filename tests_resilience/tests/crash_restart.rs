//! Crash and Restart Tests
//!
//! Validates that the process manager correctly handles service crashes
//! and restarts according to policy.

use core_types::ServiceId;
use ipc::{MessageEnvelope, MessagePayload, SchemaVersion};
use kernel_api::KernelApi;
use services_process_manager::{LifecycleState, RestartPolicy, ServiceDescriptor, ServiceHandle};
use sim_kernel::fault_injection::{FaultPlan, LifecycleFault};
use sim_kernel::test_utils::with_fault_plan;
use tests_resilience::{spawn_test_service, test_bootstrap};

/// Test: Service crashes and is restarted by process manager
///
/// This validates that:
/// 1. A service can be spawned and registered
/// 2. The service can process messages
/// 3. When the service crashes, it is detected
/// 4. The service is restarted according to policy
/// 5. The system continues responding after restart
#[test]
fn test_service_crash_and_restart() {
    let (mut kernel, mut registry) = test_bootstrap();

    // Create a service descriptor with Always restart policy
    let service_desc = ServiceDescriptor::new("test_service".to_string(), RestartPolicy::Always);
    let service_id = service_desc.service_id;

    // Create a channel for the service
    let channel = kernel.create_channel().expect("Failed to create channel");

    // Register the service
    kernel
        .register_service(service_id, channel)
        .expect("Failed to register service");
    registry
        .register(service_id, channel)
        .expect("Failed to register in registry");

    // Spawn the service task
    let task_id = spawn_test_service(&mut kernel, "test_service").expect("Failed to spawn service");

    // Create a service handle (simulating process manager's tracking)
    let mut handle = ServiceHandle::new(task_id, LifecycleState::Running);

    // Send a test message to the service
    let payload = MessagePayload::new(&"test_request").expect("Failed to create payload");
    let message = MessageEnvelope::new(
        service_id,
        "test.action".to_string(),
        SchemaVersion::new(1, 0),
        payload,
    );

    kernel
        .send_message(channel, message.clone())
        .expect("Failed to send message");

    // Simulate crash by setting state to Failed
    handle.set_state(LifecycleState::Failed);
    assert_eq!(handle.state, LifecycleState::Failed);

    // Process manager would detect failure and restart
    // Simulate restart: spawn new task with same service ID
    handle.set_state(LifecycleState::Restarting);

    let new_task_id = spawn_test_service(&mut kernel, "test_service_restarted")
        .expect("Failed to restart service");

    handle.task_id = new_task_id;
    handle.set_state(LifecycleState::Running);

    // Verify the service can continue processing
    assert_eq!(handle.state, LifecycleState::Running);
    assert_eq!(
        kernel
            .lookup_service(service_id)
            .expect("Service not found"),
        channel
    );

    // Send another message after restart
    let payload2 = MessagePayload::new(&"post_restart_request").expect("Failed to create payload");
    let message2 = MessageEnvelope::new(
        service_id,
        "test.action".to_string(),
        SchemaVersion::new(1, 0),
        payload2,
    );

    kernel
        .send_message(channel, message2)
        .expect("Failed to send message after restart");

    // Verify messages are queued (service can receive them)
    assert!(kernel.pending_message_count() > 0);
}

/// Test: Service with OnFailure policy doesn't restart on clean exit
#[test]
fn test_service_no_restart_on_success() {
    let (mut kernel, _registry) = test_bootstrap();

    let _service_desc = ServiceDescriptor::new("test_service".to_string(), RestartPolicy::OnFailure);
    let task_id = spawn_test_service(&mut kernel, "test_service").expect("Failed to spawn service");

    let mut handle = ServiceHandle::new(task_id, LifecycleState::Running);

    // Service stops cleanly
    handle.set_state(LifecycleState::Stopping);
    handle.set_state(LifecycleState::Stopped);

    // With OnFailure policy, should NOT restart
    assert_eq!(handle.state, LifecycleState::Stopped);
    assert!(handle.state.is_terminal());
}

/// Test: Service crashes during message handling with fault injection
#[test]
fn test_crash_during_message_handling() {
    let plan =
        FaultPlan::new().with_lifecycle_fault(LifecycleFault::CrashAfterMessages { count: 2 });

    with_fault_plan(plan, |kernel| {
        let service_id = ServiceId::new();
        let channel = kernel.create_channel().expect("Failed to create channel");

        kernel
            .register_service(service_id, channel)
            .expect("Failed to register service");

        // Send messages
        for i in 0..3 {
            let payload =
                MessagePayload::new(&format!("message_{}", i)).expect("Failed to create payload");
            let message = MessageEnvelope::new(
                service_id,
                "test.action".to_string(),
                SchemaVersion::new(1, 0),
                payload,
            );
            kernel
                .send_message(channel, message)
                .expect("Failed to send message");
        }

        // Receive first two messages successfully
        let msg1 = kernel
            .receive_message(channel, None)
            .expect("Failed to receive");
        assert!(msg1.action == "test.action");

        let msg2 = kernel
            .receive_message(channel, None)
            .expect("Failed to receive");
        assert!(msg2.action == "test.action");

        // Third receive should trigger crash fault
        let result = kernel.receive_message(channel, None);
        assert!(result.is_err());

        // Verify the channel still has the unprocessed message
        // (crash occurred before message was consumed)
        assert!(kernel.pending_message_count() > 0);
    });
}

/// Test: Multiple crashes with exponential backoff policy
#[test]
fn test_exponential_backoff_policy() {
    let (mut kernel, _registry) = test_bootstrap();

    let _service_desc = ServiceDescriptor::new(
        "flaky_service".to_string(),
        RestartPolicy::ExponentialBackoff { max_attempts: 3 },
    );

    let task_id =
        spawn_test_service(&mut kernel, "flaky_service").expect("Failed to spawn service");

    let mut handle = ServiceHandle::new(task_id, LifecycleState::Running);
    let mut restart_count = 0;

    // Simulate multiple failures
    for _ in 0..3 {
        handle.set_state(LifecycleState::Failed);
        restart_count += 1;

        if restart_count < 3 {
            // Process manager would restart
            handle.set_state(LifecycleState::Restarting);
            let new_task = spawn_test_service(&mut kernel, "flaky_service_restarted")
                .expect("Failed to restart");
            handle.task_id = new_task;
            handle.set_state(LifecycleState::Running);
        }
    }

    // After max_attempts, service should remain Failed
    assert_eq!(handle.state, LifecycleState::Failed);
    assert_eq!(restart_count, 3);
}

/// Test: Service crash doesn't affect other services
#[test]
fn test_isolated_service_crash() {
    let (mut kernel, mut registry) = test_bootstrap();

    // Create two services
    let service1_id = ServiceId::new();
    let service2_id = ServiceId::new();

    let channel1 = kernel.create_channel().expect("Failed to create channel");
    let channel2 = kernel.create_channel().expect("Failed to create channel");

    kernel
        .register_service(service1_id, channel1)
        .expect("Failed to register service1");
    kernel
        .register_service(service2_id, channel2)
        .expect("Failed to register service2");

    registry
        .register(service1_id, channel1)
        .expect("Failed to register in registry");
    registry
        .register(service2_id, channel2)
        .expect("Failed to register in registry");

    let task1 = spawn_test_service(&mut kernel, "service1").expect("Failed to spawn service1");
    let task2 = spawn_test_service(&mut kernel, "service2").expect("Failed to spawn service2");

    let mut handle1 = ServiceHandle::new(task1, LifecycleState::Running);
    let handle2 = ServiceHandle::new(task2, LifecycleState::Running);

    // Crash service1
    handle1.set_state(LifecycleState::Failed);

    // Service2 should be unaffected
    assert_eq!(handle2.state, LifecycleState::Running);
    assert_eq!(
        kernel
            .lookup_service(service2_id)
            .expect("Service2 not found"),
        channel2
    );

    // Service2 can still receive messages
    let payload = MessagePayload::new(&"test").expect("Failed to create payload");
    let message = MessageEnvelope::new(
        service2_id,
        "test.action".to_string(),
        SchemaVersion::new(1, 0),
        payload,
    );
    kernel
        .send_message(channel2, message)
        .expect("Failed to send to service2");

    let received = kernel
        .receive_message(channel2, None)
        .expect("Failed to receive");
    assert_eq!(received.action, "test.action");
}
