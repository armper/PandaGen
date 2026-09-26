# Phase 429: the Web card (WEB-001)

## What changed

The desk has a web reader: **Web**, on the dock (a drawn globe) and in
the Apps grid, and in the palette ("Web: pages over HTTP, read as text").

### The card (`kernel_bootstrap/src/web.rs`)

- **An address bar** with Back and Go (Reload once a page is up). Type an
  address -- `example.com`, `10.0.2.2:18080/page.html`, `http://...` --
  and Enter. Typing on a page starts a new address; Esc goes to the bar.
- **The page as text**, wrapped to the card: headings (bold), paragraphs,
  lists, preformatted blocks kept as written, rules, and images as their
  alt text. Scripts, styles, SVG and templates are left out; `<noscript>`
  is shown, since scripts never run here. Entities are decoded, and
  characters outside the desk's ASCII font are shown as their nearest
  ASCII (curly quotes, dashes, accented letters) or `?`.
- **Links** are the accent colour, underlined. A click follows one; so do
  Tab (which walks them, scrolling to each) and Enter. Links resolve
  against the page (`../`, `/root`, `//host`, `?query`); `#fragment`,
  `mailto:` and `javascript:` are not links. Backspace goes back.
- **Redirects** are followed, up to five, over plain HTTP. A redirect to
  `https://` -- and an `https` address -- is said plainly: there is no TLS
  yet. A failure (no such name, no answer, refused) is shown in the card.
- Plain-text responses are shown line for line; any other status than
  2xx shows its page with the status in the footer.
- The wheel, arrows and Page Up/Down scroll. The page is wrapped once per
  width and kept.
- **A reboot brings the card back on its page** (the layout keeps the
  URL as the card's document, DESK-020).

### Underneath

- The network stack's fetch (NET-033) can answer a card instead of the
  Terminal: `start_web_fetch` and `take_web_outcome`, with the response's
  status, reason, `Location`, `Content-Type` (new in `net_stack::http`)
  and decoded body. While a card's fetch runs its progress lines stay out
  of the Terminal; a failure becomes the card's message.
- Our own SYN now backs off (1, 2, 4, 8, 16 s, RFC 6298) rather than
  giving up after five one-second tries; a server slow to answer a first
  SYN is found. `SynSent` has its own reaper limit to match.
- The Apps grid is two rows of six (eleven apps now), 640 wide.
- The globe icon is drawn by `tools/art/drawn_icons.py` like the desk's
  other system icons and exported by `tools/art/icons.py`; the existing
  icons export byte-identical.

## Gauntlet

- New shape "the desk: the Web card loads a page and follows its link":
  the palette opens Web, the address loads an HTML page from the
  harness's server (which now serves `/page.html` and `/second.html`), and
  Tab and Enter follow the link; both loads are asserted on serial.
- The dock's three pixel pins move with the eleventh tile: the glass gap
  between the first two tiles to x=281, the lights under Notepad and
  Files to 246 and 316 (verified on boots).

By hand, online: `example.com` renders ("Example Domain", its paragraph
and its "Learn more" link).

## Tests

- `web`: a page's headings, paragraphs, lists, pre, rule, alt text,
  entities, skipped script/style/comments and resolved links; URL
  resolution; wrapping (words, links across a wrap, hard-broken pre); the
  card's address, Tab/Enter, Back and a click on a link; redirects and
  https said plainly; plain text.
- `desk`: the palette opens Web, the typed address becomes a `Fetch`, the
  loaded page titles the card, and the layout keeps its URL.
- `tcp`: the SYN backoff.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo xtask gauntlet`: exit 0.
