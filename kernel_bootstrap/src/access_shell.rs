//! The shell's words for who may do what (FS-005).
//!
//! `whoami`, `people`, `adduser`, `passwd`, `setpass`, `login`, `logout`,
//! `share`, `unshare`, `grants`, `label`, `clearance`, `roles`, `role` and
//! `audit`: each is a line in, lines out, against the filesystem and its
//! guard -- so the whole of it runs under `cargo test` on a RAM disk.
//!
//! The machine's first person (the one the console signs in as at boot)
//! administers it: they add people, set passphrases and clearances. Their
//! own documents are theirs like anyone's; administering is not a way into
//! someone else's.

extern crate alloc;

use crate::bare_metal_storage::BareMetalFilesystem;
use crate::guard::{label_of, Actor};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use authority::{Label, PrincipalId, PrincipalKind, Rights, Scope, Terms, SYSTEM};

/// The words this module answers.
pub const WORDS: [&str; 15] = [
    "whoami",
    "people",
    "adduser",
    "passwd",
    "setpass",
    "login",
    "logout",
    "share",
    "unshare",
    "grants",
    "label",
    "clearance",
    "roles",
    "role",
    "audit",
];

/// How many audit lines `audit` shows.
pub const AUDIT_LINES: usize = 12;

/// Whether `word` is one of these.
pub fn handles(word: &str) -> bool {
    WORDS.contains(&word)
}

