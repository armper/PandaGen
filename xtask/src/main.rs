use std::env;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const TARGET: &str = "x86_64-unknown-none";
const KERNEL_CRATE: &str = "kernel_bootstrap";
/// Cargo profile for the bare-metal image (see `[profile.kernel]` in Cargo.toml).
const KERNEL_PROFILE: &str = "kernel";
const LIMINE_VENDOR_DIR: &str = "third_party/limine";
const ISO_OUTPUT: &str = "dist/pandagen.iso";
const DISK_OUTPUT: &str = "dist/pandagen.disk";
const DEFAULT_DISK_SIZE_MB: usize = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("iso") => cmd_iso(),
        Some("qemu") => cmd_qemu(),
        Some("qemu-smoke") => cmd_qemu_smoke(),
        Some("qemu-script") => cmd_qemu_script(args),
        Some("remote") => cmd_remote(args),
        Some("remote-tcp") => cmd_remote_tcp(args),
        Some("remote-key") => cmd_remote_key(args),
        Some("image") => cmd_image(),
        Some("limine-fetch") => cmd_limine_fetch(args),
        _ => usage(),
    }
}

fn usage() -> Result<(), Box<dyn std::error::Error>> {
    println!("Usage:");
    println!("  cargo xtask iso");
    println!("  cargo xtask qemu");
    println!("  cargo xtask qemu-smoke");
    println!("  cargo xtask qemu-script [--keys k1,k2,sleep:0.5,shot:name,...] [--boot-wait secs]");
    println!(
        "      [--expect-serial TEXT] [--forbid-serial TEXT] [--port-base N] [--allow-exception]"
    );
    println!("                          [--after secs] [--out prefix] [--expect-serial text]");
    println!("  cargo xtask remote <command...>   (read-only kernel command over UDP remote IPC)");
    println!("  cargo xtask remote-tcp <command...>   (same, over the signed TCP line protocol)");
    println!(
        "  cargo xtask remote-key <caller>       (derive a caller's key from the master token)"
    );
    println!("  cargo xtask image");
    println!("  cargo xtask limine-fetch [--repo <url>] [--branch <name>] [--source <path>]");
    Err(io::Error::other("unknown xtask command").into())
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live under workspace root")
        .to_path_buf()
}

fn cmd_iso() -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let vendor = root.join(LIMINE_VENDOR_DIR);
    ensure_limine_files(&vendor)?;

    build_kernel(&root)?;
    let staging = stage_iso(&root, &vendor)?;
    build_iso(&root, &staging)?;
    install_limine(&root, &vendor)?;

    println!("ISO ready: {}", root.join(ISO_OUTPUT).display());
    Ok(())
}

/// vCPUs given to QEMU; the kernel brings the extra ones online at boot.
const QEMU_SMP: &str = "4";
/// Kernel UDP echo port, forwarded from the host loopback by QEMU.
const UDP_ECHO_PORT: u16 = 7777;
/// Kernel TCP echo port, forwarded from the host loopback by QEMU.
const TCP_ECHO_PORT: u16 = 7779;
/// Kernel signed-line command port over TCP.
const TCP_COMMAND_PORT: u16 = remote_ipc::line::KERNEL_COMMAND_PORT;
/// Kernel HTTP port.
const HTTP_PORT: u16 = 8080;
/// Kernel remote IPC port, forwarded from the host loopback by QEMU.
const REMOTE_PORT: u16 = remote_ipc::KERNEL_REMOTE_PORT;

/// The four forwarded ports, shifted by a base so several QEMU instances can
/// run at once (`qemu-script --port-base N`).
#[derive(Clone, Copy)]
struct Ports {
    udp_echo: u16,
    remote: u16,
    tcp_echo: u16,
    tcp_command: u16,
    http: u16,
}

impl Ports {
    fn with_base(base: u16) -> Self {
        Self {
            udp_echo: UDP_ECHO_PORT + base,
            remote: REMOTE_PORT + base,
            tcp_echo: TCP_ECHO_PORT + base,
            tcp_command: TCP_COMMAND_PORT + base,
            http: HTTP_PORT + base,
        }
    }

    /// The guest always listens on its own fixed ports; only the host side moves.
    fn hostfwd(&self) -> String {
        format!(
            "user,id=n0,hostfwd=udp:127.0.0.1:{}-:{UDP_ECHO_PORT},hostfwd=udp:127.0.0.1:{}-:{REMOTE_PORT},hostfwd=tcp:127.0.0.1:{}-:{TCP_ECHO_PORT},hostfwd=tcp:127.0.0.1:{}-:{TCP_COMMAND_PORT},hostfwd=tcp:127.0.0.1:{}-:{HTTP_PORT}",
            self.udp_echo, self.remote, self.tcp_echo, self.tcp_command, self.http
        )
    }
}

impl Default for Ports {
    fn default() -> Self {
        Self::with_base(0)
    }
}

/// UDP datagram transport for `remote_ipc` against the kernel's port.
struct UdpTransport {
    socket: std::net::UdpSocket,
    key: remote_ipc::CallerKey,
    port: u16,
}

/// Where `cargo xtask iso` leaves the secret it baked into the image.
const REMOTE_TOKEN_FILE: &str = "dist/remote-token";

