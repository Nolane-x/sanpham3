use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteSnapshot {
    pub ipv4_default_interfaces: HashSet<String>,
    pub ipv6_default_interfaces: HashSet<String>,
}

impl RouteSnapshot {
    pub fn has_ipv4_default(&self, interface: &str) -> bool {
        self.ipv4_default_interfaces.contains(interface)
    }

    pub fn has_ipv6_default(&self, interface: &str) -> bool {
        self.ipv6_default_interfaces.contains(interface)
    }
}

pub fn read_route_snapshot(
    ipv4_path: impl AsRef<Path>,
    ipv6_path: impl AsRef<Path>,
) -> io::Result<RouteSnapshot> {
    let ipv4 = fs::read_to_string(ipv4_path)?;
    let ipv6 = fs::read_to_string(ipv6_path)?;

    Ok(RouteSnapshot {
        ipv4_default_interfaces: parse_ipv4_default_interfaces(&ipv4),
        ipv6_default_interfaces: parse_ipv6_default_interfaces(&ipv6),
    })
}

pub fn parse_ipv4_default_interfaces(input: &str) -> HashSet<String> {
    input
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 8 {
                return None;
            }

            let interface = fields[0];
            let destination = fields[1];
            let flags = u16::from_str_radix(fields[3], 16).ok()?;

            if destination == "00000000" && flags & 0x1 != 0 {
                Some(interface.to_owned())
            } else {
                None
            }
        })
        .collect()
}

pub fn parse_ipv6_default_interfaces(input: &str) -> HashSet<String> {
    input
        .lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 10 {
                return None;
            }

            let destination = fields[0];
            let prefix_len = fields[1];
            let interface = fields[9];

            if destination.chars().all(|ch| ch == '0') && prefix_len == "00" {
                Some(interface.to_owned())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv4_default_routes() {
        let input = "Iface\tDestination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT\n\
wlan0\t00000000 0102A8C0 0003 0 0 600 00000000 0 0 0\n\
eth0\t0002A8C0 00000000 0001 0 0 100 00FFFFFF 0 0 0\n";

        let interfaces = parse_ipv4_default_interfaces(input);
        assert!(interfaces.contains("wlan0"));
        assert!(!interfaces.contains("eth0"));
    }

    #[test]
    fn parses_ipv6_default_routes() {
        let zeros = "00000000000000000000000000000000";
        let input = format!(
            "{zeros} 00 {zeros} 00 {zeros} 00000000 00000000 00000001 00000000 wlan0\n\
20010db8000000000000000000000000 40 {zeros} 00 {zeros} 00000000 00000000 00000001 00000000 eth0\n"
        );

        let interfaces = parse_ipv6_default_interfaces(&input);
        assert!(interfaces.contains("wlan0"));
        assert!(!interfaces.contains("eth0"));
    }
}
