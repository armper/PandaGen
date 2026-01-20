# System Image Layout Specification

## Overview
A PandaGen system image is a self-contained bootable artifact containing the kernel, services, and components needed to run the system.

## Image Structure

```
pandagen-system.img (disk image)
├── Boot Sector (512 bytes)
│   └── Bootloader stub / handoff to Limine
├── Kernel Binary (blocks 1-N)
│   ├── kernel_bootstrap (ELF binary)
│   └── Embedded manifest
├── System Manifest (JSON metadata)
│   ├── Version
│   ├── Component registry
│   ├── Service descriptors
│   └── Dependency graph
└── Component Storage (blocks N+1 onwards)
    ├── Core Services
    │   ├── services_storage.so / services_storage (static)
    │   ├── services_network.so
    │   └── services_logger.so
    ├── Components
    │   ├── cli_console (static or .so)
    │   ├── services_editor_vi
    │   └── text_renderer_host
    └── User Data (persistent filesystem)
        └── (managed by PersistentFilesystem)
```

## Manifest Format

### System Manifest (system.json)
```json
{
  "format_version": 1,
  "kernel": {
    "binary": "kernel_bootstrap",
    "entry_point": "_start",
    "size_bytes": 524288,
    "checksum": "sha256:..."
  },
  "services": [
    {
      "name": "services_storage",
      "type": "native",
      "binary": "services_storage.elf",
      "capabilities": ["block_device", "persistent_storage"],
      "provides": ["storage_api"]
    },
    {
      "name": "services_network",
      "type": "native",
      "binary": "services_network.elf",
      "capabilities": ["network_device"],
      "provides": ["network_api"],
      "depends_on": []
    }
  ],
  "components": [
    {
      "name": "cli_console",
      "type": "native",
      "binary": "cli_console.elf",
      "depends_on": ["services_storage", "services_fs_view"],
      "provides": ["cli_interface"]
    }
  ],
  "init_sequence": [
    "services_storage",
    "services_logger",
    "services_network",
    "cli_console"
  ]
}
```

## Block Allocation

### Superblock Layout (Block 0)
```
Offset  Size    Field
0       8       Magic ("PANDAGEN")
8       4       Format version
12      4       Kernel size (blocks)
16      8       Kernel start block
24      8       Manifest start block
32      8       Data start block
40      8       Total blocks
48      464     Reserved
```

### Kernel Blocks (Blocks 1-N)
Raw ELF binary of kernel_bootstrap, padded to block boundary.

### Manifest Block (Block N+1)
JSON-encoded system manifest, padded to block boundary.

### Service Blocks (Blocks N+2 onwards)
Each service/component as separate ELF binary or statically linked.

### User Data Blocks (Blocks M onwards)
Managed by PersistentFilesystem with its own allocation scheme.

## Building a System Image

### Option 1: QEMU Disk Image (Development)
```bash
# Create empty disk image
qemu-img create -f raw pandagen-system.img 64M

# Write kernel to image
dd if=target/x86_64-unknown-none/debug/kernel_bootstrap of=pandagen-system.img bs=4096 seek=1 conv=notrunc

# Write manifest
dd if=system.json of=pandagen-system.img bs=4096 seek=128 conv=notrunc

# Boot with QEMU
qemu-system-x86_64 -drive format=raw,file=pandagen-system.img -bios /usr/share/OVMF/OVMF_CODE.fd
```

### Option 2: xtask Builder (Automated)
```bash
cargo xtask build-image --output pandagen-system.img
```

This would:
1. Build kernel in release mode
2. Build all services
3. Generate system manifest
4. Assemble image with correct block layout
5. Calculate checksums

### Option 3: ISO Image (Physical Hardware)
```bash
cargo xtask build-iso --output pandagen-boot.iso
```

Creates ISO image with:
- Limine bootloader
- Kernel binary
- System manifest
- Initial ramdisk with services

## Packaging Components

### Static Linking (Current)
All components compiled into kernel_bootstrap binary:
```rust
// kernel_bootstrap/src/main.rs
mod cli_console;
mod services_storage;
mod workspace;

fn init_components() {
    let cli = cli_console::PersistentCommandHandler::new(...);
    let storage = services_storage::init(...);
    // ...
}
```

### Dynamic Loading (Future)
Components as separate binaries loaded at runtime:
```rust
// kernel API for loading components
let cli_binary = fs.read_file("cli_console.elf")?;
let cli_handle = kernel.load_component(&cli_binary, capabilities)?;
```

## Deployment Scenarios

### Scenario 1: Development (QEMU)
- Single disk image with everything embedded
- Fast iteration: `cargo build && qemu-system-x86_64 ...`
- No persistent storage between reboots

### Scenario 2: Testing (sim_kernel)
- No image needed
- Pure Rust tests with RamDisk
- Instantiate services directly

### Scenario 3: Production (Physical Hardware)
- ISO image with Limine bootloader
- Persistent storage on real block device
- Services loaded from disk

## Security Considerations

### Capability-Based Loading
Each component/service declares required capabilities:
```json
{
  "name": "services_network",
  "capabilities": [
    "network_device:eth0",
    "port_range:1024-65535"
  ]
}
```

Kernel validates and grants capabilities at load time.

### Signature Verification (Future)
```json
{
  "name": "services_storage",
  "binary": "services_storage.elf",
  "signature": "ed25519:...",
  "public_key_id": "system_key_v1"
}
```

Kernel verifies signature before loading.

## Comparison with Traditional Systems

### Linux
- Initramfs + modules + rootfs (complex layering)
- GRUB/LILO bootloader (separate from kernel)
- /lib, /usr/lib for shared libraries (filesystem-dependent)

### PandaGen
- Single image with kernel + services
- Limine bootloader (minimal, UEFI-aware)
- No filesystem needed for boot (everything in blocks)

## Implementation Status

### Phase 67 (This Phase)
- ✅ Specification document (this file)
- ✅ Manifest format definition
- ⬜ Build tool (`cargo xtask build-image`) - stub only

### Phase 68 (Polish)
- ⬜ Automated image builder
- ⬜ Checksum verification
- ⬜ Documentation for adding new components

### Future Phases
- ⬜ Dynamic component loading
- ⬜ Signature verification
- ⬜ Incremental updates (delta images)
- ⬜ Multi-device support (network boot)

## Rationale

### Why Not Use Existing Tools?
- **Buildroot/Yocto**: Too POSIX-centric, assumes Linux kernel
- **Docker images**: Runtime containers, not bootable
- **ISO 9660**: Read-only, not suitable for persistent storage

PandaGen's image format is:
1. **Simple**: Single file, linear block layout
2. **Bootable**: Direct boot with Limine
3. **Persistent**: Includes filesystem for user data
4. **Testable**: Works with QEMU and sim_kernel

This aligns with PandaGen's core principle: mechanism over policy. The image format is just a mechanism; build tools can implement various policies.