/// The secret for the image currently in `dist/`, creating one if this is the
/// first build. Not in version control: it is per-machine, per-build.
fn provision_remote_token(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let path = root.join(REMOTE_TOKEN_FILE);
    if let Ok(existing) = fs::read_to_string(&path) {
        let existing = existing.trim().to_string();
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    // 128 bits from the OS, hex encoded so it survives a kernel command line.
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom").and_then(|mut f| {
        use std::io::Read;
        f.read_exact(&mut bytes)
    })?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, format!("{token}\n"))?;
    println!("remote secret written to {REMOTE_TOKEN_FILE} (keep it out of version control)");
    Ok(token)
}

/// Master secret for remote calls: `PANDAGEN_REMOTE_TOKEN`, else the secret
/// baked into the image in `dist/`, else the development default.
fn remote_token() -> String {
    if let Ok(from_env) = env::var("PANDAGEN_REMOTE_TOKEN") {
        return from_env;
    }
    if let Ok(from_file) = fs::read_to_string(repo_root().join(REMOTE_TOKEN_FILE)) {
        let trimmed = from_file.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    remote_ipc::DEFAULT_REMOTE_TOKEN.to_string()
}

/// Caller name for remote calls: `PANDAGEN_REMOTE_CALLER` or `xtask`.
fn remote_caller() -> String {
    env::var("PANDAGEN_REMOTE_CALLER").unwrap_or_else(|_| "xtask".to_string())
}

/// This caller's key: `PANDAGEN_REMOTE_KEY` (base64, handed out by an
/// operator with the master) or derived from `master`.
fn caller_key(master: &str) -> remote_ipc::CallerKey {
    let caller = remote_caller();
    if let Ok(encoded) = env::var("PANDAGEN_REMOTE_KEY") {
        if let Some(bytes) = remote_ipc::b64::decode(&encoded) {
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return remote_ipc::CallerKey { caller, key };
            }
        }
    }
    remote_ipc::CallerKey::derived(master.as_bytes(), &caller)
}

/// `cargo xtask remote-key <caller>`: derive a caller's key from the master.
fn cmd_remote_key(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let caller = args
        .next()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "remote-key expects a caller"))?;
    let key = remote_ipc::derive_caller_key(remote_token().as_bytes(), &caller);
    println!("PANDAGEN_REMOTE_CALLER={caller}");
    println!("PANDAGEN_REMOTE_KEY={}", remote_ipc::b64::encode(&key));
    Ok(())
}

impl remote_ipc::RemoteTransport for UdpTransport {
    fn send(&mut self, message: ipc::MessageEnvelope) -> Result<(), remote_ipc::RemoteIpcError> {
        let bytes = remote_ipc::envelope_to_bytes(&message, &self.key.caller, &self.key.key)?;
        self.socket
            .send_to(&bytes, ("127.0.0.1", self.port))
            .map(|_| ())
            .map_err(|err| remote_ipc::RemoteIpcError::Codec(err.to_string()))
    }

    fn receive(&mut self) -> Result<ipc::MessageEnvelope, remote_ipc::RemoteIpcError> {
        let mut buf = [0u8; 4096];
        // The kernel drops unauthenticated calls silently, so a timeout is
        // the expected symptom of a bad token; report it stably.
        let (n, _) = self.socket.recv_from(&mut buf).map_err(|err| {
            if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) {
                remote_ipc::RemoteIpcError::Codec("no reply (timeout)".to_string())
            } else {
                remote_ipc::RemoteIpcError::Codec(err.to_string())
            }
        })?;
        remote_ipc::envelope_from_bytes(&buf[..n], &self.key).map(|(envelope, _)| envelope)
    }
}

/// Run one read-only kernel command through remote IPC and return its output.
fn remote_call(
    command: &str,
    timeout: Duration,
    token: &str,
    port: u16,
) -> Result<String, Box<dyn std::error::Error>> {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0")?;
    socket.set_read_timeout(Some(timeout))?;
    let key = caller_key(token);
    let authority = remote_ipc::CapabilityAuthority {
        caller: key.caller.clone(),
        allowed_caps: vec![remote_ipc::CAP_KERNEL_COMMAND],
    };
    let mut client =
        remote_ipc::RemoteIpcClient::new(UdpTransport { socket, key, port }, authority);
    let reply = client.call(
        remote_ipc::CAP_KERNEL_COMMAND,
        remote_ipc::ACTION_KERNEL_COMMAND_RUN,
        command.as_bytes().to_vec(),
    )?;
    Ok(String::from_utf8_lossy(&reply).into_owned())
}

/// Round-trip one line over the kernel's TCP echo port and expect a clean close.
fn tcp_echo_check(
    text: &str,
    timeout: Duration,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, BufReader, Write as _};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = std::net::TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(format!("{text}\n").as_bytes())?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.trim_end() != text {
        return Err(io::Error::other(format!("echoed {line:?}")).into());
    }
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut rest = String::new();
    let n = reader.read_line(&mut rest)?;
    if n != 0 {
        return Err(io::Error::other(format!("unexpected data after close: {rest:?}")).into());
    }
    Ok(())
}