/// The help text for these words.
pub fn help() -> Vec<String> {
    [
        "Access (FS-005):",
        "  whoami | people | adduser <name> [clearance] | clearance <name> <level>",
        "  passwd <new> | passwd <old> <new> | setpass <name> <new> (the admin)",
        "  login <name> <passphrase> | logout",
        "  share <doc> <person> <read,history,write,tag,delete,share> [for=<secs>] [since=<secs ago>]",
        "  unshare <doc> <grant> | grants <doc> | label <doc> [public|internal|confidential|secret]",
        "  roles | role add <name> <rights> <#tag|doc:<name>|all> | role join|leave <name> <person>",
        "  role remove <name> | audit [doc]",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn who(fs: &BareMetalFilesystem) -> Option<PrincipalId> {
    fs.guard.principal()
}

/// The machine's administrator: the person the console signs in as at
/// boot.
fn is_admin(fs: &BareMetalFilesystem, p: PrincipalId) -> bool {
    let name = fs.guard.authority.principal(p).map(|p| p.name.as_str());
    name.is_some() && name == fs.guard.console.as_deref()
}

fn err(msg: impl Into<String>) -> Vec<String> {
    alloc::vec![msg.into()]
}

/// Run one line. `salt` seeds a passphrase verifier if the line sets one;
/// `now` is the filesystem's clock.
pub fn run(fs: &mut BareMetalFilesystem, line: &str, now: u64, salt: [u8; 16]) -> Vec<String> {
    fs.set_clock(now);
    let mut parts = line.split_whitespace();
    let Some(word) = parts.next() else {
        return Vec::new();
    };
    let args: Vec<&str> = parts.collect();
    // Signing in is the one thing a closed session may do.
    if word == "login" {
        return login(fs, &args, now);
    }
    let Some(me) = who(fs) else {
        return err("not signed in: login <name> <passphrase>");
    };
    match word {
        "whoami" => {
            let p = fs.guard.authority.principal(me);
            match p {
                Some(p) => alloc::vec![format!(
                    "{} (clearance {}){}",
                    p.name,
                    p.clearance,
                    if is_admin(fs, me) {
                        ", administers this machine"
                    } else {
                        ""
                    }
                )],
                None => err("nobody"),
            }
        }
        "people" => fs
            .guard
            .authority
            .principals()
            .iter()
            .filter(|p| p.id != SYSTEM)
            .map(|p| {
                format!(
                    "{:<16} {:<13} {}{}",
                    p.name,
                    p.clearance.name(),
                    if fs.guard.accounts.has_passphrase(p.id) {
                        "passphrase set"
                    } else {
                        "no passphrase"
                    },
                    if is_admin(fs, p.id) { ", admin" } else { "" }
                )
            })
            .collect(),
        "adduser" => {
            if !is_admin(fs, me) {
                return err("adduser: only the administrator adds people");
            }
            let Some(name) = args.first() else {
                return err("usage: adduser <name> [clearance]");
            };
            let clearance = match args.get(1) {
                Some(l) => match Label::parse(l) {
                    Some(l) => l,
                    None => return err("clearance: public, internal, confidential or secret"),
                },
                None => Label::Internal,
            };
            match fs
                .guard
                .authority
                .add_principal(name, PrincipalKind::Person, clearance)
            {
                Ok(_) => {
                    fs.guard.mark_dirty();
                    alloc::vec![
                        format!("added {name}, cleared for {clearance}"),
                        format!("give them a passphrase: setpass {name} <passphrase>"),
                    ]
                }
                Err(e) => err(format!("adduser: {e}")),
            }
        }
        "clearance" => {
            if !is_admin(fs, me) {
                return err("clearance: only the administrator sets clearances");
            }
            let (Some(name), Some(level)) = (args.first(), args.get(1)) else {
                return err("usage: clearance <name> <level>");
            };
            let Some(label) = Label::parse(level) else {
                return err("clearance: public, internal, confidential or secret");
            };
            let Some(p) = fs.guard.authority.principal_named(name).map(|p| p.id) else {
                return err(format!("no one called {name}"));
            };
            match fs.guard.authority.set_clearance(SYSTEM, p, label) {
                Ok(()) => {
                    fs.guard.mark_dirty();
                    alloc::vec![format!("{name} is cleared for {label}")]
                }
                Err(e) => err(format!("clearance: {e}")),
            }
        }
        "passwd" => {
            let (old, new) = match args.as_slice() {
                [new] => (None, *new),
                [old, new] => (Some(*old), *new),
                _ => return err("usage: passwd <new>, or passwd <old> <new>"),
            };
            let g = &mut fs.guard;
            match g
                .accounts
                .set_passphrase(&g.authority, me, me, old, new, salt)
            {
                Ok(()) => {
                    g.mark_dirty();
                    err("passphrase set")
                }
                Err(authority::AuthError::BadName) => err(format!(
                    "passwd: at least {} characters",
                    authority::credential::MIN_PASSPHRASE
                )),
                Err(_) => err("passwd: give the current passphrase first: passwd <old> <new>"),
            }
        }
        "setpass" => {
            if !is_admin(fs, me) {
                return err("setpass: only the administrator sets others' passphrases");
            }
            let (Some(name), Some(new)) = (args.first(), args.get(1)) else {
                return err("usage: setpass <name> <new>");
            };
            let Some(p) = fs.guard.authority.principal_named(name).map(|p| p.id) else {
                return err(format!("no one called {name}"));
            };
            let g = &mut fs.guard;
            match g
                .accounts
                .set_passphrase(&g.authority, SYSTEM, p, None, new, salt)
            {
                Ok(()) => {
                    g.mark_dirty();
                    g.sessions.close_all(p);
                    alloc::vec![format!("{name}'s passphrase set")]
                }
                Err(e) => err(format!("setpass: {e}")),
            }
        }
        "logout" => {
            if let Actor::Session(h) = fs.guard.actor() {
                fs.guard.sessions.close(h);
            }
            err("signed out: nothing opens until login <name> <passphrase>")
        }
        "share" => share(fs, &args, now),
        "unshare" => {
            let (Some(doc), Some(id)) = (args.first(), args.get(1)) else {
                return err("usage: unshare <doc> <grant>");
            };
            let Ok(id) = id.trim_start_matches('#').parse::<u32>() else {
                return err("unshare: a grant is a number, from `grants <doc>`");
            };
            match fs.revoke(doc, authority::GrantId(id)) {
                Ok(n) => alloc::vec![format!(
                    "revoked grant {id}{}",
                    if n > 1 {
                        format!(" and {} derived from it", n - 1)
                    } else {
                        String::new()
                    }
                )],
                Err(e) => err(format!("unshare: {}", why(&e))),
            }
        }
        "grants" => {
            let Some(doc) = args.first() else {
                return err("usage: grants <doc>");
            };
            match fs.grants_on(doc) {
                Ok(grants) if grants.is_empty() => err(format!("{doc} is shared with no one")),
                Ok(grants) => grants
                    .iter()
                    .map(|g| {
                        let mut line = format!(
                            "#{:<3} {:<12} {}",
                            g.id.0,
                            fs.guard.name_of(g.holder.0),
                            g.rights
                        );
                        if let Some(t) = g.expires_at {
                            line.push_str(&format!("  until {t}"));
                        }
                        if let Some(t) = g.history_since {
                            line.push_str(&format!("  history since {t}"));
                        }
                        if let Some(p) = g.parent {
                            line.push_str(&format!("  from #{}", p.0));
                        }
                        if g.revoked {
                            line.push_str("  revoked");
                        } else if !fs.guard.authority.grant_live(g.id, now) {
                            line.push_str("  expired");
                        }
                        line
                    })
                    .collect(),
                Err(e) => err(format!("grants: {}", why(&e))),
            }
        }
        "label" => {
            let Some(doc) = args.first() else {
                return err("usage: label <doc> [level]");
            };
            match args.get(1) {
                None => match fs.document(doc) {
                    Ok(Some(e)) => alloc::vec![format!(
                        "{doc}: {}, owned by {}",
                        label_of(&e),
                        fs.guard.name_of(e.owner)
                    )],
                    _ => err(format!("label: no {doc} here")),
                },
                Some(level) => {
                    let Some(label) = Label::parse(level) else {
                        return err("label: public, internal, confidential or secret");
                    };
                    match fs.relabel(doc, label) {
                        Ok(()) => alloc::vec![format!("{doc} is now {label}")],
                        Err(e) => err(format!("label: {}", why(&e))),
                    }
                }
            }
        }
        "roles" => {
            let roles: Vec<String> = fs
                .guard
                .authority
                .roles()
                .iter()
                .filter(|r| r.owner == me || r.members.contains(&me))
                .map(|r| {
                    let scope = match &r.scope {
                        Scope::Tag(t) => format!("#{t}"),
                        Scope::Doc(d) => format!("doc {}", d.0),
                        Scope::Everything => "all".to_string(),
                    };
                    let members: Vec<String> =
                        r.members.iter().map(|m| fs.guard.name_of(m.0)).collect();
                    format!(
                        "{:<12} {:<18} {:<10} {} (by {})",
                        r.name,
                        r.rights.to_string(),
                        scope,
                        if members.is_empty() {
                            "no one".to_string()
                        } else {
                            members.join(", ")
                        },
                        fs.guard.name_of(r.owner.0)
                    )
                })
                .collect();
            if roles.is_empty() {
                err("no roles")
            } else {
                roles
            }
        }
        "role" => role(fs, me, &args),
        "audit" => audit(fs, me, args.first().copied()),
        _ => err(format!("{word}: not an access word")),
    }
}

fn why(e: &services_storage::TransactionError) -> String {
    match e {
        services_storage::TransactionError::Denied(why) => why.clone(),
        services_storage::TransactionError::ObjectNotFound(_) => "no such document here".into(),
        other => format!("{other}"),
    }
}

fn login(fs: &mut BareMetalFilesystem, args: &[&str], now: u64) -> Vec<String> {
    let (Some(name), Some(pass)) = (args.first(), args.get(1)) else {
        return err("usage: login <name> <passphrase>");
    };
    let g = &mut fs.guard;
    match g
        .sessions
        .login(&mut g.accounts, &g.authority, name, pass, now)
    {
        Ok(handle) => {
            if let Actor::Session(old) = g.actor() {
                g.sessions.close(old);
            }
            g.act_as(Actor::Session(handle));
            g.mark_dirty();
            alloc::vec![format!("signed in as {name}")]
        }
        Err(e) => {
            g.mark_dirty();
            err(format!("login: {e}"))
        }
    }
}

fn share(fs: &mut BareMetalFilesystem, args: &[&str], now: u64) -> Vec<String> {
    let (Some(doc), Some(to), Some(rights)) = (args.first(), args.get(1), args.get(2)) else {
        return err("usage: share <doc> <person> <rights> [for=<secs>] [since=<secs ago>]");
    };
    let Some(rights) = Rights::parse(rights) else {
        return err("share: rights are read, history, write, tag, delete, share, own");
    };
    let mut terms = Terms::of(rights);
    for extra in &args[3..] {
        let parsed = |v: &str| v.parse::<u64>().ok();
        if let Some(secs) = extra.strip_prefix("for=").and_then(parsed) {
            terms = terms.until(now + secs);
        } else if let Some(secs) = extra.strip_prefix("since=").and_then(parsed) {
            terms = terms.history_since(now.saturating_sub(secs));
        } else {
            return err(format!("share: what is {extra}?"));
        }
    }
    match fs.share(doc, to, terms) {
        Ok(id) => alloc::vec![format!("shared {doc} with {to}: {rights} (grant {})", id.0)],
        Err(e) => err(format!("share: {}", why(&e))),
    }
}

fn role(fs: &mut BareMetalFilesystem, me: PrincipalId, args: &[&str]) -> Vec<String> {
    let result = match args {
        ["add", name, rights, scope] => {
            let Some(rights) = Rights::parse(rights) else {
                return err("role: rights are read, history, write, tag, delete, share");
            };
            let scope = if let Some(tag) = scope.strip_prefix('#') {
                Scope::Tag(tag.to_string())
            } else if *scope == "all" {
                Scope::Everything
            } else if let Some(doc) = scope.strip_prefix("doc:") {
                match fs.document(doc) {
                    Ok(Some(e)) => Scope::Doc(authority::DocId(e.doc_id)),
                    _ => return err(format!("role: no {doc} here")),
                }
            } else {
                return err("role: the scope is #tag, doc:<name> or all");
            };
            fs.guard
                .authority
                .add_role(me, name, rights, scope)
                .map(|_| format!("role {name}: {rights}, over your documents in it"))
        }
        ["join", name, person] | ["leave", name, person] => {
            let Some(p) = fs.guard.authority.principal_named(person).map(|p| p.id) else {
                return err(format!("no one called {person}"));
            };
            if args[0] == "join" {
                fs.guard
                    .authority
                    .join(me, name, p)
                    .map(|_| format!("{person} is in {name}"))
            } else {
                fs.guard
                    .authority
                    .leave(me, name, p)
                    .map(|_| format!("{person} left {name}"))
            }
        }
        ["remove", name] => fs
            .guard
            .authority
            .remove_role(me, name)
            .map(|_| format!("role {name} removed")),
        _ => return err("usage: role add <name> <rights> <#tag|doc:<name>|all> | role join|leave <name> <person> | role remove <name>"),
    };
    match result {
        Ok(msg) => {
            fs.guard.mark_dirty();
            alloc::vec![msg]
        }
        Err(e) => err(format!("role: {e}")),
    }
}

/// The decisions a person may read: those on documents they own (all of
/// them, for the administrator), newest last.
fn audit(fs: &mut BareMetalFilesystem, me: PrincipalId, doc: Option<&str>) -> Vec<String> {
    let admin = is_admin(fs, me);
    let only = match doc {
        Some(name) => match fs.document(name) {
            Ok(Some(e)) if e.owner == me.0 || admin => Some(authority::DocId(e.doc_id)),
            Ok(Some(_)) => return err(format!("audit: {name} is not yours")),
            _ => return err(format!("audit: no {name} here")),
        },
        None => None,
    };
    // Every document by id, with its name and owner, to name them.
    let dir: Vec<(u64, String, u32)> = fs.document_index().unwrap_or_default();
    let name_of_doc = |id: u64| {
        dir.iter()
            .find(|(d, _, _)| *d == id)
            .map(|(_, n, _)| n.clone())
            .unwrap_or_else(|| format!("doc {id}"))
    };
    let owner_of = |id: u64| dir.iter().find(|(d, _, _)| *d == id).map(|(_, _, o)| *o);
    let events: Vec<String> = fs
        .guard
        .authority
        .audit()
        .filter(|e| only.is_none_or(|d| e.doc == d))
        .filter(|e| admin || owner_of(e.doc.0) == Some(me.0))
        .rev()
        .take(AUDIT_LINES)
        .map(|e| {
            format!(
                "{:>10}  {:<10} {:<8} {:<14} {}  {}",
                e.at,
                fs.guard.name_of(e.principal.0),
                e.right.to_string(),
                name_of_doc(e.doc.0),
                if e.allowed { "allowed" } else { "REFUSED" },
                e.reason
            )
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if events.is_empty() {
        err("nothing in the audit log for you")
    } else {
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::{boot, Guard};

    fn sh(fs: &mut BareMetalFilesystem, line: &str) -> String {
        run(fs, line, 1000, [3; 16]).join("\n")
    }

    /// The whole story at the shell: the administrator adds Alice and
    /// Armando; Alice writes a plan with history and shares it for reading;
    /// Armando reads it and is refused its history; Alice sees the refusal
    /// in her audit log, and revokes.
    #[test]
    fn the_shell_tells_the_whole_story() {
        let mut fs = BareMetalFilesystem::new().unwrap();
        fs.guard = Guard::for_tests();
        boot(&mut fs, "admin", 1).unwrap();
        assert!(sh(&mut fs, "whoami").contains("admin (clearance secret), administers"));
        assert!(sh(&mut fs, "adduser alice secret").contains("added alice"));
        assert!(sh(&mut fs, "adduser armando").contains("cleared for internal"));
        sh(&mut fs, "setpass alice alice-pass");
        sh(&mut fs, "setpass armando armando-pass");
        assert!(sh(&mut fs, "people").contains("armando          internal      passphrase set"));
        assert!(sh(&mut fs, "login alice wrong").contains("wrong name or passphrase"));
        assert_eq!(sh(&mut fs, "login alice alice-pass"), "signed in as alice");
        assert!(sh(&mut fs, "adduser mallory").contains("only the administrator"));
        for (t, text) in [(100, "one"), (200, "two"), (300, "three")] {
            fs.write_named("plan", text.as_bytes(), t, None).unwrap();
        }
        assert!(sh(&mut fs, "share plan armando read").contains("grant 1"));
        assert!(sh(&mut fs, "share plan armando fly").contains("rights are"));
        assert!(sh(&mut fs, "grants plan").starts_with("#1   armando      read"));
        assert_eq!(
            sh(&mut fs, "login armando armando-pass"),
            "signed in as armando"
        );
        assert_eq!(fs.read_file_by_name("plan").unwrap(), b"three");
        assert!(fs.list_versions("plan").is_err());
        assert!(sh(&mut fs, "grants plan").contains("holds only read"));
        assert!(sh(&mut fs, "label plan secret").contains("label:"));
        assert!(sh(&mut fs, "audit plan").contains("not yours"));
        sh(&mut fs, "login alice alice-pass");
        let log = sh(&mut fs, "audit plan");
        assert!(
            log.contains("armando") && log.contains("REFUSED  holds only read"),
            "{log}"
        );
        assert!(sh(&mut fs, "unshare plan 1").contains("revoked grant 1"));
        assert!(sh(&mut fs, "grants plan").contains("revoked"));
        assert_eq!(
            sh(&mut fs, "label plan confidential"),
            "plan is now confidential"
        );
        // A role over Alice's #work documents.
        fs.update_entry("plan", 400, |e| e.tags.push("work".into()))
            .unwrap();
        assert!(sh(&mut fs, "role add workers read #work").contains("role workers"));
        assert!(sh(&mut fs, "role join workers armando").contains("armando is in workers"));
        assert!(sh(&mut fs, "roles").contains("workers"));
        // Confidential is above Armando's clearance: the role does not reach.
        sh(&mut fs, "login armando armando-pass");
        assert!(fs.read_file_by_name("plan").is_err());
        assert!(sh(&mut fs, "passwd armando-pass new-pass").contains("passphrase set"));
        assert!(sh(&mut fs, "logout").contains("signed out"));
        assert!(sh(&mut fs, "whoami").contains("not signed in"));
        assert!(fs.read_file_by_name("plan").is_err());
        assert_eq!(
            sh(&mut fs, "login armando new-pass"),
            "signed in as armando"
        );
        assert!(sh(&mut fs, "clearance armando secret").contains("only the administrator"));
        assert!(handles("share") && !handles("ls"));
    }
}
