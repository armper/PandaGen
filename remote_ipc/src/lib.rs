//! Remote IPC with explicit capability authority.
//!
//! `no_std` + `alloc`, so the same envelope code runs in the bare-metal
//! kernel (server side, over UDP) and in host tools (client side).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core_types::ServiceId;
use ipc::{MessageEnvelope, MessageId, MessagePayload, SchemaVersion};
use serde::{Deserialize, Serialize};

const REMOTE_CALL_ACTION: &str = "remote.capability.call";
const REMOTE_RESPONSE_ACTION: &str = "remote.capability.response";
const REMOTE_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

/// Base64 (standard alphabet, padded) for byte fields on the wire: JSON
/// would otherwise spell every byte out as a number, which does not fit a
/// UDP datagram once the response is nested inside the envelope.
pub mod b64 {
    use alloc::string::String;
    use alloc::vec::Vec;

    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                TABLE[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                TABLE[n as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    fn value(c: u8) -> Option<u32> {
        TABLE.iter().position(|&t| t == c).map(|p| p as u32)
    }

    pub fn decode(text: &str) -> Option<Vec<u8>> {
        let bytes = text.as_bytes();
        if bytes.len() % 4 != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
        for chunk in bytes.chunks(4) {
            let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
            if pad > 2 {
                return None;
            }
            let mut n = 0u32;
            for (i, &c) in chunk.iter().enumerate() {
                let v = if c == b'=' && i >= 4 - pad {
                    0
                } else {
                    value(c)?
                };
                n = n << 6 | v;
            }
            out.push((n >> 16) as u8);
            if pad < 2 {
                out.push((n >> 8) as u8);
            }
            if pad < 1 {
                out.push(n as u8);
            }
        }
        Some(out)
    }

    pub mod bytes {
        use super::{decode, encode};
        use alloc::string::String;
        use alloc::vec::Vec;
        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
            encode(bytes).serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
            let text = String::deserialize(d)?;
            decode(&text).ok_or_else(|| serde::de::Error::custom("invalid base64"))
        }
    }

    pub mod result {
        use super::{decode, encode};
        use alloc::string::String;
        use alloc::vec::Vec;
        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        #[derive(Serialize, Deserialize)]
        enum Wire {
            Ok(String),
            Err(String),
        }

        pub fn serialize<S: Serializer>(
            value: &Result<Vec<u8>, String>,
            s: S,
        ) -> Result<S::Ok, S::Error> {
            match value {
                Ok(bytes) => Wire::Ok(encode(bytes)),
                Err(err) => Wire::Err(err.clone()),
            }
            .serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            d: D,
        ) -> Result<Result<Vec<u8>, String>, D::Error> {
            match Wire::deserialize(d)? {
                Wire::Ok(text) => decode(&text)
                    .map(Ok)
                    .ok_or_else(|| serde::de::Error::custom("invalid base64")),
                Wire::Err(err) => Ok(Err(err)),
            }
        }
    }
}

/// Capability that lets a remote caller run a read-only kernel command.
pub const CAP_KERNEL_COMMAND: u64 = 0x5047_0001;
/// Action name for `CAP_KERNEL_COMMAND`; the payload is the command line.
pub const ACTION_KERNEL_COMMAND_RUN: &str = "kernel.command.run";
/// UDP port the kernel serves remote calls on.
pub const KERNEL_REMOTE_PORT: u16 = 7778;

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
    #[serde(with = "b64::bytes")]
    pub payload: Vec<u8>,
    pub authority: CapabilityAuthority,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteResponse {
    pub request_id: MessageId,
    #[serde(with = "b64::result")]
    pub result: Result<Vec<u8>, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteIpcError {
    Unauthorized,
    Codec(String),
}

impl fmt::Display for RemoteIpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "Authorization denied"),
            Self::Codec(err) => write!(f, "Codec error: {err}"),
        }
    }
}

impl core::error::Error for RemoteIpcError {}

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

        response.result.map_err(RemoteIpcError::Codec)
    }
}

pub struct RemoteIpcServer<H: RemoteHandler> {
    handler: H,
    allowed_caps: Vec<u64>,
}

impl<H: RemoteHandler> RemoteIpcServer<H> {
    pub fn new(handler: H, allowed_caps: Vec<u64>) -> Self {
        Self {
            handler,
            allowed_caps,
        }
    }

    pub fn handle_message(
        &mut self,
        message: MessageEnvelope,
    ) -> Result<MessageEnvelope, RemoteIpcError> {
        let call = match authorize_call(&message, &self.allowed_caps) {
            Ok(call) => call,
            Err(RemoteIpcError::Unauthorized) => {
                let call = decode_call(&message)?;
                let response = RemoteResponse {
                    request_id: call.request_id,
                    result: Err("unauthorized".to_string()),
                };
                return encode_response(response, message.id);
            }
            Err(err) => return Err(err),
        };

        let request_id = call.request_id;
        let result = self.handler.handle(call);
        let response = RemoteResponse { request_id, result };
        encode_response(response, message.id)
    }
}

/// Decode a call envelope and check it against `allowed_caps` (both the
/// server's list and the caller's own authority must grant the capability).
/// For servers that answer asynchronously.
pub fn authorize_call(
    message: &MessageEnvelope,
    allowed_caps: &[u64],
) -> Result<RemoteCall, RemoteIpcError> {
    let call = decode_call(message)?;
    if !allowed_caps.contains(&call.cap_id) || !call.authority.allows(call.cap_id) {
        return Err(RemoteIpcError::Unauthorized);
    }
    Ok(call)
}

