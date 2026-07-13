use std::collections::HashSet;
use std::hash::BuildHasher;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpListener, ToSocketAddrs as _};

pub const MAX_PORT_ATTEMPTS: u16 = 256;

/// Resolves a forwarding bind host once so repeated port probes do not repeat DNS work.
///
/// # Errors
///
/// Returns an error when the host cannot be resolved to at least one IP address.
pub fn resolve_bind_ips(host: &str) -> io::Result<Vec<IpAddr>> {
    let addresses: Vec<_> = (host, 0)
        .to_socket_addrs()?
        .map(|address| address.ip())
        .collect();
    if addresses.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "监听地址没有解析到可用 IP",
        ));
    }
    Ok(addresses)
}

#[must_use]
pub fn is_port_available_on(port: u16, bind_ips: &[IpAddr]) -> bool {
    bind_ips
        .iter()
        .any(|ip| TcpListener::bind(SocketAddr::new(*ip, port)).is_ok())
}

/// Finds the first available port in the bounded candidate window starting at `start_port`.
///
/// # Errors
///
/// Returns an error when the bind host cannot be resolved.
pub fn find_nearest_available_port<S: BuildHasher>(
    start_port: u16,
    host: &str,
    reserved_ports: &HashSet<u16, S>,
) -> io::Result<Option<u16>> {
    let bind_ips = resolve_bind_ips(host)?;
    Ok(candidate_ports(start_port)
        .find(|port| !reserved_ports.contains(port) && is_port_available_on(*port, &bind_ips)))
}

fn candidate_ports(start_port: u16) -> impl Iterator<Item = u16> {
    let end = start_port.saturating_add(MAX_PORT_ATTEMPTS - 1);
    start_port..=end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_reserved_ports() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test listener must bind");
        let start = listener
            .local_addr()
            .expect("test listener has a local address")
            .port();
        drop(listener);
        let reserved = HashSet::from([start]);

        let selected = find_nearest_available_port(start, "127.0.0.1", &reserved)
            .expect("loopback address must resolve");

        assert!(selected.is_some_and(|port| port > start));
    }

    #[test]
    fn limits_search_to_nearby_higher_ports() {
        let ports: Vec<_> = candidate_ports(30_000).collect();

        assert_eq!(ports.len(), usize::from(MAX_PORT_ATTEMPTS));
        assert_eq!(ports.first(), Some(&30_000));
        assert_eq!(ports.last(), Some(&30_255));
        assert_eq!(candidate_ports(u16::MAX - 1).count(), 2);
    }
}
