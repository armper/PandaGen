# Phase 366: the document apart from the card; Split (GFX-074)

## What changed

- `notepad::Document` holds what belongs to the text and not to any one
  card: the buffer, its undo history, the saved/unsaved state, the name,
  and the autosave clock. `Notepad` is now one card's *view* of a
  document (caret, selection, scroll, prompts, wrap, history browser),
  holding the document through `Rc<RefCell<Document>>`.
- `Notepad::share()` makes another view of the same document;
  `shares_with` and `shared` say so. Every public method keeps its
  signature except `path()`, which now returns an owned `Option<String>`.
- The history browser no longer swaps the document's buffer for the
  version being shown: the version is an overlay in that card only, so
  another card on the same document keeps showing the text. Enter still
  restores by writing the version into the document (all cards) and
  saving it; Esc has nothing to put back.
- A view whose document another card edited brings its caret and anchor
  back inside the text before it acts or draws.
- `Ctrl+N` and `Ctrl+W` stop asking "unsaved, again to discard" when the
  document is shared, because nothing is lost; `Open` in one card gives
  that card a new document and leaves the others on the old one.
- One document in two cards autosaves once: the clock is the document's.
- Desk: a **Split** chip on every Notepad (Ctrl+D, and a palette row).
  It makes a second card on the same document and snaps the two side by
  side, the original left and the new one right, focused. The footer of
  a shared document says `in N cards`.

## Why this shape

The earlier split attempt was declined because two cards on one text
needed a shared-document model, and copying text between cards would be
two documents pretending to be one. This is the model: the `Rc<RefCell>`
is the smallest thing that lets two views hold one document in a
single-threaded desk, and every borrow is scoped to one statement so two
cards can never hold it at once (a violation would panic, and the host
tests drive every path). The history overlay follows from it: a card
browsing the past must not rewrite the present under the other card.

## Tests

- `notepad::tests::a_shared_document_is_one_text_with_two_carets`
- `notepad::tests::a_shared_document_autosaves_once`
- `desk::tests::split_puts_one_document_in_two_cards_side_by_side`
- Every earlier Notepad and desk test unchanged in behaviour.
- Machine: a Notepad split with Ctrl+D, typing shown in both halves;
  `cargo xtask gauntlet` exit 0.
