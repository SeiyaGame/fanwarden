//! `fanwarden healthcheck`: asks the running service if it answers (Docker HEALTHCHECK).

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

use anyhow::{Context, bail};

/// Address to probe for a listen address: a wildcard (`0.0.0.0`, `[::]`) becomes loopback.
pub fn probe_address(listen: &str) -> anyhow::Result<SocketAddr> {
    let mut addr: SocketAddr = listen.parse().with_context(|| format!("invalid listen address '{listen}'"))?;
    match addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => addr.set_ip(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        IpAddr::V6(ip) if ip.is_unspecified() => addr.set_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        _ => {}
    }
    Ok(addr)
}

/// Succeeds when the web server answers `GET /api/auth/status` with 200.
pub fn check(addr: SocketAddr, timeout: Duration) -> anyhow::Result<()> {
    let mut stream = TcpStream::connect_timeout(&addr, timeout).with_context(|| format!("cannot connect to {addr}"))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    // One write: a server that closes after reading the first chunk would otherwise reset us.
    let request = format!("GET /api/auth/status HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    let mut head = [0u8; 32];
    let read = stream.read(&mut head)?;
    let status_line = String::from_utf8_lossy(&head[..read]);
    match status_line.split_whitespace().nth(1) {
        Some("200") => Ok(()),
        Some(code) => bail!("{addr} answered HTTP {code}"),
        None => bail!("{addr} sent no HTTP status"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn wildcard_becomes_loopback() {
        assert_eq!(probe_address("0.0.0.0:9495").unwrap(), "127.0.0.1:9495".parse().unwrap());
        assert_eq!(probe_address("[::]:8080").unwrap(), "[::1]:8080".parse().unwrap());
        assert_eq!(probe_address("192.168.1.5:9495").unwrap(), "192.168.1.5:9495".parse().unwrap());
        assert!(probe_address("localhost").is_err());
    }

    fn serve_once(response: &'static str) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            // Read the whole request head before answering, like a real server.
            let mut request = Vec::new();
            let mut buf = [0u8; 512];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            socket.write_all(response.as_bytes()).unwrap();
        });
        addr
    }

    #[test]
    fn healthy_and_unhealthy() {
        let timeout = Duration::from_secs(2);
        assert!(check(serve_once("HTTP/1.1 200 OK\r\n\r\n{}"), timeout).is_ok());
        let err = check(serve_once("HTTP/1.1 500 Internal Server Error\r\n\r\n"), timeout).unwrap_err();
        assert!(err.to_string().contains("500"));
        // Nothing listening.
        let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        assert!(check(free, timeout).is_err());
    }
}
