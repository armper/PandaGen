# Phase 416: the gauntlet boots in parallel (DEV-001)

## What changed

- The gauntlet's boots -- the network judges, the odd machines, every desk
  shape -- are gathered as `Shape`s and run by `run_shapes`,
  `GAUNTLET_JOBS` (4) at a time, instead of one after another. A run that
  took about 31 minutes takes about 8; the boots alone, 25 minutes before,
  take under 3.
- Each worker boots on its own port base (`GAUNTLET_WORKER_BASE`, 10 and
  up), clear of a person's `cargo xtask qemu` (0) and bases used by hand,
  so forwarded ports never meet.
- Each shape starts from its own blank disk, made just before it boots.
  The shapes used to share one private disk, blank only at the start, so a
  shape could depend on what an earlier one left (the Welcome card, the
  examples, a theme, a passphrase); now none can. That is what the blank
  disk was for (H1), for every shape.
- A shape's whole output goes to `dist/<out>.gauntlet.log`; the gauntlet
  prints one line per shape (PASS or FAIL, its title, its seconds) and a
  failed shape's output in full. After a failure no new shape starts; the
  ones running finish, and every failure is named.
- `cargo xtask gauntlet --boots-only` builds the image and runs the boots
  without the test stages before them -- for going round again on a boot.
- Each stage says how far into the run it starts.

## Why

Every commit waits for the gauntlet, and most of its half hour was the
guests' own sleeps, one machine at a time on a sixteen-thread host.

## Tests

- The gauntlet itself: all stages and all boots, parallel, exit 0 (the
  boots in about 3 minutes, four at a time, each on its own disk).
- Machine: `cargo xtask gauntlet` exit 0.