/// Send a signed call twice: the first must be answered, the replay must not.
fn remote_replay_check(
    command: &str,
    timeout: Duration,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0")?;
    socket.set_read_timeout(Some(timeout))?;
    let key = caller_key(&remote_token());
    let call = remote_ipc::RemoteCall {
        request_id: ipc::MessageId::new(),
        cap_id: remote_ipc::CAP_KERNEL_COMMAND,
        action: remote_ipc::ACTION_KERNEL_COMMAND_RUN.to_string(),
        payload: command.as_bytes().to_vec(),
        authority: remote_ipc::CapabilityAuthority {
            caller: key.caller.clone(),
            allowed_caps: vec![remote_ipc::CAP_KERNEL_COMMAND],
        },
    };
    let bytes =
        remote_ipc::envelope_to_bytes(&remote_ipc::encode_call(call)?, &key.caller, &key.key)?;
    let mut buf = [0u8; 4096];
    socket.send_to(&bytes, ("127.0.0.1", port))?;
    let (n, _) = socket.recv_from(&mut buf)?;
    let (reply, _) = remote_ipc::envelope_from_bytes(&buf[..n], &key)?;
    remote_ipc::decode_response(&reply)?
        .result
        .map_err(|err| io::Error::other(format!("first call failed: {err}")))?;
    socket.send_to(&bytes, ("127.0.0.1", port))?;
    match socket.recv_from(&mut buf) {
        Ok((n, _)) => Err(io::Error::other(format!("replay was answered with {} bytes", n)).into()),
        Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// A fresh nonce for the signed line protocol (time, pid, and a counter).
fn fresh_nonce() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let salt =
        ((std::process::id() as u128) << 64) | COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    nanos ^ salt.rotate_left(17) ^ ((nanos as u64 as u128) << 64)
}

/// Run one read-only kernel command over the signed TCP line protocol.
fn remote_tcp_call(
    command: &str,
    timeout: Duration,
    token: &str,
    port: u16,
) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::{BufRead, BufReader, Write as _};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = std::net::TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let line = remote_ipc::line::sign(&caller_key(token), fresh_nonce(), command);
    stream.write_all(format!("{line}\n").as_bytes())?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut reply = String::new();
    reader.read_line(&mut reply)?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    match remote_ipc::line::parse_reply(&reply) {
        Some(Ok(bytes)) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Some(Err(err)) => Err(io::Error::other(err).into()),
        None => Err(io::Error::other(format!("malformed reply {reply:?}")).into()),
    }
}

/// `cargo xtask remote-tcp <command...>`: call a running kernel over TCP.
fn cmd_remote_tcp(args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let command: Vec<String> = args.collect();
    if command.is_empty() {
        return Err(io::Error::new(ErrorKind::InvalidInput, "remote-tcp expects a command").into());
    }
    let reply = remote_tcp_call(
        &command.join(" "),
        Duration::from_secs(3),
        &remote_token(),
        Ports::default().tcp_command,
    )?;
    print!("{reply}");
    if !reply.ends_with('\n') {
        println!();
    }
    Ok(())
}

/// `cargo xtask remote <command...>`: call a running kernel over UDP.
fn cmd_remote(args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let command: Vec<String> = args.collect();
    if command.is_empty() {
        return Err(io::Error::new(ErrorKind::InvalidInput, "remote expects a command").into());
    }
    let reply = remote_call(
        &command.join(" "),
        Duration::from_secs(3),
        &remote_token(),
        Ports::default().remote,
    )?;
    print!("{reply}");
    if !reply.ends_with('\n') {
        println!();
    }
    Ok(())
}

fn cmd_qemu() -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let iso = root.join(ISO_OUTPUT);
    if !iso.exists() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("missing {ISO_OUTPUT}; run cargo xtask iso first"),
        )
        .into());
    }

    // Ensure disk image exists
    let disk = root.join(DISK_OUTPUT);
    if !disk.exists() {
        println!("Disk image not found, creating...");
        cmd_image()?;
    }

    // Ensure dist directory exists for serial log
    let dist = root.join("dist");
    fs::create_dir_all(&dist)?;

    // Phase 78: VGA text console mode
    // Route serial to file for debug logs, use QEMU display for main UI
    let serial_log = root.join("dist/serial.log");

    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║  PandaGen QEMU Boot (VGA Text Console Mode)              ║");
    println!("╠═══════════════════════════════════════════════════════════╣");
    println!("║  • UI is in the QEMU window (VGA text mode)              ║");
    println!("║  • Serial logs: dist/serial.log                          ║");
    println!("║  • Click QEMU window to capture keyboard                 ║");
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!();

    let display_backend = select_qemu_display();

    // Print command line for debugging
    let qemu_cmd = format!(
        "qemu-system-x86_64 -machine pc -smp {QEMU_SMP} -m 512M -cdrom {} -drive file={},format=raw,if=none,id=hd0 -device virtio-blk-pci,drive=hd0 -netdev user,id=n0,hostfwd=udp:127.0.0.1:{UDP_ECHO_PORT}-:{UDP_ECHO_PORT},hostfwd=udp:127.0.0.1:{REMOTE_PORT}-:{REMOTE_PORT},hostfwd=tcp:127.0.0.1:{TCP_ECHO_PORT}-:{TCP_ECHO_PORT},hostfwd=tcp:127.0.0.1:{TCP_COMMAND_PORT}-:{TCP_COMMAND_PORT} -device virtio-net-pci,netdev=n0 -serial file:{} -display {} -no-reboot",
        iso.display(),
        disk.display(),
        serial_log.display(),
        display_backend
    );
    println!("Running QEMU with command:");
    println!("  {}", qemu_cmd);
    println!();

    run(Command::new("qemu-system-x86_64")
        .current_dir(&root)
        .arg("-machine")
        .arg("pc")
        .arg("-smp")
        .arg(QEMU_SMP)
        .arg("-m")
        .arg("512M")
        .arg("-cdrom")
        .arg(&iso)
        .arg("-drive")
        .arg(format!("file={},format=raw,if=none,id=hd0", disk.display()))
        .arg("-device")
        .arg("virtio-blk-pci,drive=hd0")
        .arg("-netdev")
        .arg(Ports::default().hostfwd())
        .arg("-device")
        .arg("virtio-net-pci,netdev=n0")
        .arg("-serial")
        .arg(format!("file:{}", serial_log.display()))
        .arg("-display")
        .arg(display_backend)
        .arg("-no-reboot"))
}

