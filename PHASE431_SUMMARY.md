# Phase 431: the network's requests wait their turn (NET-034)

## What changed

The machine had one request slot. The Terminal's `fetch` or `resolve`
and every Web card shared it, and whoever asked second was turned away:
"one request at a time; the last is still out", or "the network is busy;
try again". Two Web cards, or a card and the Terminal, could not both
ask.

- **A queue.** `NetStack` keeps up to eight requests waiting behind the
  one under way (`Queued::Fetch` for the Terminal or a card,
  `Queued::Resolve`), and starts the next as each finishes -- from the
  network pass itself (`pump_queue`), so nothing has to ask again. Only a
  malformed URL is refused up front; a request that fails when its turn
  comes answers with why, as before.
- **Answers by request.** A card's fetch carries a request number; the
  loop keeps which card and URL each number is for, and
  `take_web_outcomes` hands back every finished one with its number.
- **Stale answers are dropped.** A card takes an answer only for the URL
  it is still waiting on (`WebView::loaded_for`): click one link, then
  another before the first loads, and the first's page no longer lands
  on the card under the second's address.

## Tests

- `web`: an answer for a page no longer wanted is dropped; the wanted
  one is shown.
- The gauntlet's Terminal network shape now types `fetch` while its
  `resolve` is still out, so the fetch waits in the queue and both lines
  must arrive.
- QEMU: `resolve` and `fetch` typed back to back, then a Web card page,
  all answered.
- `cargo xtask gauntlet`: exit 0.
