//! A DNS client's two halves: the query for a name's IPv4 address, and
//! the answer read back out (NET-031).
//!
//! No allocation and no socket: the caller sends the query over UDP to
//! the resolver DHCP named and hands the reply here. Answers are read the
//! way resolvers must, not the way a friendly server happens to write
//! them: names may be compressed (a pointer back into the message), a
//! CNAME may come before the address, and a reply must carry the query's
//! id and be a response, or it is someone else's.

use crate::Ipv4;

/// The port resolvers listen on.
pub const PORT: u16 = 53;
/// Longest name we ask for (RFC 1035: 255 octets on the wire).
pub const MAX_NAME: usize = 253;

const TYPE_A: u16 = 1;
const TYPE_CNAME: u16 = 5;
const CLASS_IN: u16 = 1;
const HEADER_LEN: usize = 12;
/// Most compression pointers followed in one name: a loop of pointers
/// would otherwise never end.
const MAX_POINTERS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsError {
    /// The name is empty, too long, or has an empty or over-long label.
    BadName,
    /// The buffer cannot hold the query.
    BufferTooSmall,
    /// Not an answer to our query (wrong id, not a response, truncated).
    NotOurs,
    /// The resolver said the name does not exist.
    NoSuchName,
    /// The resolver failed (any other error code).
    ServerFailure(u8),
    /// A well-formed answer with no IPv4 address in it.
    NoAddress,
    /// The message is malformed.
    Malformed,
}

