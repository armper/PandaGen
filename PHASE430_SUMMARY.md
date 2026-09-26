# Phase 430: the Web card grows up -- history, a start page, bookmarks, find (WEB-002)

## What changed

### Getting around

- **Back and Forward**, as buttons in the bar (`<` `>`), Backspace for
  Back. Pages already read are kept -- eight of them, with where each was
  scrolled to -- so Back and Forward show them at once, without fetching;
  Reload (or Ctrl+R) always fetches.
- **A start page** (`about:start`, made in the card, never fetched): the
  bookmarks, the pages visited lately, and the keys. A new Web card opens
  on it, ready for an address; with nothing typed, Tab, the arrows and
  Enter walk and open its links. The **Start** chip returns to it.
- **Bookmarks**: the **Bookmark** chip, Ctrl+D, or the palette's "Bookmark
  this page" keeps the page (again takes it off). Every Web card shares
  them, and they are kept per person on disk (`.bookmarks`, read back with
  the look, like the layout).
- **Find in page**: the **Find** chip or Ctrl+F. Matches are marked as the
  query is typed, the current one in the accent colour, and the page
  scrolls to it; Enter or Down goes to the next, Up back; the footer says
  "2 of 5".
- **Where a link goes**: the link under the pointer shows its address in
  the footer.
- **Copy**: Ctrl+C copies the link Tab has reached, or the page's
  address, onto the desk's clipboard (and its history).
- The palette offers "Bookmark this page" and "Web start page" only when a
  Web card has the focus.

### Reading pages better

- Numbered lists are numbered (and honour `start`), bullets stay bullets.
- Nested lists and blockquotes indent; a wrapped list item hangs under
  its text, not under its number.

### Underneath

- `AppState::Web` is boxed: a page, its cache and its history are far
  bigger than any other card's state, and every card paid for them.

## Tests

- `web`: numbered, nested and quoted blocks with the hanging indent; the
  start page and walking its links from the empty address; bookmarking
  and unbookmarking, and the list's round trip through its file; find
  (count, next, previous, scrolling, closing); the hovered link's address
  and Ctrl+C; Back and Forward from the cache.
- `desk`: Web rows offered only over a Web card; a bookmark made in one
  card appears on another's start page and survives being written and
  read back.
- QEMU: the start page, a find with its highlight, and a page bookmarked
  with Ctrl+D appearing on the start page.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo xtask gauntlet`: exit 0.
