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

/// SHA-256 and HMAC-SHA256 for signing datagrams (no dependencies).
pub mod sha256 {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    fn compress(state: &mut [u32; 8], block: &[u8]) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().enumerate().take(16) {
            *word = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }

    /// SHA-256 digest of `data`.
    pub fn digest(data: &[u8]) -> [u8; 32] {
        let mut state: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];
        let mut chunks = data.chunks_exact(64);
        for block in &mut chunks {
            compress(&mut state, block);
        }
        let rest = chunks.remainder();
        let mut tail = [0u8; 128];
        tail[..rest.len()].copy_from_slice(rest);
        tail[rest.len()] = 0x80;
        let tail_len = if rest.len() < 56 { 64 } else { 128 };
        tail[tail_len - 8..tail_len].copy_from_slice(&((data.len() as u64) * 8).to_be_bytes());
        for block in tail[..tail_len].chunks_exact(64) {
            compress(&mut state, block);
        }
        let mut out = [0u8; 32];
        for (i, word) in state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    /// HMAC-SHA256 (RFC 2104).
    pub fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
        let mut block = [0u8; 64];
        if key.len() > 64 {
            block[..32].copy_from_slice(&digest(key));
        } else {
            block[..key.len()].copy_from_slice(key);
        }
        let mut inner = alloc::vec::Vec::with_capacity(64 + message.len());
        inner.extend(block.iter().map(|b| b ^ 0x36));
        inner.extend_from_slice(message);
        let inner_hash = digest(&inner);
        let mut outer = alloc::vec::Vec::with_capacity(96);
        outer.extend(block.iter().map(|b| b ^ 0x5c));
        outer.extend_from_slice(&inner_hash);
        digest(&outer)
    }

    /// Constant-time equality for tags.
    pub fn tags_equal(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

/// Remembers the last `N` accepted message ids so a captured, correctly
/// signed datagram cannot simply be sent again. The window is bounded;
/// a replay older than `N` accepted messages would be accepted.
pub struct ReplayGuard<const N: usize> {
    seen: [u128; N],
    next: usize,
    len: usize,
}

impl<const N: usize> ReplayGuard<N> {
    pub const fn new() -> Self {
        Self {
            seen: [0; N],
            next: 0,
            len: 0,
        }
    }

    fn key(id: MessageId) -> u128 {
        id.as_uuid().as_u128()
    }

    /// Whether `id` was accepted recently.
    pub fn is_replay(&self, id: MessageId) -> bool {
        self.is_replay_key(Self::key(id))
    }

    pub fn is_replay_key(&self, key: u128) -> bool {
        self.seen[..self.len].contains(&key)
    }

    /// Record `id` as accepted; returns false (and records nothing) if it
    /// was already in the window.
    pub fn accept(&mut self, id: MessageId) -> bool {
        self.accept_key(Self::key(id))
    }

    /// Same as `accept` for a raw 128-bit nonce (the TCP line protocol).
    pub fn accept_key(&mut self, key: u128) -> bool {
        if self.is_replay_key(key) {
            return false;
        }
        self.seen[self.next] = key;
        self.next = (self.next + 1) % N;
        if self.len < N {
            self.len += 1;
        }
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<const N: usize> Default for ReplayGuard<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Signed line protocol for the TCP command port: one request per line,
/// `<nonce as 32 hex digits> <base64 tag> <command>`, where the tag is
/// HMAC-SHA256(token, nonce_hex || 0 || command). Replies are one line:
/// `+<base64 output>` on success or `-<message>` on failure.
pub mod line {
    use super::{b64, sha256};
    use alloc::string::String;
    use alloc::vec::Vec;

    /// TCP port the kernel serves signed command lines on.
    pub const KERNEL_COMMAND_PORT: u16 = 7780;

    fn signing_input(nonce_hex: &str, command: &str) -> Vec<u8> {
        let mut input = Vec::with_capacity(nonce_hex.len() + 1 + command.len());
        input.extend_from_slice(nonce_hex.as_bytes());
        input.push(0);
        input.extend_from_slice(command.as_bytes());
        input
    }

    pub fn nonce_hex(nonce: u128) -> String {
        let mut s = String::with_capacity(32);
        for byte in nonce.to_be_bytes() {
            let hi = b"0123456789abcdef"[(byte >> 4) as usize] as char;
            let lo = b"0123456789abcdef"[(byte & 15) as usize] as char;
            s.push(hi);
            s.push(lo);
        }
        s
    }

    fn parse_nonce(text: &str) -> Option<u128> {
        if text.len() != 32 {
            return None;
        }
        let mut value: u128 = 0;
        for c in text.bytes() {
            let digit = (c as char).to_digit(16)? as u128;
            value = (value << 4) | digit;
        }
        Some(value)
    }

    /// Build a request line (without the trailing newline).
    pub fn sign(token: &[u8], nonce: u128, command: &str) -> String {
        let nonce_hex = nonce_hex(nonce);
        let tag = sha256::hmac(token, &signing_input(&nonce_hex, command));
        let mut line = nonce_hex;
        line.push(' ');
        line.push_str(&b64::encode(&tag));
        line.push(' ');
        line.push_str(command);
        line
    }

    /// Verify a request line; returns the nonce and command.
    pub fn verify<'a>(token: &[u8], line: &'a str) -> Option<(u128, &'a str)> {
        let line = line.trim_end_matches(['\r', '\n']);
        let (nonce_hex, rest) = line.split_once(' ')?;
        let (tag_b64, command) = rest.split_once(' ')?;
        let nonce = parse_nonce(nonce_hex)?;
        let tag = b64::decode(tag_b64)?;
        let expected = sha256::hmac(token, &signing_input(nonce_hex, command));
        if !sha256::tags_equal(&expected, &tag) {
            return None;
        }
        Some((nonce, command))
    }

    /// Encode a reply line (without the trailing newline).
    pub fn reply(result: &Result<Vec<u8>, String>) -> String {
        match result {
            Ok(bytes) => {
                let mut s = String::from("+");
                s.push_str(&b64::encode(bytes));
                s
            }
            Err(err) => {
                let mut s = String::from("-");
                s.push_str(err);
                s
            }
        }
    }

    /// Decode a reply line.
    pub fn parse_reply(line: &str) -> Option<Result<Vec<u8>, String>> {
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some(payload) = line.strip_prefix('+') {
            return b64::decode(payload).map(Ok);
        }
        line.strip_prefix('-').map(|err| Err(String::from(err)))
    }
}

/// Shared secret used when no `remote_token=` is configured.
pub const DEFAULT_REMOTE_TOKEN: &str = "pandagen-dev";

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
    /// HMAC-SHA256 over id, action and payload with the shared token.
    #[serde(with = "b64::bytes")]
    tag: Vec<u8>,
}

/// What the tag covers: the message id, the action, and the payload bytes.
fn signing_input(id: MessageId, action: &str, payload: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(16 + action.len() + payload.len() + 2);
    input.extend_from_slice(id.as_uuid().as_bytes());
    input.push(0);
    input.extend_from_slice(action.as_bytes());
    input.push(0);
    input.extend_from_slice(payload);
    input
}

/// Serialize and sign an envelope for a datagram transport.
pub fn envelope_to_bytes(
    message: &MessageEnvelope,
    token: &[u8],
) -> Result<Vec<u8>, RemoteIpcError> {
    let payload = message.payload.as_bytes().to_vec();
    let tag = sha256::hmac(token, &signing_input(message.id, &message.action, &payload));
    let wire = WireEnvelope {
        id: message.id,
        destination: message.destination,
        source: message.source,
        action: message.action.clone(),
        schema_version: message.schema_version,
        correlation_id: message.correlation_id,
        payload,
        tag: tag.to_vec(),
    };
    serde_json::to_vec(&wire).map_err(|err| RemoteIpcError::Codec(err.to_string()))
}

/// Parse an envelope received from a datagram transport, rejecting it as
/// `Unauthorized` unless its tag matches `token`.
pub fn envelope_from_bytes(bytes: &[u8], token: &[u8]) -> Result<MessageEnvelope, RemoteIpcError> {
    let wire: WireEnvelope =
        serde_json::from_slice(bytes).map_err(|err| RemoteIpcError::Codec(err.to_string()))?;
    let expected = sha256::hmac(token, &signing_input(wire.id, &wire.action, &wire.payload));
    if !sha256::tags_equal(&expected, &wire.tag) {
        return Err(RemoteIpcError::Unauthorized);
    }
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
        let token = DEFAULT_REMOTE_TOKEN.as_bytes();
        let bytes = envelope_to_bytes(&encode_call(call.clone()).unwrap(), token).unwrap();
        let envelope = envelope_from_bytes(&bytes, token).unwrap();
        assert!(matches!(
            envelope_from_bytes(&bytes, b"other-token"),
            Err(RemoteIpcError::Unauthorized)
        ));
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
        let back =
            envelope_from_bytes(&envelope_to_bytes(&response, token).unwrap(), token).unwrap();
        assert_eq!(back.correlation_id, Some(envelope.id));
        assert_eq!(
            decode_response(&back).unwrap().result,
            Ok(b"cpus: online=4".to_vec())
        );
        assert!(envelope_from_bytes(b"not json", token).is_err());
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
        let bytes = envelope_to_bytes(&response, b"k").unwrap();
        assert!(bytes.len() <= 1472, "datagram is {} bytes", bytes.len());
        let back = decode_response(&envelope_from_bytes(&bytes, b"k").unwrap()).unwrap();
        assert_eq!(back.result, Ok(vec![b'x'; 256]));
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn test_sha256_and_hmac_vectors() {
        assert_eq!(
            hex(&sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256::digest(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // 64-byte input exercises the two-block tail.
        assert_eq!(
            hex(&sha256::digest(&[b'a'; 64])),
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
        // RFC 4231 test case 2.
        assert_eq!(
            hex(&sha256::hmac(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert!(sha256::tags_equal(b"ab", b"ab"));
        assert!(!sha256::tags_equal(b"ab", b"ac"));
        assert!(!sha256::tags_equal(b"ab", b"abc"));
    }

    #[test]
    fn test_replay_guard_window() {
        let mut guard = ReplayGuard::<3>::new();
        assert!(guard.is_empty());
        let ids: Vec<MessageId> = (0..5).map(|_| MessageId::new()).collect();
        assert!(guard.accept(ids[0]));
        assert!(!guard.accept(ids[0]), "immediate replay refused");
        assert!(guard.is_replay(ids[0]));
        assert!(guard.accept(ids[1]));
        assert!(guard.accept(ids[2]));
        assert_eq!(guard.len(), 3);
        // Window is full; the oldest id falls out after the next accept.
        assert!(guard.accept(ids[3]));
        assert!(!guard.is_replay(ids[0]));
        assert!(guard.is_replay(ids[1]));
        assert!(!guard.accept(ids[3]));
        assert!(guard.accept(ids[4]));
        assert_eq!(guard.len(), 3);
    }

    #[test]
    fn test_line_protocol_sign_verify_and_replies() {
        let token = b"pandagen-dev";
        let request = line::sign(token, 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210, "cpus");
        assert!(request.starts_with("0123456789abcdeffedcba9876543210 "));
        assert!(request.ends_with(" cpus"));
        let with_newline = format!("{request}\n");
        let (nonce, command) = line::verify(token, &with_newline).unwrap();
        assert_eq!(nonce, 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210);
        assert_eq!(command, "cpus");
        assert!(line::verify(b"other", &request).is_none(), "wrong token");
        let tampered = request.replace("cpus", "halt");
        assert!(line::verify(token, &tampered).is_none(), "tampered command");
        assert!(line::verify(token, "nonce tag").is_none(), "malformed");
        assert!(line::verify(token, "zz tag cpus").is_none(), "bad nonce");
        let ok = line::reply(&Ok(b"cpus: online=4".to_vec()));
        assert_eq!(line::parse_reply(&ok), Some(Ok(b"cpus: online=4".to_vec())));
        let err = line::reply(&Err("unauthorized".to_string()));
        assert_eq!(err, "-unauthorized");
        assert_eq!(line::parse_reply("-nope\n"), Some(Err("nope".to_string())));
        assert_eq!(line::parse_reply("?"), None);
        let mut guard = ReplayGuard::<4>::new();
        assert!(guard.accept_key(nonce));
        assert!(!guard.accept_key(nonce));
    }
}