fn cmd_qemu_smoke() -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let iso = root.join(ISO_OUTPUT);
    if !iso.exists() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("missing {ISO_OUTPUT}; run cargo xtask iso first"),
        )
        .into());
    }

    // Ensure disk image exists
    let disk = root.join(DISK_OUTPUT);
    if !disk.exists() {
        println!("Disk image not found, creating...");
        cmd_image()?;
    }

    // Ensure dist directory exists for serial log
    let dist = root.join("dist");
    fs::create_dir_all(&dist)?;

    let serial_log = root.join("dist/serial.log");

    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║  PandaGen QEMU Keyboard Smoke Test                       ║");
    println!("╠═══════════════════════════════════════════════════════════╣");
    println!("║  • Press any key in QEMU window to emit scancode         ║");
    println!("║  • Close QEMU window to finish the test                  ║");
    println!("║  • Serial logs: dist/serial.log                          ║");
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!();

    let display_backend = select_qemu_display();

    run(Command::new("qemu-system-x86_64")
        .current_dir(&root)
        .arg("-machine")
        .arg("pc")
        .arg("-smp")
        .arg(QEMU_SMP)
        .arg("-m")
        .arg("512M")
        .arg("-vga")
        .arg("std")
        .arg("-cdrom")
        .arg(&iso)
        .arg("-drive")
        .arg(format!("file={},format=raw,if=none,id=hd0", disk.display()))
        .arg("-device")
        .arg("virtio-blk-pci,drive=hd0")
        .arg("-netdev")
        .arg(Ports::default().hostfwd())
        .arg("-device")
        .arg("virtio-net-pci,netdev=n0")
        .arg("-serial")
        .arg(format!("file:{}", serial_log.display()))
        .arg("-display")
        .arg(display_backend)
        .arg("-no-reboot"))?;

    let log = fs::read_to_string(&serial_log).unwrap_or_default();
    if log.contains("kbd scancode=") {
        println!("QEMU smoke test: PASS (scancode observed)");
        Ok(())
    } else {
        Err(io::Error::other("QEMU smoke test: FAIL (no scancode log found)").into())
    }
}

