# Phase 442: programs live on the disk, and arrive over the network (PROC-010)

## What changed

Programs run from the disk now, not from the boot image. A program is a
document: its image, kept in storage as `<name>.pgx`, of kind
"program", with versions like anything else a person keeps. The boot
image only brings programs to the disk. `install <url>` brings them from
anywhere else.

### From the boot image to the disk

At boot, before anything runs, each program the boot image carries is
**installed** if the disk has no program of that name, and **updated**
(written as a new version, the old one kept) if the disk's differs. An
identical copy is left alone, so a second boot writes nothing:
`programs: from the boot image, 0 installed, 0 updated`. Every `run`,
and every dock app, loads its image from the disk. The boot image is
the fallback only when the disk can't be read.

### Over the network

- **`install <url>`** fetches an image over HTTP or HTTPS (the same
  fetch the Web card uses, up to 64 KiB) and checks it exactly as the
  loader will before running it. Anything else is refused: a web page,
  a truncated image, a writable-and-runnable piece. It is then kept
  under the name the image carries: `install: hello (16 KiB, asks for
  console) installed; `run hello` starts it`. Installing over a program
  makes a new version.
- **`uninstall <name>`** puts the program in the bin, where Files can
  bring it back. A program in the bin is uninstalled: `run` refuses it,
  and `programs` doesn't list it.
- **`programs`** now lists what is installed on the disk, each with what
  it asks for (including a Documents pattern), then the built-in
  programs.

### In Files

A program document shows as **Program** and wears its dock app's icon
if it is one (the Calculator's, the Calendar's). **Enter on it runs
it**, with a card if the program asks for one, rather than opening its
bytes in a Notepad.

### Housekeeping

- The built-in ring-3 demo that was called `hello` is now **`probe`**
  (it probes a handle and a pointer it was not given), so the name
  `hello` is free for the first program installed over the network.
- `apps/hello` is built with the rest but listed in `INSTALL_ONLY`, so
  it is not on the boot image. xtask writes every image to
  `target/program-images/`, and the gauntlet's HTTP server offers them
  at `/programs/<name>.pgx`.
- Program names are interned: a program run again reuses its name
  rather than leaking a new one each time.

### `program_store.rs`

The policy, host-tested: a program's document name and the program a
document is (`calendar.pgx` → `calendar`; `../x.pgx` is not one); the
installed programs among a listing; the seed decision (install, update,
keep); the install check; a program described in a line.

## Tests

- `program_store`: a program is a document named for it; the boot image
  installs or updates and never rewrites the same; only a real image is
  installed (a web page and a truncated image are refused).
- `desk`: a program in Files is a Program with its dock app's icon, and
  Enter runs it without opening a Notepad.
- Gauntlet, "programs on disk: installed over the network, run,
  uninstalled": on a blank disk the boot image installs 8 programs;
  `install http://10.0.2.2:18084/programs/hello.pgx`, `run hello` (it
  says hello), `uninstall hello`, and `run hello` is refused. The reboot
  shape now expects its second boot to install nothing. The ring-3
  shape runs `probe`.
