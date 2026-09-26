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
        Some("gauntlet") => cmd_gauntlet(args),
        Some("wallpaper") => cmd_wallpaper(args),
        _ => usage(),
    }
}

/// The whole verification, in one command that cannot quietly pass.
///
/// Every step's exit status is checked. A run once went green against a
/// stale image because I read the build log for "ISO ready" instead of
/// looking at the status, so this exists to make that mistake unavailable.
/// Judges are discovered from the gauntlet directory rather than listed, so
/// a new one is included the moment it is written.
/// One pixel the final screendump must show.
#[derive(Debug, Clone)]
struct PixelExpectation {
    x: usize,
    y: usize,
    rgb: [u8; 3],
    tolerance: u8,
}

impl std::str::FromStr for PixelExpectation {
    type Err = Box<dyn std::error::Error>;

    fn from_str(spec: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = spec.split(',').collect();
        if parts.len() != 5 && parts.len() != 6 {
            return Err(format!("--expect-pixel wants x,y,r,g,b[,tolerance], got {spec:?}").into());
        }
        Ok(Self {
            x: parts[0].trim().parse()?,
            y: parts[1].trim().parse()?,
            rgb: [
                parts[2].trim().parse()?,
                parts[3].trim().parse()?,
                parts[4].trim().parse()?,
            ],
            tolerance: parts
                .get(5)
                .map(|t| t.trim().parse())
                .transpose()?
                .unwrap_or(4),
        })
    }
}

impl PixelExpectation {
    /// `None` when the image shows the expected colour, else what it showed.
    fn check(&self, image: &Ppm) -> Option<String> {
        let Some(actual) = image.pixel(self.x, self.y) else {
            return Some(format!(
                "({},{}) is outside the {}x{} screendump",
                self.x, self.y, image.width, image.height
            ));
        };
        let off = actual
            .iter()
            .zip(self.rgb.iter())
            .any(|(a, e)| (*a as i32 - *e as i32).unsigned_abs() > self.tolerance as u32);
        off.then(|| {
            format!(
                "({},{}) expected rgb({},{},{}) got rgb({},{},{})",
                self.x,
                self.y,
                self.rgb[0],
                self.rgb[1],
                self.rgb[2],
                actual[0],
                actual[1],
                actual[2]
            )
        })
    }
}

/// A binary PPM (P6), which is what QEMU's `screendump` writes.
struct Ppm {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl Ppm {
    fn pixel(&self, x: usize, y: usize) -> Option<[u8; 3]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = (y * self.width + x) * 3;
        self.data.get(at..at + 3).map(|p| [p[0], p[1], p[2]])
    }
}

fn read_ppm(path: &str) -> Result<Ppm, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    // Header: "P6", whitespace, width, height, maxval, one whitespace byte,
    // then the pixels. Comments are not something QEMU writes.
    let mut fields = Vec::new();
    let mut at = 0;
    while fields.len() < 4 && at < bytes.len() {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
    }
    if fields.len() != 4 || fields[0] != "P6" || fields[3] != "255" {
        return Err(format!("{path}: not an 8-bit P6 ppm").into());
    }
    let width: usize = fields[1].parse()?;
    let height: usize = fields[2].parse()?;
    let data = bytes.get(at + 1..).ok_or("ppm truncated")?.to_vec();
    if data.len() < width * height * 3 {
        return Err(format!("{path}: {} bytes for {width}x{height}", data.len()).into());
    }
    Ok(Ppm {
        width,
        height,
        data,
    })
}

/// Held for the duration of a gauntlet run.
///
/// Every stage of the suite writes the same three paths -- `dist/pandagen.iso`,
/// `dist/pandagen.disk` and the forwarded host ports -- so two runs at once
/// silently corrupt one another: one reformats the disk the other is booting
/// from, and the loser fails somewhere unrelated with an error that names
/// none of this. A wrong answer from the verifier is worse than no answer,
/// so refuse the second run instead of letting it interleave.
struct GauntletLock(PathBuf);

impl GauntletLock {
    fn acquire(root: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let path = root.join("dist/.gauntlet.lock");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write as _;
                let _ = writeln!(file, "{}", std::process::id());
                Ok(Self(path))
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                let holder = fs::read_to_string(&path).unwrap_or_default();
                Err(format!(
                    "another gauntlet run is in progress (pid {}); \
                     it owns dist/pandagen.iso, dist/pandagen.disk and the \
                     forwarded ports. Wait for it, or remove {} if no run is \
                     alive.",
                    holder.trim(),
                    path.display()
                )
                .into())
            }
            Err(err) => Err(err.into()),
        }
    }
}

impl Drop for GauntletLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// The port base every gauntlet boot uses, leaving base 0 -- the default
/// forwarded ports -- for a person's own `cargo xtask qemu` session.
const GAUNTLET_PORT_BASE: u32 = 1;