/// Headless scripted QEMU session: boot the ISO with no display, inject keys
/// through the QEMU monitor, take screendumps, and check the serial log.
///
/// This is the bare-metal counterpart of `pandagend` key scripts: it lets the
/// real kernel image be exercised end to end without a human at the window.
/// Screendumps are written as PPM (`<out>.<name>.ppm`, plus `<out>.final.ppm`);
/// the serial log lands at `<out>.serial.log`.
///
/// Key spec entries are QEMU `sendkey` names (`h`, `ret`, `spc`, `esc`,
/// `shift-semicolon`, `ctrl-p`, ...) plus directives: `sleep:<secs>` pauses,
/// `shot:<name>` takes a screendump, `mouse:dx;dy[;dz]` moves the pointer,
/// and `mbtn:<mask>` sets the button state (1 left, 2 right, 4 middle).
fn cmd_qemu_script(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let root = repo_root();
    let iso = root.join(ISO_OUTPUT);
    if !iso.exists() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("missing {ISO_OUTPUT}; run cargo xtask iso first"),
        )
        .into());
    }
    let mut keys: Vec<String> = Vec::new();
    let mut boot_wait = 10.0f64;
    let mut after = 1.0f64;
    let mut out = root.join("dist/qemu_script");
    let mut expect_serial: Vec<String> = Vec::new();
    let mut allow_exception = false;
    let mut forbid_serial: Vec<String> = Vec::new();
    let mut port_base: u16 = 0;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next().ok_or_else(|| {
                io::Error::new(ErrorKind::InvalidInput, format!("{name} expects a value"))
            })
        };
        match arg.as_str() {
            "--keys" => keys.extend(
                value("--keys")?
                    .split(',')
                    .filter(|k| !k.is_empty())
                    .map(str::to_string),
            ),
            "--boot-wait" => boot_wait = value("--boot-wait")?.parse()?,
            "--after" => after = value("--after")?.parse()?,
            "--out" => out = root.join(value("--out")?),
            "--expect-serial" => expect_serial.push(value("--expect-serial")?.replace("\\n", "\n")),
            // The mirror of --expect-serial: the run fails if this appears.
            "--forbid-serial" => forbid_serial.push(value("--forbid-serial")?.replace("\\n", "\n")),
            "--allow-exception" => allow_exception = true,
            "--port-base" => port_base = value("--port-base")?.parse()?,
            other => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("unknown qemu-script argument: {other}"),
                )
                .into())
            }
        }
    }
    // Each port base gets its own disk image so concurrent runs do not
    // corrupt one another's filesystem.
    let base_disk = root.join(DISK_OUTPUT);
    if !base_disk.exists() {
        cmd_image()?;
    }
    let disk = if port_base == 0 {
        base_disk
    } else {
        let private = root.join(format!("dist/pandagen-{port_base}.disk"));
        if !private.exists() {
            fs::copy(&base_disk, &private)?;
        }
        private
    };
    let ports = Ports::with_base(port_base);

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let out_str = out.to_string_lossy().into_owned();
    let serial_log = PathBuf::from(format!("{out_str}.serial.log"));
    let _ = fs::remove_file(&serial_log);
    // AF_UNIX paths are short; keep the socket out of the (long) repo path.
    let sock = PathBuf::from(format!(
        "/tmp/pandagen-mon-{}-{}.sock",
        std::process::id(),
        port_base
    ));
    let _ = fs::remove_file(&sock);

    let mut child = Command::new("qemu-system-x86_64")
        .current_dir(&root)
        .arg("-machine")
        .arg("pc")
        .arg("-smp")
        .arg(QEMU_SMP)
        .arg("-m")
        .arg("512M")
        .arg("-cdrom")
        .arg(&iso)
        .arg("-drive")
        .arg(format!("file={},format=raw,if=none,id=hd0", disk.display()))
        .arg("-device")
        .arg("virtio-blk-pci,drive=hd0")
        .arg("-netdev")
        .arg(ports.hostfwd())
        .arg("-device")
        .arg("virtio-net-pci,netdev=n0")
        .arg("-serial")
        .arg(format!("file:{}", serial_log.display()))
        .arg("-display")
        .arg("none")
        .arg("-monitor")
        .arg(format!("unix:{},server,nowait", sock.display()))
        .arg("-no-reboot")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    std::thread::sleep(Duration::from_secs_f64(boot_wait));

    let mut monitor = UnixStream::connect(&sock)?;
    monitor.set_read_timeout(Some(Duration::from_millis(400)))?;
    let mut mon = |cmd: &str| -> io::Result<()> {
        monitor.write_all(cmd.as_bytes())?;
        monitor.write_all(b"\n")?;
        std::thread::sleep(Duration::from_millis(120));
        let mut sink = [0u8; 8192];
        let _ = monitor.read(&mut sink);
        Ok(())
    };
    mon("")?;

    let mut shots = Vec::new();
    let mut udp_failures: Vec<String> = Vec::new();
    let mut background_remote: Option<std::thread::JoinHandle<Result<String, String>>> = None;
    for key in &keys {
        if let Some(secs) = key.strip_prefix("sleep:") {
            std::thread::sleep(Duration::from_secs_f64(secs.parse()?));
        } else if let Some(name) = key.strip_prefix("shot:") {
            let path = format!("{out_str}.{name}.ppm");
            mon(&format!("screendump {path}"))?;
            shots.push(path);
        } else if let Some(motion) = key.strip_prefix("mouse:") {
            // mouse:dx;dy[;dz] -> relative motion (semicolons: commas split keys)
            let parts: Vec<&str> = motion.split(';').collect();
            let dx = parts.first().copied().unwrap_or("0");
            let dy = parts.get(1).copied().unwrap_or("0");
            let dz = parts.get(2).copied().unwrap_or("0");
            mon(&format!("mouse_move {dx} {dy} {dz}"))?;
            std::thread::sleep(Duration::from_millis(60));
        } else if let Some(spec) = key.strip_prefix("udp:") {
            // udp:<text> -> send <text> to the kernel's echo port from the
            // host and require the echo back within 2 s.
            let socket = std::net::UdpSocket::bind("127.0.0.1:0")?;
            socket.set_read_timeout(Some(Duration::from_secs(2)))?;
            socket.send_to(spec.as_bytes(), ("127.0.0.1", ports.udp_echo))?;
            let mut reply = [0u8; 2048];
            match socket.recv_from(&mut reply) {
                Ok((n, _)) if &reply[..n] == spec.as_bytes() => {
                    println!("udp echo ok: {spec}");
                }
                Ok((n, _)) => {
                    udp_failures.push(format!(
                        "<udp echo of {spec:?}, got {:?}>",
                        String::from_utf8_lossy(&reply[..n])
                    ));
                }
                Err(err) => udp_failures.push(format!("<udp echo of {spec:?}: {err}>")),
            }
        } else if let Some(text) = key.strip_prefix("tcp:") {
            // tcp:<text> -> connect to the kernel's TCP echo port, send the
            // line, require it back, then close and require EOF.
            match tcp_echo_check(text, Duration::from_secs(3), ports.tcp_echo) {
                Ok(()) => println!("tcp echo ok: {text}"),
                Err(err) => udp_failures.push(format!("<tcp echo of {text:?}: {err}>")),
            }
        } else if let Some(name) = key.strip_prefix("gauntlet:") {
            // gauntlet:<name> -> run gauntlet/<name>.py against this boot.
            // The script sees the forwarded ports in the environment and
            // fails the run by exiting non-zero.
            let script = root.join(format!("gauntlet/{name}.py"));
            if !script.exists() {
                udp_failures.push(format!("<gauntlet {name}: no {}>", script.display()));
            } else {
                let started = std::time::Instant::now();
                let output = Command::new("python3")
                    .arg(&script)
                    .current_dir(&root)
                    .env("PANDAGEN_UDP_PORT", ports.udp_echo.to_string())
                    .env("PANDAGEN_REMOTE_PORT", ports.remote.to_string())
                    .env("PANDAGEN_TCP_ECHO_PORT", ports.tcp_echo.to_string())
                    .env("PANDAGEN_TCP_COMMAND_PORT", ports.tcp_command.to_string())
                    .env("PANDAGEN_HTTP_PORT", ports.http.to_string())
                    .output()?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let secs = started.elapsed().as_secs_f64();
                if output.status.success() {
                    println!("gauntlet {name}: PASS ({secs:.1}s) {}", stdout.trim());
                } else {
                    println!("gauntlet {name}: FAIL ({secs:.1}s)");
                    for line in stdout.lines().chain(stderr.lines()) {
                        println!("    {line}");
                    }
                    udp_failures.push(format!(
                        "<gauntlet {name}: {}>",
                        stdout.lines().last().unwrap_or("failed").trim()
                    ));
                }
            }
        } else if let Some(spec) = key.strip_prefix("remote-tcp:") {
            // remote-tcp:<command>;<expected>[;<token>] like remote:, over TCP.
            let (command, rest) = spec.split_once(';').unwrap_or((spec, ""));
            let (expected, token) = match rest.split_once(';') {
                Some((expected, token)) => (expected, token.to_string()),
                None => (rest, remote_token()),
            };
            match (
                remote_tcp_call(command, Duration::from_secs(3), &token, ports.tcp_command),
                expected.strip_prefix('!'),
            ) {
                (Ok(reply), None) if reply.contains(expected) => {
                    println!("remote-tcp ok: {command} -> {}", reply.trim_end());
                }
                (Err(err), Some(want)) if err.to_string().contains(want) => {
                    println!("remote-tcp rejected as expected: {command} -> {err}");
                }
                (Ok(reply), _) => udp_failures.push(format!(
                    "<remote-tcp {command:?} reply {reply:?} vs {expected:?}>"
                )),
                (Err(err), _) => udp_failures.push(format!("<remote-tcp {command:?}: {err}>")),
            }
        } else if let Some(command) = key.strip_prefix("remote-tcp-bg:") {
            // remote-tcp-bg:<command> -> start the call on a thread so later
            // steps (mouse, keys) overlap it; remote-join waits for it.
            let command = command.to_string();
            let token = remote_token();
            let bg_port = ports.tcp_command;
            background_remote = Some(std::thread::spawn(move || {
                remote_tcp_call(&command, Duration::from_secs(10), &token, bg_port)
                    .map(|r| format!("{command} -> {}", r.trim_end()))
                    .map_err(|e| format!("{command}: {e}"))
            }));
        } else if key == "remote-join" {
            match background_remote.take().map(|h| h.join()) {
                Some(Ok(Ok(reply))) => println!("remote-tcp (background) ok: {reply}"),
                Some(Ok(Err(err))) => udp_failures.push(format!("<background {err}>")),
                Some(Err(_)) => udp_failures.push("<background remote panicked>".to_string()),
                None => udp_failures.push("<remote-join without remote-tcp-bg>".to_string()),
            }
        } else if let Some(command) = key.strip_prefix("replay:") {
            // replay:<command> -> send one signed call, expect a reply, then
            // resend the identical datagram and expect silence.
            match remote_replay_check(command, Duration::from_secs(3), ports.remote) {
                Ok(()) => println!("replay refused as expected: {command}"),
                Err(err) => udp_failures.push(format!("<replay {command:?}: {err}>")),
            }
        } else if let Some(spec) = key.strip_prefix("remote:") {
            // remote:<command>;<expected substring> -> remote IPC call from
            // the host; the reply must contain the expected text.
            // An expectation starting with '!' means the call must be
            // rejected with that error text.
            // A third field overrides the token (to test rejection).
            let (command, rest) = spec.split_once(';').unwrap_or((spec, ""));
            let (expected, token) = match rest.split_once(';') {
                Some((expected, token)) => (expected, token.to_string()),
                None => (rest, remote_token()),
            };
            match (
                remote_call(command, Duration::from_secs(3), &token, ports.remote),
                expected.strip_prefix('!'),
            ) {
                (Ok(reply), None) if reply.contains(expected) => {
                    println!("remote ok: {command} -> {}", reply.trim_end());
                }
                (Err(err), Some(want)) if err.to_string().contains(want) => {
                    println!("remote rejected as expected: {command} -> {err}");
                }
                (Ok(reply), _) => udp_failures.push(format!(
                    "<remote {command:?} reply {reply:?} vs {expected:?}>"
                )),
                (Err(err), _) => udp_failures.push(format!("<remote {command:?}: {err}>")),
            }
        } else if let Some(mask) = key.strip_prefix("mbtn:") {
            // mbtn:<mask> -> button state bitmask (1 left, 2 right, 4 middle)
            mon(&format!("mouse_button {mask}"))?;
            std::thread::sleep(Duration::from_millis(60));
        } else {
            mon(&format!("sendkey {key}"))?;
            std::thread::sleep(Duration::from_millis(60));
        }
    }
    std::thread::sleep(Duration::from_secs_f64(after));
    let final_path = format!("{out_str}.final.ppm");
    mon(&format!("screendump {final_path}"))?;
    shots.push(final_path);
    std::thread::sleep(Duration::from_millis(400));
    mon("quit")?;

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = fs::remove_file(&sock);

    let log = fs::read_to_string(&serial_log).unwrap_or_default();
    let mut missing = Vec::new();
    for needle in &expect_serial {
        if !log.contains(needle.as_str()) {
            missing.push(needle.clone());
        }
    }
    for needle in &forbid_serial {
        if log.contains(needle.as_str()) {
            missing.push(format!("<forbidden in serial: {needle:?}>"));
        }
    }
    if log.contains("KERNEL PANIC") {
        missing.push("<no kernel panic>".to_string());
    }
    if log.contains("KERNEL EXCEPTION") && !allow_exception {
        missing.push("<no kernel exception>".to_string());
    }
    missing.extend(udp_failures);
    if log.contains("framebuffer present rejected") {
        missing.push("<no rejected framebuffer present>".to_string());
    }

    println!("serial log: {}", serial_log.display());
    for shot in &shots {
        println!("screendump: {shot}");
    }
    if missing.is_empty() {
        println!("qemu-script: PASS");
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "qemu-script: FAIL, unmet expectations: {missing:?}"
        ))
        .into())
    }
}

