//! After the first start: is the FIBER on this network, and how far is its installation? It
//! announces `<name>.local` over mDNS (avahi is in Raspberry Pi OS), so this works without
//! knowing its IP address.
//!
//! The installation's progress is in two Avahi records on the FIBER: the first-start script's
//! (`_fiber-setup._tcp`: what the script does) and the FIBER platform's (`_fiber._tcp`: the step
//! of the apply). Their TXT records are asked for with a plain DNS query sent straight to the
//! FIBER's address and port 5353, which an mDNS responder answers to the sender ("legacy
//! unicast", RFC 6762, section 6.7). Nothing on this computer listens on port 5353 for it, so no
//! firewall asks about it, and the computer's own mDNS service is left alone.

use serde::Serialize;
use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

/// The first-start script's record: what it does (`state`), the release, the last error.
pub const SETUP_SERVICE: &str = "_fiber-setup._tcp.local";
/// The FIBER platform's record: the apply's `state`, `step`, `component` and `release`.
pub const PLATFORM_SERVICE: &str = "_fiber._tcp.local";

/// How long to wait for the FIBER's answers. A FIBER without a record does not answer at all.
const ASK_TIMEOUT: Duration = Duration::from_millis(1500);

const PTR: u16 = 12;
const TXT: u16 = 16;

#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub ip: String,
    /// SSH answers on port 22.
    pub ssh: bool,
    /// The first-start script's TXT records, while it runs; asked for only with `progress`.
    pub setup: Option<BTreeMap<String, String>>,
    /// The FIBER platform's TXT records, once its first apply has begun.
    pub platform: Option<BTreeMap<String, String>>,
}

/// Looks up `<hostname>.local`, tries SSH, and with `progress` asks the FIBER for its two
/// records. Blocking: a name that is not there takes a few seconds to give up on.
pub fn find(hostname: &str, progress: bool) -> Option<Found> {
    let addrs: Vec<SocketAddr> = (format!("{hostname}.local"), 22).to_socket_addrs().ok()?.collect();
    // IPv4 first: it is what people type, and link-local IPv6 needs an interface name.
    let addr = addrs.iter().find(|a| a.is_ipv4()).or(addrs.first()).copied()?;
    let ssh = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok();
    let (setup, platform) = if progress {
        let mut records = ask(addr, &[SETUP_SERVICE, PLATFORM_SERVICE]);
        (records.remove(SETUP_SERVICE), records.remove(PLATFORM_SERVICE))
    } else {
        (None, None)
    };
    Some(Found { ip: addr.ip().to_string(), ssh, setup, platform })
}

/// Asks the mDNS responder at `addr` for the TXT records of each service's instance: by service.
/// What it does not answer is left out.
fn ask(addr: SocketAddr, services: &[&str]) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut to = addr;
    to.set_port(5353);
    let bind: SocketAddr = if to.is_ipv4() { ([0, 0, 0, 0], 0).into() } else { ([0u16; 8], 0).into() };
    let Ok(socket) = UdpSocket::bind(bind) else { return BTreeMap::new() };
    // Each service has a query id of its own; the TXT query for an instance, if one is needed,
    // has the same id with the high bit set.
    for (i, service) in services.iter().enumerate() {
        let _ = socket.send_to(&query(0x4600 + i as u16, service, PTR), to);
    }
    let mut found = BTreeMap::new();
    let mut asked_txt: Vec<String> = Vec::new();
    let deadline = Instant::now() + ASK_TIMEOUT;
    let mut buf = [0u8; 9000];
    while found.len() < services.len() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || socket.set_read_timeout(Some(left)).is_err() {
            break;
        }
        let Ok((n, from)) = socket.recv_from(&mut buf) else { break };
        if from.ip() != to.ip() {
            continue;
        }
        let Some(answer) = parse(&buf[..n]) else { continue };
        let Some(service) = services.get(usize::from(answer.id & 0x7fff).wrapping_sub(0x4600)) else { continue };
        if found.contains_key(*service) {
            continue;
        }
        // The instance the service points at, and its TXT records if they came along (Avahi
        // sends them with the answer).
        let instance = answer
            .records
            .iter()
            .find(|r| r.kind == PTR && r.name.eq_ignore_ascii_case(service))
            .and_then(|r| r.target.clone())
            .or_else(|| (answer.id & 0x8000 != 0).then(|| answer.question.clone()));
        let Some(instance) = instance else { continue };
        if let Some(txt) = answer.records.iter().find(|r| r.kind == TXT && r.name.eq_ignore_ascii_case(&instance)) {
            found.insert((*service).to_string(), txt.txt.clone());
        } else if !asked_txt.contains(&instance) {
            let _ = socket.send_to(&query(answer.id | 0x8000, &instance, TXT), to);
            asked_txt.push(instance);
        }
    }
    found
}

/// A DNS query for one name and type, class IN, without recursion.
fn query(id: u16, name: &str, kind: u16) -> Vec<u8> {
    let mut q = Vec::with_capacity(64);
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in labels(name) {
        let bytes = label.as_bytes();
        q.push(bytes.len().min(63) as u8);
        q.extend_from_slice(&bytes[..bytes.len().min(63)]);
    }
    q.push(0);
    q.extend_from_slice(&kind.to_be_bytes());
    q.extend_from_slice(&1u16.to_be_bytes());
    q
}

