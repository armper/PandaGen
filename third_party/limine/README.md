# Limine Boot Files

This directory is intended to hold Limine bootloader artifacts used to build the bootable ISO.

Populate it with the required files by running:

```
cargo xtask limine-fetch
```

By default, `limine-fetch` clones the Limine binary release branch (`v10.x-binary`)
from the official repository on Codeberg. You can override this with `--repo` or
`--branch`, or provide a pre-downloaded directory via `--source`.

You can also provide a local Limine binary release directory:

```
cargo xtask limine-fetch --source /path/to/limine-binary
```

Required files (copied into this directory):
- limine-bios.sys
- limine-bios-cd.bin
- limine-uefi-cd.bin
- BOOTX64.EFI

If the Limine host utility is available, it will be copied as `limine` or `limine-deploy`.

Licensing: Limine is distributed under its own license. When you run `limine-fetch`, the
license file is copied here as `LICENSE`.
