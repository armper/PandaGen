//! Authority (FS-001): who may do what to a document.
//!
//! PandaGen's documents have owners, and people hold *grants* to them:
//! records that say "this principal may do these things to this
//! document". A request is allowed because a grant the requester holds
//! allows it -- never because of who they are or where a file sits. Rights
//! are specific (reading a document's current text and reading its history
//! are different rights), sharing only narrows (a grant is derived from one
//! the sharer holds, with the same rights or fewer), revoking a grant
//! revokes what was derived from it, and a sensitivity *label* on a
//! document can refuse a read that every grant would allow. Each decision
//! names the rule that made it, and is kept in an audit log.
//!
//! This crate decides; it stores nothing. The filesystem gives it a
//! [`DocRef`] -- a document's stable id, owner, label and tags -- and asks.
//! See `docs/design/authority.md` for the design and why.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

/// A person, a service or the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PrincipalId(pub u32);

/// The system: the kernel acting for itself. It is not subject to grants
/// or labels, and no session is ever opened as it.
pub const SYSTEM: PrincipalId = PrincipalId(0);

/// A document, by the id it was given when it was made. Names change and
/// storage objects change on every save; this does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DocId(pub u64);

/// A grant, by number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GrantId(pub u32);

/// What may be done to a document. Each right is its own bit; only `OWN`
/// implies the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rights(pub u8);

impl Rights {
    pub const NONE: Rights = Rights(0);
    /// Read the current text.
    pub const READ: Rights = Rights(1);
    /// Read earlier versions.
    pub const HISTORY: Rights = Rights(2);
    /// Write a new version.
    pub const WRITE: Rights = Rights(4);
    /// Rename, tag and change metadata.
    pub const TAG: Rights = Rights(8);
    /// Send to the bin, restore, remove.
    pub const DELETE: Rights = Rights(16);
    /// Derive grants for others from one's own.
    pub const SHARE: Rights = Rights(32);
    /// All of the above, plus relabelling and revoking any grant on the
    /// document.
    pub const OWN: Rights = Rights(64);
    /// Every right.
    pub const ALL: Rights = Rights(127);

    /// Each right with its name, in order.
    pub const NAMED: [(Rights, &'static str); 7] = [
        (Rights::READ, "read"),
        (Rights::HISTORY, "history"),
        (Rights::WRITE, "write"),
        (Rights::TAG, "tag"),
        (Rights::DELETE, "delete"),
        (Rights::SHARE, "share"),
        (Rights::OWN, "own"),
    ];

    /// Every right in `other` is here. `OWN` holds everything.
    pub const fn contains(self, other: Rights) -> bool {
        self.0 & Rights::OWN.0 != 0 || self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Rights) -> Rights {
        Rights(self.0 | other.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The rights a holder of `self` may pass on: all of them for an
    /// owner, otherwise exactly the bits held.
    pub const fn expanded(self) -> Rights {
        if self.0 & Rights::OWN.0 != 0 {
            Rights::ALL
        } else {
            self
        }
    }

    /// `read,history` and the like; unknown words are refused.
    pub fn parse(text: &str) -> Option<Rights> {
        let mut rights = Rights::NONE;
        for word in text.split([',', '+', ' ']).filter(|w| !w.is_empty()) {
            let word = word.to_ascii_lowercase();
            let (bit, _) = Rights::NAMED.iter().find(|(_, name)| *name == word)?;
            rights = rights.union(*bit);
        }
        (!rights.is_empty()).then_some(rights)
    }
}

impl fmt::Display for Rights {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("nothing");
        }
        let mut first = true;
        for (bit, name) in Rights::NAMED {
            if self.0 & bit.0 != 0 {
                if !first {
                    f.write_str("+")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        Ok(())
    }
}

/// How sensitive a document is, and how much a person is cleared for.
/// Ordered: a principal may read a document only when their clearance is
/// at least its label. That rule is mandatory: no grant waives it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum Label {
    Public,
    #[default]
    Internal,
    Confidential,
    Secret,
}

impl Label {
    pub const ALL: [Label; 4] = [
        Label::Public,
        Label::Internal,
        Label::Confidential,
        Label::Secret,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Label::Public => "public",
            Label::Internal => "internal",
            Label::Confidential => "confidential",
            Label::Secret => "secret",
        }
    }

    pub fn parse(text: &str) -> Option<Label> {
        let lower = text.to_ascii_lowercase();
        Label::ALL.into_iter().find(|l| l.name() == lower)
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrincipalKind {
    Person,
    Service,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: PrincipalId,
    pub name: String,
    pub kind: PrincipalKind,
    pub clearance: Label,
}

/// What the authority needs to know about a document to decide: the
/// filesystem supplies it with every question.
#[derive(Debug, Clone, Copy)]
pub struct DocRef<'a> {
    pub id: DocId,
    pub owner: PrincipalId,
    pub label: Label,
    pub tags: &'a [String],
}

/// "This principal may do these things to this document."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub id: GrantId,
    pub doc: DocId,
    pub holder: PrincipalId,
    pub rights: Rights,
    /// The grant this was derived from; `None` when the owner gave it.
    pub parent: Option<GrantId>,
    pub issued_by: PrincipalId,
    /// No longer good at or after this tick.
    pub expires_at: Option<u64>,
    /// History reaches back only to versions saved at or after this tick.
    pub history_since: Option<u64>,
    pub revoked: bool,
}

/// Where a role's rights apply: the documents with a tag, one document, or
/// every document -- always only among the role owner's documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    Tag(String),
    Doc(DocId),
    Everything,
}

/// A grant for many documents at once: its members hold `rights` on every
/// document in `scope` that `owner` owns. (A role made by the system
/// reaches every owner's documents.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    pub owner: PrincipalId,
    pub rights: Rights,
    pub scope: Scope,
    pub members: Vec<PrincipalId>,
}

