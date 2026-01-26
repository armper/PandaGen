//! Kernel API trait and task management types

use crate::{Duration, Instant, KernelError};
use alloc::string::String;
use alloc::vec::Vec;
use core_types::{Cap, ServiceId, TaskId};
use ipc::{ChannelId, MessageEnvelope};
use serde::{Deserialize, Serialize};

/// Descriptor for creating a new task
///
/// Unlike `fork()`, task creation is explicit. The caller must specify:
/// - What code to run (entry point)
/// - What capabilities the task has
/// - Resource limits (future)
///
/// This is construction, not duplication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDescriptor {
    /// Human-readable name for debugging
    pub name: String,
    /// Initial capabilities granted to the task
    pub capabilities: Vec<Cap<()>>, // Type-erased for simplicity
}

impl TaskDescriptor {
    /// Creates a new task descriptor
    pub fn new(name: String) -> Self {
        Self {
            name,
            capabilities: Vec::new(),
        }
    }

    /// Adds a capability to the task descriptor
    pub fn with_capability(mut self, cap: Cap<()>) -> Self {
        self.capabilities.push(cap);
        self
    }
}

/// Handle to a spawned task
///
/// This is returned when a task is created. Unlike Unix PIDs,
/// this is a capability - having the handle grants authority to interact
/// with the task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHandle {
    /// The task's unique identifier
    pub task_id: TaskId,
}

impl TaskHandle {
    /// Creates a new task handle
    pub fn new(task_id: TaskId) -> Self {
        Self { task_id }
    }
}

/// The kernel API trait
///
/// This defines the interface between user-space and kernel.
/// Multiple implementations are possible:
/// - Simulated kernel (for testing)
/// - Real kernel (syscalls)
/// - Remote kernel (for distributed systems)
///
/// # Design Principles
///
/// **Explicit construction**: Tasks are created with specific capabilities,
/// not forked with inherited state.
///
/// **Message passing**: Communication is through typed messages, not
/// shared memory or signals.
///
/// **Simulated time**: Time is explicit and controllable (for testing).
///
/// **Capability transfer**: Authority is explicitly passed, never ambient.
///
/// # Example
///
/// ```
/// use kernel_api::{KernelApi, TaskDescriptor};
///
/// fn spawn_service<K: KernelApi>(kernel: &mut K) -> Result<(), kernel_api::KernelError> {
///     let descriptor = TaskDescriptor::new("my_service".to_string());
///     let handle = kernel.spawn_task(descriptor)?;
///     println!("Spawned task: {:?}", handle.task_id);
///     Ok(())
/// }
/// ```
pub trait KernelApi {
    /// Spawns a new task
    ///
    /// Unlike `fork()`, this creates a fresh task with explicit capabilities.
    /// The task does not inherit ambient authority from the caller.
    ///
    /// # Arguments
    ///
    /// * `descriptor` - Specification of what to spawn and with what capabilities
    ///
    /// # Returns
    ///
    /// A handle to the spawned task, which is itself a capability.
    fn spawn_task(&mut self, descriptor: TaskDescriptor) -> Result<TaskHandle, KernelError>;

    /// Creates a new bidirectional communication channel
    ///
    /// Channels are the primitive for message passing. Unlike Unix pipes:
    /// - They are typed (carry structured messages)
    /// - They are bidirectional
    /// - They can transfer capabilities
    ///
    /// # Returns
    ///
    /// A channel ID that can be used with send/receive operations.
    fn create_channel(&mut self) -> Result<ChannelId, KernelError>;

    /// Sends a message through a channel
    ///
    /// This is non-blocking. If the channel is full, it returns an error.
    ///
    /// # Arguments
    ///
    /// * `channel` - The channel to send through
    /// * `message` - The message to send
    fn send_message(
        &mut self,
        channel: ChannelId,
        message: MessageEnvelope,
    ) -> Result<(), KernelError>;

    /// Receives a message from a channel
    ///
    /// This blocks until a message is available or a timeout occurs.
    ///
    /// # Arguments
    ///
    /// * `channel` - The channel to receive from
    /// * `timeout` - Maximum time to wait (None = wait forever)
    ///
    /// # Returns
    ///
    /// The received message, or an error if timeout or channel closed.
    fn receive_message(
        &mut self,
        channel: ChannelId,
        timeout: Option<Duration>,
    ) -> Result<MessageEnvelope, KernelError>;

    /// Returns the current time
    ///
    /// Unlike POSIX `time()`, this is explicit. In simulated kernels,
    /// time can be controlled for deterministic testing.
    fn now(&self) -> Instant;

    /// Sleeps for the specified duration
    ///
    /// This yields control to the kernel. In simulated kernels,
    /// this can advance simulated time without real delay.
    fn sleep(&mut self, duration: Duration) -> Result<(), KernelError>;

    /// Grants a capability to another task
    ///
    /// This is how authority is explicitly transferred. The caller must
    /// have the capability to grant.
    ///
    /// # Arguments
    ///
    /// * `task` - The task to grant the capability to
    /// * `capability` - The capability to grant
    ///
    /// # Security Note
    ///
    /// In a real system, this would validate that the caller has authority
    /// to grant the capability.
    fn grant_capability(&mut self, task: TaskId, capability: Cap<()>) -> Result<(), KernelError>;

    /// Registers a service by ID
    ///
    /// This makes a service discoverable. Unlike Unix services (which use
    /// filesystem paths or ports), services are identified by unique IDs
    /// and accessed through capabilities.
    ///
    /// # Arguments
    ///
    /// * `service_id` - Unique identifier for the service
    /// * `channel` - Channel for communicating with the service
    fn register_service(
        &mut self,
        service_id: ServiceId,
        channel: ChannelId,
    ) -> Result<(), KernelError>;

    /// Looks up a service by ID
    ///
    /// Returns a capability to communicate with the service.
    /// This is how services are discovered without global namespaces.
    fn lookup_service(&self, service_id: ServiceId) -> Result<ChannelId, KernelError>;
}