fn select_qemu_display() -> String {
    if let Ok(value) = env::var("QEMU_DISPLAY") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if cfg!(target_os = "macos") {
        "cocoa".to_string()
    } else if cfg!(target_os = "linux") {
        "gtk".to_string()
    } else {
        "sdl".to_string()
    }
}

fn cmd_image() -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let dist = root.join("dist");
    fs::create_dir_all(&dist)?;

    let disk = root.join(DISK_OUTPUT);
    let size_bytes = DEFAULT_DISK_SIZE_MB * 1024 * 1024;

    // Create empty disk image
    println!(
        "Creating disk image: {} ({} MB)",
        disk.display(),
        DEFAULT_DISK_SIZE_MB
    );
    let disk_file = fs::File::create(&disk)?;
    disk_file.set_len(size_bytes as u64)?;

    println!("Disk image created: {}", disk.display());
    println!("  Size: {} MB ({} bytes)", DEFAULT_DISK_SIZE_MB, size_bytes);
    println!("  Blocks: {}", size_bytes / 4096);

    Ok(())
}

fn build_kernel(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    run(Command::new("cargo")
        .current_dir(root)
        .arg("build")
        .arg("-p")
        .arg(KERNEL_CRATE)
        .arg("--profile")
        .arg(KERNEL_PROFILE)
        .arg("--target")
        .arg(TARGET)
        .arg("-Zbuild-std=core,alloc"))
}

