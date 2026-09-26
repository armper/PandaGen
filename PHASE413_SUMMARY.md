# Phase 413: the Sharing card (FS-007)

## What changed

- A card for one document (`sharing::SharingView`), opened from Files'
  new **Share** chip or the palette's "Share this document" (the focused
  Notepad's): its name, and "yours" or who owns it and what you hold;
  its **sensitivity** as four segments (Public / Internal / Confidential /
  Secret) the owner can click, within their clearance; **who has it** --
  each grant with the holder, their rights as pills, any expiry or
  history window, and a Revoke button where the viewer may revoke; and
  **Share with**: a pill per person, a pill per right (Read, History,
  Write, Tag, Delete, Share -- only what the viewer holds can be picked),
  and a Share button.
- The card shows what the guard says (`guard::sharing_info` builds a
  `SharingInfo`) and asks for every change as a request
  (`LoadSharing`, `ShareDoc`, `RevokeGrant`, `RelabelDoc`); the kernel
  does it through the filesystem (`guard::share_doc` / `revoke_doc` /
  `relabel_doc`) and shows the card again as the guard now sees it, with
  what happened in words. The card never claims more than the guard
  allowed.

## Why

Sharing had only the shell's words; this is the document's own view of
who can see it, and the way to change that by pointer.

## Tests

- `sharing::tests`: sharing, revoking and relabelling by key and by
  click; what cannot be given is not offered (rights not held, grants not
  one's own, relabelling someone else's document).
- `guard::tests::the_sharing_card_sees_what_the_guard_allows`: the owner
  and a reader see different cards; the acts go through the guard.
- `desk::tests::the_share_chip_opens_the_sharing_card_and_its_acts_are_requests`.
- Machine: `cargo xtask gauntlet` exit 0.
