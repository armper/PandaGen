//! Syscall boundary implemented as typed IPC messages.
//!
//! This module provides a message-based adapter for KernelApi. Requests are
//! serialized into MessageEnvelope payloads, routed via a transport, and
//! decoded back into typed responses.

use alloc::format;
use alloc::string::{String, ToString};
use core::cell::RefCell;
use core::fmt;
use crate::{Duration, Instant, KernelApi, KernelError, TaskDescriptor, TaskHandle};
use core_types::{Cap, ServiceId, TaskId};
use ipc::{ChannelId, MessageEnvelope, MessageId, MessagePayload, SchemaVersion};
use serde::{Deserialize, Serialize};

const SYSCALL_REQUEST_ACTION: &str = "kernel.syscall.request";
const SYSCALL_RESPONSE_ACTION: &str = "kernel.syscall.response";
const SYSCALL_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// Syscall request wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyscallRequest {
    pub request_id: MessageId,
    pub payload: SyscallRequestPayload,
}

/// Typed syscall request payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyscallRequestPayload {
    SpawnTask {
        descriptor: TaskDescriptor,
    },
    CreateChannel,
    SendMessage {
        channel: ChannelId,
        message: MessageEnvelope,
    },
    ReceiveMessage {
        channel: ChannelId,
        timeout: Option<Duration>,
    },
    Now,
    Sleep {
        duration: Duration,
    },
    GrantCapability {
        task: TaskId,
        capability: Cap<()>,
    },
    RegisterService {
        service_id: ServiceId,
        channel: ChannelId,
    },
    LookupService {
        service_id: ServiceId,
    },
}

/// Syscall response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyscallResponse {
    pub request_id: MessageId,
    pub payload: SyscallResponsePayload,
}

/// Typed syscall response payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyscallResponsePayload {
    SpawnTask(SyscallResult<TaskHandle>),
    CreateChannel(SyscallResult<ChannelId>),
    SendMessage(SyscallResult<()>),
    ReceiveMessage(SyscallResult<MessageEnvelope>),
    Now(SyscallResult<Instant>),
    Sleep(SyscallResult<()>),
    GrantCapability(SyscallResult<()>),
    RegisterService(SyscallResult<()>),
    LookupService(SyscallResult<ChannelId>),
}

/// Result type used in syscall responses.
pub type SyscallResult<T> = Result<T, SyscallError>;

/// Serializable syscall error details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyscallError {
    pub kind: SyscallErrorKind,
    pub message: String,
    pub resource_type: Option<String>,
    pub limit: Option<u64>,
    pub usage: Option<u64>,
    pub identity: Option<String>,
    pub operation: Option<String>,
}

impl SyscallError {
    fn new(kind: SyscallErrorKind, message: String) -> Self {
        Self {
            kind,
            message,
            resource_type: None,
            limit: None,
            usage: None,
            identity: None,
            operation: None,
        }
    }
}

/// Error kinds aligned with KernelError variants.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyscallErrorKind {
    SpawnFailed,
    ChannelError,
    SendFailed,
    ReceiveFailed,
    Timeout,
    ServiceNotFound,
    ServiceAlreadyRegistered,
    InsufficientAuthority,
    InvalidCapability,
    ResourceExhausted,
    ResourceBudgetExceeded,
    ResourceBudgetExhausted,
    Unknown,
}

impl From<KernelError> for SyscallError {
    fn from(error: KernelError) -> Self {
        match error {
            KernelError::SpawnFailed(message) => {
                SyscallError::new(SyscallErrorKind::SpawnFailed, message)
            }
            KernelError::ChannelError(message) => {
                SyscallError::new(SyscallErrorKind::ChannelError, message)
            }
            KernelError::SendFailed(message) => {
                SyscallError::new(SyscallErrorKind::SendFailed, message)
            }
            KernelError::ReceiveFailed(message) => {
                SyscallError::new(SyscallErrorKind::ReceiveFailed, message)
            }
            KernelError::Timeout => {
                SyscallError::new(SyscallErrorKind::Timeout, "Operation timed out".to_string())
            }
            KernelError::ServiceNotFound(message) => {
                SyscallError::new(SyscallErrorKind::ServiceNotFound, message)
            }
            KernelError::ServiceAlreadyRegistered(message) => {
                SyscallError::new(SyscallErrorKind::ServiceAlreadyRegistered, message)
            }
            KernelError::InsufficientAuthority(message) => {
                SyscallError::new(SyscallErrorKind::InsufficientAuthority, message)
            }
            KernelError::InvalidCapability(message) => {
                SyscallError::new(SyscallErrorKind::InvalidCapability, message)
            }
            KernelError::ResourceExhausted(message) => {
                SyscallError::new(SyscallErrorKind::ResourceExhausted, message)
            }
            KernelError::ResourceBudgetExceeded {
                resource_type,
                limit,
                usage,
                identity,
                operation,
            } => SyscallError {
                kind: SyscallErrorKind::ResourceBudgetExceeded,
                message: "Resource budget exceeded".to_string(),
                resource_type: Some(resource_type),
                limit: Some(limit),
                usage: Some(usage),
                identity: Some(identity),
                operation: Some(operation),
            },
            KernelError::ResourceBudgetExhausted {
                resource_type,
                limit,
                usage,
                identity,
                operation,
            } => SyscallError {
                kind: SyscallErrorKind::ResourceBudgetExhausted,
                message: "Resource budget exhausted".to_string(),
                resource_type: Some(resource_type),
                limit: Some(limit),
                usage: Some(usage),
                identity: Some(identity),
                operation: Some(operation),
            },
        }
    }
}

