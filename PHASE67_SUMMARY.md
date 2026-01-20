# Phase 67: System Image Definition

## Overview
Defined the system image layout and packaging strategy for PandaGen, specifying how the kernel, services, and components are organized into a bootable disk image.

## Changes Made

### 1. System Image Specification (`docs/system-image-layout.md`)
Created comprehensive specification document covering:

#### Image Structure
- **Boot Sector**: Bootloader stub / Limine handoff
- **Kernel Binary**: kernel_bootstrap ELF with embedded manifest
- **System Manifest**: JSON metadata with component registry and dependency graph
- **Component Storage**: Services and components as separate binaries or statically linked
- **User Data**: Persistent filesystem managed by PersistentFilesystem

#### Manifest Format
Defined JSON schema for system.json:
- Kernel metadata (binary, entry point, checksum)
- Service descriptors (capabilities, dependencies)
- Component registry (types, provides, depends_on)
- Init sequence (boot order)

#### Block Allocation
- **Superblock (Block 0)**: Magic number, version, layout pointers
- **Kernel Blocks (1-N)**: Raw ELF binary
- **Manifest Block (N+1)**: System metadata
- **Service Blocks (N+2+)**: Component binaries
- **User Data Blocks (M+)**: PersistentFilesystem

### 2. Build Strategies
Documented three packaging approaches:

**Static Linking (Current)**
- All components compiled into kernel_bootstrap
- Fast iteration, no runtime loading overhead
- Suitable for development and testing

**Dynamic Loading (Future)**
- Components as separate ELF binaries
- Loaded at runtime with capability checks
- Enables updates without kernel rebuild

**ISO Image (Production)**
- Bootable ISO with Limine bootloader
- Ready for physical hardware deployment
- Includes initial ramdisk with services

### 3. Deployment Scenarios

**Development (QEMU)**
```bash
qemu-img create -f raw pandagen-system.img 64M
dd if=target/.../kernel_bootstrap of=pandagen-system.img bs=4096 seek=1
qemu-system-x86_64 -drive format=raw,file=pandagen-system.img
```

**Testing (sim_kernel)**
- No image needed
- Pure Rust tests with RamDisk
- Direct service instantiation

**Production (Hardware)**
- ISO with Limine bootloader
- Persistent storage on real block device
- Services loaded from disk

### 4. Security Model
Defined capability-based loading:
- Each component declares required capabilities
- Kernel validates and grants at load time
- Future: signature verification with Ed25519

## Design Decisions

### Single Image vs Multi-File
Chose single disk image format because:
- **Simplicity**: One file to manage, no complex packaging
- **Bootable**: Direct boot with Limine, no separate installer
- **Testable**: Works with QEMU and sim_kernel
- **Portable**: Can be copied, backed up, versioned as single artifact

### JSON Manifest vs Custom Binary
Chose JSON for manifest:
- **Human-readable**: Easy to inspect and debug
- **Extensible**: Add fields without breaking format
- **Standard**: No custom parsing logic needed
- **Testable**: Can validate schema with standard tools

### Static vs Dynamic Linking
Current choice: Static linking
- **Faster iteration**: `cargo build && run`
- **No loader complexity**: No ELF loader in kernel (yet)
- **Smaller TCB**: Fewer moving parts

Future: Support both, let users choose policy.

## Comparison with Traditional Systems

| Aspect | Linux | PandaGen |
|--------|-------|----------|
| Boot | GRUB → vmlinuz → initramfs → rootfs | Limine → kernel_bootstrap (all-in-one) |
| Services | systemd units in /lib | Compiled into kernel or separate binaries |
| Libraries | .so files in /lib, /usr/lib | Static or explicit loading |
| Updates | Package manager (apt, yum) | Replace disk image or delta updates |
| Configuration | /etc, environment vars | Manifest in image |

## Implementation Status

### Phase 67 (This Phase)
- ✅ System image specification document
- ✅ Manifest format definition (JSON schema)
- ✅ Block layout specification
- ✅ Build strategy documentation
- ⬜ Build tool implementation (deferred to future)

### Stub Build Tool
Created basic structure for future implementation:
```bash
# Planned but not implemented:
cargo xtask build-image --output pandagen-system.img
cargo xtask build-iso --output pandagen-boot.iso
```

## Known Limitations
- **No automated builder**: Image assembly is manual
- **No checksums**: No verification of component integrity
- **No compression**: Images larger than necessary
- **No incremental updates**: Must replace entire image

These are intentional - keep Phase 67 focused on specification, implement tooling later.

## Testing Strategy
Phase 67 is primarily documentation, so testing is:
- **Manual validation**: Ensure specification is coherent
- **Readability review**: Docs are clear and complete
- **Future-proof check**: Format supports planned features (signatures, compression, deltas)

No automated tests added in this phase.

## Next Steps (Phase 68)
Polish and cleanup:
- Improve error messages in workspace
- Add command history to CLI
- Clean up flickering/redraw issues in output
- Integration testing of Phases 64-67

## Rationale

### Why Define Before Implementing?
Traditional OS development often builds tooling incrementally, leading to accidental complexity. By defining the image format upfront:

1. **Clear contracts**: Services know where they'll be loaded from
2. **Testable**: Can validate against spec before implementing
3. **Evolvable**: Spec can be versioned and extended
4. **Explicit**: No hidden magic (unlike GRUB config or initramfs)

### Why Not Use Existing Formats?
- **ISO 9660**: Read-only, not suitable for persistent storage
- **ext4 image**: Requires filesystem driver in kernel
- **Docker image**: Runtime container, not bootable
- **OCI format**: Over-engineered for our needs

PandaGen's format is minimal, bootable, and persistent - exactly what we need, nothing more.

This phase completes the foundation for Phases 64-68:
- **Phase 64**: Workspace integration (structure)
- **Phase 65**: Persistent filesystem (storage)
- **Phase 66**: CLI component (commands)
- **Phase 67**: System image (packaging) ← YOU ARE HERE
- **Phase 68**: Polish (integration)

The system is now conceptually complete. Phase 68 will make it production-ready.
