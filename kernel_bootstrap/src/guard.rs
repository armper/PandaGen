//! The filesystem's guard (FS-003): the authority, the people who can log
//! in, the sessions open now, and who the filesystem is acting for.
//!
//! The filesystem is the reference monitor: every public call on
//! [`BareMetalFilesystem`](crate::bare_metal_storage::BareMetalFilesystem)
//! asks its guard before it touches storage, and the guard asks the
//! authority on behalf of the *actor* -- the system while the kernel boots,
//! then the session the console is signed in as. A closed session acts for
//! nobody: everything is refused, rather than falling back to the system.
//!
//! What the guard keeps -- principals, grants, roles, the audit log and the
//! accounts' verifiers -- is saved in `.authority`, a document owned by the
//! system and labelled secret, so no person's grant can reach it.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use authority::credential::{Accounts, SessionHandle, Sessions};
use authority::{Authority, DocId, DocRef, Label, PrincipalId, Rights, SYSTEM};
use serde::{Deserialize, Serialize};
use services_storage::persistent_fs::DirectoryEntry;

/// Where the guard keeps what it knows.
pub const AUTHORITY_FILE: &str = ".authority";
/// How often, in ticks, the kernel keeps a changed guard (FS-004).
pub const SAVE_EVERY: u64 = 500;

/// Who the filesystem is acting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// The kernel for itself: at boot, before anyone signs in.
    System,
    /// A signed-in person.
    Session(SessionHandle),
}

/// What `.authority` holds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GuardState {
    pub authority: Authority,
    pub accounts: Accounts,
    /// Who the console signs in as at boot (FS-004): the machine's first
    /// person, until the desk asks who is there.
    #[serde(default)]
    pub console: Option<String>,
}

/// The first person on a new disk is called `owner=<name>` from the
/// kernel command line, or "owner" (FS-004).
pub const DEFAULT_OWNER: &str = "owner";

/// The first person's name from the kernel command line.
pub fn owner_name(cmdline: &str) -> String {
    cmdline
        .split_ascii_whitespace()
        .filter_map(|t| t.strip_prefix("owner="))
        .rfind(|n| authority::valid_name(n))
        .unwrap_or(DEFAULT_OWNER)
        .into()
}

/// What booting the guard did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootReport {
    /// This disk had no `.authority`: its first person was made.
    pub first_person: bool,
    /// Who the console is signed in as.
    pub console: String,
    /// Documents nobody owned, now the console person's.
    pub adopted: usize,
}

/// Boot the guard (FS-004): take up `.authority`, or on a disk without
/// one make its first person, called `owner`, cleared for everything; give
/// that person whatever nobody owns (the example files, a disk from before
/// owners); keep it all; and sign the console in as them.
pub fn boot(
    fs: &mut crate::bare_metal_storage::BareMetalFilesystem,
    owner: &str,
    now: u64,
) -> Result<BootReport, services_storage::TransactionError> {
    fs.set_clock(now);
    fs.guard.act_as(Actor::System);
    let loaded = fs.load_guard()?;
    let console = fs
        .guard
        .console
        .clone()
        .filter(|name| fs.guard.authority.principal_named(name).is_some());
    let (who, first_person) = match console {
        Some(name) if loaded => (fs.guard.authority.principal_named(&name).unwrap().id, false),
        _ => {
            let id = match fs.guard.authority.principal_named(owner) {
                Some(p) => p.id,
                None => fs
                    .guard
                    .authority
                    .add_principal(owner, authority::PrincipalKind::Person, Label::Secret)
                    .map_err(|e| {
                        services_storage::TransactionError::StorageError(alloc::format!("{e}"))
                    })?,
            };
            fs.guard.console = Some(owner.into());
            (id, true)
        }
    };
    let adopted = fs.adopt_unowned(who)?;
    fs.save_guard()?;
    let session = fs.guard.sessions.open(who, now);
    fs.guard.act_as(Actor::Session(session));
    Ok(BootReport {
        first_person,
        console: fs
            .guard
            .authority
            .principal(who)
            .map(|p| p.name.clone())
            .unwrap_or_default(),
        adopted,
    })
}

