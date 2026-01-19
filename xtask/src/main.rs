use std::env;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const TARGET: &str = "x86_64-unknown-none";
const KERNEL_CRATE: &str = "kernel_bootstrap";
const LIMINE_VENDOR_DIR: &str = "third_party/limine";
const ISO_OUTPUT: &str = "dist/pandagen.iso";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("iso") => cmd_iso(),
        Some("qemu") => cmd_qemu(),
        Some("limine-fetch") => cmd_limine_fetch(args),
        _ => usage(),
    }
}

fn usage() -> Result<(), Box<dyn std::error::Error>> {
    println!("Usage:");
    println!("  cargo xtask iso");
    println!("  cargo xtask qemu");
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

    run(Command::new("qemu-system-x86_64")
        .current_dir(&root)
        .arg("-m")
        .arg("512M")
        .arg("-cdrom")
        .arg(&iso)
        .arg("-serial")
        .arg("stdio")
        .arg("-no-reboot"))
}

fn build_kernel(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    run(Command::new("cargo")
        .current_dir(root)
        .arg("build")
        .arg("-p")
        .arg(KERNEL_CRATE)
        .arg("--target")
        .arg(TARGET))
}

fn stage_iso(root: &Path, vendor: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let staging = root.join("target/iso_root");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }

    fs::create_dir_all(staging.join("boot"))?;
    fs::create_dir_all(staging.join("EFI/BOOT"))?;

    let limine_cfg = root.join("boot/limine.cfg");
    copy_file(limine_cfg.clone(), staging.join("boot/limine.cfg"))?;
    copy_file(limine_cfg, staging.join("limine.cfg"))?;

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
        return run(Command::new(limine)
            .current_dir(root)
            .arg("bios-install")
            .arg(&iso));
    }

    if limine_deploy.exists() {
        return run(Command::new(limine_deploy).current_dir(root).arg(&iso));
    }

    Err(io::Error::new(
        ErrorKind::NotFound,
        "missing limine host utility; provide third_party/limine/limine or limine-deploy",
    )
    .into())
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
        Err(io::Error::new(
            ErrorKind::Other,
            format!("command failed with status {status}"),
        )
        .into())
    }
}
