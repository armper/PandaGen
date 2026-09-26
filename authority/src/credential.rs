//! Proving who you are (FS-002): passphrases, accounts and sessions.
//!
//! A passphrase is never kept. An account keeps a random salt and a
//! *verifier* -- PBKDF2-HMAC-SHA256 of the passphrase with that salt, many
//! rounds -- and a login derives the same from what was typed and compares
//! in constant time. A good login opens a *session*: a slot and a
//! generation, which the desk and the shell hold and present. Nothing
//! below the session sees a passphrase; nothing above it sees a verifier.
//!
//! Guessing is slowed: after `FREE_TRIES` wrong passphrases an account is
//! locked for a time that doubles with each further miss. A name that does
//! not exist fails the same way, after the same work, so a login does not
//! say which names are real.

use crate::{AuthError, Authority, PrincipalId, PrincipalKind, SYSTEM};
use alloc::vec::Vec;
use remote_ipc::sha256;
use serde::{Deserialize, Serialize};

/// PBKDF2 rounds for a new verifier: slow enough to make guessing costly,
/// quick enough to log in on the boot processor.
pub const ROUNDS: u32 = 4096;
/// Wrong passphrases before an account locks.
pub const FREE_TRIES: u32 = 5;
/// The first lock, in seconds (the filesystem's clock); each further miss
/// doubles it.
pub const LOCK_SECS: u64 = 30;
/// The longest lock: ten minutes.
pub const LOCK_MAX_SECS: u64 = 600;
/// Shortest passphrase an account will take.
pub const MIN_PASSPHRASE: usize = 4;

/// A salt and what the passphrase derives to with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    pub salt: [u8; 16],
    pub verifier: [u8; 32],
    pub rounds: u32,
}

/// PBKDF2-HMAC-SHA256, one block (RFC 8018).
pub fn pbkdf2(passphrase: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
    let mut first = Vec::with_capacity(salt.len() + 4);
    first.extend_from_slice(salt);
    first.extend_from_slice(&1u32.to_be_bytes());
    let mut u = sha256::hmac(passphrase, &first);
    let mut out = u;
    for _ in 1..rounds.max(1) {
        u = sha256::hmac(passphrase, &u);
        for (o, b) in out.iter_mut().zip(u.iter()) {
            *o ^= b;
        }
    }
    out
}

/// Equal, in time that does not depend on where they differ.
fn same(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

impl Credential {
    pub fn new(passphrase: &str, salt: [u8; 16], rounds: u32) -> Self {
        Self {
            salt,
            verifier: pbkdf2(passphrase.as_bytes(), &salt, rounds),
            rounds,
        }
    }

    pub fn verify(&self, passphrase: &str) -> bool {
        same(
            &pbkdf2(passphrase.as_bytes(), &self.salt, self.rounds),
            &self.verifier,
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Tries {
    misses: u32,
    locked_until: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Account {
    principal: PrincipalId,
    credential: Credential,
    #[serde(default)]
    tries: Tries,
}

/// Why a login did not open a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginError {
    /// The name or the passphrase is wrong -- deliberately not saying which.
    Wrong,
    /// Too many wrong passphrases: try again at this time.
    Locked { until: u64 },
}

impl core::fmt::Display for LoginError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LoginError::Wrong => f.write_str("wrong name or passphrase"),
            LoginError::Locked { until } => write!(f, "locked until {until}"),
        }
    }
}

/// The people who can log in, by their verifiers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Accounts {
    accounts: Vec<Account>,
    /// Rounds for new verifiers (lower in tests).
    #[serde(default = "default_rounds")]
    rounds: u32,
}

fn default_rounds() -> u32 {
    ROUNDS
}

impl Accounts {
    pub fn new() -> Self {
        Self::with_rounds(ROUNDS)
    }