fn stage_iso(root: &Path, vendor: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let staging = root.join("target/iso_root");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }

    fs::create_dir_all(staging.join("boot"))?;
    fs::create_dir_all(staging.join("EFI/BOOT"))?;
    fs::create_dir_all(staging.join("limine"))?;

    // Every build gets its own remote secret, baked into the boot config and
    // left in dist/ for the client. The kernel refuses to open its remote
    // ports without one, so an image built from this repository is not
    // controllable by anyone who merely has the repository.
    let token = provision_remote_token(&root)?;
    let limine_conf = root.join("boot/limine.conf");
    let limine_cfg = root.join("boot/limine.cfg");
    let conf_text = fs::read_to_string(&limine_conf)?;
    let conf_text = conf_text.replace("@REMOTE_TOKEN@", &token);
    let staged_conf = staging.join("boot/limine.conf");
    if let Some(parent) = staged_conf.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&staged_conf, &conf_text)?;
    fs::write(staging.join("limine.conf"), &conf_text)?;
    let limine_dir = staging.join("limine");
    fs::create_dir_all(&limine_dir)?;
    fs::write(limine_dir.join("limine.conf"), &conf_text)?;
    copy_file(limine_cfg.clone(), staging.join("boot/limine.cfg"))?;
    copy_file(limine_cfg.clone(), staging.join("limine.cfg"))?;
    copy_file(limine_cfg, staging.join("limine/limine.cfg"))?;

    let kernel_path = root
        .join("target")
        .join(TARGET)
        .join(KERNEL_PROFILE)
        .join(KERNEL_CRATE);
    if !kernel_path.exists() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("missing kernel binary at {}", kernel_path.display()),
        )
        .into());
    }
    copy_file(kernel_path, staging.join("boot/kernel.elf"))?;

    let bios_sys = vendor.join("limine-bios.sys");
    copy_file(bios_sys.clone(), staging.join("boot/limine-bios.sys"))?;
    copy_file(bios_sys, staging.join("limine-bios.sys"))?;
    copy_file(
        vendor.join("limine-bios.sys"),
        staging.join("limine/limine-bios.sys"),
    )?;
    copy_file(
        vendor.join("limine-bios-cd.bin"),
        staging.join("boot/limine-bios-cd.bin"),
    )?;
    copy_file(
        vendor.join("limine-uefi-cd.bin"),
        staging.join("boot/limine-uefi-cd.bin"),
    )?;
    copy_file(
        vendor.join("BOOTX64.EFI"),
        staging.join("EFI/BOOT/BOOTX64.EFI"),
    )?;

    Ok(staging)
}

