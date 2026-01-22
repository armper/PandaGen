# Manual Testing Guide for Editor :w Persistence Fix

This document provides step-by-step instructions for manually testing the editor save functionality in QEMU after the fix for Phase 101.

## Prerequisites

Follow the instructions in `docs/qemu_boot.md` to set up the environment:
- Install Rust toolchain with `x86_64-unknown-none` target
- Install `xorriso` and `qemu-system-x86_64`
- Run `cargo xtask limine-fetch` to get bootloader files

## Build and Run

```bash
# Build the ISO
cargo xtask iso

# Run in QEMU (opens VGA text console window)
cargo xtask qemu
```

## Test Scenarios

### Test 1: Create New File, Edit, Save, Verify Persistence

**Goal**: Verify that `:w` saves content to disk and survives reboot.

**Steps**:
1. Boot PandaGen in QEMU
2. At the workspace prompt (`> `), type: `open editor hi.txt`
3. Expected output:
   ```
   File not found: hi.txt
   Starting with empty buffer [filesystem available]
   Keys: i=insert, Esc=normal, h/j/k/l=move, :q=quit, :w=save
   ```
   - The key message is **"[filesystem available]"** - confirms editor has IO adapter
4. Press `i` to enter INSERT mode
5. Type: `Hello from PandaGen!`
6. Press `Esc` to return to NORMAL mode
7. Type `:w` and press `Enter`
8. Expected status line: `Saved to hi.txt`
   - If you see "Filesystem unavailable", the fix didn't work
   - If you see "Error: failed to save file", there's a storage issue
9. Type `:q` to quit editor
10. At workspace prompt, type: `cat hi.txt`
11. Expected output: `Hello from PandaGen!`
12. Type: `reboot` (or restart QEMU)
13. After reboot, type: `cat hi.txt`
14. Expected output: `Hello from PandaGen!` (proves persistence)

**Success Criteria**: File content survives reboot.

### Test 2: Open Existing File, Edit, Save

**Goal**: Verify editing and saving an existing file works.

**Steps**:
1. Assuming `hi.txt` exists from Test 1
2. Type: `open editor hi.txt`
3. Expected output:
   ```
   Opened: hi.txt [filesystem available]
   Keys: i=insert, Esc=normal, h/j/k/l=move, :q=quit, :w=save
   ```
4. The file content should be visible in the editor
5. Press `i`, add more text: ` Second line.`
6. Press `Esc`, type `:w`, press `Enter`
7. Expected status: `Saved to hi.txt`
8. Type `:q`
9. Type: `cat hi.txt`
10. Expected output: `Hello from PandaGen! Second line.`

**Success Criteria**: Changes are persisted immediately (no need to reboot to verify).

### Test 3: Save-As (`:w <path>`)

**Goal**: Verify save-as creates a new file.

**Steps**:
1. Type: `open editor`
2. Expected output:
   ```
   New buffer [filesystem available]
   Keys: i=insert, Esc=normal, h/j/k/l=move, :q=quit, :w=save
   ```
3. Press `i`, type: `This is a test.`
4. Press `Esc`, type `:w test.txt`, press `Enter`
5. Expected status: `Saved as test.txt`
6. Type `:q`
7. Type: `cat test.txt`
8. Expected output: `This is a test.`

**Success Criteria**: Save-as creates new file and subsequent `:w` updates it.

### Test 4: No Filesystem Warning

**Goal**: Verify graceful handling when filesystem is unavailable.

**Note**: This test requires modifying the code to simulate no filesystem. In normal operation, filesystem should always be available.

**Steps** (for developers only):
1. Edit `kernel_bootstrap/src/main.rs`, comment out the line that calls `workspace.set_filesystem(fs)`
2. Rebuild ISO
3. Run QEMU
4. Type: `open editor test.txt`
5. Expected output: `Warning: No filesystem - :w will not work`
6. Press `i`, type text, press `Esc`, type `:w`, press `Enter`
7. Expected status: `Filesystem unavailable` (graceful degradation)

**Success Criteria**: Editor doesn't crash, shows clear error message.

## Debugging Tips

### Check Serial Logs

The serial log file `dist/serial.log` contains detailed debug output:

```bash
tail -f dist/serial.log
```

Look for these debug markers:
- `route_input:` - Shows keyboard input routing
- `action=process_byte_start` / `action=process_byte_end` - Editor input processing
- Any error messages or panics

### Status Messages

The key diagnostic messages are:
- **"[filesystem available]"** - Editor has IO adapter attached (good)
- **"Warning: No filesystem"** - Filesystem not available (bad for normal operation)
- **"Saved to <path>"** - Save succeeded
- **"Saved as <path>"** - Save-as succeeded
- **"Filesystem unavailable"** - Editor has no IO adapter (indicates bug)
- **"Error: failed to save file"** - Storage operation failed

### Common Issues

**Issue**: `:w` shows "Filesystem unavailable" even after fix
- Check that the status message when opening editor includes "[filesystem available]"
- If not, the editor_io wasn't attached - bug in workspace.rs

**Issue**: `:w` shows "Error: failed to save file"
- Storage layer issue, not an editor IO lifecycle issue
- Check serial logs for storage errors

**Issue**: File doesn't exist after reboot
- RamDisk is non-persistent by default
- This is expected if using RamDisk (in-memory storage)
- For true persistence, need to wire VirtioBlkDevice

## Notes

- The current implementation uses `RamDisk` (32MB in-memory storage)
- For reboot persistence testing, you need to wire a persistent block device
- The fix ensures the editor *can* save; actual persistence depends on storage backend
- See `kernel_bootstrap/src/bare_metal_storage.rs` line 26 comment about VirtioBlkDevice

## Expected Test Results (Phase 101)

After this fix:
- ✅ Test 1: Should PASS (editor saves to disk)
- ✅ Test 2: Should PASS (editing existing file works)
- ✅ Test 3: Should PASS (save-as works)
- ✅ Test 4: Should PASS (graceful degradation when no FS)
- ⚠️ Reboot persistence: Depends on storage backend (RamDisk = not persistent)

## Reporting Issues

If any test fails, include:
1. Which test scenario failed
2. Exact error message in editor status line
3. Output of `cat hi.txt` (if applicable)
4. Last 50 lines of `dist/serial.log`
5. Steps to reproduce