    /// Accounts whose new verifiers take `rounds` (tests use few).
    pub fn with_rounds(rounds: u32) -> Self {
        Self {
            accounts: Vec::new(),
            rounds: rounds.max(1),
        }
    }

    pub fn has_passphrase(&self, who: PrincipalId) -> bool {
        self.accounts.iter().any(|a| a.principal == who)
    }

    /// Set `who`'s passphrase. The system sets anyone's (a new account, or
    /// a reset); a person sets their own, and must give the old one.
    pub fn set_passphrase(
        &mut self,
        authority: &Authority,
        by: PrincipalId,
        who: PrincipalId,
        old: Option<&str>,
        new: &str,
        salt: [u8; 16],
    ) -> Result<(), AuthError> {
        let p = authority
            .principal(who)
            .ok_or(AuthError::UnknownPrincipal)?;
        if p.kind != PrincipalKind::Person {
            return Err(AuthError::SystemOnly);
        }
        if new.chars().count() < MIN_PASSPHRASE {
            return Err(AuthError::BadName);
        }
        if by != SYSTEM {
            if by != who {
                return Err(AuthError::SystemOnly);
            }
            let current = self.accounts.iter().find(|a| a.principal == who);
            let ok = match (current, old) {
                (Some(a), Some(old)) => a.credential.verify(old),
                (None, _) => true,
                (Some(_), None) => false,
            };
            if !ok {
                return Err(AuthError::Refused(crate::Reason::NoRight {
                    held: crate::Rights::NONE,
                }));
            }
        }
        let credential = Credential::new(new, salt, self.rounds);
        match self.accounts.iter_mut().find(|a| a.principal == who) {
            Some(a) => {
                a.credential = credential;
                a.tries = Tries::default();
            }
            None => self.accounts.push(Account {
                principal: who,
                credential,
                tries: Tries::default(),
            }),
        }
        Ok(())
    }

    /// Check `passphrase` for the person called `name`. A name with no
    /// account costs the same as a wrong passphrase and says the same.
    pub fn authenticate(
        &mut self,
        authority: &Authority,
        name: &str,
        passphrase: &str,
        now: u64,
    ) -> Result<PrincipalId, LoginError> {
        let principal = authority.principal_named(name).map(|p| p.id);
        let rounds = self.rounds;
        let Some(account) =
            principal.and_then(|id| self.accounts.iter_mut().find(|a| a.principal == id))
        else {
            // The same work as a real check, against nothing.
            let _ = pbkdf2(passphrase.as_bytes(), &[0u8; 16], rounds);
            return Err(LoginError::Wrong);
        };
        if now < account.tries.locked_until {
            return Err(LoginError::Locked {
                until: account.tries.locked_until,
            });
        }
        if account.credential.verify(passphrase) {
            account.tries = Tries::default();
            return Ok(account.principal);
        }
        account.tries.misses += 1;
        if account.tries.misses >= FREE_TRIES {
            let doublings = (account.tries.misses - FREE_TRIES).min(8);
            let lock = (LOCK_SECS << doublings).min(LOCK_MAX_SECS);
            account.tries.locked_until = now + lock;
        }
        Err(LoginError::Wrong)
    }

    /// Forget `who`'s account (they can no longer log in).
    pub fn remove(&mut self, who: PrincipalId) {
        self.accounts.retain(|a| a.principal != who);
    }
}

/// A session, as the desk and the shell hold it: a slot and the
/// generation it was opened in. A closed session's slot is reused with a
/// new generation, so an old handle stops working.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionHandle {
    pub slot: u16,
    pub generation: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Slot {
    generation: u32,
    open: Option<(PrincipalId, u64)>,
}

/// The sessions open now. They are not kept across a boot.
#[derive(Debug, Clone, Default)]
pub struct Sessions {
    slots: Vec<Slot>,
}