fn build_iso(root: &Path, staging: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let dist = root.join("dist");
    fs::create_dir_all(&dist)?;
    let iso = root.join(ISO_OUTPUT);

    run(Command::new("xorriso")
        .current_dir(root)
        .arg("-as")
        .arg("mkisofs")
        .arg("-R")
        .arg("-J")
        .arg("-joliet-long")
        .arg("-iso-level")
        .arg("3")
        .arg("-b")
        .arg("boot/limine-bios-cd.bin")
        .arg("-no-emul-boot")
        .arg("-boot-load-size")
        .arg("4")
        .arg("-boot-info-table")
        .arg("--efi-boot")
        .arg("boot/limine-uefi-cd.bin")
        .arg("-efi-boot-part")
        .arg("--efi-boot-image")
        .arg("--protective-msdos-label")
        .arg("-o")
        .arg(&iso)
        .arg(staging))
}

fn install_limine(root: &Path, vendor: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let iso = root.join(ISO_OUTPUT);
    let limine = vendor.join("limine");
    let limine_deploy = vendor.join("limine-deploy");

    if limine.exists() {
        // Try to run limine bios-install, but don't fail if it's not executable
        // (e.g., wrong architecture binary). UEFI boot will still work.
        match run(Command::new(limine)
            .current_dir(root)
            .arg("bios-install")
            .arg(&iso))
        {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!(
                    "Warning: limine bios-install failed ({}), continuing with UEFI-only boot",
                    e
                );
                return Ok(());
            }
        }
    }

    if limine_deploy.exists() {
        match run(Command::new(limine_deploy).current_dir(root).arg(&iso)) {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!(
                    "Warning: limine-deploy failed ({}), continuing with UEFI-only boot",
                    e
                );
                return Ok(());
            }
        }
    }

    eprintln!("Warning: no limine host utility found, ISO will be UEFI-only");
    Ok(())
}

fn ensure_limine_files(vendor: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let required = [
        "limine-bios.sys",
        "limine-bios-cd.bin",
        "limine-uefi-cd.bin",
        "BOOTX64.EFI",
    ];

    let mut missing = Vec::new();
    for file in required {
        if !vendor.join(file).exists() {
            missing.push(file);
        }
    }

    if !missing.is_empty() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!(
                "missing Limine files in {}: {:?} (run cargo xtask limine-fetch)",
                vendor.display(),
                missing
            ),
        )
        .into());
    }

    Ok(())
}

fn cmd_limine_fetch(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut repo = "https://codeberg.org/Limine/Limine.git".to_string();
    let mut branch = "v10.x-binary".to_string();
    let mut source: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repo" => {
                repo = args.next().ok_or_else(|| {
                    io::Error::new(ErrorKind::InvalidInput, "--repo expects a value")
                })?;
            }
            "--branch" => {
                branch = args.next().ok_or_else(|| {
                    io::Error::new(ErrorKind::InvalidInput, "--branch expects a value")
                })?;
            }
            "--source" => {
                let path = args.next().ok_or_else(|| {
                    io::Error::new(ErrorKind::InvalidInput, "--source expects a value")
                })?;
                source = Some(PathBuf::from(path));
            }
            _ => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("unknown argument: {arg}"),
                )
                .into());
            }
        }
    }

    let root = repo_root();
    let vendor = root.join(LIMINE_VENDOR_DIR);
    fs::create_dir_all(&vendor)?;

    let limine_root = if let Some(source) = source {
        source
    } else {
        let clone_dir = root.join("target/limine-src");
        if clone_dir.exists() {
            fs::remove_dir_all(&clone_dir)?;
        }

        run(Command::new("git")
            .current_dir(&root)
            .arg("clone")
            .arg("--depth=1")
            .arg("--branch")
            .arg(&branch)
            .arg(&repo)
            .arg(&clone_dir))?;

        clone_dir
    };

    copy_limine_assets(&limine_root, &vendor)?;
    println!("Limine assets copied to {}", vendor.display());
    Ok(())
}

fn copy_limine_assets(src: &Path, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let required = [
        "limine-bios.sys",
        "limine-bios-cd.bin",
        "limine-uefi-cd.bin",
        "BOOTX64.EFI",
    ];

    for file in required {
        let path = find_file(src, file).ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                format!("could not find {file} under {}", src.display()),
            )
        })?;
        copy_file(path, dest.join(file))?;
    }

    if let Some(path) = find_file(src, "limine") {
        copy_file(path, dest.join("limine"))?;
        make_executable(dest.join("limine"))?;
    } else if let Some(path) = find_file(src, "limine-deploy") {
        copy_file(path, dest.join("limine-deploy"))?;
        make_executable(dest.join("limine-deploy"))?;
    }

    if let Some(license) = find_file(src, "LICENSE") {
        copy_file(license, dest.join("LICENSE"))?;
    } else if let Some(license) = find_file(src, "COPYING") {
        copy_file(license, dest.join("LICENSE"))?;
    }

    Ok(())
}

fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(path);
            }
            if path.is_dir() {
                if path.file_name().and_then(|n| n.to_str()) == Some(".git") {
                    continue;
                }
                stack.push(path);
            }
        }
    }
    None
}

fn copy_file(src: PathBuf, dest: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&src, &dest)?;
    Ok(())
}

fn make_executable(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms)?;
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}

fn run(command: &mut Command) -> Result<(), Box<dyn std::error::Error>> {
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());

    let program = command.get_program().to_string_lossy().to_string();
    let status = match command.status() {
        Ok(status) => status,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(io::Error::new(
                ErrorKind::NotFound,
                format!("{program} not found; ensure it is installed and on PATH"),
            )
            .into());
        }
        Err(err) => return Err(err.into()),
    };
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("command failed with status {status}")).into())
    }
}