fn cmd_gauntlet(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let _lock = GauntletLock::acquire(&root)?;
    // `--boots-only`: the image and the boots, without the tests before
    // them -- for going round again on a boot that failed (DEV-001).
    let boots_only = args.any(|a| a == "--boots-only");
    let began = std::time::Instant::now();
    if !boots_only {
        println!("== cargo test --workspace");
        let status = Command::new("cargo")
            .current_dir(&root)
            .args(["test", "--workspace"])
            .status()?;
        if !status.success() {
            return Err("workspace tests failed".into());
        }

        // `--workspace` builds default features only, so anything behind a
        // non-default feature is never compiled, let alone tested. `hal_mode`
        // held a real defect for exactly that reason.
        println!("== cargo test --workspace --all-features");
        let status = Command::new("cargo")
            .current_dir(&root)
            .args(["test", "--workspace", "--all-features"])
            .status()?;
        if !status.success() {
            return Err("workspace tests with all features failed".into());
        }

        // Every crate on its own. Cargo unifies features across a workspace
        // build, so a crate that does not declare what it actually needs
        // compiles anyway as long as some *other* member happens to turn the
        // feature on. Two crates were in that state: `workspace_access` and
        // `console_vga`, the latter declaring serde only as a dev-dependency
        // while its library derives Serialize. `--workspace` was green for both.
        println!("== cargo check, one crate at a time");
        // `cargo metadata`'s JSON has `name` on targets and dependencies too,
        // so ask for the package list in a form with one name per line.
        let members = Command::new("cargo")
            .current_dir(&root)
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .output()?;
        let metadata = String::from_utf8_lossy(&members.stdout);
        // Every workspace member appears in `"workspace_members"` as an id whose
        // first token is the package name.
        let mut names: Vec<String> = Vec::new();
        if let Some(list) = metadata
            .split_once("\"workspace_members\":[")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(list, _)| list)
        {
            for entry in list.split(',') {
                let entry = entry.trim().trim_matches('"');
                // Ids look like `path+file:///…/crate#1.0.0` or `crate 1.0.0 (…)`.
                let name = entry
                    .rsplit_once('#')
                    .map(|(_, tail)| match tail.split_once('@') {
                        Some((name, _)) => name,
                        None => {
                            // `#1.0.0` means the name is the last path segment.
                            entry
                                .split('#')
                                .next()
                                .and_then(|p| p.rsplit('/').next())
                                .unwrap_or(tail)
                        }
                    })
                    .unwrap_or_else(|| entry.split(' ').next().unwrap_or(entry));
                if !name.is_empty() && !names.iter().any(|seen| seen == name) {
                    names.push(name.to_string());
                }
            }
        }
        if names.is_empty() {
            return Err("could not list workspace members".into());
        }
        println!("   {} crates", names.len());
        let mut alone_failed = Vec::new();
        for name in &names {
            let status = Command::new("cargo")
                .current_dir(&root)
                .args(["check", "-p", name, "--all-targets", "--quiet"])
                .status()?;
            if !status.success() {
                alone_failed.push(name.clone());
            }
        }
        if !alone_failed.is_empty() {
            return Err(format!(
                "these crates do not build on their own, and pass only through \
             workspace feature unification: {}",
                alone_failed.join(", ")
            )
            .into());
        }
    }
    println!(
        "== cargo xtask iso ({:.0}s in)",
        began.elapsed().as_secs_f64()
    );
    cmd_iso()?;

    // Every boot below is a shape, run by `run_shapes`: several at once,
    // each on its own ports and its own blank disk (DEV-001).
    let mut shapes: Vec<Shape> = Vec::new();
    let mut title: String;

    let mut judges: Vec<String> = fs::read_dir(root.join("gauntlet"))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            // `_harness` and `_sign` are libraries, not judges.
            let stem = name.strip_suffix(".py")?;
            if stem.starts_with('_') {
                return None;
            }
            Some(format!("gauntlet:{stem}"))
        })
        .collect();
    if judges.is_empty() {
        return Err("no gauntlet judges found; the suite cannot pass vacuously".into());
    }
    judges.sort();
    title = format!("{} judges: {}", judges.len(), judges.join(" "));

    let keys = format!("sleep:3,remote:cpus;online=,{},sleep:1", judges.join(","));
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        keys,
        "--expect-serial".to_string(),
        "flush=negotiated".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    // A machine without a working 8254. The LAPIC calibration used to wait
    // for a PIT tick that never came, so the boot CPU stopped for good after
    // "PIT configured for 100 Hz" -- no prompt, no message, no recovery.
    // It must reach the workspace either way.
    // A machine with a fraction of the memory. The heap asked for 32 MiB of
    // contiguous frames and gave up if it could not have it, then carried on
    // for ten more steps and died in the global allocator. It must either
    // run on a smaller heap or refuse legibly -- never abort.
    // A ping to an unreachable address holds the network lock while it
    // spins. The machine must keep serving and must not wedge.
    title = "serve while a ping is in flight".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // The machine boots into the desk, where typing is a palette
        // search: the console is a card, so open one (Ctrl+T) and type
        // into it. Before this the keys went to the palette and the ping
        // never ran -- and nothing here noticed, because nothing asked.
        "sleep:4,ctrl-t,sleep:1,n,e,t,spc,p,i,n,g,spc,1,0,dot,9,9,dot,9,9,dot,9,9,ret,\
         gauntlet:http_health,remote-tcp:cpus;online=,sleep:6"
            .to_string(),
        "--out".to_string(),
        "dist/qemu_ping".to_string(),
        "--expect-serial".to_string(),
        "WS > net ping 10.99.99.99".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "boot on a small machine".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--memory".to_string(),
        "16M".to_string(),
        "--keys".to_string(),
        "sleep:8".to_string(),
        "--out".to_string(),
        "dist/qemu_lowmem".to_string(),
        "--expect-serial".to_string(),
        "PandaGen Workspace".to_string(),
        "--forbid-serial".to_string(),
        "ALLOCATION ERROR".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    // More CPUs than there are per-CPU tables. Those without a GDT and TSS
    // must park rather than join: a double fault on one of them would load
    // RSP = 0 and triple-fault the machine.
    // A different chipset, so PCI enumeration is exercised against
    // something other than the one topology the default machine has.
    // Two CPUs: the sole application processor used to submit jobs and then
    // wait on itself, so the console answered nothing for twenty seconds.
    // It must answer, whichever way.
    title = "smp run on a two-CPU machine".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--smp".to_string(),
        "2".to_string(),
        "--keys".to_string(),
        // Into a Terminal card; see the ping shape.
        "sleep:4,ctrl-t,sleep:1,s,m,p,spc,r,u,n,spc,2,ret,sleep:3".to_string(),
        "--out".to_string(),
        "dist/qemu_smp2".to_string(),
        // The banner alone is not an assertion: it is printed long before
        // the command runs, so the step passed whether `smp run` answered
        // or hung for its full timeout. Require the answer.
        "--expect-serial".to_string(),
        "PandaGen Workspace".to_string(),
        "--expect-serial".to_string(),
        "smp: no application processor free to run jobs".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "boot on q35".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--machine".to_string(),
        "q35".to_string(),
        "--keys".to_string(),
        "sleep:6".to_string(),
        "--out".to_string(),
        "dist/qemu_q35".to_string(),
        "--expect-serial".to_string(),
        "backend: virtio-blk-pci".to_string(),
        "--expect-serial".to_string(),
        "net: virtio-net-pci".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "boot with more CPUs than tables".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--smp".to_string(),
        "16".to_string(),
        "--keys".to_string(),
        "sleep:6".to_string(),
        "--out".to_string(),
        "dist/qemu_smp16".to_string(),
        "--expect-serial".to_string(),
        "PandaGen Workspace".to_string(),
        "--forbid-serial".to_string(),
        "[timeout]".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    // The desk (GFX-050): switch to it, launch Notepad from the keyboard,
    // type into it, and check the pixels a card, a dock and a top bar must
    // put on the screen. The card sits at a known cascade position, so the
    // focus ring and the surface can be asserted by coordinate.
    title = "desk: a card, a dock and a top bar".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // Notepad first, then a Terminal on top of it: the console as a
        // card, with `help` run in it.
        "sleep:6,ctrl-n,sleep:1,h,e,l,l,o,sleep:1,ctrl-t,sleep:1,h,e,l,p,ret,sleep:2,shot:desk"
            .to_string(),
        "--out".to_string(),
        "dist/qemu_desk".to_string(),
        // The machine boots into the desk; the boot log says so, by label.
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // Top bar: frosted glass over the blurred wallpaper (GFX-114), four
        // pixels down, mid-screen.
        "--expect-pixel".to_string(),
        "640,4,35,52,73".to_string(),
        // Dock capsule (GFX-114): liquid glass over the blurred wallpaper, in
        // the gap between the first two tiles (tile 0 at x=249..313, 64
        // wide, 6 apart), within a few levels -- the blur is the
        // wallpaper's, and the wallpaper is fixed.
        "--expect-pixel".to_string(),
        "316,740,54,56,54,8".to_string(),
        // The Notepad card: 720 wide, centred, first cascade step, at y=44.
        // It has lost focus to the Terminal, so its ring is the hairline...
        "--expect-pixel".to_string(),
        "300,44,48,56,72".to_string(),
        // ...and its header, between the title and the action chips, is
        // the plain surface. (This sat at x=880 until the chips arrived
        // there, and at x=600 until the Replace chip, GFX-090, pushed them
        // further left.)
        "--expect-pixel".to_string(),
        "500,56,28,34,48".to_string(),
        // The Terminal card: 800 wide, second cascade step (x=272, y=76),
        // focused, so its top edge is the accent ring.
        "--expect-pixel".to_string(),
        "400,76,52,211,153".to_string(),
        "--expect-pixel".to_string(),
        "900,88,28,34,48".to_string(),
        // The console answered `help` inside the card: the serial log mirrors
        // its output.
        "--expect-serial".to_string(),
        "WS > help".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: the Calculator from the palette, worked with the keyboard".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // Open the Calculator through the palette, type 12*3, Enter.
        "sleep:6,ctrl-spc,sleep:1,c,a,l,c,ret,sleep:2,1,2,shift-8,3,ret,sleep:2,shot:calc"
            .to_string(),
        "--out".to_string(),
        "dist/qemu_desk_calc".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // The card: 300 wide, centred (x=490), first cascade step at y=44,
        // focused, so its top edge is the accent ring...
        "--expect-pixel".to_string(),
        "640,44,52,211,153".to_string(),
        // ...the "=" key (row 4, column 3 of the grid at canvas 498,76) is
        // an accent fill, checked away from its glyph...
        "--expect-pixel".to_string(),
        "725,370,52,211,153".to_string(),
        // ...and the "5" key is the raised surface (GFX-081).
        "--expect-pixel".to_string(),
        "583,270,22,28,40".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: the Calendar from the palette, a month on screen".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // Open the Calendar through the palette; PageDown turns the month.
        "sleep:6,ctrl-spc,sleep:1,c,a,l,e,n,ret,sleep:2,pgdn,sleep:2,shot:calendar".to_string(),
        "--out".to_string(),
        "dist/qemu_desk_calendar".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // The card: 420 wide, centred (x=430), first cascade step at y=44,
        // focused, so its top edge is the accent ring...
        "--expect-pixel".to_string(),
        "640,44,52,211,153".to_string(),
        // ...and a day cell (Sunday of the second week, canvas 438,76) is
        // the raised surface, left of its number (GFX-083).
        "--expect-pixel".to_string(),
        "800,200,22,28,40".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: the Timer from the palette, a stopwatch running".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // Open the Timer through the palette, start the stopwatch, lap.
        "sleep:6,ctrl-spc,sleep:1,t,i,m,e,r,ret,sleep:2,spc,sleep:2,l,sleep:1,shot:timer"
            .to_string(),
        "--out".to_string(),
        "dist/qemu_desk_timer".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // The card: 400 wide, centred (x=440), first cascade step at y=44,
        // focused, so its top edge is the accent ring...
        "--expect-pixel".to_string(),
        "640,44,52,211,153".to_string(),
        // ...and the Reset key (third of three on the controls row, canvas
        // 456,76) is the raised surface, checked right of its label
        // (GFX-082).
        "--expect-pixel".to_string(),
        "830,200,22,28,40".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: Tiles from the palette, four moves".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        "sleep:6,ctrl-spc,sleep:1,t,i,l,e,s,ret,sleep:2,left,up,right,down,sleep:1,shot:tiles"
            .to_string(),
        "--out".to_string(),
        "dist/qemu_desk_tiles".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // The card: 340 wide, centred (x=470), first cascade step at y=44,
        // focused, so its top edge is the accent ring...
        "--expect-pixel".to_string(),
        "640,44,52,211,153".to_string(),
        // ...and its body, right of the board, the plain surface.
        "--expect-pixel".to_string(),
        "795,200,28,34,48".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: Tasks from the palette, one added and saved".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        "sleep:6,ctrl-spc,sleep:1,t,a,s,k,s,ret,sleep:2,a,m,i,l,k,ret,sleep:3,shot:tasks"
            .to_string(),
        "--out".to_string(),
        "dist/qemu_desk_tasks".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // The card: 460 wide, centred (x=410), first cascade step at y=44,
        // focused, so its top edge is the accent ring...
        "--expect-pixel".to_string(),
        "640,44,52,211,153".to_string(),
        // ...and its body, right of the text, the plain surface.
        "--expect-pixel".to_string(),
        "850,200,28,34,48".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: Sketch from the palette, the colour swatch".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // Open Sketch, press C once: the toolbar's ring moves to the red
        // swatch (GFX-106).
        "sleep:6,ctrl-spc,sleep:1,s,k,e,t,c,h,ret,sleep:2,c,sleep:1,shot:sketch".to_string(),
        "--out".to_string(),
        "dist/qemu_desk_sketch".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // The card: 520 wide, centred (x=380), first cascade step at y=44,
        // focused: the accent ring on its top edge...
        "--expect-pixel".to_string(),
        "640,44,52,211,153".to_string(),
        // ...the red swatch, second on the toolbar (canvas x=388, y=76;
        // swatch at canvas 42,9, 22px), and the ring around it after one
        // C, its left edge at canvas x=38...
        "--expect-pixel".to_string(),
        "441,96,239,83,80".to_string(),
        "--expect-pixel".to_string(),
        "426,96,226,232,240".to_string(),
        // ...and an empty canvas is the card's surface.
        "--expect-pixel".to_string(),
        "640,250,28,34,48".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: Apps from Ctrl+Space twice, Timer by name".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        "sleep:6,ctrl-spc,sleep:1,ctrl-spc,sleep:2,shot:apps,t,i,m,ret,sleep:2".to_string(),
        "--out".to_string(),
        "dist/qemu_desk_apps".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // The Timer opened from the grid: 400 wide, centred, first cascade
        // step, focused -- the accent ring on its top edge (GFX-089).
        "--expect-pixel".to_string(),
        "640,44,52,211,153".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: a save, Files from the palette, a file opened from it".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // Write and save n.txt, open Files through the palette (Ctrl+Space,
        // "files", Enter), then Enter on the first row: n.txt opens in a
        // second Notepad and a notice says so.
        "sleep:6,ctrl-n,sleep:1,h,i,ctrl-s,sleep:1,n,dot,t,x,t,ret,sleep:1,ctrl-spc,sleep:1,f,i,l,e,s,ret,sleep:2,ret,sleep:2,shot:opened"
            .to_string(),
        "--out".to_string(),
        "dist/qemu_desk_files".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // The opened Notepad is the third cascade step (x=344, y=108),
        // focused: accent ring on its top edge, plain surface in its body.
        "--expect-pixel".to_string(),
        "500,108,52,211,153".to_string(),
        "--expect-pixel".to_string(),
        "700,300,28,34,48".to_string(),
        // The Files card behind it, second step (680 wide, so x=332, y=76):
        // hairline ring now that it has lost focus.
        "--expect-pixel".to_string(),
        "332,90,48,56,72".to_string(),
        // The "Opened n.txt" notice at the top right: a card surface where
        // the bare gradient would otherwise be.
        "--expect-pixel".to_string(),
        "1200,81,28,34,48".to_string(),
        // Two dock tiles lit (GFX-114): the lights under Notepad (focused,
        // longer) and Files. Ten app tiles and Apps past a divider are 782px
        // wide centred on 640, so the first centre is at 281, the next 351;
        // the lights sit three pixels under the tiles, at y=777..779.
        "--expect-pixel".to_string(),
        "281,778,52,211,153".to_string(),
        "--expect-pixel".to_string(),
        "351,778,52,211,153".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: Files with its chips, columns and details".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // Save memo.txt from a Notepad, open Files from the palette: the
        // card lists it, selected, with its size, kind and the RTC's date.
        "sleep:6,ctrl-n,sleep:1,h,i,ctrl-s,sleep:1,m,e,m,o,dot,t,x,t,ret,sleep:1,ctrl-spc,sleep:1,f,i,l,e,s,ret,sleep:2,shot:files"
            .to_string(),
        "--out".to_string(),
        "dist/qemu_desk_files_chips".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // The "New" chip in the Files header is a raised pill...
        "--expect-pixel".to_string(),
        "685,88,22,28,40".to_string(),
        // ...the selected row (memo.txt, first alphabetically) is on the
        // selection fill across the whole row...
        "--expect-pixel".to_string(),
        "600,117,44,82,96".to_string(),
        "--expect-pixel".to_string(),
        "500,117,44,82,96".to_string(),
        // ...and the Files card has focus.
        "--expect-pixel".to_string(),
        "333,300,52,211,153".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: Look previews a theme as the highlight moves".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // "look" from the bare desk opens the card on Dusk; Down twice is
        // Ember, previewed at once. Nothing is kept, so the gauntlet's
        // disk stays on the default look for every other shape.
        "sleep:6,l,o,o,k,ret,sleep:2,down,down,sleep:2,shot:ember".to_string(),
        "--out".to_string(),
        "dist/qemu_desk_look".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // Ember's gradient on the desk, its raised bar, the card's surface,
        // and the Mint ring still on the focused card.
        "--expect-pixel".to_string(),
        // (100,400) is the wallpaper now, whatever the theme (GFX-066).
        "100,400,9,9,5".to_string(),
        "--expect-pixel".to_string(),
        "640,4,40,48,65".to_string(),
        "--expect-pixel".to_string(),
        "600,200,40,28,30".to_string(),
        "--expect-pixel".to_string(),
        "361,200,52,211,153".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: a document's history, browsed and restored".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // Three saves of `hist`, then Ctrl+Y opens the newest kept version,
        // Left steps older, Enter restores it -- which is a save, so the
        // "Saved" notice card appears.
        "sleep:6,ctrl-n,sleep:1,o,n,e,ctrl-s,sleep:1,h,i,s,t,ret,sleep:1,spc,t,w,o,ctrl-s,sleep:1,spc,t,h,r,e,e,ctrl-s,sleep:1,ctrl-y,sleep:2,left,sleep:2,ret,sleep:2,shot:restored"
            .to_string(),
        "--out".to_string(),
        "dist/qemu_desk_history".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // The notice card at the top right is a card surface; without the
        // restore there is only the gradient there.
        "--expect-pixel".to_string(),
        "1200,81,28,34,48".to_string(),
        "--expect-pixel".to_string(),
        "1200,50,28,34,48".to_string(),
        // The Notepad card is focused and drawn.
        "--expect-pixel".to_string(),
        "400,44,52,211,153".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: the light theme, from the palette".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // From the bare desk, "look" opens the Look card on Dusk; Down is
        // Daylight, previewed at once. The card itself is the surface the
        // third pixel reads.
        "sleep:6,l,o,o,k,ret,sleep:1,down,sleep:2,shot:light".to_string(),
        "--out".to_string(),
        "dist/qemu_desk_light".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // The desk background, the top bar, and the Look card's surface,
        // all in the light palette.
        "--expect-pixel".to_string(),
        "100,400,9,9,5".to_string(),
        "--expect-pixel".to_string(),
        "640,4,144,159,177".to_string(),
        "--expect-pixel".to_string(),
        "640,300,250,250,252".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: select, copy, paste, find".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // "hello world", Shift+Left five times selects "world"; Ctrl+C,
        // End, Enter, Ctrl+V puts it on a second line; Ctrl+F "wo" wraps
        // to the first match and selects those two cells.
        "sleep:6,ctrl-n,sleep:1,h,e,l,l,o,spc,w,o,r,l,d,sleep:1,shift-left,shift-left,shift-left,shift-left,shift-left,sleep:1,ctrl-c,end,ret,ctrl-v,sleep:1,ctrl-f,w,o,sleep:1,shot:find"
            .to_string(),
        "--out".to_string(),
        "dist/qemu_desk_select".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        // Line 0 of the Notepad card (text origin x=288, y=76): "wo" at
        // columns 6-7 is on the selection fill, "r" at 8 and "hello" are
        // on the surface.
        "--expect-pixel".to_string(),
        "340,80,44,82,96".to_string(),
        "--expect-pixel".to_string(),
        // Mid-cell, above the x-height: Fira's 'l' next door inks the
        // cell edge at 360 (GFX-102).
        "356,80,28,34,48".to_string(),
        "--expect-pixel".to_string(),
        "300,80,28,34,48".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "the desk: typing on the bare desk is a palette search, and the console is a row in it"
        .to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--keys".to_string(),
        // No card has focus. The first build sent these keys to the
        // invisible console; now "t" opens the palette and "text" + Enter
        // runs "Switch to the text console".
        "sleep:6,t,e,x,t,ret,sleep:2".to_string(),
        "--out".to_string(),
        "dist/qemu_desk_search".to_string(),
        "--expect-serial".to_string(),
        "display_mode=Some(\"desk\")".to_string(),
        "--expect-serial".to_string(),
        "display: switched to text mode".to_string(),
        "--forbid-serial".to_string(),
        "KERNEL PANIC".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));

    title = "boot without a PIT".to_string();
    let args = [
        "--port-base".to_string(),
        GAUNTLET_PORT_BASE.to_string(),
        "--machine".to_string(),
        "pc,pit=off".to_string(),
        "--keys".to_string(),
        "sleep:25".to_string(),
        "--out".to_string(),
        "dist/qemu_nopit".to_string(),
        "--expect-serial".to_string(),
        "PandaGen Workspace".to_string(),
    ];
    shapes.push(Shape::new(&title, &args));
    run_shapes(&root, shapes, began)
}

