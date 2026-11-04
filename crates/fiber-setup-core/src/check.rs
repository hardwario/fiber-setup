//! After the first start: is the FIBER on this network? It announces `<name>.local` over mDNS
//! (avahi is in Raspberry Pi OS), so this works without knowing its IP address.

use serde::Serialize;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub ip: String,
    /// SSH answers on port 22.
    pub ssh: bool,
}

/// Looks up `<hostname>.local` and tries SSH. Blocking: a name that is not there takes a few
/// seconds to give up on.
pub fn find(hostname: &str) -> Option<Found> {
    let addrs: Vec<SocketAddr> = (format!("{hostname}.local"), 22).to_socket_addrs().ok()?.collect();
    // IPv4 first: it is what people type, and link-local IPv6 needs an interface name.
    let addr = addrs.iter().find(|a| a.is_ipv4()).or(addrs.first()).copied()?;
    let ssh = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok();
    Some(Found { ip: addr.ip().to_string(), ssh })
}
