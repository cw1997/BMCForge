use anyhow::{Result, bail, ensure};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr},
};

pub fn canonical_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    match host.parse::<IpAddr>() {
        Ok(ip) => ip.to_string(),
        Err(_) => host,
    }
}

pub fn canonical_endpoint(host: &str, port: u16) -> String {
    let host = canonical_host(host);
    if host.parse::<IpAddr>().is_ok_and(|ip| ip.is_ipv6()) {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

#[derive(Default, Debug)]
pub struct Preview {
    pub hosts: Vec<String>,
    pub duplicates: usize,
    pub errors: Vec<String>,
}

// Bound memory before expanding hostile/accidental inputs such as /0.
pub const MAX_TARGETS: usize = 1_048_576;

pub fn expand_line(line: &str, discovery: bool) -> Result<Vec<String>> {
    let line = line.trim();
    if line.contains('/') {
        let net: ipnet::Ipv4Net = line.parse()?;
        let count = u64::from(u32::from(net.broadcast())) - u64::from(u32::from(net.network())) + 1;
        ensure!(
            count <= MAX_TARGETS as u64,
            "range exceeds {MAX_TARGETS} addresses; split it into smaller networks"
        );
        return Ok(net.hosts().map(|ip| ip.to_string()).collect());
    }
    if let Some((start, end)) = line.split_once('-')
        && let (Ok(start), Ok(end)) = (
            start.trim().parse::<Ipv4Addr>(),
            end.trim().parse::<Ipv4Addr>(),
        )
    {
        let (start, end) = (u32::from(start), u32::from(end));
        ensure!(end >= start, "range is reversed");
        ensure!(
            u64::from(end) - u64::from(start) < MAX_TARGETS as u64,
            "range too large"
        );
        return Ok((start..=end)
            .map(|v| Ipv4Addr::from(v).to_string())
            .collect());
    }
    if discovery {
        let ip: Ipv4Addr = line.parse()?;
        return Ok(vec![ip.to_string()]);
    }
    if let Ok(ip) = line.parse::<IpAddr>() {
        return Ok(vec![ip.to_string()]);
    }
    if line.is_empty()
        || line.len() > 253
        || !line.trim_end_matches('.').split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        bail!("invalid IP or hostname: {line}");
    }
    Ok(vec![line.trim_end_matches('.').to_ascii_lowercase()])
}

pub fn preview(
    input: &str,
    existing: impl IntoIterator<Item = String>,
    _port: u16,
    discovery: bool,
) -> Preview {
    let mut out = Preview::default();
    let mut seen: HashSet<String> = existing.into_iter().map(|h| canonical_host(&h)).collect();
    for (index, line) in input.lines().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        match expand_line(line, discovery) {
            Ok(hosts) => {
                if out.hosts.len().saturating_add(hosts.len()) > MAX_TARGETS {
                    out.errors.push(format!(
                        "line {}: total expansion exceeds {MAX_TARGETS}",
                        index + 1
                    ));
                    continue;
                }
                for host in hosts {
                    if seen.insert(canonical_host(&host)) {
                        out.hosts.push(host);
                    } else {
                        out.duplicates += 1;
                    }
                }
            }
            Err(err) => out.errors.push(format!("line {}: {err}", index + 1)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_canonicalization_ignores_case_trailing_dot_and_ip_spelling() {
        assert_eq!(canonical_host(" BMC.Example. "), "bmc.example");
        assert_eq!(canonical_host("2001:0db8:0:0::1"), "2001:db8::1");
        assert_eq!(canonical_endpoint("2001:0db8::1", 623), "[2001:db8::1]:623");
    }

    #[test]
    fn ranges_dedup_and_errors() {
        let p = preview(
            "192.0.2.0/30\n192.0.2.1-192.0.2.3\nBMC.EXAMPLE.\nbmc.example\ninvalid!",
            [],
            623,
            false,
        );
        assert_eq!(
            p.hosts,
            ["192.0.2.1", "192.0.2.2", "192.0.2.3", "bmc.example"]
        );
        assert_eq!(p.duplicates, 3);
        assert_eq!(p.errors.len(), 1);
        assert!(expand_line("0.0.0.0/0", true).is_err());
        assert!(expand_line("host.local", true).is_err());
        assert_eq!(expand_line("192.0.2.1/32", true).unwrap().len(), 1);
        assert_eq!(expand_line("192.0.2.0/31", true).unwrap().len(), 2);
    }

    #[test]
    fn preview_skips_registered_hosts_regardless_of_port() {
        let p = preview(
            "BMC.EXAMPLE.\nnew.example",
            ["bmc.example".into()],
            624,
            false,
        );
        assert_eq!(p.hosts, ["new.example"]);
        assert_eq!(p.duplicates, 1);
    }
}
