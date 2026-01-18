//! # Simulated Kernel
//!
//! This crate provides a simulated implementation of the kernel API.
//!
//! ## Purpose
//!
//! The simulated kernel allows testing system behavior without hardware:
//! - Runs under `cargo test`
//! - Deterministic (controlled time, no real concurrency)
//! - Fast (no real I/O or context switches)
//! - Inspectable (all state is accessible)
//!
//! ## Philosophy
//!
//! **Testability is a first-class design constraint.**
//!
//! Most OS code is hard to test because it was never designed to be tested.
//! By providing a simulated kernel from day one, we ensure that services
//! and applications can be thoroughly tested in isolation.
//!
//! This is not a "toy" or "mock" - it's a full implementation of the
//! kernel API that happens to run in-process for testing.

use core_types::{Cap, ServiceId, TaskId};
use ipc::{ChannelId, MessageEnvelope};
use kernel_api::{Duration, Instant, KernelApi, KernelError, TaskDescriptor, TaskHandle};
use std::collections::{HashMap, VecDeque};

/// Simulated kernel state
///
/// This maintains all the state needed to simulate a kernel.
/// Unlike a real kernel, this state is directly accessible for testing.
pub struct SimulatedKernel {
    /// Current simulated time
    current_time: Instant,
    /// Spawned tasks
    tasks: HashMap<TaskId, TaskInfo>,
    /// Message channels
    channels: HashMap<ChannelId, Channel>,
    /// Service registry
    services: HashMap<ServiceId, ChannelId>,
}

#[derive(Debug)]
struct TaskInfo {
    #[allow(dead_code)]
    descriptor: TaskDescriptor,
}

struct Channel {
    /// Messages waiting to be received
    messages: VecDeque<MessageEnvelope>,
}

impl SimulatedKernel {
    /// Creates a new simulated kernel
    pub fn new() -> Self {
        Self {
            current_time: Instant::from_nanos(0),
            tasks: HashMap::new(),
            channels: HashMap::new(),
            services: HashMap::new(),
        }
    }

    /// Advances simulated time
    pub fn advance_time(&mut self, duration: Duration) {
        self.current_time = self.current_time + duration;
    }

    /// Returns the number of spawned tasks
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Returns the number of channels
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Returns the number of registered services
    pub fn service_count(&self) -> usize {
        self.services.len()
    }
}

impl Default for SimulatedKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelApi for SimulatedKernel {
    fn spawn_task(&mut self, descriptor: TaskDescriptor) -> Result<TaskHandle, KernelError> {
        let task_id = TaskId::new();
        let task_info = TaskInfo { descriptor };
        self.tasks.insert(task_id, task_info);
        Ok(TaskHandle::new(task_id))
    }

    fn create_channel(&mut self) -> Result<ChannelId, KernelError> {
        let channel_id = ChannelId::new();
        let channel = Channel {
            messages: VecDeque::new(),
        };
        self.channels.insert(channel_id, channel);
        Ok(channel_id)
    }

    fn send_message(
        &mut self,
        channel: ChannelId,
        message: MessageEnvelope,
    ) -> Result<(), KernelError> {
        let channel = self
            .channels
            .get_mut(&channel)
            .ok_or_else(|| KernelError::ChannelError("Channel not found".to_string()))?;
        channel.messages.push_back(message);
        Ok(())
    }

    fn receive_message(
        &mut self,
        channel: ChannelId,
        _timeout: Option<Duration>,
    ) -> Result<MessageEnvelope, KernelError> {
        let channel = self
            .channels
            .get_mut(&channel)
            .ok_or_else(|| KernelError::ChannelError("Channel not found".to_string()))?;

        channel.messages.pop_front().ok_or(KernelError::Timeout)
    }

    fn now(&self) -> Instant {
        self.current_time
    }

    fn sleep(&mut self, duration: Duration) -> Result<(), KernelError> {
        self.advance_time(duration);
        Ok(())
    }

