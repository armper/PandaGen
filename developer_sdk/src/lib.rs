//! Developer SDK: tracing, replay, and remote debugger host.

use ipc::{MessageEnvelope, MessagePayload, SchemaVersion};
use kernel_api::{KernelApi, KernelError};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use core_types::ServiceId;
use ipc::ChannelId;

const DEBUG_TRACE_ACTION: &str = "debug.trace";
const DEBUG_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceEvent {
    pub timestamp_ns: u64,
    pub category: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceLog {
    pub events: Vec<TraceEvent>,
}

impl TraceLog {
    pub fn record(&mut self, event: TraceEvent) {
        self.events.push(event);
    }
}

#[derive(Debug)]
pub struct TraceRecorder {
    log: TraceLog,
}

impl TraceRecorder {
    pub fn new() -> Self {
        Self { log: TraceLog::default() }
    }

    pub fn record(&mut self, event: TraceEvent) {
        self.log.record(event);
    }

    pub fn log(&self) -> &TraceLog {
        &self.log
    }
}

#[derive(Debug, Clone)]
pub struct ReplaySession {
    log: TraceLog,
    cursor: usize,
}

impl ReplaySession {
    pub fn new(log: TraceLog) -> Self {
        Self { log, cursor: 0 }
    }

    pub fn next(&mut self) -> Option<TraceEvent> {
        if self.cursor >= self.log.events.len() {
            return None;
        }
        let event = self.log.events[self.cursor].clone();
        self.cursor += 1;
        Some(event)
    }
}

#[derive(Debug, Error)]
pub enum DebuggerError {
    #[error("Kernel error: {0}")]
    Kernel(String),

    #[error("Encoding error: {0}")]
    Encode(String),
}

impl From<KernelError> for DebuggerError {
    fn from(error: KernelError) -> Self {
        DebuggerError::Kernel(error.to_string())
    }
}

pub trait TraceSink {
    fn send(&mut self, event: TraceEvent) -> Result<(), DebuggerError>;
}

pub struct DebuggerHost {
    sinks: Vec<Box<dyn TraceSink>>,
}

impl DebuggerHost {
    pub fn new() -> Self {
        Self { sinks: Vec::new() }
    }

    pub fn add_sink(&mut self, sink: Box<dyn TraceSink>) {
        self.sinks.push(sink);
    }

    pub fn publish(&mut self, event: TraceEvent) -> Result<(), DebuggerError> {
        for sink in &mut self.sinks {
            sink.send(event.clone())?;
        }
        Ok(())
    }
}

pub struct IpcTraceSink<K: KernelApi> {
    kernel: K,
    channel: ChannelId,
    destination: ServiceId,
}

impl<K: KernelApi> IpcTraceSink<K> {
    pub fn new(kernel: K, channel: ChannelId, destination: ServiceId) -> Self {
        Self { kernel, channel, destination }
    }
}

impl<K: KernelApi> TraceSink for IpcTraceSink<K> {
    fn send(&mut self, event: TraceEvent) -> Result<(), DebuggerError> {
        let payload = MessagePayload::new(&event).map_err(|err| DebuggerError::Encode(err.to_string()))?;
        let message = MessageEnvelope::new(
            self.destination,
            DEBUG_TRACE_ACTION.to_string(),
            DEBUG_SCHEMA_VERSION,
            payload,
        );
        self.kernel.send_message(self.channel, message)?;
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryTraceSink {
    pub events: Vec<TraceEvent>,
}

impl TraceSink for InMemoryTraceSink {
    fn send(&mut self, event: TraceEvent) -> Result<(), DebuggerError> {
        self.events.push(event);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockKernel {
        sent: Arc<Mutex<Vec<MessageEnvelope>>>,
    }

    impl KernelApi for MockKernel {
        fn spawn_task(&mut self, _descriptor: kernel_api::TaskDescriptor) -> Result<kernel_api::TaskHandle, KernelError> {
            Err(KernelError::SpawnFailed("not supported".to_string()))
        }

        fn create_channel(&mut self) -> Result<ChannelId, KernelError> {
            Err(KernelError::ChannelError("not supported".to_string()))
        }

        fn send_message(&mut self, _channel: ChannelId, message: MessageEnvelope) -> Result<(), KernelError> {
            let mut sent = self.sent.lock().expect("lock sent");
            sent.push(message);
            Ok(())
        }

        fn receive_message(&mut self, _channel: ChannelId, _timeout: Option<kernel_api::Duration>) -> Result<MessageEnvelope, KernelError> {
            Err(KernelError::ReceiveFailed("not supported".to_string()))
        }

        fn now(&self) -> kernel_api::Instant {
            kernel_api::Instant::from_nanos(0)
        }

        fn sleep(&mut self, _duration: kernel_api::Duration) -> Result<(), KernelError> {
            Ok(())
        }

        fn grant_capability(&mut self, _task: core_types::TaskId, _capability: core_types::Cap<()>) -> Result<(), KernelError> {
            Ok(())
        }

        fn register_service(&mut self, _service_id: ServiceId, _channel: ChannelId) -> Result<(), KernelError> {
            Ok(())
        }

        fn lookup_service(&self, _service_id: ServiceId) -> Result<ChannelId, KernelError> {
            Err(KernelError::ServiceNotFound("not supported".to_string()))
        }
    }

    #[test]
    fn test_trace_recorder_and_replay() {
        let mut recorder = TraceRecorder::new();
        recorder.record(TraceEvent {
            timestamp_ns: 1,
            category: "input".to_string(),
            message: "key press".to_string(),
        });

        let mut replay = ReplaySession::new(recorder.log().clone());
        let event = replay.next().unwrap();
        assert_eq!(event.category, "input");
        assert!(replay.next().is_none());
    }

    #[test]
    fn test_ipc_trace_sink() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let kernel = MockKernel { sent: sent.clone() };
        let channel = ChannelId::new();
        let destination = ServiceId::new();
        let mut sink = IpcTraceSink::new(kernel, channel, destination);

        sink.send(TraceEvent {
            timestamp_ns: 2,
            category: "debug".to_string(),
            message: "trace".to_string(),
        })
        .unwrap();

        let sent = sent.lock().expect("lock sent");
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].action, DEBUG_TRACE_ACTION.to_string());
    }
}