impl From<SyscallError> for KernelError {
    fn from(error: SyscallError) -> Self {
        match error.kind {
            SyscallErrorKind::SpawnFailed => KernelError::SpawnFailed(error.message),
            SyscallErrorKind::ChannelError => KernelError::ChannelError(error.message),
            SyscallErrorKind::SendFailed => KernelError::SendFailed(error.message),
            SyscallErrorKind::ReceiveFailed => KernelError::ReceiveFailed(error.message),
            SyscallErrorKind::Timeout => KernelError::Timeout,
            SyscallErrorKind::ServiceNotFound => KernelError::ServiceNotFound(error.message),
            SyscallErrorKind::ServiceAlreadyRegistered => {
                KernelError::ServiceAlreadyRegistered(error.message)
            }
            SyscallErrorKind::InsufficientAuthority => {
                KernelError::InsufficientAuthority(error.message)
            }
            SyscallErrorKind::InvalidCapability => KernelError::InvalidCapability(error.message),
            SyscallErrorKind::ResourceExhausted => KernelError::ResourceExhausted(error.message),
            SyscallErrorKind::ResourceBudgetExceeded => KernelError::ResourceBudgetExceeded {
                resource_type: error.resource_type.unwrap_or_else(|| "Unknown".to_string()),
                limit: error.limit.unwrap_or(0),
                usage: error.usage.unwrap_or(0),
                identity: error.identity.unwrap_or_else(|| "unknown".to_string()),
                operation: error.operation.unwrap_or_else(|| "unknown".to_string()),
            },
            SyscallErrorKind::ResourceBudgetExhausted => KernelError::ResourceBudgetExhausted {
                resource_type: error.resource_type.unwrap_or_else(|| "Unknown".to_string()),
                limit: error.limit.unwrap_or(0),
                usage: error.usage.unwrap_or(0),
                identity: error.identity.unwrap_or_else(|| "unknown".to_string()),
                operation: error.operation.unwrap_or_else(|| "unknown".to_string()),
            },
            SyscallErrorKind::Unknown => KernelError::ReceiveFailed(error.message),
        }
    }
}

/// Errors when encoding or decoding syscall messages.
#[derive(Debug)]
pub enum SyscallCodecError {
    UnexpectedAction(String),
    SchemaMismatch {
        expected: SchemaVersion,
        actual: SchemaVersion,
    },
    Payload(String),
}

impl fmt::Display for SyscallCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyscallCodecError::UnexpectedAction(action) => {
                write!(f, "Unexpected syscall action: {}", action)
            }
            SyscallCodecError::SchemaMismatch { expected, actual } => {
                write!(f, "Schema mismatch: expected {}, got {}", expected, actual)
            }
            SyscallCodecError::Payload(message) => write!(f, "Payload error: {}", message),
        }
    }
}

/// Encoder/decoder for syscall IPC messages.
pub struct SyscallCodec {
    service_id: ServiceId,
}

impl SyscallCodec {
    /// Creates a codec for the given kernel service ID.
    pub fn new(service_id: ServiceId) -> Self {
        Self { service_id }
    }

    /// Encodes a syscall request into a MessageEnvelope.
    pub fn encode_request(
        &self,
        request: SyscallRequest,
    ) -> Result<MessageEnvelope, SyscallCodecError> {
        let payload = MessagePayload::new(&request)
            .map_err(|err| SyscallCodecError::Payload(err.to_string()))?;
        Ok(MessageEnvelope::new(
            self.service_id,
            SYSCALL_REQUEST_ACTION,
            SYSCALL_SCHEMA_VERSION,
            payload,
        ))
    }