    fn grant_capability(&mut self, task: TaskId, _capability: Cap<()>) -> Result<(), KernelError> {
        // Verify task exists
        if !self.tasks.contains_key(&task) {
            return Err(KernelError::SendFailed("Task not found".to_string()));
        }
        // In simulation, we just verify the task exists
        // Real implementation would track capabilities
        Ok(())
    }

    fn register_service(
        &mut self,
        service_id: ServiceId,
        channel: ChannelId,
    ) -> Result<(), KernelError> {
        if self.services.contains_key(&service_id) {
            return Err(KernelError::ServiceAlreadyRegistered(
                service_id.to_string(),
            ));
        }
        self.services.insert(service_id, channel);
        Ok(())
    }

    fn lookup_service(&self, service_id: ServiceId) -> Result<ChannelId, KernelError> {
        self.services
            .get(&service_id)
            .copied()
            .ok_or_else(|| KernelError::ServiceNotFound(service_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulated_kernel_creation() {
        let kernel = SimulatedKernel::new();
        assert_eq!(kernel.task_count(), 0);
        assert_eq!(kernel.channel_count(), 0);
    }

    #[test]
    fn test_spawn_task() {
        let mut kernel = SimulatedKernel::new();
        let descriptor = TaskDescriptor::new("test_task".to_string());
        let handle = kernel.spawn_task(descriptor).unwrap();
        assert_eq!(kernel.task_count(), 1);
        assert!(kernel.tasks.contains_key(&handle.task_id));
    }

    #[test]
    fn test_create_channel() {
        let mut kernel = SimulatedKernel::new();
        let channel = kernel.create_channel().unwrap();
        assert_eq!(kernel.channel_count(), 1);
        assert!(kernel.channels.contains_key(&channel));
    }

    #[test]
    fn test_send_receive_message() {
        let mut kernel = SimulatedKernel::new();
        let channel = kernel.create_channel().unwrap();

        let service_id = ServiceId::new();
        let payload = ipc::MessagePayload::new(&"test").unwrap();
        let message = ipc::MessageEnvelope::new(
            service_id,
            "test".to_string(),
            ipc::SchemaVersion::new(1, 0),
            payload,
        );

        kernel.send_message(channel, message.clone()).unwrap();
        let received = kernel.receive_message(channel, None).unwrap();
        assert_eq!(received.id, message.id);
    }

    #[test]
    fn test_time_advancement() {
        let mut kernel = SimulatedKernel::new();
        let initial = kernel.now();
        kernel.advance_time(Duration::from_secs(1));
        let after = kernel.now();
        assert_eq!(after.duration_since(initial), Duration::from_secs(1));
    }

    #[test]
    fn test_sleep() {
        let mut kernel = SimulatedKernel::new();
        let initial = kernel.now();
        kernel.sleep(Duration::from_millis(100)).unwrap();
        let after = kernel.now();
        assert_eq!(after.duration_since(initial), Duration::from_millis(100));
    }

    #[test]
    fn test_service_registration() {
        let mut kernel = SimulatedKernel::new();
        let service_id = ServiceId::new();
        let channel = kernel.create_channel().unwrap();

        kernel.register_service(service_id, channel).unwrap();
        assert_eq!(kernel.service_count(), 1);

        let looked_up = kernel.lookup_service(service_id).unwrap();
        assert_eq!(looked_up, channel);
    }

    #[test]
    fn test_duplicate_service_registration() {
        let mut kernel = SimulatedKernel::new();
        let service_id = ServiceId::new();
        let channel1 = kernel.create_channel().unwrap();
        let channel2 = kernel.create_channel().unwrap();

        kernel.register_service(service_id, channel1).unwrap();
        let result = kernel.register_service(service_id, channel2);
        assert!(matches!(
            result,
            Err(KernelError::ServiceAlreadyRegistered(_))
        ));
    }

    #[test]
    fn test_service_not_found() {
        let kernel = SimulatedKernel::new();
        let service_id = ServiceId::new();
        let result = kernel.lookup_service(service_id);
        assert!(matches!(result, Err(KernelError::ServiceNotFound(_))));
    }
}