/// The labels of a name. An instance name may have dots of its own, written `\.` (RFC 6763,
/// section 4.3), and so does the name [`read_name`] gives back.
fn labels(name: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = name.trim_end_matches('.').chars();
    while let Some(c) = chars.next() {
        let c = match c {
            '.' => {
                out.push(String::new());
                continue;
            }
            '\\' => match chars.next() {
                Some(escaped) => escaped,
                None => break,
            },
            c => c,
        };
        if let Some(label) = out.last_mut() {
            label.push(c);
        }
    }
    out
}

/// What of an answer is needed here.
struct Answer {
    id: u16,
    /// The first question's name.
    question: String,
    /// Every record of the answer, authority and additional sections.
    records: Vec<Record>,
}

struct Record {
    name: String,
    kind: u16,
    /// A PTR record's name.
    target: Option<String>,
    /// A TXT record's `key=value` strings.
    txt: BTreeMap<String, String>,
}

/// Reads a DNS response; `None` for anything that is not a well-formed one.
fn parse(msg: &[u8]) -> Option<Answer> {
    let u16_at = |at: usize| Some(u16::from_be_bytes([*msg.get(at)?, *msg.get(at + 1)?]));
    let id = u16_at(0)?;
    if u16_at(2)? & 0x8000 == 0 {
        return None; // a query, not a response
    }
    let questions = u16_at(4)?;
    let count = usize::from(u16_at(6)?) + usize::from(u16_at(8)?) + usize::from(u16_at(10)?);
    let mut at = 12;
    let mut question = String::new();
    for i in 0..questions {
        let (name, next) = read_name(msg, at)?;
        if i == 0 {
            question = name;
        }
        at = next + 4;
    }
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let (name, next) = read_name(msg, at)?;
        let kind = u16_at(next)?;
        let len = usize::from(u16_at(next + 8)?);
        let data = next + 10;
        let rdata = msg.get(data..data + len)?;
        let target = if kind == PTR { Some(read_name(msg, data)?.0) } else { None };
        let txt = if kind == TXT { read_txt(rdata) } else { BTreeMap::new() };
        records.push(Record { name, kind, target, txt });
        at = data + len;
    }
    Some(Answer { id, question, records })
}

/// A name at `at`, with the compression of RFC 1035 (section 4.1.4), and where it ends. A dot
/// inside a label is written `\.`, a backslash `\\`.
fn read_name(msg: &[u8], mut at: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    let mut end = None;
    // Each pointer has to go back, so a loop of pointers cannot go on for ever.
    let mut limit = at;
    loop {
        let len = *msg.get(at)?;
        match len {
            0 => break,
            l if l & 0xc0 == 0xc0 => {
                let to = usize::from(u16::from_be_bytes([l & 0x3f, *msg.get(at + 1)?]));
                if to >= limit {
                    return None;
                }
                end.get_or_insert(at + 2);
                limit = to;
                at = to;
            }
            l if l & 0xc0 == 0 => {
                let label = msg.get(at + 1..at + 1 + usize::from(l))?;
                if !name.is_empty() {
                    name.push('.');
                }
                for c in String::from_utf8_lossy(label).chars() {
                    if c == '.' || c == '\\' {
                        name.push('\\');
                    }
                    name.push(c);
                }
                at += 1 + usize::from(l);
            }
            _ => return None,
        }
    }
    Some((name, end.unwrap_or(at + 1)))
}

/// TXT strings as `key=value`; a key without `=` has an empty value.
fn read_txt(mut data: &[u8]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    while let Some((&len, rest)) = data.split_first() {
        let Some(item) = rest.get(..usize::from(len)) else { break };
        let item = String::from_utf8_lossy(item);
        let (key, value) = item.split_once('=').unwrap_or((&item, ""));
        if !key.is_empty() {
            out.insert(key.to_string(), value.to_string());
        }
        data = &rest[usize::from(len)..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query() {
        assert_eq!(
            query(0x4600, "_fiber._tcp.local", PTR),
            b"\x46\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x06_fiber\x04_tcp\x05local\x00\x00\x0c\x00\x01"
        );
        assert_eq!(labels(r"FIBER fiber\.1._fiber._tcp.local"), ["FIBER fiber.1", "_fiber", "_tcp", "local"]);
    }

    #[test]
    fn names_with_pointers_and_dots() {
        // "a.b" at 0, then the label "x\.y" and a pointer to it.
        let msg = b"\x01a\x01b\x00\x03x.y\xc0\x00";
        assert_eq!(read_name(msg, 0), Some(("a.b".into(), 5)));
        assert_eq!(read_name(msg, 5), Some((r"x\.y.a.b".into(), 11)));
        // A pointer to itself, or forward, is refused.
        assert_eq!(read_name(b"\xc0\x00", 0), None);
        assert_eq!(read_name(b"\x01a\xc0\x05\x00\x00", 0), None);
        assert_eq!(read_name(b"\x05ab", 0), None);
    }

    #[test]
    fn txt_strings() {
        let txt = read_txt(b"\x0dstate=running\x08step=2/5\x05flag=\x04bare\x00");
        assert_eq!(txt.get("state").map(String::as_str), Some("running"));
        assert_eq!(txt.get("step").map(String::as_str), Some("2/5"));
        assert_eq!(txt.get("flag").map(String::as_str), Some(""));
        assert_eq!(txt.get("bare").map(String::as_str), Some(""));
        assert_eq!(read_txt(b"\x09short").len(), 0);
    }
}
