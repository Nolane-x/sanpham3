use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::Path;

pub fn read_resolvers(path: impl AsRef<Path>) -> io::Result<Vec<IpAddr>> {
    let content = fs::read_to_string(path)?;
    Ok(parse_resolvers(&content))
}

pub fn parse_resolvers(input: &str) -> Vec<IpAddr> {
    let mut resolvers = Vec::new();

    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut fields = line.split_whitespace();
        if fields.next() != Some("nameserver") {
            continue;
        }

        let Some(value) = fields.next() else {
            continue;
        };

        if let Ok(address) = value.parse::<IpAddr>() {
            if !resolvers.contains(&address) {
                resolvers.push(address);
            }
        }
    }

    resolvers
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn parses_unique_v4_and_v6_nameservers() {
        let input = "# generated\n\
nameserver 1.1.1.1\n\
nameserver 2001:4860:4860::8888\n\
nameserver 1.1.1.1\n";

        assert_eq!(
            parse_resolvers(input),
            vec![
                IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
                IpAddr::V6(Ipv6Addr::new(
                    0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888,
                )),
            ]
        );
    }
}
