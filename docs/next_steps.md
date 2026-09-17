# PandaGen: Top 5 Next Features/Fixes/Enhancements

Saved list of the highest-impact next steps:

1) **Multi-core (SMP) support** — in progress: Phase 254 brings all CPUs online (spinlock, CPU registry, `cpus`), Phase 255 adds the LAPIC, wake IPIs, and a job queue (`smp run <n>`), Phase 256 splits the desktop present across CPUs (3.2x faster), Phase 261 gives every AP a calibrated 100 Hz LAPIC timer, Phase 262 installs per-CPU GDT/TSS with a double-fault stack and exception diagnostics (`fault pf|ud|de`); next is cross-CPU scheduling.
2) **Persistent bare-metal storage** — done (Phases 252-253): virtio-blk over PCI, disks are mounted across reboots.
3) **Graphics/UI framework beyond text mode** — enables richer apps and UI primitives.
4) **Advanced network protocols + hardened remote IPC** — in progress: Phase 257 brings up virtio-net on bare metal with ARP/IPv4/ICMP (`net ping`), Phase 258 adds UDP with a host-reachable echo port, Phase 259 serves `remote_ipc` calls over UDP (`cargo xtask remote cpus`) with a read-only allowlist, Phase 260 signs every call with HMAC-SHA256 over a shared token (`remote_token=` on the command line); next are replay protection and DHCP.
5) **Formal verification of critical paths** — guarantees for capabilities, IPC, and scheduling invariants.