impl Sessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a session for `who` (already authenticated).
    pub fn open(&mut self, who: PrincipalId, now: u64) -> SessionHandle {
        if let Some((i, slot)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.open.is_none())
        {
            slot.open = Some((who, now));
            return SessionHandle {
                slot: i as u16,
                generation: slot.generation,
            };
        }
        self.slots.push(Slot {
            generation: 1,
            open: Some((who, now)),
        });
        SessionHandle {
            slot: (self.slots.len() - 1) as u16,
            generation: 1,
        }
    }

    /// Log in: authenticate, then open a session.
    pub fn login(
        &mut self,
        accounts: &mut Accounts,
        authority: &Authority,
        name: &str,
        passphrase: &str,
        now: u64,
    ) -> Result<SessionHandle, LoginError> {
        let who = accounts.authenticate(authority, name, passphrase, now)?;
        Ok(self.open(who, now))
    }

    /// Who a handle speaks for, if it is still open.
    pub fn principal(&self, handle: SessionHandle) -> Option<PrincipalId> {
        let slot = self.slots.get(handle.slot as usize)?;
        match slot.open {
            Some((who, _)) if slot.generation == handle.generation => Some(who),
            _ => None,
        }
    }

    /// Close a session; its handle stops working.
    pub fn close(&mut self, handle: SessionHandle) -> bool {
        match self.slots.get_mut(handle.slot as usize) {
            Some(slot) if slot.generation == handle.generation && slot.open.is_some() => {
                slot.open = None;
                slot.generation = slot.generation.wrapping_add(1).max(1);
                true
            }
            _ => false,
        }
    }

    /// Close every session of `who` (their account went, or was reset).
    pub fn close_all(&mut self, who: PrincipalId) -> usize {
        let mut n = 0;
        for slot in self.slots.iter_mut() {
            if slot.open.is_some_and(|(p, _)| p == who) {
                slot.open = None;
                slot.generation = slot.generation.wrapping_add(1).max(1);
                n += 1;
            }
        }
        n
    }

    /// The open sessions: who, since when.
    pub fn open_now(&self) -> impl Iterator<Item = (SessionHandle, PrincipalId, u64)> + '_ {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            s.open.map(|(who, at)| {
                (
                    SessionHandle {
                        slot: i as u16,
                        generation: s.generation,
                    },
                    who,
                    at,
                )
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Label;

    fn setup() -> (Authority, Accounts, PrincipalId) {
        let mut auth = Authority::new();
        let alice = auth
            .add_principal("alice", PrincipalKind::Person, Label::Secret)
            .unwrap();
        let mut accounts = Accounts::with_rounds(8);
        accounts
            .set_passphrase(&auth, SYSTEM, alice, None, "correct horse", [7; 16])
            .unwrap();
        (auth, accounts, alice)
    }

    #[test]
    fn pbkdf2_matches_the_rfc_7914_vector() {
        // RFC 7914 section 11: PBKDF2-HMAC-SHA256("passwd", "salt", 1).
        let out = pbkdf2(b"passwd", b"salt", 1);
        assert_eq!(out[..8], [0x55, 0xac, 0x04, 0x6e, 0x56, 0xe3, 0x08, 0x9f]);
    }

    #[test]
    fn a_passphrase_opens_a_session_and_nothing_else_does() {
        let (auth, mut accounts, alice) = setup();
        let mut sessions = Sessions::new();
        let s = sessions
            .login(&mut accounts, &auth, "alice", "correct horse", 0)
            .unwrap();
        assert_eq!(sessions.principal(s), Some(alice));
        assert_eq!(
            sessions.login(&mut accounts, &auth, "alice", "wrong", 1),
            Err(LoginError::Wrong)
        );
        assert_eq!(
            sessions.login(&mut accounts, &auth, "mallory", "correct horse", 1),
            Err(LoginError::Wrong),
            "an unknown name says the same"
        );
        // A forged handle (the next generation, another slot) is nobody.
        assert_eq!(
            sessions.principal(SessionHandle {
                slot: 0,
                generation: 2
            }),
            None
        );
        assert_eq!(
            sessions.principal(SessionHandle {
                slot: 5,
                generation: 1
            }),
            None
        );
        // Closed: the handle stops working; the slot is reused with a new
        // generation.
        assert!(sessions.close(s));
        assert_eq!(sessions.principal(s), None);
        let again = sessions.open(alice, 2);
        assert_eq!(again.slot, s.slot);
        assert_ne!(again.generation, s.generation);
        assert_eq!(sessions.principal(s), None);
        assert_eq!(sessions.open_now().count(), 1);
        assert_eq!(sessions.close_all(alice), 1);
    }

    #[test]
    fn guessing_locks_the_account_for_longer_each_time() {
        let (auth, mut accounts, _) = setup();
        for t in 0..FREE_TRIES as u64 {
            assert_eq!(
                accounts.authenticate(&auth, "alice", "guess", t),
                Err(LoginError::Wrong)
            );
        }
        let locked = accounts.authenticate(&auth, "alice", "correct horse", 10);
        assert_eq!(
            locked,
            Err(LoginError::Locked {
                until: 4 + LOCK_SECS
            })
        );
        // After the lock, the right passphrase works and clears the count.
        assert!(accounts
            .authenticate(&auth, "alice", "correct horse", 5 + LOCK_SECS)
            .is_ok());
        // Another run of misses locks again, from the start.
        for t in 0..FREE_TRIES as u64 {
            let _ = accounts.authenticate(&auth, "alice", "guess", 10_000 + t);
        }
        let _ = accounts.authenticate(&auth, "alice", "guess", 20_000);
        assert_eq!(
            accounts.authenticate(&auth, "alice", "correct horse", 20_001),
            Err(LoginError::Locked {
                until: 20_000 + 2 * LOCK_SECS
            })
        );
    }

    #[test]
    fn a_person_changes_their_own_passphrase_only_with_the_old_one() {
        let (mut auth, mut accounts, alice) = setup();
        let bob = auth
            .add_principal("bob", PrincipalKind::Person, Label::Internal)
            .unwrap();
        assert!(accounts
            .set_passphrase(&auth, alice, alice, Some("nope"), "new one", [1; 16])
            .is_err());
        assert!(accounts
            .set_passphrase(&auth, alice, alice, None, "new one", [1; 16])
            .is_err());
        assert_eq!(
            accounts.set_passphrase(&auth, bob, alice, None, "take over", [1; 16]),
            Err(AuthError::SystemOnly)
        );
        assert_eq!(
            accounts.set_passphrase(&auth, SYSTEM, alice, None, "abc", [1; 16]),
            Err(AuthError::BadName),
            "too short"
        );
        accounts
            .set_passphrase(
                &auth,
                alice,
                alice,
                Some("correct horse"),
                "new one",
                [1; 16],
            )
            .unwrap();
        assert!(accounts.authenticate(&auth, "alice", "new one", 0).is_ok());
        assert!(accounts
            .authenticate(&auth, "alice", "correct horse", 0)
            .is_err());
        // The system is not a person: no passphrase logs in as it.
        assert!(accounts
            .set_passphrase(&auth, SYSTEM, SYSTEM, None, "root pass", [1; 16])
            .is_err());
    }

    #[test]
    fn what_is_kept_holds_no_passphrase() {
        let (_, accounts, _) = setup();
        let text = serde_json::to_string(&accounts).unwrap();
        assert!(!text.contains("correct horse"));
        assert!(text.contains("verifier"));
        let back: Accounts = serde_json::from_str(&text).unwrap();
        let auth = {
            let mut a = Authority::new();
            a.add_principal("alice", PrincipalKind::Person, Label::Secret)
                .unwrap();
            a
        };
        let mut back = back;
        assert!(back
            .authenticate(&auth, "alice", "correct horse", 0)
            .is_ok());
    }
}