/// Why a decision went the way it did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reason {
    /// The system acts for itself.
    System,
    /// The principal owns the document.
    Owner,
    /// A grant the principal holds allows it.
    Grant(GrantId),
    /// A role the principal is in allows it.
    Role(String),
    /// Nothing the principal holds gives the right; `held` is what they do
    /// have on the document.
    NoRight { held: Rights },
    /// The document's label is above the principal's clearance.
    AboveClearance { label: Label, clearance: Label },
    /// The version asked for is older than the grant's history reaches.
    BeforeHistoryWindow { since: u64 },
    /// No such principal.
    UnknownPrincipal,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::System => f.write_str("the system"),
            Reason::Owner => f.write_str("owner"),
            Reason::Grant(id) => write!(f, "grant {}", id.0),
            Reason::Role(name) => write!(f, "role {name}"),
            Reason::NoRight { held } if held.is_empty() => f.write_str("no grant"),
            Reason::NoRight { held } => write!(f, "holds only {held}"),
            Reason::AboveClearance { label, clearance } => {
                write!(f, "{label} is above {clearance} clearance")
            }
            Reason::BeforeHistoryWindow { since } => {
                write!(f, "history reaches back only to tick {since}")
            }
            Reason::UnknownPrincipal => f.write_str("no such principal"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub allowed: bool,
    pub reason: Reason,
}

/// One decision, kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub at: u64,
    pub principal: PrincipalId,
    pub doc: DocId,
    pub right: Rights,
    pub allowed: bool,
    pub reason: Reason,
}

/// How many decisions the audit log keeps; the oldest go first.
pub const AUDIT_KEEP: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    UnknownPrincipal,
    NameTaken,
    BadName,
    NoSuchGrant,
    NoSuchRole,
    /// The actor may not do this; the reason says why.
    Refused(Reason),
    /// A grant may only carry rights its giver holds.
    MoreThanHeld {
        held: Rights,
    },
    /// The holder's clearance is below the document's label: the grant
    /// could never be used.
    HolderNotCleared {
        label: Label,
        clearance: Label,
    },
    /// Only the system may do this.
    SystemOnly,
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::UnknownPrincipal => f.write_str("no such person"),
            AuthError::NameTaken => f.write_str("that name is taken"),
            AuthError::BadName => f.write_str("a name is 1-24 letters, digits, '-' or '_'"),
            AuthError::NoSuchGrant => f.write_str("no such grant"),
            AuthError::NoSuchRole => f.write_str("no such role"),
            AuthError::Refused(reason) => write!(f, "refused: {reason}"),
            AuthError::MoreThanHeld { held } => write!(f, "you hold only {held}"),
            AuthError::HolderNotCleared { label, clearance } => {
                write!(f, "{label} is above their {clearance} clearance")
            }
            AuthError::SystemOnly => f.write_str("only the system may do that"),
        }
    }
}

/// What a new grant carries: its rights, when it ends, and how far back
/// its history reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terms {
    pub rights: Rights,
    pub expires_at: Option<u64>,
    pub history_since: Option<u64>,
}

impl Terms {
    /// `rights`, for good, with all the history they allow.
    pub const fn of(rights: Rights) -> Self {
        Self {
            rights,
            expires_at: None,
            history_since: None,
        }
    }

    /// No longer good at or after tick `t`.
    pub const fn until(mut self, t: u64) -> Self {
        self.expires_at = Some(t);
        self
    }

    /// History from tick `t` on only.
    pub const fn history_since(mut self, t: u64) -> Self {
        self.history_since = Some(t);
        self
    }
}

