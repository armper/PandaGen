# Authority: who may do what to a document

PandaGen's filesystem is being given owners, people, and rights. This is
the design; `PHASE407_SUMMARY.md` onward record what landed.

## What we would want, starting over

Unix answered "who may do what" with nine bits per file and a user id per
process: ambient authority, checked by path, blind to history, with root
above it all. Access-control lists fixed the granularity but kept the
shape: a list on the object saying who, checked against whoever asks.
PandaGen starts from the other end, the one its kernel already takes:

1. **Authority is held, not looked up.** A person holds *grants*: records
   that say "this principal may do these things to this document". A
   request is allowed because a grant the requester holds allows it, never
   because of who they are or where the file is.
2. **Rights are specific.** Reading a document's current text and reading
   its history are different rights. So are writing, tagging, deleting and
   sharing. Nothing is implied except by `Own`.
3. **Sharing only narrows.** A grant can be *derived* into a new grant for
   someone else with the same rights or fewer, an earlier expiry, or a
   shorter view of history -- never more. The derived grant remembers its
   parent, so revoking a grant revokes everything derived from it.
4. **Some rules are not anyone's to waive.** Documents carry a sensitivity
   *label*; people carry a *clearance*. A grant cannot let someone read
   above their clearance, even the owner's grant. That is the mandatory
   layer, and it is checked first.
5. **Roles are grants for many documents at once.** A role gives its
   members rights over every document with a tag (`#work` editors, `#hr`
   readers), so sharing a collection is one act.
6. **Every decision says why, and is kept.** A check returns the rule that
   decided it -- the grant, the role, the label -- and the decision goes in
   an audit log the owner can read.
7. **Proving who you are is separate from what you may do.** A passphrase
   opens a *session*; the session holds the person's grants. Nothing below
   the session sees a passphrase, and nothing above it sees a verifier.

## The pieces

| Piece | What it is |
|---|---|
| `Principal` | A person, a service or the system, with a clearance. |
| `Rights` | `READ` (current text), `HISTORY` (earlier versions), `WRITE`, `TAG` (name, tags, metadata), `DELETE`, `SHARE` (derive grants), `OWN` (all of these, plus relabel and revoke anything on the document). |
| `Label` | `Public < Internal < Confidential < Secret`, on every document. |
| `Grant` | holder, document, rights, parent grant, who issued it, expiry, how far back its history reaches, revoked or not. |
| `Role` | a name, members, rights, and a scope: a tag, one document, or every document. |
| `Decision` | allowed or not, and the `Reason`: owner, grant n, role r, no grant, label above clearance, expired, revoked, session ended. |
| `AuditEvent` | when, who, which document, which right, the decision. |
| `Credential` | a salt and a verifier: PBKDF2-HMAC-SHA256 of the passphrase. |
| `Session` | a principal logged in, by an unforgeable handle (slot + generation). |

Documents are named by a stable `DocId` given when they are created, not
by their name (renaming keeps rights) and not by storage object ids (which
change on every save, and are never handed out).

## Layers

- `authority` (new crate, `no_std` + `alloc`, host-tested): principals,
  rights, grants, roles, labels, the decision function, the audit log,
  credentials and sessions. It knows nothing about storage; it decides.
- Storage (`services_storage` entries, `kernel_bootstrap`'s filesystem):
  every document entry carries its `DocId`, owner and label. The raw
  by-object-id read stops being public: history is only reachable through
  the history call, which asks for `HISTORY`.
- The vault (`kernel_bootstrap/src/vault.rs`): the filesystem and the
  authority together; every read, write, history, tag and delete takes a
  session and is decided, then audited. The desk, the Notepad and the shell
  go through it.
- The kernel: loads the authority from `.authority` at boot, keeps it
  saved, opens the owner's session, and answers shell commands (`whoami`,
  `users`, `useradd`, `login`, `share`, `unshare`, `label`, `grants`,
  `audit`, `role`).
- The desk (later): a sign-in screen, a Sharing sheet on documents, an
  Access card for people and roles, and the audit log.

## Alice and Armando

Alice creates `plan`; she owns it (`OWN`). She edits it five times, so it
has history. She shares it with Armando for `READ` only. Armando's Notepad
opens the current text; asking for an earlier version is refused with
"no HISTORY right (grant 7 gives READ)", and the refusal is in Alice's
audit log. If Alice had given Armando `READ | HISTORY` with a history
window of one day, he would see the versions saved since then and no
earlier. If `plan` is labelled Confidential and Armando's clearance is
Internal, no grant lets him read it. When Alice revokes her grant, Armando's
next read is refused; any grant he had derived from it is gone too.
