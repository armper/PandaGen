# Phase 414: the Access card -- people and roles (FS-008)

## What changed

- A card for the machine's people (`access_card::AccessView`, from the
  palette's "Access"): each person with their initial, name, whether they
  can sign in (or are the administrator), and their clearance as four
  segments (P / I / C / S) the administrator can click.
- **Add someone** (the administrator): a name, a passphrase shown only as
  dots, a clearance, and Add -- the person can sign in at once.
- **Roles** the viewer owns or is in: what each gives over what (`#work`,
  a document, everything), and on their own roles a pill per person to
  put them in or take them out. (New roles are still made in the Terminal.)
- Requests `LoadAccess` / `AccessDo`; the kernel acts through the guard
  (`guard::access_info`, `guard::access_act`) with a fresh salt for a new
  passphrase, and shows the card again.

## Tests

- `access_card::tests`: the administrator adds someone (name, masked
  passphrase, clearance), sets a clearance, moves people in and out of a
  role, by key and by click; someone else can change nothing.
- `guard::tests::the_access_card_adds_people_and_moves_them_between_roles`:
  the added person signs in; they see the role but cannot add people or
  change the role.
- `desk::tests::access_opens_from_the_palette_and_its_acts_are_requests`.
- Machine: `cargo xtask gauntlet` exit 0.