pub struct Guard {
    pub authority: Authority,
    pub accounts: Accounts,
    pub sessions: Sessions,
    /// Who the console signs in as at boot.
    pub console: Option<String>,
    actor: Actor,
    /// Something to save has changed since the last save.
    dirty: bool,
}

impl Default for Guard {
    fn default() -> Self {
        Self::new()
    }
}

/// An entry's label, from the byte it is kept as.
pub fn label_of(entry: &DirectoryEntry) -> Label {
    Label::ALL
        .get(entry.label as usize)
        .copied()
        .unwrap_or(Label::Secret)
}

/// What the authority needs to know about an entry.
pub fn doc_ref(entry: &DirectoryEntry) -> DocRef<'_> {
    DocRef {
        id: DocId(entry.doc_id),
        owner: PrincipalId(entry.owner),
        label: label_of(entry),
        tags: &entry.tags,
    }
}

impl Guard {
    pub fn new() -> Self {
        Self {
            authority: Authority::new(),
            accounts: Accounts::new(),
            sessions: Sessions::new(),
            console: None,
            actor: Actor::System,
            dirty: false,
        }
    }

    /// A guard with cheap verifiers, for tests.
    pub fn for_tests() -> Self {
        Self {
            accounts: Accounts::with_rounds(4),
            ..Self::new()
        }
    }

    pub fn actor(&self) -> Actor {
        self.actor
    }

    /// Act for `actor` from now on.
    pub fn act_as(&mut self, actor: Actor) {
        self.actor = actor;
    }

    /// The principal the filesystem acts for, or `None` when the session
    /// it was acting for has closed.
    pub fn principal(&self) -> Option<PrincipalId> {
        match self.actor {
            Actor::System => Some(SYSTEM),
            Actor::Session(handle) => self.sessions.principal(handle),
        }
    }

