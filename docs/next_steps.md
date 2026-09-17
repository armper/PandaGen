# PandaGen: Top 5 Next Features/Fixes/Enhancements

Saved list of the highest-impact next steps:

1) **Multi-core (SMP) support** — in progress: Phase 254 brings all CPUs online (spinlock, CPU registry, `cpus`), Phase 255 adds the LAPIC, wake IPIs, and a job queue (`smp run <n>`), Phase 256 splits the desktop present across CPUs (3.2x faster); next are per-CPU GDT/TSS, LAPIC timer, and cross-CPU scheduling.
2) **Persistent bare-metal storage** — done (Phases 252-253): virtio-blk over PCI, disks are mounted across reboots.
3) **Graphics/UI framework beyond text mode** — enables richer apps and UI primitives.
4) **Advanced network protocols + hardened remote IPC** — in progress: Phase 257 brings up virtio-net on bare metal with ARP/IPv4/ICMP (`net ping`), Phase 258 adds UDP with a host-reachable echo port; next is carrying `remote_ipc` over UDP.
5) **Formal verification of critical paths** — guarantees for capabilities, IPC, and scheduling invariants.