    /// Encodes a syscall response into a MessageEnvelope.
    pub fn encode_response(
        &self,
        response: SyscallResponse,
        correlation_id: MessageId,
    ) -> Result<MessageEnvelope, SyscallCodecError> {
        let payload = MessagePayload::new(&response)
            .map_err(|err| SyscallCodecError::Payload(err.to_string()))?;
        Ok(MessageEnvelope::new(
            self.service_id,
            SYSCALL_RESPONSE_ACTION,
            SYSCALL_SCHEMA_VERSION,
            payload,
        )
        .with_correlation(correlation_id))
    }

    /// Decodes a syscall request from a MessageEnvelope.
    pub fn decode_request(
        &self,
        message: &MessageEnvelope,
    ) -> Result<SyscallRequest, SyscallCodecError> {
        if message.action != SYSCALL_REQUEST_ACTION {
            return Err(SyscallCodecError::UnexpectedAction(message.action.clone()));
        }
        if !message
            .schema_version
            .is_compatible_with(&SYSCALL_SCHEMA_VERSION)
        {
            return Err(SyscallCodecError::SchemaMismatch {
                expected: SYSCALL_SCHEMA_VERSION,
                actual: message.schema_version,
            });
        }
        message
            .payload
            .deserialize::<SyscallRequest>()
            .map_err(|err| SyscallCodecError::Payload(err.to_string()))
    }

    /// Decodes a syscall response from a MessageEnvelope.
    pub fn decode_response(
        &self,
        message: &MessageEnvelope,
    ) -> Result<SyscallResponse, SyscallCodecError> {
        if message.action != SYSCALL_RESPONSE_ACTION {
            return Err(SyscallCodecError::UnexpectedAction(message.action.clone()));
        }
        if !message
            .schema_version
            .is_compatible_with(&SYSCALL_SCHEMA_VERSION)
        {
            return Err(SyscallCodecError::SchemaMismatch {
                expected: SYSCALL_SCHEMA_VERSION,
                actual: message.schema_version,
            });
        }
        message
            .payload
            .deserialize::<SyscallResponse>()
            .map_err(|err| SyscallCodecError::Payload(err.to_string()))
    }

    pub fn service_id(&self) -> ServiceId {
        self.service_id
    }
}

/// Transport abstraction for syscall messages.
pub trait SyscallTransport {
    fn send(&mut self, message: MessageEnvelope) -> Result<(), KernelError>;
    fn receive(&mut self, timeout: Option<Duration>) -> Result<MessageEnvelope, KernelError>;
}

/// Syscall server that executes requests using a KernelApi implementation.
pub struct SyscallServer<K: KernelApi> {
    kernel: K,
    codec: SyscallCodec,
}

impl<K: KernelApi> SyscallServer<K> {
    pub fn new(kernel: K, codec: SyscallCodec) -> Self {
        Self { kernel, codec }
    }

    pub fn handle_message(
        &mut self,
        message: MessageEnvelope,
    ) -> Result<MessageEnvelope, KernelError> {
        let request = self
            .codec
            .decode_request(&message)
            .map_err(|err| KernelError::ReceiveFailed(format!("Syscall decode failed: {}", err)))?;

        let response_payload = match request.payload {
            SyscallRequestPayload::SpawnTask { descriptor } => SyscallResponsePayload::SpawnTask(
                self.kernel
                    .spawn_task(descriptor)
                    .map_err(SyscallError::from),
            ),
            SyscallRequestPayload::CreateChannel => SyscallResponsePayload::CreateChannel(
                self.kernel.create_channel().map_err(SyscallError::from),
            ),
            SyscallRequestPayload::SendMessage { channel, message } => {
                SyscallResponsePayload::SendMessage(
                    self.kernel
                        .send_message(channel, message)
                        .map_err(SyscallError::from),
                )
            }
            SyscallRequestPayload::ReceiveMessage { channel, timeout } => {
                SyscallResponsePayload::ReceiveMessage(
                    self.kernel
                        .receive_message(channel, timeout)
                        .map_err(SyscallError::from),
                )
            }
            SyscallRequestPayload::Now => SyscallResponsePayload::Now(Ok(self.kernel.now())),
            SyscallRequestPayload::Sleep { duration } => SyscallResponsePayload::Sleep(
                self.kernel.sleep(duration).map_err(SyscallError::from),
            ),
            SyscallRequestPayload::GrantCapability { task, capability } => {
                SyscallResponsePayload::GrantCapability(
                    self.kernel
                        .grant_capability(task, capability)
                        .map_err(SyscallError::from),
                )
            }
            SyscallRequestPayload::RegisterService {
                service_id,
                channel,
            } => SyscallResponsePayload::RegisterService(
                self.kernel
                    .register_service(service_id, channel)
                    .map_err(SyscallError::from),
            ),
            SyscallRequestPayload::LookupService { service_id } => {
                SyscallResponsePayload::LookupService(
                    self.kernel
                        .lookup_service(service_id)
                        .map_err(SyscallError::from),
                )
            }
        };

        let response = SyscallResponse {
            request_id: request.request_id,
            payload: response_payload,
        };

        self.codec
            .encode_response(response, message.id)
            .map_err(|err| KernelError::SendFailed(format!("Syscall encode failed: {}", err)))
    }