    /// The name of whoever the filesystem acts for.
    pub fn principal_name(&self) -> Option<&str> {
        let who = self.principal()?;
        self.authority.principal(who).map(|p| p.name.as_str())
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Whether anything needs saving; clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        core::mem::take(&mut self.dirty)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// What to save.
    pub fn state(&self) -> GuardState {
        GuardState {
            authority: self.authority.clone(),
            accounts: self.accounts.clone(),
            console: self.console.clone(),
        }
    }

    /// Take up a saved state. Sessions are not kept across a boot.
    pub fn restore(&mut self, state: GuardState) {
        self.authority = state.authority;
        self.accounts = state.accounts;
        self.console = state.console;
        self.dirty = false;
    }

    /// Decide `right` on `entry` for the actor, and log it (the system's
    /// own acts are not logged: they are the kernel doing its job).
    pub fn decide(
        &mut self,
        entry: &DirectoryEntry,
        right: Rights,
        now: u64,
    ) -> Result<(), String> {
        let who = self.principal().ok_or_else(|| String::from("no session"))?;
        if who == SYSTEM {
            return Ok(());
        }
        let d = self
            .authority
            .check_and_log(who, &doc_ref(entry), right, now);
        self.dirty = true;
        if d.allowed {
            Ok(())
        } else {
            Err(alloc::format!("{right} on {}: {}", entry.name, d.reason))
        }
    }

    /// Decide reading the version of `entry` saved at `saved_at`.
    pub fn decide_version(
        &mut self,
        entry: &DirectoryEntry,
        saved_at: u64,
        now: u64,
    ) -> Result<(), String> {
        let who = self.principal().ok_or_else(|| String::from("no session"))?;
        if who == SYSTEM {
            return Ok(());
        }
        let d = self
            .authority
            .check_version_and_log(who, &doc_ref(entry), saved_at, now);
        self.dirty = true;
        if d.allowed {
            Ok(())
        } else {
            Err(alloc::format!("history of {}: {}", entry.name, d.reason))
        }
    }

    /// Whether the actor may see `entry` in a listing: they hold some
    /// right on it, and its label is within their clearance. Not logged --
    /// a listing is not an access.
    pub fn visible(&self, entry: &DirectoryEntry, now: u64) -> bool {
        let Some(who) = self.principal() else {
            return false;
        };
        if who == SYSTEM {
            return true;
        }
        let Some(p) = self.authority.principal(who) else {
            return false;
        };
        let doc = doc_ref(entry);
        p.clearance >= doc.label && !self.authority.held(who, &doc, now).is_empty()
    }

    /// Everything the actor holds on `entry`.
    pub fn held(&self, entry: &DirectoryEntry, now: u64) -> Rights {
        match self.principal() {
            Some(who) => self.authority.held(who, &doc_ref(entry), now),
            None => Rights::NONE,
        }
    }

    /// The owner of a document the actor creates: the person, or nobody
    /// yet when the system writes before anyone exists (adopted later).
    pub fn owner_of_new(&self) -> u32 {
        match self.principal() {
            Some(SYSTEM) | None => services_storage::persistent_fs::NOBODY,
            Some(who) => who.0,
        }
    }

    /// The names of the principals, by number, for listings.
    pub fn name_of(&self, id: u32) -> String {
        if id == services_storage::persistent_fs::NOBODY {
            return String::from("nobody");
        }
        self.authority
            .principal(PrincipalId(id))
            .map(|p| p.name.clone())
            .unwrap_or_else(|| alloc::format!("#{id}"))
    }

    /// The persons, by name.
    pub fn people(&self) -> Vec<String> {
        self.authority
            .principals()
            .iter()
            .filter(|p| p.id != SYSTEM)
            .map(|p| p.name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bare_metal_storage::BareMetalFilesystem;
    use authority::{PrincipalKind, Terms};
    use services_storage::TransactionError;

    struct World {
        fs: BareMetalFilesystem,
        alice: SessionHandle,
        armando: SessionHandle,
    }

    fn denied(r: Result<impl core::fmt::Debug, TransactionError>) -> String {
        match r {
            Err(TransactionError::Denied(why)) => why,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn not_there(r: Result<impl core::fmt::Debug, TransactionError>) {
        assert!(
            matches!(r, Err(TransactionError::ObjectNotFound(_))),
            "expected nothing there, got {r:?}"
        );
    }

    /// Alice (cleared for secret) and Armando (internal), each signed in.
    fn world() -> World {
        let mut fs = BareMetalFilesystem::new().unwrap();
        fs.guard = Guard::for_tests();
        let g = &mut fs.guard;
        let alice = g
            .authority
            .add_principal("alice", PrincipalKind::Person, Label::Secret)
            .unwrap();
        let armando = g
            .authority
            .add_principal("armando", PrincipalKind::Person, Label::Internal)
            .unwrap();
        g.accounts
            .set_passphrase(&g.authority, SYSTEM, alice, None, "alice pass", [1; 16])
            .unwrap();
        g.accounts
            .set_passphrase(&g.authority, SYSTEM, armando, None, "armando pass", [2; 16])
            .unwrap();
        let a = g
            .sessions
            .login(&mut g.accounts, &g.authority, "alice", "alice pass", 1)
            .unwrap();
        let b = g
            .sessions
            .login(&mut g.accounts, &g.authority, "armando", "armando pass", 1)
            .unwrap();
        World {
            fs,
            alice: a,
            armando: b,
        }
    }

    /// The scenario the design starts from, through the filesystem: Alice
    /// writes `plan` three times; Armando, given READ, reads it and is
    /// refused its history, its writing, its tags and its bin; Alice's
    /// unshared `diary` is not even there for him.
    #[test]
    fn armando_reads_alices_plan_but_not_its_history() {
        let mut w = world();
        let fs = &mut w.fs;
        fs.guard.act_as(Actor::Session(w.alice));
        for (t, text) in [(100, "one"), (200, "two"), (300, "three")] {
            fs.write_named("plan", text.as_bytes(), t, Some("text/plain"))
                .unwrap();
        }
        fs.write_named("diary", b"mine", 300, None).unwrap();
        assert_eq!(fs.list_versions("plan").unwrap().len(), 2);
        let grant = fs
            .share("plan", "armando", Terms::of(Rights::READ))
            .unwrap();

        fs.guard.act_as(Actor::Session(w.armando));
        assert_eq!(fs.read_file_by_name("plan").unwrap(), b"three");
        assert!(denied(fs.list_versions("plan")).contains("holds only read"));
        denied(fs.read_version("plan", 0));
        denied(fs.write_named("plan", b"mine now", 400, None));
        denied(fs.update_entry("plan", 400, |e| e.tags.push("x".into())));
        denied(fs.update_entry("plan", 400, |e| e.trashed = true));
        denied(fs.delete_file_at("plan", 400));
        assert_eq!(fs.list_files().unwrap(), alloc::vec!["plan".to_string()]);
        let listed = fs.list_entries().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            (listed[0].owner.as_str(), listed[0].held),
            ("alice", Rights::READ.0)
        );
        not_there(fs.read_file_by_name("diary"));
        // A name he cannot see is still someone's: he cannot write over it.
        denied(fs.write_named("diary", b"hi", 400, None));
        // Nor can he share what he was given without SHARE.
        denied(fs.share("plan", "alice", Terms::of(Rights::READ)));

        // Alice's audit log has his read and his refusals.
        let doc = authority::DocId(fs.document("plan").unwrap().unwrap().doc_id);
        let refused = fs
            .guard
            .authority
            .audit_of(doc)
            .filter(|e| !e.allowed)
            .count();
        assert!(refused >= 5, "{refused}");

        // Revoked: gone from his listing and his reach.
        fs.guard.act_as(Actor::Session(w.alice));
        assert_eq!(fs.revoke("plan", grant).unwrap(), 1);
        fs.guard.act_as(Actor::Session(w.armando));
        assert!(fs.list_files().unwrap().is_empty());
        not_there(fs.read_file_by_name("plan"));
    }

    #[test]
    fn a_history_window_and_a_label_bound_what_is_shared() {
        let mut w = world();
        let fs = &mut w.fs;
        fs.guard.act_as(Actor::Session(w.alice));
        for (t, text) in [(100, "one"), (200, "two"), (300, "three")] {
            fs.write_named("plan", text.as_bytes(), t, None).unwrap();
        }
        fs.share(
            "plan",
            "armando",
            Terms::of(Rights::READ.union(Rights::HISTORY)).history_since(150),
        )
        .unwrap();
        fs.guard.act_as(Actor::Session(w.armando));
        // Versions saved at 200 and 100; he reaches back to 150.
        let versions = fs.list_versions("plan").unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].modified_at, 200);
        assert_eq!(fs.read_version("plan", 0).unwrap(), b"two");
        assert!(fs.read_version("plan", 1).is_err());

        // Confidential is above his clearance: not there for him, grant or
        // no grant. He cannot relabel it back.
        fs.guard.act_as(Actor::Session(w.alice));
        fs.relabel("plan", Label::Confidential).unwrap();
        fs.guard.act_as(Actor::Session(w.armando));
        not_there(fs.read_file_by_name("plan"));
        assert!(fs.list_files().unwrap().is_empty());
        not_there(fs.relabel("plan", Label::Public));
        // A save keeps the label and the owner.
        fs.guard.act_as(Actor::Session(w.alice));
        fs.write_named("plan", b"four", 400, None).unwrap();
        let e = fs.document("plan").unwrap().unwrap();
        assert_eq!((label_of(&e), e.owner), (Label::Confidential, 1));
        // Tagging cannot change who owns it.
        fs.update_entry("plan", 401, |e| {
            e.tags.push("work".into());
            e.owner = 2;
            e.label = 0;
        })
        .unwrap();
        let e = fs.document("plan").unwrap().unwrap();
        assert_eq!((e.owner, e.label, e.tags.len()), (1, 2, 1));
    }

    #[test]
    fn a_closed_session_acts_for_nobody() {
        let mut w = world();
        let fs = &mut w.fs;
        fs.guard.act_as(Actor::Session(w.alice));
        fs.write_named("plan", b"x", 1, None).unwrap();
        fs.guard.sessions.close(w.alice);
        assert!(fs.list_files().unwrap().is_empty());
        not_there(fs.read_file_by_name("plan"));
        denied(fs.write_named("new", b"x", 2, None));
        assert_eq!(fs.guard.principal(), None);
    }

    #[test]
    fn the_first_boot_makes_a_person_and_later_boots_sign_them_back_in() {
        assert_eq!(owner_name("display=desk owner=armando x=1"), "armando");
        assert_eq!(owner_name("owner=no/slash"), DEFAULT_OWNER);
        assert_eq!(owner_name(""), DEFAULT_OWNER);
        let mut fs = BareMetalFilesystem::new().unwrap();
        fs.guard = Guard::for_tests();
        // The kernel seeds a file as the system, before anyone exists.
        fs.write_named("welcome.txt", b"hi", 1, None).unwrap();
        let first = boot(&mut fs, "armando", 10).unwrap();
        assert_eq!(
            first,
            BootReport {
                first_person: true,
                console: "armando".into(),
                adopted: 1
            },
            "welcome.txt"
        );
        assert_eq!(fs.guard.principal_name(), Some("armando"));
        assert_eq!(fs.read_file_by_name("welcome.txt").unwrap(), b"hi");
        fs.write_named("plan", b"mine", 11, None).unwrap();
        assert_eq!(
            fs.document("plan").unwrap().unwrap().owner,
            first_owner(&fs)
        );
        // Next boot: the same person, from `.authority`, whatever the
        // command line now says.
        fs.guard = Guard::for_tests();
        let again = boot(&mut fs, "someone-else", 20).unwrap();
        assert_eq!(
            again,
            BootReport {
                first_person: false,
                console: "armando".into(),
                adopted: 0
            }
        );
        assert_eq!(fs.read_file_by_name("plan").unwrap(), b"mine");
    }

    fn first_owner(fs: &BareMetalFilesystem) -> u32 {
        fs.guard.authority.principal_named("armando").unwrap().id.0
    }

    /// What the system wrote before anyone existed is nobody's until
    /// adopted; `.authority` is the system's, secret, and out of every
    /// person's reach; and it carries the grants across a remount.
    #[test]
    fn adoption_and_the_authority_file() {
        let mut w = world();
        let fs = &mut w.fs;
        fs.guard.act_as(Actor::System);
        fs.write_named("readme", b"hello", 1, None).unwrap();
        let alice = fs.guard.authority.principal_named("alice").unwrap().id;
        assert_eq!(
            fs.document("readme").unwrap().unwrap().owner,
            services_storage::persistent_fs::NOBODY
        );
        fs.guard.act_as(Actor::Session(w.alice));
        not_there(fs.read_file_by_name("readme"));
        denied(fs.adopt_unowned(alice));
        fs.guard.act_as(Actor::System);
        assert_eq!(fs.adopt_unowned(alice).unwrap(), 1);
        fs.guard.act_as(Actor::Session(w.alice));
        assert_eq!(fs.read_file_by_name("readme").unwrap(), b"hello");
        fs.share("readme", "armando", Terms::of(Rights::READ))
            .unwrap();
        fs.save_guard().unwrap();
        // Even Alice, cleared for secret, cannot see it.
        assert!(!fs.list_files().unwrap().iter().any(|n| n == AUTHORITY_FILE));
        not_there(fs.read_file_by_name(AUTHORITY_FILE));
        denied(fs.write_named(AUTHORITY_FILE, b"{}", 5, None));
        // A fresh guard takes up the saved grants.
        fs.guard = Guard::for_tests();
        assert!(fs.load_guard().unwrap());
        let armando = fs
            .guard
            .sessions
            .open(fs.guard.authority.principal_named("armando").unwrap().id, 9);
        fs.guard.act_as(Actor::Session(armando));
        assert_eq!(fs.read_file_by_name("readme").unwrap(), b"hello");
        assert!(fs.guard.accounts.has_passphrase(alice));
    }
}
