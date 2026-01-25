//! Remote IPC with explicit capability authority.

use core_types::ServiceId;
use ipc::{MessageEnvelope, MessageId, MessagePayload, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

const REMOTE_CALL_ACTION: &str = "remote.capability.call";
const REMOTE_RESPONSE_ACTION: &str = "remote.capability.response";
const REMOTE_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityAuthority {
    pub caller: String,
    pub allowed_caps: Vec<u64>,
}

impl CapabilityAuthority {
    pub fn allows(&self, cap_id: u64) -> bool {
        self.allowed_caps.contains(&cap_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteCall {
    pub request_id: MessageId,
    pub cap_id: u64,
    pub action: String,
    pub payload: Vec<u8>,
    pub authority: CapabilityAuthority,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteResponse {
    pub request_id: MessageId,
    pub result: Result<Vec<u8>, String>,
}

#[derive(Debug, Error)]
pub enum RemoteIpcError {
    #[error("Authorization denied")]
    Unauthorized,

    #[error("Codec error: {0}")]
    Codec(String),
}

pub trait RemoteHandler {
    fn handle(&mut self, call: RemoteCall) -> Result<Vec<u8>, String>;
}

pub trait RemoteTransport {
    fn send(&mut self, message: MessageEnvelope) -> Result<(), RemoteIpcError>;
    fn receive(&mut self) -> Result<MessageEnvelope, RemoteIpcError>;
}

pub struct RemoteIpcClient<T: RemoteTransport> {
    transport: T,
    authority: CapabilityAuthority,
}

impl<T: RemoteTransport> RemoteIpcClient<T> {
    pub fn new(transport: T, authority: CapabilityAuthority) -> Self {
        Self {
            transport,
            authority,
        }
    }

    pub fn call(
        &mut self,
        cap_id: u64,
        action: impl Into<String>,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, RemoteIpcError> {
        if !self.authority.allows(cap_id) {
            return Err(RemoteIpcError::Unauthorized);
        }

        let call = RemoteCall {
            request_id: MessageId::new(),
            cap_id,
            action: action.into(),
            payload,
            authority: self.authority.clone(),
        };

        let message = encode_call(call.clone())?;
        self.transport.send(message)?;

        let response = decode_response(&self.transport.receive()?)?;
        if response.request_id != call.request_id {
            return Err(RemoteIpcError::Codec("request_id mismatch".to_string()));
        }

        response.result.map_err(|err| RemoteIpcError::Codec(err))
    }
}

pub struct RemoteIpcServer<H: RemoteHandler> {
    handler: H,
    allowed_caps: HashSet<u64>,
}

impl<H: RemoteHandler> RemoteIpcServer<H> {
    pub fn new(handler: H, allowed_caps: Vec<u64>) -> Self {
        Self {
            handler,
            allowed_caps: allowed_caps.into_iter().collect(),
        }
    }

    pub fn handle_message(
        &mut self,
        message: MessageEnvelope,
    ) -> Result<MessageEnvelope, RemoteIpcError> {
        let call = decode_call(&message)?;
        if !self.allowed_caps.contains(&call.cap_id) || !call.authority.allows(call.cap_id) {
            let response = RemoteResponse {
                request_id: call.request_id,
                result: Err("unauthorized".to_string()),
            };
            return encode_response(response, message.id);
        }

        let result = self.handler.handle(call);
        let response = RemoteResponse {
            request_id: message.id,
            result,
        };
        encode_response(response, message.id)
    }
}

fn encode_call(call: RemoteCall) -> Result<MessageEnvelope, RemoteIpcError> {
    let payload =
        MessagePayload::new(&call).map_err(|err| RemoteIpcError::Codec(err.to_string()))?;
    Ok(MessageEnvelope::new(
        ServiceId::new(),
        REMOTE_CALL_ACTION,
        REMOTE_SCHEMA,
        payload,
    ))
}

fn encode_response(
    response: RemoteResponse,
    correlation_id: MessageId,
) -> Result<MessageEnvelope, RemoteIpcError> {
    let payload =
        MessagePayload::new(&response).map_err(|err| RemoteIpcError::Codec(err.to_string()))?;
    Ok(MessageEnvelope::new(
        ServiceId::new(),
        REMOTE_RESPONSE_ACTION,
        REMOTE_SCHEMA,
        payload,
    )
    .with_correlation(correlation_id))
}

fn decode_call(message: &MessageEnvelope) -> Result<RemoteCall, RemoteIpcError> {
    if message.action != REMOTE_CALL_ACTION {
        return Err(RemoteIpcError::Codec("unexpected action".to_string()));
    }
    message
        .payload
        .deserialize::<RemoteCall>()
        .map_err(|err| RemoteIpcError::Codec(err.to_string()))
}

fn decode_response(message: &MessageEnvelope) -> Result<RemoteResponse, RemoteIpcError> {
    if message.action != REMOTE_RESPONSE_ACTION {
        return Err(RemoteIpcError::Codec("unexpected action".to_string()));
    }
    message
        .payload
        .deserialize::<RemoteResponse>()
        .map_err(|err| RemoteIpcError::Codec(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoHandler;

    impl RemoteHandler for EchoHandler {
        fn handle(&mut self, call: RemoteCall) -> Result<Vec<u8>, String> {
            Ok(call.payload)
        }
    }

    struct Loopback {
        pending: Option<MessageEnvelope>,
        server: RemoteIpcServer<EchoHandler>,
    }

    impl Loopback {
        fn new(server: RemoteIpcServer<EchoHandler>) -> Self {
            Self {
                pending: None,
                server,
            }
        }
    }

    impl RemoteTransport for Loopback {
        fn send(&mut self, message: MessageEnvelope) -> Result<(), RemoteIpcError> {
            let response = self.server.handle_message(message)?;
            self.pending = Some(response);
            Ok(())
        }

        fn receive(&mut self) -> Result<MessageEnvelope, RemoteIpcError> {
            self.pending
                .take()
                .ok_or(RemoteIpcError::Codec("no response".to_string()))
        }
    }

    #[test]
    fn test_remote_capability_call_success() {
        let authority = CapabilityAuthority {
            caller: "client".to_string(),
            allowed_caps: vec![42],
        };
        let server = RemoteIpcServer::new(EchoHandler, vec![42]);
        let transport = Loopback::new(server);
        let mut client = RemoteIpcClient::new(transport, authority);

        let payload = b"hello".to_vec();
        let result = client.call(42, "echo", payload.clone()).unwrap();
        assert_eq!(result, payload);
    }

    #[test]
    fn test_remote_capability_call_denied() {
        let authority = CapabilityAuthority {
            caller: "client".to_string(),
            allowed_caps: vec![1],
        };
        let server = RemoteIpcServer::new(EchoHandler, vec![42]);
        let transport = Loopback::new(server);
        let mut client = RemoteIpcClient::new(transport, authority);

        let result = client.call(42, "echo", b"no".to_vec());
        assert!(matches!(result, Err(RemoteIpcError::Unauthorized)));
    }
}