    pub fn kernel(&self) -> &K {
        &self.kernel
    }

    pub fn kernel_mut(&mut self) -> &mut K {
        &mut self.kernel
    }
}

/// Syscall client implementing KernelApi over a SyscallTransport.
pub struct SyscallClient<T: SyscallTransport> {
    transport: RefCell<T>,
    codec: SyscallCodec,
}

impl<T: SyscallTransport> SyscallClient<T> {
    pub fn new(transport: T, codec: SyscallCodec) -> Self {
        Self {
            transport: RefCell::new(transport),
            codec,
        }
    }

    fn round_trip(&self, payload: SyscallRequestPayload) -> Result<SyscallResponse, KernelError> {
        let request = SyscallRequest {
            request_id: MessageId::new(),
            payload,
        };

        let message = self
            .codec
            .encode_request(request.clone())
            .map_err(|err| KernelError::SendFailed(format!("Syscall encode failed: {}", err)))?;

        let request_id = request.request_id;
        self.transport.borrow_mut().send(message)?;
        let response_message = self.transport.borrow_mut().receive(None)?;
        let response = self
            .codec
            .decode_response(&response_message)
            .map_err(|err| KernelError::ReceiveFailed(format!("Syscall decode failed: {}", err)))?;

        if response.request_id != request_id {
            return Err(KernelError::ReceiveFailed(
                "Syscall response request_id mismatch".to_string(),
            ));
        }

        Ok(response)
    }

    fn extract<TPayload>(
        response: SyscallResponse,
        f: fn(SyscallResponsePayload) -> Option<SyscallResult<TPayload>>,
    ) -> Result<TPayload, KernelError> {
        let payload = f(response.payload).ok_or_else(|| {
            KernelError::ReceiveFailed("Syscall response payload mismatch".to_string())
        })?;
        payload.map_err(KernelError::from)
    }

    pub fn with_transport<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut transport = self.transport.borrow_mut();
        f(&mut transport)
    }
}

impl<T: SyscallTransport> KernelApi for SyscallClient<T> {
    fn spawn_task(&mut self, descriptor: TaskDescriptor) -> Result<TaskHandle, KernelError> {
        let response = self.round_trip(SyscallRequestPayload::SpawnTask { descriptor })?;
        Self::extract(response, |payload| match payload {
            SyscallResponsePayload::SpawnTask(result) => Some(result),
            _ => None,
        })
    }

    fn create_channel(&mut self) -> Result<ChannelId, KernelError> {
        let response = self.round_trip(SyscallRequestPayload::CreateChannel)?;
        Self::extract(response, |payload| match payload {
            SyscallResponsePayload::CreateChannel(result) => Some(result),
            _ => None,
        })
    }

    fn send_message(
        &mut self,
        channel: ChannelId,
        message: MessageEnvelope,
    ) -> Result<(), KernelError> {
        let response = self.round_trip(SyscallRequestPayload::SendMessage { channel, message })?;
        Self::extract(response, |payload| match payload {
            SyscallResponsePayload::SendMessage(result) => Some(result),
            _ => None,
        })
    }

    fn receive_message(
        &mut self,
        channel: ChannelId,
        timeout: Option<Duration>,
    ) -> Result<MessageEnvelope, KernelError> {
        let response =
            self.round_trip(SyscallRequestPayload::ReceiveMessage { channel, timeout })?;
        Self::extract(response, |payload| match payload {
            SyscallResponsePayload::ReceiveMessage(result) => Some(result),
            _ => None,
        })
    }

    fn now(&self) -> Instant {
        let response = self
            .round_trip(SyscallRequestPayload::Now)
            .expect("syscall now failed");
        match response.payload {
            SyscallResponsePayload::Now(result) => result
                .map_err(KernelError::from)
                .expect("syscall now error"),
            _ => panic!("syscall response payload mismatch"),
        }
    }