/// Envelope as carried in one datagram: the payload travels as base64.
#[derive(Serialize, Deserialize)]
struct WireEnvelope {
    id: MessageId,
    destination: ServiceId,
    source: Option<core_types::TaskId>,
    action: String,
    schema_version: SchemaVersion,
    correlation_id: Option<MessageId>,
    #[serde(with = "b64::bytes")]
    payload: Vec<u8>,
}

/// Serialize an envelope for a datagram transport.
pub fn envelope_to_bytes(message: &MessageEnvelope) -> Result<Vec<u8>, RemoteIpcError> {
    let wire = WireEnvelope {
        id: message.id,
        destination: message.destination,
        source: message.source,
        action: message.action.clone(),
        schema_version: message.schema_version,
        correlation_id: message.correlation_id,
        payload: message.payload.as_bytes().to_vec(),
    };
    serde_json::to_vec(&wire).map_err(|err| RemoteIpcError::Codec(err.to_string()))
}

/// Parse an envelope received from a datagram transport.
pub fn envelope_from_bytes(bytes: &[u8]) -> Result<MessageEnvelope, RemoteIpcError> {
    let wire: WireEnvelope =
        serde_json::from_slice(bytes).map_err(|err| RemoteIpcError::Codec(err.to_string()))?;
    Ok(MessageEnvelope {
        id: wire.id,
        destination: wire.destination,
        source: wire.source,
        action: wire.action,
        schema_version: wire.schema_version,
        correlation_id: wire.correlation_id,
        payload: MessagePayload::from_raw(wire.payload),
    })
}

pub fn encode_call(call: RemoteCall) -> Result<MessageEnvelope, RemoteIpcError> {
    let payload =
        MessagePayload::new(&call).map_err(|err| RemoteIpcError::Codec(err.to_string()))?;
    Ok(MessageEnvelope::new(
        ServiceId::new(),
        REMOTE_CALL_ACTION,
        REMOTE_SCHEMA,
        payload,
    ))
}

pub fn encode_response(
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

pub fn decode_call(message: &MessageEnvelope) -> Result<RemoteCall, RemoteIpcError> {
    if message.action != REMOTE_CALL_ACTION {
        return Err(RemoteIpcError::Codec("unexpected action".to_string()));
    }
    message
        .payload
        .deserialize::<RemoteCall>()
        .map_err(|err| RemoteIpcError::Codec(err.to_string()))
}

pub fn decode_response(message: &MessageEnvelope) -> Result<RemoteResponse, RemoteIpcError> {
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

    #[test]
    fn test_authorize_and_byte_round_trip() {
        let authority = CapabilityAuthority {
            caller: "host".to_string(),
            allowed_caps: vec![CAP_KERNEL_COMMAND],
        };
        let call = RemoteCall {
            request_id: MessageId::new(),
            cap_id: CAP_KERNEL_COMMAND,
            action: ACTION_KERNEL_COMMAND_RUN.to_string(),
            payload: b"cpus".to_vec(),
            authority,
        };
        let bytes = envelope_to_bytes(&encode_call(call.clone()).unwrap()).unwrap();
        let envelope = envelope_from_bytes(&bytes).unwrap();
        let decoded = authorize_call(&envelope, &[CAP_KERNEL_COMMAND]).unwrap();
        assert_eq!(decoded, call);
        assert_eq!(
            authorize_call(&envelope, &[1]),
            Err(RemoteIpcError::Unauthorized)
        );
        let response = encode_response(
            RemoteResponse {
                request_id: call.request_id,
                result: Ok(b"cpus: online=4".to_vec()),
            },
            envelope.id,
        )
        .unwrap();
        let back = envelope_from_bytes(&envelope_to_bytes(&response).unwrap()).unwrap();
        assert_eq!(back.correlation_id, Some(envelope.id));
        assert_eq!(
            decode_response(&back).unwrap().result,
            Ok(b"cpus: online=4".to_vec())
        );
        assert!(envelope_from_bytes(b"not json").is_err());
    }

    #[test]
    fn test_base64_round_trip_and_datagram_budget() {
        for len in 0..40usize {
            let bytes: Vec<u8> = (0..len as u8).map(|b| b.wrapping_mul(37)).collect();
            let text = b64::encode(&bytes);
            assert_eq!(text.len() % 4, 0);
            assert_eq!(b64::decode(&text).unwrap(), bytes);
        }
        assert_eq!(b64::encode(b"Man"), "TWFu");
        assert_eq!(b64::encode(b"Ma"), "TWE=");
        assert_eq!(b64::decode("TWE"), None);
        assert_eq!(b64::decode("TW!="), None);
        // A maximal 256-byte command output must fit one UDP payload.
        let response = encode_response(
            RemoteResponse {
                request_id: MessageId::new(),
                result: Ok(vec![b'x'; 256]),
            },
            MessageId::new(),
        )
        .unwrap();
        let bytes = envelope_to_bytes(&response).unwrap();
        assert!(bytes.len() <= 1472, "datagram is {} bytes", bytes.len());
        let back = decode_response(&envelope_from_bytes(&bytes).unwrap()).unwrap();
        assert_eq!(back.result, Ok(vec![b'x'; 256]));
    }
}
