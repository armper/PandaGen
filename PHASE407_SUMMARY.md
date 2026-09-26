# Phase 407: the authority crate -- who may do what to a document (FS-001)

## What changed

- A new crate, `authority` (`no_std` + `alloc`, host-tested), that decides
  and stores nothing. The design is in `docs/design/authority.md`.
- **Principals**: persons, services and the system, each with a
  clearance. The system (`SYSTEM`, id 0) is not subject to grants or
  labels.
- **Rights**, one bit each: `READ` (current text), `HISTORY` (earlier
  versions), `WRITE`, `TAG`, `DELETE`, `SHARE`, and `OWN` (all of them,
  plus relabel and revoke). Reading a document and reading its history are
  different rights.
- **Grants**: holder, document (a stable `DocId`), rights, the grant it
  was derived from, issuer, expiry, and how far back its history reaches.
  `share` derives a grant from what the sharer holds: the same rights or
  fewer, expiring no later, reaching no further back. `OWN` is only the
  owner's to give. A grant to someone not cleared for the document's label
  is refused when it is made. `revoke` (by the issuer, the owner or the
  system) takes everything derived from the grant with it.
- **Labels**: `Public < Internal < Confidential < Secret`. Reading or
  reading history above a principal's clearance is refused first,
  whatever any grant or role says: the mandatory layer. Clearance is the
  system's to set; relabelling is the owner's, within their clearance.
- **Roles**: a name, members, rights and a scope (a tag, one document, or
  everything), reaching only the role owner's documents -- no one can
  make a role that opens someone else's work.
- **Decisions** carry a `Reason` (owner, grant n, role r, no grant, holds
  only x, above clearance, before the history window), and
  `check_and_log` keeps them in a bounded audit log (`AUDIT_KEEP`).
- Everything serialises, for keeping on disk.

## Why

The filesystem had names, tags and versions, but no owners and no one to
refuse. `services_storage::permissions` and `workspace_access` sketched
pieces of this, but nothing called them and neither could say "may read
the document but not its history". This is the decision half of that,
built so the filesystem can ask it.

## Tests

`authority::tests`: Alice shares the text but not the history (and the
log has both decisions); history windows; sharing only narrows and
revoking cascades (with expiry inherited); labels refuse reads above
clearance whatever the grants or roles, and clearance is the system's;
roles reach only the owner's tagged documents; names are unique and
plain, the log is bounded; the whole authority round-trips as JSON.

Machine: `cargo xtask gauntlet` exit 0.
