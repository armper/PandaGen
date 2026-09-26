#![no_std]
//! TLS for the machine's own requests (NET-040).
//!
//! A thin, sans-IO wrapper over rustls's unbuffered client: TCP bytes in
//! ([`TlsClient::feed`]), TCP bytes out ([`TlsClient::outgoing`]),
//! plaintext in and out. It owns no socket and reads no clock or random
//! source itself, so the kernel's non-blocking fetch can drive it one
//! network pass at a time, and it runs unchanged under `cargo test`.
//!
//! Crypto is RustCrypto's (through `rustls-rustcrypto`), certificates are
//! checked by `rustls-webpki` against Mozilla's roots (`webpki-roots`).
//! Nothing cryptographic is written here.
//!
//! The one extra root, [`TEST_CA`], exists so the gauntlet can serve HTTPS
//! to the kernel from the host. Its certificate carries critical name
//! constraints: it may vouch only for `pandagen.test` names and for
//! `10.0.2.2`, QEMU's own address for the host. It can sign nothing on the
//! real internet, and its private key was never kept.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicPtr, Ordering};
use rustls::client::UnbufferedClientConnection;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::time_provider::TimeProvider;
use rustls::unbuffered::{
    AppDataRecord, ConnectionState, EncodeError, EncryptError, UnbufferedStatus,
};
use rustls::{ClientConfig, RootCertStore};

/// A ready client configuration, shared by every session.
pub type Config = Arc<ClientConfig>;

/// The gauntlet's test CA: `pandagen.test` and `10.0.2.2` only.
pub const TEST_CA: &[u8] = include_bytes!("../testdata/test_ca.der");

// ---- Randomness ----

/// Where random bytes come from. rustls's crypto asks `getrandom`, which
/// has no source on bare metal; the kernel names one here at boot.
static ENTROPY: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// Name the source of random bytes: a function that fills its buffer.
pub fn set_entropy_source(fill: fn(&mut [u8])) {
    ENTROPY.store(fill as *mut (), Ordering::Release);
}

fn custom_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    let fill = ENTROPY.load(Ordering::Acquire);
    if fill.is_null() {
        return Err(getrandom::Error::UNSUPPORTED);
    }
    // SAFETY: only `set_entropy_source` stores here, and it stores a
    // `fn(&mut [u8])`.
    let fill: fn(&mut [u8]) = unsafe { core::mem::transmute(fill) };
    fill(buf);
    Ok(())
}
getrandom::register_custom_getrandom!(custom_getrandom);

// ---- Configuration ----

/// The time, for checking certificates' validity dates.
#[derive(Debug)]
struct Clock(fn() -> u64);

impl TimeProvider for Clock {
    fn current_time(&self) -> Option<UnixTime> {
        match (self.0)() {
            0 => None,
            secs => Some(UnixTime::since_unix_epoch(core::time::Duration::from_secs(
                secs,
            ))),
        }
    }
}

/// A client configuration: Mozilla's roots, the test CA, and `now` for
/// the clock (Unix seconds; 0 when the time is unknown, which fails every
/// certificate rather than trusting any).
pub fn client_config(now: fn() -> u64) -> Result<Arc<ClientConfig>, String> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    roots
        .add(CertificateDer::from(TEST_CA))
        .map_err(|e| e.to_string())?;
    let config = ClientConfig::builder_with_details(
        Arc::new(rustls_rustcrypto::provider()),
        Arc::new(Clock(now)),
    )
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Arc::new(config))
}

// ---- The session ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// The handshake is under way.
    Handshaking,
    /// Keys agreed and the server's certificate accepted.
    Open,
    /// The server said close_notify: all it will send has arrived.
    PeerClosed,
    Closed,
    /// It failed, and why (a certificate problem, a protocol error).
    Failed(String),
}

/// One TLS client session, driven by the caller's I/O.
pub struct TlsClient {
    conn: UnbufferedClientConnection,
    incoming: Vec<u8>,
    outgoing: Vec<u8>,
    plaintext: Vec<u8>,
    to_send: Vec<u8>,
    close_wanted: bool,
    close_sent: bool,
    phase: Phase,
}

impl TlsClient {
    /// Start a session with `host` (a name or an IP address): the
    /// ClientHello is ready in `outgoing` at once.
    pub fn new(config: Arc<ClientConfig>, host: &str) -> Result<Self, String> {
        let name = ServerName::try_from(host.to_string()).map_err(|_| {
            alloc::format!("{host} is not a name TLS can check a certificate against")
        })?;
        let conn = UnbufferedClientConnection::new(config, name).map_err(|e| e.to_string())?;
        let mut client = Self {
            conn,
            incoming: Vec::new(),
            outgoing: Vec::new(),
            plaintext: Vec::new(),
            to_send: Vec::new(),
            close_wanted: false,
            close_sent: false,
            phase: Phase::Handshaking,
        };
        client.process();
        Ok(client)
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// Bytes that arrived from the server.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.incoming.extend_from_slice(bytes);
        self.process();
    }