/// Write a query for `name`'s A record with `id` into `out`; returns its
/// length. Recursion is asked for: the resolver does the walking.
pub fn build_query(id: u16, name: &str, out: &mut [u8]) -> Result<usize, DnsError> {
    let name = name.strip_suffix('.').unwrap_or(name);
    if name.is_empty()
        || name.len() > MAX_NAME
        || name.split('.').any(|l| l.is_empty() || l.len() > 63)
    {
        return Err(DnsError::BadName);
    }
    let need = HEADER_LEN + name.len() + 2 + 4;
    if out.len() < need {
        return Err(DnsError::BufferTooSmall);
    }
    out[..HEADER_LEN].copy_from_slice(&[
        (id >> 8) as u8,
        id as u8,
        0x01, // RD: recursion desired
        0x00,
        0,
        1, // one question
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
    let mut at = HEADER_LEN;
    for label in name.split('.') {
        out[at] = label.len() as u8;
        out[at + 1..at + 1 + label.len()].copy_from_slice(label.as_bytes());
        at += 1 + label.len();
    }
    out[at] = 0;
    at += 1;
    out[at..at + 4].copy_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);
    Ok(at + 4)
}

fn u16_at(msg: &[u8], at: usize) -> Result<u16, DnsError> {
    let bytes = msg.get(at..at + 2).ok_or(DnsError::Malformed)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

/// Where the name starting at `at` ends in the message (the byte after
/// it, following no pointers), checking every pointer it would follow.
fn skip_name(msg: &[u8], mut at: usize) -> Result<usize, DnsError> {
    loop {
        let len = *msg.get(at).ok_or(DnsError::Malformed)? as usize;
        match len {
            0 => return Ok(at + 1),
            l if l & 0xC0 == 0xC0 => {
                // A pointer ends the name in place; check it points inside.
                let target = u16_at(msg, at)? as usize & 0x3FFF;
                if target >= msg.len() {
                    return Err(DnsError::Malformed);
                }
                return Ok(at + 2);
            }
            l if l & 0xC0 != 0 => return Err(DnsError::Malformed),
            l => at += 1 + l,
        }
    }
}

/// Whether the name at `at` (following pointers) is `name`, ignoring case.
fn name_is(msg: &[u8], mut at: usize, name: &str) -> Result<bool, DnsError> {
    let mut labels = name.strip_suffix('.').unwrap_or(name).split('.');
    let mut pointers = 0;
    loop {
        let len = *msg.get(at).ok_or(DnsError::Malformed)? as usize;
        if len == 0 {
            return Ok(labels.next().is_none());
        }
        if len & 0xC0 == 0xC0 {
            pointers += 1;
            if pointers > MAX_POINTERS {
                return Err(DnsError::Malformed);
            }
            at = u16_at(msg, at)? as usize & 0x3FFF;
            continue;
        }
        let label = msg.get(at + 1..at + 1 + len).ok_or(DnsError::Malformed)?;
        match labels.next() {
            Some(want) if want.as_bytes().eq_ignore_ascii_case(label) => {}
            _ => return Ok(false),
        }
        at += 1 + len;
    }
}

/// The IPv4 address in a reply to query `id` for `name`. A CNAME chain is
/// followed within the message: the address is the A record whose owner
/// is `name` or a name `name` is an alias of.
pub fn parse_answer(id: u16, name: &str, msg: &[u8]) -> Result<Ipv4, DnsError> {
    if msg.len() < HEADER_LEN || u16_at(msg, 0)? != id {
        return Err(DnsError::NotOurs);
    }
    let flags = u16_at(msg, 2)?;
    let is_response = flags & 0x8000 != 0;
    let truncated = flags & 0x0200 != 0;
    if !is_response || truncated {
        return Err(DnsError::NotOurs);
    }
    match (flags & 0x000F) as u8 {
        0 => {}
        3 => return Err(DnsError::NoSuchName),
        code => return Err(DnsError::ServerFailure(code)),
    }
    let questions = u16_at(msg, 4)?;
    let answers = u16_at(msg, 6)?;
    let mut at = HEADER_LEN;
    for _ in 0..questions {
        at = skip_name(msg, at)? + 4;
    }
    // The name whose address we want: `name`, then each alias it points to.
    // Kept as the offset of the alias target in the message, since that is
    // how a later record's owner is compared.
    let mut wanted: Option<usize> = None;
    let mut found_cname = true;
    // Records may come in any order: go round until a pass adds no alias.
    let answers_start = at;
    let mut rounds = 0;
    while found_cname && rounds < MAX_POINTERS {
        found_cname = false;
        rounds += 1;
        at = answers_start;
        for _ in 0..answers {
            let owner = at;
            at = skip_name(msg, at)?;
            let rtype = u16_at(msg, at)?;
            let class = u16_at(msg, at + 2)?;
            let rdlen = u16_at(msg, at + 8)? as usize;
            let rdata = at + 10;
            if msg.len() < rdata + rdlen {
                return Err(DnsError::Malformed);
            }
            at = rdata + rdlen;
            if class != CLASS_IN {
                continue;
            }
            let owner_matches = match wanted {
                None => name_is(msg, owner, name)?,
                Some(target) => same_name(msg, owner, target)?,
            };
            if !owner_matches {
                continue;
            }
            match rtype {
                TYPE_A if rdlen == 4 => {
                    return Ok([msg[rdata], msg[rdata + 1], msg[rdata + 2], msg[rdata + 3]]);
                }
                TYPE_CNAME if wanted != Some(rdata) => {
                    wanted = Some(rdata);
                    found_cname = true;
                }
                _ => {}
            }
        }
    }
    Err(DnsError::NoAddress)
}

/// Whether the names at `a` and `b` in the message are the same.
fn same_name(msg: &[u8], mut a: usize, mut b: usize) -> Result<bool, DnsError> {
    let mut pointers = 0;
    loop {
        // Follow pointers on either side to a label.
        loop {
            let la = *msg.get(a).ok_or(DnsError::Malformed)?;
            if la & 0xC0 != 0xC0 {
                break;
            }
            a = u16_at(msg, a)? as usize & 0x3FFF;
            pointers += 1;
            if pointers > MAX_POINTERS {
                return Err(DnsError::Malformed);
            }
        }
        loop {
            let lb = *msg.get(b).ok_or(DnsError::Malformed)?;
            if lb & 0xC0 != 0xC0 {
                break;
            }
            b = u16_at(msg, b)? as usize & 0x3FFF;
            pointers += 1;
            if pointers > MAX_POINTERS {
                return Err(DnsError::Malformed);
            }
        }
        let la = msg[a] as usize;
        let lb = msg[b] as usize;
        if la != lb {
            return Ok(false);
        }
        if la == 0 {
            return Ok(true);
        }
        let ta = msg.get(a + 1..a + 1 + la).ok_or(DnsError::Malformed)?;
        let tb = msg.get(b + 1..b + 1 + lb).ok_or(DnsError::Malformed)?;
        if !ta.eq_ignore_ascii_case(tb) {
            return Ok(false);
        }
        a += 1 + la;
        b += 1 + lb;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply to `query` (id and question copied) with these answer
    /// records appended, and `rcode`.
    fn reply(query: &[u8], rcode: u8, answers: &[&[u8]]) -> ([u8; 512], usize) {
        let mut msg = [0u8; 512];
        msg[..query.len()].copy_from_slice(query);
        msg[2] = 0x81; // response, RD
        msg[3] = 0x80 | rcode; // RA
        msg[7] = answers.len() as u8;
        let mut at = query.len();
        for a in answers {
            msg[at..at + a.len()].copy_from_slice(a);
            at += a.len();
        }
        (msg, at)
    }

    /// A record whose owner is a pointer to `owner_at`.
    fn record(owner_at: u8, rtype: u8, rdata: &[u8]) -> [u8; 64] {
        let mut r = [0u8; 64];
        r[..12].copy_from_slice(&[
            0xC0,
            owner_at,
            0,
            rtype,
            0,
            1,
            0,
            0,
            0x0E,
            0x10,
            0,
            rdata.len() as u8,
        ]);
        r[12..12 + rdata.len()].copy_from_slice(rdata);
        r
    }

    #[test]
    fn the_query_asks_for_an_a_record_with_recursion() {
        let mut out = [0u8; 64];
        let n = build_query(0xBEEF, "example.com", &mut out).unwrap();
        assert_eq!(&out[..4], &[0xBE, 0xEF, 0x01, 0x00]);
        assert_eq!(&out[4..6], &[0, 1]);
        assert_eq!(&out[12..n], b"\x07example\x03com\x00\x00\x01\x00\x01");
        assert_eq!(build_query(1, "", &mut out), Err(DnsError::BadName));
        assert_eq!(build_query(1, "a..b", &mut out), Err(DnsError::BadName));
        assert_eq!(
            build_query(1, "example.com", &mut out[..20]),
            Err(DnsError::BufferTooSmall)
        );
        let long = [b'a'; 64];
        let long = core::str::from_utf8(&long).unwrap();
        assert_eq!(build_query(1, long, &mut out), Err(DnsError::BadName));
    }

    #[test]
    fn an_answer_with_a_compressed_owner_gives_the_address() {
        let mut q = [0u8; 64];
        let n = build_query(7, "Example.COM", &mut q).unwrap();
        let a = record(12, 1, &[93, 184, 216, 34]);
        let (msg, len) = reply(&q[..n], 0, &[&a[..16]]);
        assert_eq!(
            parse_answer(7, "example.com", &msg[..len]),
            Ok([93, 184, 216, 34])
        );
        // Someone else's reply.
        assert_eq!(
            parse_answer(8, "example.com", &msg[..len]),
            Err(DnsError::NotOurs)
        );
        // Our own query echoed back is not an answer.
        assert_eq!(
            parse_answer(7, "example.com", &q[..n]),
            Err(DnsError::NotOurs)
        );
    }

    #[test]
    fn a_cname_is_followed_to_its_address_in_any_order() {
        let mut q = [0u8; 64];
        let n = build_query(9, "www.example.com", &mut q).unwrap();
        // The alias target, spelled out, after the question.
        let cname_rdata = b"\x04edge\x03net\x00";
        let cname = record(12, 5, cname_rdata);
        // The CNAME's rdata starts at: question end + 12 bytes of record head.
        let target_at = (n + 12) as u8;
        let a = record(target_at, 1, &[10, 0, 2, 2]);
        // Address first, alias second: still found.
        let a_len = 16;
        let c_len = 12 + cname_rdata.len();
        let mut msg = [0u8; 512];
        msg[..n].copy_from_slice(&q[..n]);
        msg[2] = 0x81;
        msg[3] = 0x80;
        msg[7] = 2;
        msg[n..n + c_len].copy_from_slice(&cname[..c_len]);
        msg[n + c_len..n + c_len + a_len].copy_from_slice(&a[..a_len]);
        let len = n + c_len + a_len;
        assert_eq!(
            parse_answer(9, "www.example.com", &msg[..len]),
            Ok([10, 0, 2, 2])
        );
    }

    #[test]
    fn errors_and_nonsense_are_said_as_such() {
        let mut q = [0u8; 64];
        let n = build_query(3, "nowhere.test", &mut q).unwrap();
        let (msg, len) = reply(&q[..n], 3, &[]);
        assert_eq!(
            parse_answer(3, "nowhere.test", &msg[..len]),
            Err(DnsError::NoSuchName)
        );
        let (msg, len) = reply(&q[..n], 2, &[]);
        assert_eq!(
            parse_answer(3, "nowhere.test", &msg[..len]),
            Err(DnsError::ServerFailure(2))
        );
        let (msg, len) = reply(&q[..n], 0, &[]);
        assert_eq!(
            parse_answer(3, "nowhere.test", &msg[..len]),
            Err(DnsError::NoAddress)
        );
        // An answer for another name is not ours to use.
        let a = record(12, 1, &[1, 2, 3, 4]);
        let (msg, len) = reply(&q[..n], 0, &[&a[..16]]);
        assert_eq!(
            parse_answer(3, "elsewhere.test", &msg[..len]),
            Err(DnsError::NoAddress)
        );
        // A pointer loop, and a record running off the end.
        let mut looped = msg;
        looped[n] = 0xC0;
        looped[n + 1] = n as u8;
        assert!(parse_answer(3, "nowhere.test", &looped[..len]).is_err());
        assert_eq!(
            parse_answer(3, "nowhere.test", &msg[..len - 2]),
            Err(DnsError::Malformed)
        );
    }
}