/// Principals, grants and roles, and the log of what was decided.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Authority {
    principals: Vec<Principal>,
    grants: Vec<Grant>,
    roles: Vec<Role>,
    next_grant: u32,
    #[serde(default)]
    audit: VecDeque<AuditEvent>,
}

/// A name for a person, a role: short, plain, unambiguous in a shell.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 24
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl Authority {
    /// An authority with only the system in it.
    pub fn new() -> Self {
        Self {
            principals: alloc::vec![Principal {
                id: SYSTEM,
                name: "system".to_string(),
                kind: PrincipalKind::System,
                clearance: Label::Secret,
            }],
            grants: Vec::new(),
            roles: Vec::new(),
            next_grant: 1,
            audit: VecDeque::new(),
        }
    }

    // ------------------------------------------------------------------
    // Principals

    pub fn principals(&self) -> &[Principal] {
        &self.principals
    }

    pub fn principal(&self, id: PrincipalId) -> Option<&Principal> {
        self.principals.iter().find(|p| p.id == id)
    }

    pub fn principal_named(&self, name: &str) -> Option<&Principal> {
        self.principals.iter().find(|p| p.name == name)
    }

    /// A new person or service, cleared to `clearance`.
    pub fn add_principal(
        &mut self,
        name: &str,
        kind: PrincipalKind,
        clearance: Label,
    ) -> Result<PrincipalId, AuthError> {
        if !valid_name(name) || kind == PrincipalKind::System {
            return Err(AuthError::BadName);
        }
        if self.principal_named(name).is_some() {
            return Err(AuthError::NameTaken);
        }
        let id = PrincipalId(self.principals.iter().map(|p| p.id.0).max().unwrap_or(0) + 1);
        self.principals.push(Principal {
            id,
            name: name.to_string(),
            kind,
            clearance,
        });
        Ok(id)
    }

    /// Clearance is the system's to set: a person cannot raise their own.
    pub fn set_clearance(
        &mut self,
        by: PrincipalId,
        who: PrincipalId,
        clearance: Label,
    ) -> Result<(), AuthError> {
        if by != SYSTEM {
            return Err(AuthError::SystemOnly);
        }
        let p = self
            .principals
            .iter_mut()
            .find(|p| p.id == who)
            .ok_or(AuthError::UnknownPrincipal)?;
        p.clearance = clearance;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Deciding

    /// A grant is live when it and every grant it was derived from are
    /// unrevoked and unexpired at `now`.
    pub fn grant_live(&self, id: GrantId, now: u64) -> bool {
        let mut next = Some(id);
        let mut depth = 0;
        while let Some(id) = next {
            let Some(g) = self.grant(id) else {
                return false;
            };
            if g.revoked || g.expires_at.is_some_and(|t| now >= t) {
                return false;
            }
            next = g.parent;
            depth += 1;
            if depth > self.grants.len() {
                return false;
            }
        }
        true
    }

    fn role_applies(role: &Role, doc: &DocRef) -> bool {
        (role.owner == SYSTEM || role.owner == doc.owner)
            && match &role.scope {
                Scope::Everything => true,
                Scope::Doc(id) => *id == doc.id,
                Scope::Tag(tag) => doc.tags.iter().any(|t| t == tag),
            }
    }

    /// Everything `who` holds on `doc` at `now`, from ownership, grants and
    /// roles -- before the label is considered.
    pub fn held(&self, who: PrincipalId, doc: &DocRef, now: u64) -> Rights {
        if who == SYSTEM || who == doc.owner {
            return Rights::ALL;
        }
        let mut rights = Rights::NONE;
        for g in self
            .grants
            .iter()
            .filter(|g| g.holder == who && g.doc == doc.id)
        {
            if self.grant_live(g.id, now) {
                rights = rights.union(g.rights.expanded());
            }
        }
        for role in self.roles.iter().filter(|r| r.members.contains(&who)) {
            if Self::role_applies(role, doc) {
                rights = rights.union(role.rights.expanded());
            }
        }
        rights
    }

    /// May `who` do `right` to `doc` at `now`? The label is checked first,
    /// for reading and for history: it is mandatory. Then ownership, then
    /// grants, then roles; the first that allows it is the reason.
    pub fn check(&self, who: PrincipalId, doc: &DocRef, right: Rights, now: u64) -> Decision {
        self.decide(who, doc, right, now, None)
    }

    /// May `who` read the version of `doc` saved at tick `saved_at`? As
    /// [`check`](Self::check) for `HISTORY`, and the grant that allows it
    /// must reach back that far.
    pub fn check_version(
        &self,
        who: PrincipalId,
        doc: &DocRef,
        saved_at: u64,
        now: u64,
    ) -> Decision {
        self.decide(who, doc, Rights::HISTORY, now, Some(saved_at))
    }

    fn decide(
        &self,
        who: PrincipalId,
        doc: &DocRef,
        right: Rights,
        now: u64,
        version_at: Option<u64>,
    ) -> Decision {
        let deny = |reason| Decision {
            allowed: false,
            reason,
        };
        let allow = |reason| Decision {
            allowed: true,
            reason,
        };
        if who == SYSTEM {
            return allow(Reason::System);
        }
        let Some(p) = self.principal(who) else {
            return deny(Reason::UnknownPrincipal);
        };
        let reads = Rights::READ.union(Rights::HISTORY);
        if right.0 & reads.0 != 0 && p.clearance < doc.label {
            return deny(Reason::AboveClearance {
                label: doc.label,
                clearance: p.clearance,
            });
        }
        if who == doc.owner {
            return allow(Reason::Owner);
        }
        // Grants: the first live one that gives the right and, for a
        // version, reaches back far enough.
        let mut window: Option<u64> = None;
        for g in self
            .grants
            .iter()
            .filter(|g| g.holder == who && g.doc == doc.id)
        {
            if !g.rights.contains(right) || !self.grant_live(g.id, now) {
                continue;
            }
            match (version_at, g.history_since) {
                (Some(at), Some(since)) if at < since => {
                    window = Some(window.map_or(since, |w: u64| w.min(since)));
                    continue;
                }
                _ => return allow(Reason::Grant(g.id)),
            }
        }
        for role in self.roles.iter().filter(|r| r.members.contains(&who)) {
            if role.rights.contains(right) && Self::role_applies(role, doc) {
                return allow(Reason::Role(role.name.clone()));
            }
        }
        match window {
            Some(since) => deny(Reason::BeforeHistoryWindow { since }),
            None => deny(Reason::NoRight {
                held: self.held(who, doc, now),
            }),
        }
    }

    /// Decide, and keep the decision in the audit log.
    pub fn check_and_log(
        &mut self,
        who: PrincipalId,
        doc: &DocRef,
        right: Rights,
        now: u64,
    ) -> Decision {
        let d = self.check(who, doc, right, now);
        self.log(now, who, doc.id, right, &d);
        d
    }

    /// As [`check_version`](Self::check_version), logged.
    pub fn check_version_and_log(
        &mut self,
        who: PrincipalId,
        doc: &DocRef,
        saved_at: u64,
        now: u64,
    ) -> Decision {
        let d = self.check_version(who, doc, saved_at, now);
        self.log(now, who, doc.id, Rights::HISTORY, &d);
        d
    }

    fn log(&mut self, at: u64, principal: PrincipalId, doc: DocId, right: Rights, d: &Decision) {
        if self.audit.len() >= AUDIT_KEEP {
            self.audit.pop_front();
        }
        self.audit.push_back(AuditEvent {
            at,
            principal,
            doc,
            right,
            allowed: d.allowed,
            reason: d.reason.clone(),
        });
    }

    /// The audit log, oldest first.
    pub fn audit(&self) -> impl DoubleEndedIterator<Item = &AuditEvent> {
        self.audit.iter()
    }

    /// The decisions about `doc`, oldest first: what its owner may read.
    pub fn audit_of(&self, doc: DocId) -> impl DoubleEndedIterator<Item = &AuditEvent> {
        self.audit.iter().filter(move |e| e.doc == doc)
    }

    // ------------------------------------------------------------------
    // Grants

    pub fn grant(&self, id: GrantId) -> Option<&Grant> {
        self.grants.iter().find(|g| g.id == id)
    }

    /// The grants on `doc`, live or not.
    pub fn grants_on(&self, doc: DocId) -> impl Iterator<Item = &Grant> {
        self.grants.iter().filter(move |g| g.doc == doc)
    }

    /// The grants `who` holds.
    pub fn grants_held(&self, who: PrincipalId) -> impl Iterator<Item = &Grant> {
        self.grants.iter().filter(move |g| g.holder == who)
    }

    /// `by` shares `doc` with `to`: a new grant of `rights`, derived from
    /// what `by` holds. `by` must be able to share (own the document, or
    /// hold `SHARE` on it), `rights` must be among what `by` holds, and
    /// the new grant expires no later, and reaches no further back into
    /// history, than the grant it comes from. The holder must be cleared
    /// for the document's label.
    pub fn share(
        &mut self,
        by: PrincipalId,
        doc: &DocRef,
        to: PrincipalId,
        terms: Terms,
        now: u64,
    ) -> Result<GrantId, AuthError> {
        let Terms {
            rights,
            expires_at,
            history_since,
        } = terms;
        let holder = self.principal(to).ok_or(AuthError::UnknownPrincipal)?;
        if rights.is_empty() {
            return Err(AuthError::MoreThanHeld { held: Rights::NONE });
        }
        if holder.clearance < doc.label && rights.0 & (Rights::READ.0 | Rights::HISTORY.0) != 0 {
            return Err(AuthError::HolderNotCleared {
                label: doc.label,
                clearance: holder.clearance,
            });
        }
        let (parent, mut expires_at, mut history_since) = if by == SYSTEM || by == doc.owner {
            (None, expires_at, history_since)
        } else {
            // The live grant of `by` that can share and covers the rights.
            let from = self
                .grants
                .iter()
                .filter(|g| g.holder == by && g.doc == doc.id)
                .filter(|g| g.rights.contains(Rights::SHARE) && g.rights.contains(rights))
                .find(|g| self.grant_live(g.id, now))
                .cloned();
            let Some(from) = from else {
                let held = self.held(by, doc, now);
                return Err(if held.contains(Rights::SHARE) {
                    AuthError::MoreThanHeld { held }
                } else {
                    AuthError::Refused(Reason::NoRight { held })
                });
            };
            let exp = match (expires_at, from.expires_at) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let since = match (history_since, from.history_since) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
            (Some(from.id), exp, since)
        };
        if rights.contains(Rights::OWN) && by != SYSTEM && by != doc.owner {
            return Err(AuthError::MoreThanHeld {
                held: self.held(by, doc, now),
            });
        }
        if history_since.is_some() && !rights.contains(Rights::HISTORY) {
            history_since = None;
        }
        if expires_at.is_some_and(|t| t <= now) {
            expires_at = Some(now);
        }
        let id = GrantId(self.next_grant);
        self.next_grant += 1;
        self.grants.push(Grant {
            id,
            doc: doc.id,
            holder: to,
            rights,
            parent,
            issued_by: by,
            expires_at,
            history_since,
            revoked: false,
        });
        Ok(id)
    }

    /// Revoke grant `id`, and everything derived from it. Its issuer, the
    /// document's owner or the system may.
    pub fn revoke(
        &mut self,
        by: PrincipalId,
        doc: &DocRef,
        id: GrantId,
    ) -> Result<usize, AuthError> {
        let g = self.grant(id).ok_or(AuthError::NoSuchGrant)?;
        if g.doc != doc.id {
            return Err(AuthError::NoSuchGrant);
        }
        if by != SYSTEM && by != doc.owner && by != g.issued_by {
            return Err(AuthError::Refused(Reason::NoRight {
                held: self.held(by, doc, 0),
            }));
        }
        let mut gone = alloc::vec![id];
        let mut i = 0;
        while i < gone.len() {
            let parent = gone[i];
            for child in self.grants.iter().filter(|g| g.parent == Some(parent)) {
                if !gone.contains(&child.id) {
                    gone.push(child.id);
                }
            }
            i += 1;
        }
        let mut n = 0;
        for g in self.grants.iter_mut().filter(|g| gone.contains(&g.id)) {
            if !g.revoked {
                g.revoked = true;
                n += 1;
            }
        }
        Ok(n)
    }

    /// Forget the grants on a removed document.
    pub fn forget(&mut self, doc: DocId) {
        self.grants.retain(|g| g.doc != doc);
        self.roles.retain(|r| r.scope != Scope::Doc(doc));
    }

    /// May `by` relabel `doc` to `label`? The owner (or the system) may,
    /// and only to a label within their own clearance.
    pub fn may_relabel(
        &self,
        by: PrincipalId,
        doc: &DocRef,
        label: Label,
    ) -> Result<(), AuthError> {
        if by == SYSTEM {
            return Ok(());
        }
        let p = self.principal(by).ok_or(AuthError::UnknownPrincipal)?;
        if by != doc.owner {
            return Err(AuthError::Refused(Reason::NoRight {
                held: self.held(by, doc, 0),
            }));
        }
        if label > p.clearance {
            return Err(AuthError::Refused(Reason::AboveClearance {
                label,
                clearance: p.clearance,
            }));
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Roles

    pub fn roles(&self) -> &[Role] {
        &self.roles
    }

    pub fn role(&self, name: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.name == name)
    }

    /// A new role owned by `by`: its rights reach only `by`'s documents.
    pub fn add_role(
        &mut self,
        by: PrincipalId,
        name: &str,
        rights: Rights,
        scope: Scope,
    ) -> Result<(), AuthError> {
        if !valid_name(name) {
            return Err(AuthError::BadName);
        }
        if self.principal(by).is_none() {
            return Err(AuthError::UnknownPrincipal);
        }
        if self.role(name).is_some() {
            return Err(AuthError::NameTaken);
        }
        self.roles.push(Role {
            name: name.to_string(),
            owner: by,
            rights,
            scope,
            members: Vec::new(),
        });
        Ok(())
    }

    fn role_mut(&mut self, by: PrincipalId, name: &str) -> Result<&mut Role, AuthError> {
        let role = self
            .roles
            .iter_mut()
            .find(|r| r.name == name)
            .ok_or(AuthError::NoSuchRole)?;
        if by != SYSTEM && by != role.owner {
            return Err(AuthError::Refused(Reason::NoRight { held: Rights::NONE }));
        }
        Ok(role)
    }

    /// Put `who` in the role; only its owner (or the system) may.
    pub fn join(&mut self, by: PrincipalId, name: &str, who: PrincipalId) -> Result<(), AuthError> {
        if self.principal(who).is_none() {
            return Err(AuthError::UnknownPrincipal);
        }
        let role = self.role_mut(by, name)?;
        if !role.members.contains(&who) {
            role.members.push(who);
        }
        Ok(())
    }

    /// Take `who` out of the role.
    pub fn leave(
        &mut self,
        by: PrincipalId,
        name: &str,
        who: PrincipalId,
    ) -> Result<(), AuthError> {
        let role = self.role_mut(by, name)?;
        role.members.retain(|m| *m != who);
        Ok(())
    }

    /// Remove the role; its members lose what it gave them.
    pub fn remove_role(&mut self, by: PrincipalId, name: &str) -> Result<(), AuthError> {
        self.role_mut(by, name)?;
        self.roles.retain(|r| r.name != name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct World {
        auth: Authority,
        alice: PrincipalId,
        armando: PrincipalId,
        bea: PrincipalId,
    }

    fn work() -> Vec<String> {
        alloc::vec!["work".to_string()]
    }

    fn world() -> World {
        let mut auth = Authority::new();
        let alice = auth
            .add_principal("alice", PrincipalKind::Person, Label::Secret)
            .unwrap();
        let armando = auth
            .add_principal("armando", PrincipalKind::Person, Label::Internal)
            .unwrap();
        let bea = auth
            .add_principal("bea", PrincipalKind::Person, Label::Internal)
            .unwrap();
        World {
            auth,
            alice,
            armando,
            bea,
        }
    }

    fn plan(owner: PrincipalId, tags: &[String]) -> DocRef<'_> {
        DocRef {
            id: DocId(7),
            owner,
            label: Label::Internal,
            tags,
        }
    }

    #[test]
    fn rights_name_parse_and_own_holds_everything() {
        let r = Rights::parse("read,history").unwrap();
        assert!(r.contains(Rights::READ) && r.contains(Rights::HISTORY));
        assert!(!r.contains(Rights::WRITE));
        assert_eq!(alloc::format!("{r}"), "read+history");
        assert!(Rights::OWN.contains(Rights::DELETE));
        assert_eq!(Rights::parse("read,fly"), None);
        assert_eq!(Rights::parse(""), None);
        assert_eq!(Label::parse("Secret"), Some(Label::Secret));
        assert!(Label::Public < Label::Secret);
    }

    /// The scenario the design starts from: Alice shares her document for
    /// reading, not its history; Armando reads the text and is refused
    /// the versions, and both decisions are in the log.
    #[test]
    fn alice_shares_the_text_but_not_its_history() {
        let mut w = world();
        let tags = work();
        let doc = plan(w.alice, &tags);
        let (alice, armando) = (w.alice, w.armando);
        assert!(w.auth.check(alice, &doc, Rights::HISTORY, 0).allowed);
        assert_eq!(
            w.auth.check(armando, &doc, Rights::READ, 0).reason,
            Reason::NoRight { held: Rights::NONE }
        );
        let g = w
            .auth
            .share(alice, &doc, armando, Terms::of(Rights::READ), 0)
            .unwrap();
        let read = w.auth.check_and_log(armando, &doc, Rights::READ, 5);
        assert_eq!(
            read,
            Decision {
                allowed: true,
                reason: Reason::Grant(g)
            }
        );
        let history = w.auth.check_version_and_log(armando, &doc, 3, 6);
        assert!(!history.allowed);
        assert_eq!(history.reason, Reason::NoRight { held: Rights::READ });
        assert_eq!(alloc::format!("{}", history.reason), "holds only read");
        let log: Vec<bool> = w.auth.audit_of(doc.id).map(|e| e.allowed).collect();
        assert_eq!(log, alloc::vec![true, false]);
        // Revoked: nothing.
        assert_eq!(w.auth.revoke(alice, &doc, g), Ok(1));
        assert!(!w.auth.check(armando, &doc, Rights::READ, 7).allowed);
    }

    #[test]
    fn a_history_window_lets_in_recent_versions_only() {
        let mut w = world();
        let tags = work();
        let doc = plan(w.alice, &tags);
        w.auth
            .share(
                w.alice,
                &doc,
                w.armando,
                Terms::of(Rights::READ.union(Rights::HISTORY)).history_since(100),
                0,
            )
            .unwrap();
        assert!(w.auth.check_version(w.armando, &doc, 150, 200).allowed);
        assert_eq!(
            w.auth.check_version(w.armando, &doc, 50, 200).reason,
            Reason::BeforeHistoryWindow { since: 100 }
        );
    }

    #[test]
    fn sharing_only_narrows_and_revoking_cascades() {
        let mut w = world();
        let tags = work();
        let doc = plan(w.alice, &tags);
        let (alice, armando, bea) = (w.alice, w.armando, w.bea);
        // Armando gets read+share, expiring at 1000.
        let a = w
            .auth
            .share(
                alice,
                &doc,
                armando,
                Terms::of(Rights::READ.union(Rights::SHARE)).until(1000),
                0,
            )
            .unwrap();
        // He cannot pass on write, which he does not hold.
        assert_eq!(
            w.auth
                .share(armando, &doc, bea, Terms::of(Rights::WRITE), 1),
            Err(AuthError::MoreThanHeld {
                held: Rights::READ.union(Rights::SHARE)
            })
        );
        // He can pass on read; it expires no later than his.
        let b = w
            .auth
            .share(armando, &doc, bea, Terms::of(Rights::READ).until(5000), 1)
            .unwrap();
        let derived = w.auth.grant(b).unwrap().clone();
        assert_eq!(derived.parent, Some(a));
        assert_eq!(derived.expires_at, Some(1000));
        assert!(w.auth.check(bea, &doc, Rights::READ, 10).allowed);
        assert!(
            !w.auth.check(bea, &doc, Rights::READ, 1000).allowed,
            "expired with its parent"
        );
        // Bea cannot share at all.
        assert!(matches!(
            w.auth.share(bea, &doc, alice, Terms::of(Rights::READ), 2),
            Err(AuthError::Refused(Reason::NoRight { .. }))
        ));
        // Alice revokes Armando's grant: Bea's goes with it.
        assert_eq!(w.auth.revoke(alice, &doc, a), Ok(2));
        assert!(!w.auth.check(bea, &doc, Rights::READ, 10).allowed);
        // Nobody but the issuer, owner or system may revoke.
        let c = w
            .auth
            .share(alice, &doc, armando, Terms::of(Rights::READ), 0)
            .unwrap();
        assert!(w.auth.revoke(bea, &doc, c).is_err());
        // Owning is not shareable except by the owner.
        let s = w
            .auth
            .share(
                alice,
                &doc,
                armando,
                Terms::of(Rights::SHARE.union(Rights::READ)),
                0,
            )
            .unwrap();
        let _ = s;
        assert!(w
            .auth
            .share(armando, &doc, bea, Terms::of(Rights::OWN), 0)
            .is_err());
    }

    /// The label is mandatory: above a principal's clearance nothing gives
    /// a read, not a grant and not a role; and a grant that could never be
    /// used is refused when it is made.
    #[test]
    fn labels_refuse_reads_above_clearance_whatever_the_grants() {
        let mut w = world();
        let tags = work();
        let secret = DocRef {
            id: DocId(9),
            owner: w.alice,
            label: Label::Confidential,
            tags: &tags,
        };
        assert_eq!(
            w.auth
                .share(w.alice, &secret, w.armando, Terms::of(Rights::READ), 0),
            Err(AuthError::HolderNotCleared {
                label: Label::Confidential,
                clearance: Label::Internal
            })
        );
        // Write-only is allowed to be granted (a drop box), and is not a read.
        w.auth
            .share(w.alice, &secret, w.armando, Terms::of(Rights::WRITE), 0)
            .unwrap();
        assert!(w.auth.check(w.armando, &secret, Rights::WRITE, 0).allowed);
        w.auth
            .add_role(
                w.alice,
                "workers",
                Rights::READ,
                Scope::Tag("work".to_string()),
            )
            .unwrap();
        w.auth.join(w.alice, "workers", w.armando).unwrap();
        assert_eq!(
            w.auth.check(w.armando, &secret, Rights::READ, 0).reason,
            Reason::AboveClearance {
                label: Label::Confidential,
                clearance: Label::Internal
            }
        );
        // Raising his clearance is the system's to do, not Alice's.
        assert_eq!(
            w.auth.set_clearance(w.alice, w.armando, Label::Secret),
            Err(AuthError::SystemOnly)
        );
        w.auth
            .set_clearance(SYSTEM, w.armando, Label::Confidential)
            .unwrap();
        assert_eq!(
            w.auth.check(w.armando, &secret, Rights::READ, 0).reason,
            Reason::Role("workers".to_string())
        );
        // Relabelling: the owner, within her clearance.
        assert!(w.auth.may_relabel(w.alice, &secret, Label::Secret).is_ok());
        assert!(w
            .auth
            .may_relabel(w.armando, &secret, Label::Public)
            .is_err());
    }

    /// A role reaches its owner's documents with its tag, and only those:
    /// Bea cannot make a role that opens Alice's work.
    #[test]
    fn a_role_gives_its_members_the_owners_tagged_documents() {
        let mut w = world();
        let tags = work();
        let doc = plan(w.alice, &tags);
        let other = DocRef {
            id: DocId(8),
            owner: w.alice,
            label: Label::Internal,
            tags: &[],
        };
        w.auth
            .add_role(
                w.alice,
                "editors",
                Rights::READ.union(Rights::WRITE),
                Scope::Tag("work".into()),
            )
            .unwrap();
        w.auth.join(w.alice, "editors", w.armando).unwrap();
        assert_eq!(
            w.auth.check(w.armando, &doc, Rights::WRITE, 0).reason,
            Reason::Role("editors".into())
        );
        assert!(
            !w.auth.check(w.armando, &other, Rights::READ, 0).allowed,
            "untagged"
        );
        // Bea's role over #work reaches only Bea's documents.
        w.auth
            .add_role(w.bea, "grab", Rights::ALL, Scope::Tag("work".into()))
            .unwrap();
        w.auth.join(w.bea, "grab", w.bea).unwrap();
        let alices = DocRef {
            id: DocId(7),
            owner: w.alice,
            label: Label::Internal,
            tags: &tags,
        };
        assert!(!w.auth.check(w.bea, &alices, Rights::READ, 0).allowed);
        // Only the role's owner manages it.
        assert!(w.auth.join(w.bea, "editors", w.bea).is_err());
        w.auth.leave(w.alice, "editors", w.armando).unwrap();
        assert!(!w.auth.check(w.armando, &doc, Rights::WRITE, 0).allowed);
        assert_eq!(
            w.auth
                .add_role(w.alice, "editors", Rights::READ, Scope::Everything),
            Err(AuthError::NameTaken)
        );
        w.auth.remove_role(w.alice, "editors").unwrap();
        assert!(w.auth.role("editors").is_none());
    }

    #[test]
    fn names_are_unique_and_plain_and_the_log_is_bounded() {
        let mut w = world();
        assert_eq!(
            w.auth
                .add_principal("alice", PrincipalKind::Person, Label::Public),
            Err(AuthError::NameTaken)
        );
        assert_eq!(
            w.auth
                .add_principal("no spaces", PrincipalKind::Person, Label::Public),
            Err(AuthError::BadName)
        );
        assert_eq!(
            w.auth
                .add_principal("root", PrincipalKind::System, Label::Secret),
            Err(AuthError::BadName)
        );
        let tags = work();
        let doc = plan(w.alice, &tags);
        for t in 0..(AUDIT_KEEP as u64 + 10) {
            w.auth.check_and_log(w.armando, &doc, Rights::READ, t);
        }
        assert_eq!(w.auth.audit().count(), AUDIT_KEEP);
        assert_eq!(w.auth.audit().next().unwrap().at, 10);
        assert!(w.auth.check(SYSTEM, &doc, Rights::ALL, 0).allowed);
        assert_eq!(
            w.auth.check(PrincipalId(99), &doc, Rights::READ, 0).reason,
            Reason::UnknownPrincipal
        );
    }

    #[test]
    fn the_whole_authority_round_trips_as_json() {
        let mut w = world();
        let tags = work();
        let doc = plan(w.alice, &tags);
        w.auth
            .share(
                w.alice,
                &doc,
                w.armando,
                Terms::of(Rights::READ).until(9).history_since(1),
                0,
            )
            .unwrap();
        w.auth
            .add_role(w.alice, "r", Rights::READ, Scope::Doc(DocId(7)))
            .unwrap();
        w.auth.check_and_log(w.armando, &doc, Rights::READ, 1);
        let text = serde_json::to_string(&w.auth).unwrap();
        let back: Authority = serde_json::from_str(&text).unwrap();
        assert_eq!(back.principals(), w.auth.principals());
        assert_eq!(back.grants_on(DocId(7)).count(), 1);
        assert_eq!(back.audit().count(), 1);
        assert!(back.check(w.armando, &doc, Rights::READ, 2).allowed);
        w.auth.forget(DocId(7));
        assert_eq!(w.auth.grants_on(DocId(7)).count(), 0);
        assert!(w.auth.roles().is_empty());
    }
}
