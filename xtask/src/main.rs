use std::env;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const TARGET: &str = "x86_64-unknown-none";
const KERNEL_CRATE: &str = "kernel_bootstrap";
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
    println!("  cargo xtask image");
    println!("  cargo xtask limine-fetch [--repo <url>] [--branch <name>] [--source <path>]");
    Err(io::Error::new(ErrorKind::Other, "unknown xtask command").into())
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

    // Print command line for debugging
    let qemu_cmd = format!(
        "qemu-system-x86_64 -machine pc -m 512M -cdrom {} -drive file={},format=raw,if=none,id=hd0 -device virtio-blk-pci,drive=hd0 -serial file:{} -display cocoa -no-reboot",
        iso.display(),
        disk.display(),
        serial_log.display()
    );
    println!("Running QEMU with command:");
    println!("  {}", qemu_cmd);
    println!();

    run(Command::new("qemu-system-x86_64")
        .current_dir(&root)
        .arg("-machine")
        .arg("pc")
        .arg("-m")
        .arg("512M")
        .arg("-cdrom")
        .arg(&iso)
        .arg("-drive")
        .arg(format!("file={},format=raw,if=none,id=hd0", disk.display()))
        .arg("-device")
        .arg("virtio-blk-pci,drive=hd0")
        .arg("-serial")
        .arg(format!("file:{}", serial_log.display()))
        .arg("-display")
        .arg("cocoa") // Can also be "gtk", "sdl" depending on platform
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

    run(Command::new("qemu-system-x86_64")
        .current_dir(&root)
        .arg("-machine")
        .arg("pc")
        .arg("-m")
        .arg("512M")
        .arg("-cdrom")
        .arg(&iso)
        .arg("-drive")
        .arg(format!("file={},format=raw,if=none,id=hd0", disk.display()))
        .arg("-device")
        .arg("virtio-blk-pci,drive=hd0")
        .arg("-serial")
        .arg(format!("file:{}", serial_log.display()))
        .arg("-display")
        .arg("cocoa")
        .arg("-no-reboot"))?;

    let log = fs::read_to_string(&serial_log).unwrap_or_default();
    if log.contains("kbd scancode=") {
        println!("QEMU smoke test: PASS (scancode observed)");
        Ok(())
    } else {
        Err(io::Error::new(
            ErrorKind::Other,
            "QEMU smoke test: FAIL (no scancode log found)",
        )
        .into())
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
        .join("debug")
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
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();
    let full_command = if args.is_empty() {
        program.clone()
    } else {
        format!("{} {}", program, args.join(" "))
    };

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
        Err(io::Error::new(
            ErrorKind::Other,
            format!("command `{}` failed with status {}", full_command, status),
        )
        .into())
    }
}
