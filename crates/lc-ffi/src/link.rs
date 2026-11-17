//! The companion over a network link (TCP): the address the user types, and how long to wait
//! before trying again when the link drops. One rule for both apps.

/// A companion's network address: MeshCore's Wi-Fi companion firmware, or a USB radio relayed by
/// `lichen bridge`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TcpAddress {
    pub host: String,
    pub port: u16,
}

/// The port MeshCore's Wi-Fi companion firmware listens on, and `lichen bridge`'s default.
pub const TCP_DEFAULT_PORT: u16 = 5000;

/// Reads what the user typed: `host`, `host:port`, `[v6]` or `[v6]:port`; a bare IPv6 address
/// (several colons) takes the default port. `None` when it is empty, holds a space or a
/// character no host name has, or the port is not a number from 1 to 65535.
#[uniffi::export]
pub fn tcp_address(text: String) -> Option<TcpAddress> {
    let t = text.trim();
    let (host, port) = if let Some(rest) = t.strip_prefix('[') {
        let (h, after) = rest.split_once(']')?;
        let port = match after {
            "" => None,
            p => Some(p.strip_prefix(':')?),
        };
        (h, port)
    } else if t.matches(':').count() > 1 {
        (t, None)
    } else {
        match t.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (t, None),
        }
    };
    let host_ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '%'));
    if !host_ok {
        return None;
    }
    let port = match port {
        None => TCP_DEFAULT_PORT,
        Some(p) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => {
            p.parse::<u16>().ok().filter(|&p| p != 0)?
        }
        Some(_) => return None,
    };
    Some(TcpAddress {
        host: host.to_owned(),
        port,
    })
}

/// How long to wait before retry `attempt` (1 for the first) to reach a companion whose
/// network link dropped: 1, 2, 4, 8, 16, then every 30 s. A radio that restarts, or a bridge
/// started again, is found within 30 s, and one that stays away is not hammered.
#[uniffi::export]
pub fn link_retry_delay_ms(attempt: u32) -> u64 {
    let doublings = attempt.saturating_sub(1).min(5);
    (1000u64 << doublings).min(30_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(host: &str, port: u16) -> Option<TcpAddress> {
        Some(TcpAddress {
            host: host.into(),
            port,
        })
    }

    #[test]
    fn addresses_as_typed() {
        assert_eq!(tcp_address("10.0.2.2:5000".into()), at("10.0.2.2", 5000));
        assert_eq!(
            tcp_address(" 192.168.1.20 ".into()),
            at("192.168.1.20", 5000)
        );
        assert_eq!(
            tcp_address("radio.local:4403".into()),
            at("radio.local", 4403)
        );
        assert_eq!(
            tcp_address("[fe80::1%en0]:5001".into()),
            at("fe80::1%en0", 5001)
        );
        assert_eq!(tcp_address("[::1]".into()), at("::1", 5000));
        assert_eq!(tcp_address("fd00::7".into()), at("fd00::7", 5000));
        assert_eq!(
            tcp_address("localhost:65535".into()),
            at("localhost", 65535)
        );
    }

    #[test]
    fn what_is_not_an_address() {
        for bad in [
            "",
            "   ",
            ":5000",
            "host:",
            "host:0",
            "host:65536",
            "host:+50",
            "host:50a",
            "two words:5000",
            "[::1",
            "[::1]5000",
            "[]:5000",
            "a/b:5000",
            "http://radio:5000",
        ] {
            assert_eq!(tcp_address(bad.into()), None, "{bad:?}");
        }
    }

    #[test]
    fn retries_back_off_to_thirty_seconds() {
        let delays: Vec<u64> = (0..=8).map(link_retry_delay_ms).collect();
        assert_eq!(
            delays,
            [1000, 1000, 2000, 4000, 8000, 16_000, 30_000, 30_000, 30_000]
        );
        assert_eq!(link_retry_delay_ms(u32::MAX), 30_000);
    }
}