fn usage() -> Result<(), Box<dyn std::error::Error>> {
    println!("Usage:");
    println!("  cargo xtask gauntlet   (the whole verification: tests, iso, every judge)");
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
    println!(
        "  cargo xtask wallpaper <picture.ppm>   (1280x800 P6; becomes the built-in wallpaper)"
    );
    println!("  cargo xtask limine-fetch [--repo <url>] [--branch <name>] [--source <path>]");
    Err(io::Error::other("unknown xtask command").into())
}

/// Make `picture.ppm` the desk's built-in wallpaper (GFX-070): quantised
/// to 256 colours with a median-cut palette and Floyd-Steinberg dithering,
/// written as `kernel_bootstrap/assets/wallpaper.{pal,idx}` for the
/// kernel's `include_bytes!`. A P6 of exactly 1280x800 is what it takes;
/// any image tool writes one (`convert in.png -resize 1280x800^ -gravity
/// center -extent 1280x800 out.ppm`).
fn cmd_wallpaper(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let Some(path) = args.next() else {
        return Err(io::Error::other("wallpaper: a P6 ppm path is needed").into());
    };
    // `--name <name>` writes wallpaper_<name>.{pal,idx}, one of Look's
    // choices (GFX-097); without it, the default picture.
    let mut name: Option<String> = None;
    while let Some(arg) = args.next() {
        if arg == "--name" {
            name = args.next();
        }
    }
    let picture = read_ppm(&path)?;
    // Full size, or half: a half-size wallpaper is a quarter of the bytes
    // and the compositor smooths it when it enlarges it.
    if !matches!((picture.width, picture.height), (1280, 800) | (640, 400)) {
        return Err(format!(
            "{path}: {}x{}, and a wallpaper is 1280x800 or 640x400",
            picture.width, picture.height
        )
        .into());
    }
    let pixels: Vec<[u8; 3]> = picture
        .data
        .chunks_exact(3)
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    let palette = median_cut(&pixels, 256);
    let indices = dither(&pixels, picture.width, &palette);
    let root = repo_root();
    let mut pal = Vec::with_capacity(768);
    for entry in &palette {
        pal.extend_from_slice(entry);
    }
    let stem = match &name {
        Some(name) => format!("wallpaper_{name}"),
        None => "wallpaper".to_string(),
    };
    fs::write(
        root.join(format!("kernel_bootstrap/assets/{stem}.pal")),
        &pal,
    )?;
    fs::write(
        root.join(format!("kernel_bootstrap/assets/{stem}.idx")),
        &indices,
    )?;
    println!(
        "wallpaper: {} colours, {} bytes; rebuild with `cargo xtask iso`",
        palette.len(),
        indices.len()
    );
    Ok(())
}

