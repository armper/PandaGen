use std::env;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    println!("                          [--after secs] [--out prefix] [--expect-serial text]");
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
        "qemu-system-x86_64 -machine pc -smp {QEMU_SMP} -m 512M -cdrom {} -drive file={},format=raw,if=none,id=hd0 -device virtio-blk-pci,drive=hd0 -netdev user,id=n0,hostfwd=udp:127.0.0.1:{UDP_ECHO_PORT}-:{UDP_ECHO_PORT} -device virtio-net-pci,netdev=n0 -serial file:{} -display {} -no-reboot",
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
        .arg(format!(
            "user,id=n0,hostfwd=udp:127.0.0.1:{UDP_ECHO_PORT}-:{UDP_ECHO_PORT}"
        ))
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
        .arg(format!(
            "user,id=n0,hostfwd=udp:127.0.0.1:{UDP_ECHO_PORT}-:{UDP_ECHO_PORT}"
        ))
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
    let disk = root.join(DISK_OUTPUT);
    if !disk.exists() {
        cmd_image()?;
    }

    let mut keys: Vec<String> = Vec::new();
    let mut boot_wait = 10.0f64;
    let mut after = 1.0f64;
    let mut out = root.join("dist/qemu_script");
    let mut expect_serial: Vec<String> = Vec::new();
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
            "--expect-serial" => expect_serial.push(value("--expect-serial")?),
            other => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("unknown qemu-script argument: {other}"),
                )
                .into())
            }
        }
    }
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let out_str = out.to_string_lossy().into_owned();
    let serial_log = PathBuf::from(format!("{out_str}.serial.log"));
    let _ = fs::remove_file(&serial_log);
    // AF_UNIX paths are short; keep the socket out of the (long) repo path.
    let sock = PathBuf::from(format!("/tmp/pandagen-mon-{}.sock", std::process::id()));
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
        .arg(format!(
            "user,id=n0,hostfwd=udp:127.0.0.1:{UDP_ECHO_PORT}-:{UDP_ECHO_PORT}"
        ))
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
            socket.send_to(spec.as_bytes(), ("127.0.0.1", UDP_ECHO_PORT))?;
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
    if log.contains("KERNEL PANIC") {
        missing.push("<no kernel panic>".to_string());
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

    let limine_conf = root.join("boot/limine.conf");
    let limine_cfg = root.join("boot/limine.cfg");
    copy_file(limine_conf.clone(), staging.join("boot/limine.conf"))?;
    copy_file(limine_conf.clone(), staging.join("limine.conf"))?;
    copy_file(limine_conf, staging.join("limine/limine.conf"))?;
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
