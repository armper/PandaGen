//! Remote UI host for snapshot streaming.

use core_types::ServiceId;
use ipc::ChannelId;
use ipc::{MessageEnvelope, MessagePayload, SchemaVersion};
use kernel_api::{KernelApi, KernelError};
use serde::{Deserialize, Serialize};
use services_workspace_manager::WorkspaceRenderSnapshot;
use std::io::Write;
use thiserror::Error;

const REMOTE_UI_ACTION: &str = "ui.snapshot";
const REMOTE_UI_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

/// Snapshot frame streamed to remote UI clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSnapshotFrame {
    pub revision: u64,
    pub timestamp_ns: u64,
    pub snapshot: WorkspaceRenderSnapshot,
}

/// Errors for remote UI streaming.
#[derive(Debug, Error)]
pub enum RemoteUiError {
    #[error("Kernel error: {0}")]
    Kernel(String),

    #[error("Serialization error: {0}")]
    Encode(String),

    #[error("I/O error: {0}")]
    Io(String),
}

impl From<KernelError> for RemoteUiError {
    fn from(error: KernelError) -> Self {
        RemoteUiError::Kernel(error.to_string())
    }
}

/// Snapshot sink abstraction.
pub trait SnapshotSink {
    fn send(&mut self, frame: RemoteSnapshotFrame) -> Result<(), RemoteUiError>;
}

/// Remote UI host that fans out snapshots to sinks.
pub struct RemoteUiHost {
    revision: u64,
    sinks: Vec<Box<dyn SnapshotSink>>,
}

impl RemoteUiHost {
    pub fn new() -> Self {
        Self {
            revision: 0,
            sinks: Vec::new(),
        }
    }

    pub fn add_sink(&mut self, sink: Box<dyn SnapshotSink>) {
        self.sinks.push(sink);
    }

    pub fn push_snapshot(
        &mut self,
        snapshot: WorkspaceRenderSnapshot,
        timestamp_ns: u64,
    ) -> Result<RemoteSnapshotFrame, RemoteUiError> {
        self.revision += 1;
        let frame = RemoteSnapshotFrame {
            revision: self.revision,
            timestamp_ns,
            snapshot,
        };

        for sink in &mut self.sinks {
            sink.send(frame.clone())?;
        }

        Ok(frame)
    }
}

/// IPC sink for remote UI snapshots.
pub struct IpcSnapshotSink<K: KernelApi> {
    kernel: K,
    channel: ChannelId,
    destination: ServiceId,
}

impl<K: KernelApi> IpcSnapshotSink<K> {
    pub fn new(kernel: K, channel: ChannelId, destination: ServiceId) -> Self {
        Self {
            kernel,
            channel,
            destination,
        }
    }
}

impl<K: KernelApi> SnapshotSink for IpcSnapshotSink<K> {
    fn send(&mut self, frame: RemoteSnapshotFrame) -> Result<(), RemoteUiError> {
        let payload =
            MessagePayload::new(&frame).map_err(|err| RemoteUiError::Encode(err.to_string()))?;
        let message = MessageEnvelope::new(
            self.destination,
            REMOTE_UI_ACTION.to_string(),
            REMOTE_UI_SCHEMA,
            payload,
        );
        self.kernel.send_message(self.channel, message)?;
        Ok(())
    }
}

/// JSON-line sink for network transports.
pub struct JsonLineSink<W: Write> {
    writer: W,
}

impl<W: Write> JsonLineSink<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write> SnapshotSink for JsonLineSink<W> {
    fn send(&mut self, frame: RemoteSnapshotFrame) -> Result<(), RemoteUiError> {
        serde_json::to_writer(&mut self.writer, &frame)
            .map_err(|err| RemoteUiError::Encode(err.to_string()))?;
        self.writer
            .write_all(b"\n")
            .map_err(|err| RemoteUiError::Io(err.to_string()))?;
        Ok(())
    }
}

/// In-memory sink for tests.
#[derive(Default)]
pub struct InMemorySink {
    pub frames: Vec<RemoteSnapshotFrame>,
}

impl SnapshotSink for InMemorySink {
    fn send(&mut self, frame: RemoteSnapshotFrame) -> Result<(), RemoteUiError> {
        self.frames.push(frame);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockKernel {
        sent: Arc<Mutex<Vec<(ChannelId, MessageEnvelope)>>>,
    }

    impl KernelApi for MockKernel {
        fn spawn_task(
            &mut self,
            _descriptor: kernel_api::TaskDescriptor,
        ) -> Result<kernel_api::TaskHandle, KernelError> {
            Err(KernelError::SpawnFailed("not supported".to_string()))
        }

        fn create_channel(&mut self) -> Result<ChannelId, KernelError> {
            Err(KernelError::ChannelError("not supported".to_string()))
        }

        fn send_message(
            &mut self,
            channel: ChannelId,
            message: MessageEnvelope,
        ) -> Result<(), KernelError> {
            let mut sent = self.sent.lock().expect("lock sent");
            sent.push((channel, message));
            Ok(())
        }

        fn receive_message(
            &mut self,
            _channel: ChannelId,
            _timeout: Option<kernel_api::Duration>,
        ) -> Result<MessageEnvelope, KernelError> {
            Err(KernelError::ReceiveFailed("not supported".to_string()))
        }

        fn now(&self) -> kernel_api::Instant {
            kernel_api::Instant::from_nanos(0)
        }

        fn sleep(&mut self, _duration: kernel_api::Duration) -> Result<(), KernelError> {
            Ok(())
        }

        fn grant_capability(
            &mut self,
            _task: core_types::TaskId,
            _capability: core_types::Cap<()>,
        ) -> Result<(), KernelError> {
            Ok(())
        }

        fn register_service(
            &mut self,
            _service_id: ServiceId,
            _channel: ChannelId,
        ) -> Result<(), KernelError> {
            Ok(())
        }

        fn lookup_service(&self, _service_id: ServiceId) -> Result<ChannelId, KernelError> {
            Err(KernelError::ServiceNotFound("not supported".to_string()))
        }
    }

    #[test]
    fn test_remote_ui_host_revision_increments() {
        let mut host = RemoteUiHost::new();
        let sink = InMemorySink::default();
        host.add_sink(Box::new(sink));

        let snapshot = WorkspaceRenderSnapshot {
            focused_component: None,
            main_view: None,
            status_view: None,
            component_count: 0,
            running_count: 0,
            #[cfg(debug_assertions)]
            debug_info: None,
        };

        let frame1 = host.push_snapshot(snapshot.clone(), 10).unwrap();
        let frame2 = host.push_snapshot(snapshot, 11).unwrap();

        assert_eq!(frame1.revision, 1);
        assert_eq!(frame2.revision, 2);
    }

    #[test]
    fn test_ipc_snapshot_sink_sends_message() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let kernel = MockKernel { sent: sent.clone() };
        let channel = ChannelId::new();
        let destination = ServiceId::new();

        let mut sink = IpcSnapshotSink::new(kernel, channel, destination);

        let frame = RemoteSnapshotFrame {
            revision: 1,
            timestamp_ns: 5,
            snapshot: WorkspaceRenderSnapshot {
                focused_component: None,
                main_view: None,
                status_view: None,
                component_count: 0,
                running_count: 0,
                #[cfg(debug_assertions)]
                debug_info: None,
            },
        };

        sink.send(frame).unwrap();

        let sent = sent.lock().expect("lock sent");
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, channel);
        assert_eq!(sent[0].1.action, REMOTE_UI_ACTION.to_string());
    }
}