    fn sleep(&mut self, duration: Duration) -> Result<(), KernelError> {
        let response = self.round_trip(SyscallRequestPayload::Sleep { duration })?;
        Self::extract(response, |payload| match payload {
            SyscallResponsePayload::Sleep(result) => Some(result),
            _ => None,
        })
    }

    fn grant_capability(&mut self, task: TaskId, capability: Cap<()>) -> Result<(), KernelError> {
        let response =
            self.round_trip(SyscallRequestPayload::GrantCapability { task, capability })?;
        Self::extract(response, |payload| match payload {
            SyscallResponsePayload::GrantCapability(result) => Some(result),
            _ => None,
        })
    }

    fn register_service(
        &mut self,
        service_id: ServiceId,
        channel: ChannelId,
    ) -> Result<(), KernelError> {
        let response = self.round_trip(SyscallRequestPayload::RegisterService {
            service_id,
            channel,
        })?;
        Self::extract(response, |payload| match payload {
            SyscallResponsePayload::RegisterService(result) => Some(result),
            _ => None,
        })
    }

    fn lookup_service(&self, service_id: ServiceId) -> Result<ChannelId, KernelError> {
        let response = self.round_trip(SyscallRequestPayload::LookupService { service_id })?;
        Self::extract(response, |payload| match payload {
            SyscallResponsePayload::LookupService(result) => Some(result),
            _ => None,
        })
    }
}

/// Simple loopback transport for tests.
pub struct LoopbackTransport<K: KernelApi> {
    server: SyscallServer<K>,
    pending: Option<MessageEnvelope>,
}

impl<K: KernelApi> LoopbackTransport<K> {
    pub fn new(server: SyscallServer<K>) -> Self {
        Self {
            server,
            pending: None,
        }
    }
}

impl<K: KernelApi> SyscallTransport for LoopbackTransport<K> {
    fn send(&mut self, message: MessageEnvelope) -> Result<(), KernelError> {
        let response = self.server.handle_message(message)?;
        self.pending = Some(response);
        Ok(())
    }

    fn receive(&mut self, _timeout: Option<Duration>) -> Result<MessageEnvelope, KernelError> {
        self.pending
            .take()
            .ok_or_else(|| KernelError::ReceiveFailed("No syscall response pending".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Instant;
    use core_types::ServiceId;
    use ipc::MessageEnvelope;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockKernel {
        last_spawn: Arc<Mutex<Option<TaskDescriptor>>>,
    }

    impl KernelApi for MockKernel {
        fn spawn_task(&mut self, descriptor: TaskDescriptor) -> Result<TaskHandle, KernelError> {
            let mut last_spawn = self.last_spawn.lock().expect("lock last_spawn");
            *last_spawn = Some(descriptor);
            Ok(TaskHandle::new(TaskId::new()))
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
            Err(KernelError::ReceiveFailed("No messages".to_string()))
        }

        fn now(&self) -> Instant {
            Instant::from_nanos(42)
        }

        fn sleep(&mut self, _duration: Duration) -> Result<(), KernelError> {
            Ok(())
        }

        fn grant_capability(
            &mut self,
            _task: TaskId,
            _capability: Cap<()>,
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
            Ok(ChannelId::new())
        }
    }

    #[test]
    fn test_syscall_spawn_round_trip() {
        let codec = SyscallCodec::new(ServiceId::new());
        let last_spawn = Arc::new(Mutex::new(None));
        let kernel = MockKernel {
            last_spawn: last_spawn.clone(),
        };
        let server = SyscallServer::new(kernel, SyscallCodec::new(codec.service_id()));
        let transport = LoopbackTransport::new(server);
        let mut client = SyscallClient::new(transport, codec);

        let descriptor = TaskDescriptor::new("test".to_string());
        let handle = client.spawn_task(descriptor.clone()).unwrap();

        assert!(!handle.task_id.as_uuid().is_nil());

        let last = last_spawn.lock().expect("lock last_spawn");
        assert!(last.is_some());
        assert_eq!(last.as_ref().unwrap().name, descriptor.name);
    }

    #[test]
    fn test_syscall_codec_round_trip() {
        let codec = SyscallCodec::new(ServiceId::new());
        let request = SyscallRequest {
            request_id: MessageId::new(),
            payload: SyscallRequestPayload::CreateChannel,
        };

        let message = codec.encode_request(request.clone()).unwrap();
        let decoded = codec.decode_request(&message).unwrap();

        assert_eq!(decoded.request_id, request.request_id);
        assert!(matches!(
            decoded.payload,
            SyscallRequestPayload::CreateChannel
        ));
    }
}