    /// Plaintext to send; it waits for the handshake if need be.
    pub fn send(&mut self, bytes: &[u8]) {
        self.to_send.extend_from_slice(bytes);
        self.process();
    }

    /// Say close_notify once everything queued has gone.
    pub fn close(&mut self) {
        self.close_wanted = true;
        self.process();
    }

    /// TLS bytes for the server, not yet taken.
    pub fn outgoing(&self) -> &[u8] {
        &self.outgoing
    }

    /// The first `n` of `outgoing` went out.
    pub fn consume(&mut self, n: usize) {
        self.outgoing.drain(..n.min(self.outgoing.len()));
    }

    /// Plaintext the server sent, taken.
    pub fn take_plaintext(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.plaintext)
    }

    fn fail(&mut self, why: String) {
        self.phase = Phase::Failed(why);
    }

    /// Run the state machine until it needs more from the server.
    fn process(&mut self) {
        for _ in 0..64 {
            if matches!(self.phase, Phase::Failed(_) | Phase::Closed) {
                return;
            }
            let UnbufferedStatus { mut discard, state } =
                self.conn.process_tls_records(&mut self.incoming);
            let state = match state {
                Ok(state) => state,
                Err(e) => {
                    self.phase = Phase::Failed(e.to_string());
                    return;
                }
            };
            let mut stop = false;
            let mut failure = None;
            match state {
                ConnectionState::ReadTraffic(mut read) => {
                    while let Some(record) = read.next_record() {
                        match record {
                            Ok(AppDataRecord {
                                discard: more,
                                payload,
                            }) => {
                                discard += more;
                                self.plaintext.extend_from_slice(payload);
                            }
                            Err(e) => {
                                failure = Some(e.to_string());
                                break;
                            }
                        }
                    }
                }
                ConnectionState::EncodeTlsData(mut encode) => {
                    let mut buf = vec![0u8; 4096];
                    loop {
                        match encode.encode(&mut buf) {
                            Ok(n) => {
                                self.outgoing.extend_from_slice(&buf[..n]);
                                break;
                            }
                            Err(EncodeError::InsufficientSize(need)) => {
                                buf.resize(need.required_size, 0);
                            }
                            Err(EncodeError::AlreadyEncoded) => break,
                        }
                    }
                }
                // The caller sends `outgoing` when it can; to rustls it has
                // gone.
                ConnectionState::TransmitTlsData(transmit) => transmit.done(),
                ConnectionState::BlockedHandshake => stop = true,
                ConnectionState::WriteTraffic(mut write) => {
                    if self.phase == Phase::Handshaking {
                        self.phase = Phase::Open;
                    }
                    if !self.to_send.is_empty() {
                        let data = core::mem::take(&mut self.to_send);
                        match encrypt_into(&mut self.outgoing, |out| write.encrypt(&data, out)) {
                            Ok(()) => {}
                            Err(why) => failure = Some(why),
                        }
                    }
                    if self.close_wanted && !self.close_sent {
                        self.close_sent = true;
                        if let Err(why) =
                            encrypt_into(&mut self.outgoing, |out| write.queue_close_notify(out))
                        {
                            failure = Some(why);
                        }
                    }
                    stop = true;
                }
                ConnectionState::PeerClosed => self.phase = Phase::PeerClosed,
                ConnectionState::Closed => {
                    self.phase = Phase::Closed;
                    stop = true;
                }
                // Early data is a server's; anything newer, stop and wait.
                _ => stop = true,
            }
            self.incoming.drain(..discard);
            if let Some(why) = failure {
                self.fail(why);
                return;
            }
            if stop {
                return;
            }
        }
    }
}