/// Median-cut palette of at most `count` colours.
fn median_cut(pixels: &[[u8; 3]], count: usize) -> Vec<[u8; 3]> {
    let mut boxes: Vec<Vec<[u8; 3]>> = vec![pixels.to_vec()];
    while boxes.len() < count {
        // Split the box with the widest channel range.
        let (index, channel) = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| {
                let (c, range) = (0..3)
                    .map(|c| {
                        let (lo, hi) = b
                            .iter()
                            .fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[c]), hi.max(p[c])));
                        (c, hi as i32 - lo as i32)
                    })
                    .max_by_key(|(_, r)| *r)
                    .unwrap();
                (i, c, range)
            })
            .max_by_key(|(_, _, range)| *range)
            .map(|(i, c, _)| (i, c))
            .unwrap_or((usize::MAX, 0));
        if index == usize::MAX {
            break;
        }
        let mut b = boxes.swap_remove(index);
        b.sort_by_key(|p| p[channel]);
        let half = b.len() / 2;
        let rest = b.split_off(half);
        boxes.push(b);
        boxes.push(rest);
    }
    boxes
        .iter()
        .map(|b| {
            let n = b.len().max(1) as u32;
            let sum = b.iter().fold([0u32; 3], |s, p| {
                [s[0] + p[0] as u32, s[1] + p[1] as u32, s[2] + p[2] as u32]
            });
            [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
        })
        .collect()
}

