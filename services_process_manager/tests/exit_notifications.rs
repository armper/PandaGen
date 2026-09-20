//! What a process manager owes a notification it is handed.

use core_types::TaskId;
use identity::{ExecutionId, ExitNotification, ExitReason};
use services_process_manager::{
    ExitNotificationSource, ProcessManager, RestartPolicy, ServiceDescriptor,
};
use sim_kernel::SimulatedKernel;

struct Source(Vec<ExitNotification>);

impl ExitNotificationSource for Source {
    fn drain_exit_notifications(&mut self) -> Vec<ExitNotification> {
        std::mem::take(&mut self.0)
    }
}

fn note(task_id: TaskId) -> ExitNotification {
    ExitNotification {
        execution_id: ExecutionId::new(),
        task_id: Some(task_id),
        reason: ExitReason::Failure {
            error: "boom".to_string(),
        },
        terminated_at_nanos: 0,
    }
}

/// `task_to_service` never forgot a dead generation, so a duplicated or
/// delayed notification for a task that died two restarts ago resolved to
/// the service, marked it Failed, and restarted a healthy task.
#[test]
fn a_stale_exit_notification_does_not_restart_a_healthy_service() {
    let mut kernel = SimulatedKernel::new();
    let mut manager = ProcessManager::new();
    let descriptor = ServiceDescriptor::new("svc".to_string(), RestartPolicy::OnFailure);
    let first = manager
        .start_service(&mut kernel, descriptor.clone())
        .unwrap();

    manager
        .handle_exits(&mut kernel, &mut Source(vec![note(first.task_id)]))
        .unwrap();
    let second = manager
        .service_handle(descriptor.service_id)
        .unwrap()
        .clone();
    assert_ne!(second.task_id, first.task_id, "sanity: it restarted");

    // The same notification again -- a replay, or a delayed duplicate.
    manager
        .handle_exits(&mut kernel, &mut Source(vec![note(first.task_id)]))
        .unwrap();
    let third = manager.service_handle(descriptor.service_id).unwrap();
    assert_eq!(
        third.task_id, second.task_id,
        "a notification for a task that died two generations ago killed the live one"
    );
}

/// `restart_service` replaced the handle with a fresh `ServiceHandle::new`,
/// whose `restart_count` is zero -- and that is the number `status_summary`
/// prints. A service that had crashlooped fifty times reported none.
#[test]
fn the_handle_reports_the_restarts_that_happened() {
    let mut kernel = SimulatedKernel::new();
    let mut manager = ProcessManager::new();
    let descriptor = ServiceDescriptor::new("svc".to_string(), RestartPolicy::Always);

    let mut task = manager
        .start_service(&mut kernel, descriptor.clone())
        .unwrap()
        .task_id;
    for expected in 1..=3u32 {
        manager
            .handle_exits(&mut kernel, &mut Source(vec![note(task)]))
            .unwrap();
        let handle = manager.service_handle(descriptor.service_id).unwrap();
        assert_eq!(
            handle.restart_count, expected,
            "after {expected} restarts the handle says {}",
            handle.restart_count
        );
        task = handle.task_id;
    }
}

/// `drain_exit_notifications` consumes the whole batch, so abandoning the
/// loop on the first refused restart threw away every notification behind
/// it: those services stayed recorded as Running with no notification left
/// to tell anyone otherwise.
///
/// Needs a kernel that refuses to spawn, which `SimulatedKernel` will not
/// do, so this carries its own.
#[test]
fn one_refused_restart_does_not_discard_the_rest_of_the_batch() {
    use ipc::{ChannelId, MessageEnvelope};
    use kernel_api::{Duration, Instant, KernelApi, KernelError, TaskDescriptor, TaskHandle};

    /// Spawns once for each `start_service`, then refuses everything.
    struct OutOfSlots {
        remaining: usize,
    }

    impl KernelApi for OutOfSlots {
        fn spawn_task(&mut self, _descriptor: TaskDescriptor) -> Result<TaskHandle, KernelError> {
            if self.remaining == 0 {
                return Err(KernelError::SpawnFailed("no task slots".to_string()));
            }
            self.remaining -= 1;
            Ok(TaskHandle {
                task_id: TaskId::new(),
            })
        }
        fn create_channel(&mut self) -> Result<ChannelId, KernelError> {
            Ok(ChannelId::new())
        }
        fn send_message(
            &mut self,
            _channel: ChannelId,
            _message: MessageEnvelope,
        ) -> Result<(), KernelError> {
            Ok(())
        }
        fn receive_message(
            &mut self,
            _channel: ChannelId,
            _timeout: Option<Duration>,
        ) -> Result<MessageEnvelope, KernelError> {
            Err(KernelError::Timeout)
        }
        fn now(&self) -> Instant {
            Instant::from_nanos(0)
        }
        fn sleep(&mut self, _duration: Duration) -> Result<(), KernelError> {
            Ok(())
        }
        fn grant_capability(
            &mut self,
            _task: TaskId,
            _capability: core_types::Cap<()>,
        ) -> Result<(), KernelError> {
            Ok(())
        }
        fn register_service(
            &mut self,
            _service_id: core_types::ServiceId,
            _channel: ChannelId,
        ) -> Result<(), KernelError> {
            Ok(())
        }
        fn lookup_service(
            &self,
            _service_id: core_types::ServiceId,
        ) -> Result<ChannelId, KernelError> {
            Err(KernelError::ServiceNotFound("none".to_string()))
        }
    }

    let mut kernel = OutOfSlots { remaining: 2 };
    let mut manager = ProcessManager::new();

    let a = ServiceDescriptor::new("a".to_string(), RestartPolicy::Always);
    let b = ServiceDescriptor::new("b".to_string(), RestartPolicy::Always);
    let first = manager.start_service(&mut kernel, a.clone()).unwrap();
    let second = manager.start_service(&mut kernel, b.clone()).unwrap();

    // Both exited, and the kernel has no slots left, so a's restart is
    // refused. b's notification must still be acted on.
    let outcome = manager.handle_exits(
        &mut kernel,
        &mut Source(vec![note(first.task_id), note(second.task_id)]),
    );
    assert!(outcome.is_err(), "sanity: the restart is refused");

    let handle = manager.service_handle(b.service_id).unwrap();
    assert_ne!(
        handle.state,
        services_process_manager::LifecycleState::Running,
        "b's exit notification was consumed by the drain and then thrown away \
         when a's restart was refused; b is still recorded as Running and no \
         notification for it will ever arrive again"
    );
}