/// Encrypt with `f` into `out`, growing the buffer as rustls asks.
fn encrypt_into(
    out: &mut Vec<u8>,
    mut f: impl FnMut(&mut [u8]) -> Result<usize, EncryptError>,
) -> Result<(), String> {
    let mut buf = vec![0u8; 4096];
    loop {
        match f(&mut buf) {
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                return Ok(());
            }
            Err(EncryptError::InsufficientSize(need)) => buf.resize(need.required_size, 0),
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::io::{Read, Write};

    const SERVER: &[u8] = include_bytes!("../testdata/server.der");
    const OUTSIDE: &[u8] = include_bytes!("../testdata/outside.der");
    const SERVER_KEY: &[u8] = include_bytes!("../testdata/server.key.der");

    /// 2026-06-01, inside every test certificate's validity.
    fn june_2026() -> u64 {
        1_780_272_000
    }

    fn tls_server(cert: &'static [u8]) -> rustls::ServerConnection {
        let config =
            rustls::ServerConfig::builder_with_provider(Arc::new(rustls_rustcrypto::provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from(cert)],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(SERVER_KEY)),
                )
                .unwrap();
        rustls::ServerConnection::new(Arc::new(config)).unwrap()
    }

    /// Shuttle bytes both ways until nothing moves; the server answers
    /// the first request it reads with `answer`.
    fn converse(client: &mut TlsClient, server: &mut rustls::ServerConnection, answer: &[u8]) {
        let mut answered = false;
        for _ in 0..50 {
            let out = client.outgoing().to_vec();
            client.consume(out.len());
            if !out.is_empty() {
                server.read_tls(&mut &out[..]).unwrap();
                if server.process_new_packets().is_err() {
                    // The server's alert still goes to the client.
                }
            }
            let mut request = Vec::new();
            let _ = server.reader().read_to_end(&mut request);
            if !request.is_empty() && !answered {
                server.writer().write_all(answer).unwrap();
                server.send_close_notify();
                answered = true;
            }
            let mut back = Vec::new();
            while server.wants_write() {
                server.write_tls(&mut back).unwrap();
            }
            if out.is_empty() && back.is_empty() {
                break;
            }
            client.feed(&back);
        }
    }

    #[test]
    fn a_request_and_its_answer_go_through_a_checked_handshake() {
        let config = client_config(june_2026).unwrap();
        let mut client = TlsClient::new(config, "10.0.2.2").unwrap();
        assert!(!client.outgoing().is_empty(), "the ClientHello, at once");
        client.send(b"GET / HTTP/1.1\r\nHost: 10.0.2.2\r\n\r\n");
        let mut server = tls_server(SERVER);
        converse(
            &mut client,
            &mut server,
            b"HTTP/1.1 200 OK\r\n\r\nsecret hello",
        );
        assert_eq!(
            client.take_plaintext(),
            b"HTTP/1.1 200 OK\r\n\r\nsecret hello"
        );
        assert_eq!(client.phase(), &Phase::PeerClosed);
    }

    #[test]
    fn the_name_asked_for_must_be_the_certificates() {
        let config = client_config(june_2026).unwrap();
        let mut client = TlsClient::new(config, "other.pandagen.test").unwrap();
        client.send(b"GET / HTTP/1.1\r\n\r\n");
        let mut server = tls_server(SERVER);
        converse(&mut client, &mut server, b"no");
        let Phase::Failed(why) = client.phase() else {
            panic!("{:?}", client.phase());
        };
        assert!(why.to_lowercase().contains("certificate"), "{why}");
        assert!(client.take_plaintext().is_empty());
    }

    #[test]
    fn the_test_ca_cannot_vouch_for_a_real_name() {
        // `outside.der` is signed by the test CA for example.com: its name
        // constraints forbid it.
        let config = client_config(june_2026).unwrap();
        let mut client = TlsClient::new(config, "example.com").unwrap();
        client.send(b"GET / HTTP/1.1\r\n\r\n");
        let mut server = tls_server(OUTSIDE);
        converse(&mut client, &mut server, b"no");
        let Phase::Failed(why) = client.phase() else {
            panic!("{:?}", client.phase());
        };
        // Refused for the constraint, not for some other flaw.
        assert!(
            why.contains("NameConstraint") || why.contains("name constraint"),
            "{why}"
        );
    }

    #[test]
    fn an_expired_or_unknown_clock_fails_the_certificate() {
        // 2050: after every test certificate.
        let config = client_config(|| 2_524_608_000).unwrap();
        let mut client = TlsClient::new(config, "10.0.2.2").unwrap();
        client.send(b"x");
        let mut server = tls_server(SERVER);
        converse(&mut client, &mut server, b"no");
        assert!(matches!(client.phase(), Phase::Failed(_)));
        let config = client_config(|| 0).unwrap();
        let mut client = TlsClient::new(config, "10.0.2.2").unwrap();
        client.send(b"x");
        let mut server = tls_server(SERVER);
        converse(&mut client, &mut server, b"no");
        assert!(matches!(client.phase(), Phase::Failed(_)));
    }

    #[test]
    fn a_name_that_is_not_a_name_is_refused_up_front() {
        let config = client_config(june_2026).unwrap();
        assert!(TlsClient::new(config, "not a host!").is_err());
    }
}