/// Floyd-Steinberg dithering onto `palette`, one index a pixel.
fn dither(pixels: &[[u8; 3]], width: usize, palette: &[[u8; 3]]) -> Vec<u8> {
    let height = pixels.len() / width.max(1);
    let mut work: Vec<[i32; 3]> = pixels
        .iter()
        .map(|p| [p[0] as i32, p[1] as i32, p[2] as i32])
        .collect();
    let nearest = |c: [i32; 3]| -> usize {
        let mut best = (0usize, i64::MAX);
        for (i, p) in palette.iter().enumerate() {
            let d = (0..3)
                .map(|k| {
                    let e = c[k] as i64 - p[k] as i64;
                    e * e
                })
                .sum::<i64>();
            if d < best.1 {
                best = (i, d);
            }
        }
        best.0
    };
    let mut out = vec![0u8; pixels.len()];
    for y in 0..height {
        for x in 0..width {
            let at = y * width + x;
            let c = [
                work[at][0].clamp(0, 255),
                work[at][1].clamp(0, 255),
                work[at][2].clamp(0, 255),
            ];
            let index = nearest(c);
            out[at] = index as u8;
            let chosen = palette[index];
            let err = [
                c[0] - chosen[0] as i32,
                c[1] - chosen[1] as i32,
                c[2] - chosen[2] as i32,
            ];
            let mut spread = |dx: isize, dy: usize, weight: i32| {
                let nx = x as isize + dx;
                if nx < 0 || nx as usize >= width || y + dy >= height {
                    return;
                }
                let n = (y + dy) * width + nx as usize;
                for k in 0..3 {
                    work[n][k] += err[k] * weight / 16;
                }
            };
            spread(1, 0, 7);
            spread(-1, 1, 3);
            spread(0, 1, 5);
            spread(1, 1, 1);
        }
    }
    out
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

/// How far apart consecutive port bases sit. The five forwarded ports span
/// more than one, so shifting them all by `base` made base *b*'s command
/// port the same number as base *b+1*'s echo port: two instances one base
/// apart could not run together, which is the whole point of the option.
const PORT_BASE_STRIDE: u16 = 16;

impl Ports {
    fn with_base(base: u16) -> Self {
        // Saturating, because `8080 + base * 16` overflows a u16 long before
        // anyone has that many instances, and a panic in the harness is a
        // worse answer than a refusal.
        let shift = base.saturating_mul(PORT_BASE_STRIDE);
        Self {
            udp_echo: UDP_ECHO_PORT.saturating_add(shift),
            remote: REMOTE_PORT.saturating_add(shift),
            tcp_echo: TCP_ECHO_PORT.saturating_add(shift),
            tcp_command: TCP_COMMAND_PORT.saturating_add(shift),
            http: HTTP_PORT.saturating_add(shift),
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
    let envelope = remote_ipc::encode_call_with_id(call, remote_ipc::ordered_id(fresh_nonce()))?;
    let bytes = remote_ipc::envelope_to_bytes(&envelope, &key.caller, &key.key)?;
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

/// A fresh nonce for the signed line protocol.
///
/// Nanoseconds since the epoch in the top 64 bits, uniqueness in the bottom
/// 64. The order matters: the kernel keeps a window per caller and refuses a
/// nonce older than every nonce still in *this caller's* window, so a
/// captured request goes stale instead of coming back into range once the
/// window rolls over. Only monotonicity with respect to this caller's own
/// earlier requests is required; other callers' clocks are irrelevant.
fn fresh_nonce() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let unique = ((std::process::id() as u64) << 32) ^ COUNTER.fetch_add(1, Ordering::Relaxed);
    ((nanos as u128) << 64) | unique as u128
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

    // The speaker (GFX-087): an audio backend for this host, and the PC
    // speaker wired to it, so a chime is heard at the desk. QEMU_AUDIO=none
    // keeps it quiet; the gauntlet and the smoke run never ask for one.
    let audio = select_qemu_audio();
    let machine = match &audio {
        Some(_) => "pc,pcspk-audiodev=snd0".to_string(),
        None => "pc".to_string(),
    };
    let mut command = Command::new("qemu-system-x86_64");
    command.current_dir(&root);
    if let Some(backend) = &audio {
        command.arg("-audiodev").arg(format!("{backend},id=snd0"));
        // The line printed above is the machine without sound; say what
        // was added, so the printed command is the one that runs.
        println!("  with sound: -audiodev {backend},id=snd0 -machine {machine}");
        println!();
    }
    run(command
        .arg("-machine")
        .arg(&machine)
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
    // Pixels the final screendump must show: `x,y,r,g,b[,tolerance]`.
    let mut expect_pixels: Vec<PixelExpectation> = Vec::new();
    let mut port_base: u16 = 0;
    let mut machine = "pc".to_string();
    let mut memory = "512M".to_string();
    let mut smp = QEMU_SMP.to_string();
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
            // A pixel the final screendump must show. Screendumps are how
            // graphics changes get verified, and until this existed they
            // were verified by eye -- which is to say, not by the gauntlet.
            "--expect-pixel" => expect_pixels.push(value("--expect-pixel")?.parse()?),
            "--allow-exception" => allow_exception = true,
            "--port-base" => port_base = value("--port-base")?.parse()?,
            // The QEMU machine string, so a run can boot hardware the
            // default `pc` does not have -- `pc,pit=off` is how the
            // calibration hang was reproduced.
            "--machine" => machine = value("--machine")?,
            // Guest memory, so a run can boot a machine smaller than the
            // default and find where the kernel actually stops working.
            "--memory" => memory = value("--memory")?,
            // CPU count, so a run can boot more CPUs than the kernel has
            // per-CPU tables for.
            "--smp" => smp = value("--smp")?,
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
        .arg(&machine)
        .arg("-smp")
        .arg(&smp)
        .arg("-m")
        .arg(&memory)
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
        .spawn()
        .map_err(|err| {
            // A bare `NotFound` here names neither qemu nor the file it could
            // not find, and this is the one call in the run that can fail
            // that way for three different reasons.
            io::Error::other(format!(
                "could not start qemu-system-x86_64 (machine {machine}, iso \
                 {}, disk {}): {err}",
                iso.display(),
                disk.display()
            ))
        })?;

    std::thread::sleep(Duration::from_secs_f64(boot_wait));

    // If QEMU could not start -- most often because another instance holds
    // the forwarded ports -- this connect fails with a bare "Connection
    // refused" on a Unix socket, which says nothing about the cause. Ask the
    // child what happened before reporting that.
    let mut monitor = match UnixStream::connect(&sock) {
        Ok(monitor) => monitor,
        Err(err) => {
            let detail = match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stderr = String::new();
                    if let Some(mut pipe) = child.stderr.take() {
                        use std::io::Read as _;
                        let _ = pipe.read_to_string(&mut stderr);
                    }
                    format!("qemu exited ({status}): {}", stderr.trim())
                }
                _ => format!("qemu is running but its monitor is unreachable: {err}"),
            };
            let _ = child.kill();
            let _ = fs::remove_file(&sock);
            return Err(io::Error::other(format!(
                "{detail}\nhint: another qemu may hold ports {}; \
                 use --port-base N for a second instance",
                ports.hostfwd()
            ))
            .into());
        }
    };
    monitor.set_read_timeout(Some(Duration::from_millis(400)))?;
    // QEMU's reply to each monitor command, so a refusal is not thrown away.
    let mut mon = |cmd: &str| -> io::Result<String> {
        monitor.write_all(cmd.as_bytes())?;
        monitor.write_all(b"\n")?;
        std::thread::sleep(Duration::from_millis(120));
        let mut sink = [0u8; 8192];
        let read = monitor.read(&mut sink).unwrap_or(0);
        Ok(String::from_utf8_lossy(&sink[..read]).into_owned())
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
            // Remove any image from a previous run first. A screendump QEMU
            // declines used to leave the old file in place, still listed as
            // a shot, with the run reporting PASS -- and these images are
            // how kernel changes get verified by eye.
            let _ = fs::remove_file(&path);
            let reply = mon(&format!("screendump {path}"))?;
            if !std::path::Path::new(&path).exists() {
                udp_failures.push(format!(
                    "<screendump {name} produced no file: {}>",
                    reply.trim()
                ));
            }
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
            // See the `remote:` step: an empty expectation matches anything.
            if expected.is_empty() {
                udp_failures.push(format!(
                    "<remote-tcp {command:?} has no expectation; write remote-tcp:{command};<text>>"
                ));
                continue;
            }
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
            // `reply.contains("")` is true of every reply, so a step written
            // without an expectation asserted nothing at all and reported
            // success for any answer the kernel gave -- including an error.
            // The canonical verification's only remote assertion was one of
            // these. An empty expectation is a mistake, not a wildcard.
            if expected.is_empty() {
                udp_failures.push(format!(
                    "<remote {command:?} has no expectation; write remote:{command};<text>>"
                ));
                continue;
            }
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
    let _ = fs::remove_file(&final_path);
    let reply = mon(&format!("screendump {final_path}"))?;
    if !std::path::Path::new(&final_path).exists() {
        udp_failures.push(format!(
            "<final screendump produced no file: {}>",
            reply.trim()
        ));
    }
    shots.push(final_path.clone());
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
    if !expect_pixels.is_empty() {
        match read_ppm(&final_path) {
            Ok(image) => {
                for expectation in &expect_pixels {
                    if let Some(problem) = expectation.check(&image) {
                        missing.push(format!("<pixel {problem}>"));
                    }
                }
            }
            Err(err) => missing.push(format!("<final screendump unreadable: {err}>")),
        }
    }
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

/// The audio backend for an interactive run, or none: `QEMU_AUDIO` names
/// one (`none` for silence), otherwise the host's usual.
fn select_qemu_audio() -> Option<String> {
    if let Ok(value) = env::var("QEMU_AUDIO") {
        let trimmed = value.trim();
        return match trimmed {
            "" | "none" | "off" => None,
            other => Some(other.to_string()),
        };
    }
    if cfg!(target_os = "macos") {
        Some("coreaudio".to_string())
    } else if cfg!(target_os = "linux") {
        Some("pa".to_string())
    } else {
        None
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
    // The legacy `.cfg` name is staged from the *same substituted text*.
    // It used to be copied verbatim from `boot/limine.cfg`, a stale
    // duplicate carrying neither the per-build remote token nor the
    // graphics entry -- so if Limine ever preferred that name, the machine
    // booted with no token and every remote step failed confusingly. Which
    // file Limine picks is its business; both must say the same thing.
    let _ = &limine_cfg;
    fs::write(staging.join("boot/limine.cfg"), &conf_text)?;
    fs::write(staging.join("limine.cfg"), &conf_text)?;
    fs::write(limine_dir.join("limine.cfg"), &conf_text)?;

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

/// One boot of the gauntlet (DEV-001): what it proves, and its
/// `qemu-script` arguments.
struct Shape {
    title: String,
    args: Vec<String>,
}

impl Shape {
    fn new(title: &str, args: &[String]) -> Self {
        Self {
            title: title.to_string(),
            args: args.to_vec(),
        }
    }
}

/// How many shapes boot at once, unless `GAUNTLET_JOBS` says otherwise.
/// Each machine has four CPUs; four machines keep a sixteen-thread host
/// busy without starving the guests' timers.
const GAUNTLET_JOBS: usize = 4;
/// The workers' port bases: this and the next few. Clear of 0 (a person's
/// `cargo xtask qemu`), 1 (`GAUNTLET_PORT_BASE`, the shapes' own) and the
/// low bases people pass by hand.
const GAUNTLET_WORKER_BASE: u16 = 10;

/// Boot every shape, `GAUNTLET_JOBS` at a time (DEV-001).
///
/// The shapes used to run one after another on one private disk, and a
/// run spent most of its half hour waiting on sleeps inside the guests.
/// Now each worker has its own port base -- so their forwarded ports never
/// meet -- and each shape starts from its own blank disk, made just before
/// it boots: nothing one shape leaves behind can reach another, which is
/// what a blank disk at the start was for (H1), now for every shape.
///
/// A shape's output goes to `dist/<out>.gauntlet.log` and is printed only
/// when it fails. After a failure no new shape starts; the ones running
/// finish, and every failure is named.
fn run_shapes(
    root: &Path,
    shapes: Vec<Shape>,
    began: std::time::Instant,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    let jobs = env::var("GAUNTLET_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(GAUNTLET_JOBS)
        .clamp(1, 8);
    let total = shapes.len();
    println!(
        "== {total} boots, {jobs} at a time ({:.0}s in)",
        began.elapsed().as_secs_f64()
    );
    let exe = env::current_exe()?;
    let blank_len = fs::metadata(root.join(DISK_OUTPUT))
        .map(|m| m.len())
        .unwrap_or(64 * 1024 * 1024);
    let queue: Mutex<VecDeque<(usize, Shape)>> =
        Mutex::new(shapes.into_iter().enumerate().collect());
    let stop = AtomicBool::new(false);
    let failures: Mutex<Vec<String>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for worker in 0..jobs {
            let (queue, stop, failures, exe) = (&queue, &stop, &failures, &exe);
            scope.spawn(move || loop {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let Some((index, shape)) = queue.lock().unwrap().pop_front() else {
                    break;
                };
                let base = GAUNTLET_WORKER_BASE + worker as u16;
                // Its own blank disk, all zeros: the kernel formats it.
                let disk = root.join(format!("dist/pandagen-{base}.disk"));
                let _ = fs::remove_file(&disk);
                let made = fs::File::create(&disk).and_then(|f| f.set_len(blank_len));
                // Its own ports.
                let mut args = shape.args.clone();
                if let Some(at) = args.iter().position(|a| a == "--port-base") {
                    if let Some(value) = args.get_mut(at + 1) {
                        *value = base.to_string();
                    }
                }
                let out_name = args
                    .iter()
                    .position(|a| a == "--out")
                    .and_then(|at| args.get(at + 1).cloned())
                    .unwrap_or_else(|| format!("dist/gauntlet_{index}"));
                let started = std::time::Instant::now();
                let run = match made {
                    Ok(()) => Command::new(exe)
                        .current_dir(root)
                        .arg("qemu-script")
                        .args(&args)
                        .output(),
                    Err(e) => Err(e),
                };
                let (ok, text) = match run {
                    Ok(o) => {
                        let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
                        text.push_str(&String::from_utf8_lossy(&o.stderr));
                        (o.status.success(), text)
                    }
                    Err(e) => (false, format!("could not run qemu-script: {e}")),
                };
                let _ = fs::write(root.join(format!("{out_name}.gauntlet.log")), &text);
                println!(
                    "   [{:>2}/{total}] {} {} ({:.0}s)",
                    index + 1,
                    if ok { "PASS" } else { "FAIL" },
                    shape.title,
                    started.elapsed().as_secs_f64()
                );
                if !ok {
                    stop.store(true, Ordering::SeqCst);
                    println!("---- {} ----\n{}\n----", shape.title, text.trim_end());
                    failures.lock().unwrap().push(shape.title.clone());
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    println!("== done in {:.0}s", began.elapsed().as_secs_f64());
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("gauntlet boots failed: {}", failures.join("; ")).into())
    }
}
