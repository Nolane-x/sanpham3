use peer_egress::{resolve_via_peer, serve_one, SystemResolver};
use peer_lan::{LanDiscovery, PeerBeacon};
use peer_session::{
    perform_client_handshake, perform_server_handshake, NonceReplayCache,
    PeerKey,
};
use std::env;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);
const DISCOVERY_POLL: Duration = Duration::from_millis(900);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const ADVERTISE_INTERVAL: Duration = Duration::from_millis(700);

fn main() {
    if let Err(error) = run() {
        eprintln!("peer-egress-cli: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().collect::<Vec<_>>();

    match args.get(1).map(String::as_str) {
        Some("server") => run_server(&args),
        Some("client") => run_client(&args),
        Some("advertise-server") => run_advertise_server(&args),
        Some("discover-client") => run_discover_client(&args),
        _ => Err(usage()),
    }
}

fn run_server(args: &[String]) -> Result<(), String> {
    if args.len() != 5 {
        return Err(usage());
    }

    let bind_addr = &args[2];
    let node_id = parse_node_id(&args[3])?;
    let key = parse_key(&args[4])?;

    let listener = TcpListener::bind(bind_addr)
        .map_err(|error| format!("bind {bind_addr}: {error}"))?;

    let local_addr = listener
        .local_addr()
        .map_err(|error| format!("read local address: {error}"))?;

    println!("peer egress listening on {local_addr}");
    println!("operation allow-list: RESOLVE_PUBLIC_HOST only");

    serve_authenticated_connection(&listener, node_id, &key)
}

fn run_client(args: &[String]) -> Result<(), String> {
    if args.len() != 6 {
        return Err(usage());
    }

    let peer_addr = &args[2];
    let node_id = parse_node_id(&args[3])?;
    let key = parse_key(&args[4])?;
    let hostname = &args[5];

    let mut stream = TcpStream::connect(peer_addr)
        .map_err(|error| format!("connect {peer_addr}: {error}"))?;

    let (peer_id, mut session) =
        perform_client_handshake(&mut stream, node_id, &key)
            .map_err(|error| format!("secure handshake: {error:?}"))?;

    println!("authenticated egress peer node_id={peer_id}");

    print_peer_resolution(&mut session, &mut stream, hostname)
}

fn run_advertise_server(args: &[String]) -> Result<(), String> {
    if args.len() != 7 {
        return Err(usage());
    }

    let bind_addr = &args[2];
    let discovery_port = parse_u16(&args[3], "discovery_port")?;
    let node_id = parse_node_id(&args[4])?;
    let key = parse_key(&args[5])?;
    let advertised_bps = parse_u32(&args[6], "advertised_bps")?;

    let listener = TcpListener::bind(bind_addr)
        .map_err(|error| format!("bind {bind_addr}: {error}"))?;

    let local_addr = listener
        .local_addr()
        .map_err(|error| format!("read local address: {error}"))?;

    let beacon = PeerBeacon {
        node_id,
        relay_allowed: true,
        internet_egress: true,
        listen_port: local_addr.port(),
        advertised_bps,
    };

    let stop = Arc::new(AtomicBool::new(false));
    let advertiser_stop = Arc::clone(&stop);

    let advertiser = thread::spawn(move || -> Result<(), String> {
        let discovery = LanDiscovery::bind(discovery_port)
            .map_err(|error| format!("bind discovery UDP {discovery_port}: {error}"))?;

        while !advertiser_stop.load(Ordering::Relaxed) {
            discovery
                .announce(beacon)
                .map_err(|error| format!("broadcast peer beacon: {error}"))?;
            thread::sleep(ADVERTISE_INTERVAL);
        }

        Ok(())
    });

    println!(
        "advertising egress node_id={node_id} tcp_port={} discovery_port={} advertised_bps={advertised_bps}",
        local_addr.port(),
        discovery_port,
    );
    println!("beacons are untrusted hints; relay requires secure handshake");

    let result = serve_authenticated_connection(&listener, node_id, &key);

    stop.store(true, Ordering::Relaxed);
    let advertise_result = advertiser
        .join()
        .map_err(|_| "advertiser thread panicked".to_owned())?;

    result?;
    advertise_result
}

fn run_discover_client(args: &[String]) -> Result<(), String> {
    if args.len() != 6 {
        return Err(usage());
    }

    let discovery_port = parse_u16(&args[2], "discovery_port")?;
    let node_id = parse_node_id(&args[3])?;
    let key = parse_key(&args[4])?;
    let hostname = &args[5];

    let discovery = LanDiscovery::bind(discovery_port)
        .map_err(|error| format!("bind discovery UDP {discovery_port}: {error}"))?;
    discovery
        .set_read_timeout(Some(DISCOVERY_POLL))
        .map_err(|error| format!("set discovery timeout: {error}"))?;

    let deadline = Instant::now() + DISCOVERY_TIMEOUT;
    let mut last_error = None::<String>;

    println!(
        "discovering consenting egress peers on UDP port {discovery_port} for up to {}s",
        DISCOVERY_TIMEOUT.as_secs()
    );

    while Instant::now() < deadline {
        let (beacon, source) = match discovery.receive() {
            Ok(value) => value,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                last_error = Some(format!("ignored malformed beacon: {error}"));
                continue;
            }
            Err(error) => {
                return Err(format!("receive peer beacon: {error}"));
            }
        };

        if beacon.node_id == node_id
            || !beacon.relay_allowed
            || !beacon.internet_egress
            || beacon.listen_port == 0
        {
            continue;
        }

        let peer_addr = SocketAddr::new(source.ip(), beacon.listen_port);
        println!(
            "candidate node_id={} addr={} advertised_bps={}",
            beacon.node_id, peer_addr, beacon.advertised_bps
        );

        let mut stream = match TcpStream::connect_timeout(&peer_addr, CONNECT_TIMEOUT) {
            Ok(stream) => stream,
            Err(error) => {
                last_error = Some(format!(
                    "candidate node_id={} connect failed: {error}",
                    beacon.node_id
                ));
                continue;
            }
        };

        let (authenticated_id, mut session) =
            match perform_client_handshake(&mut stream, node_id, &key) {
                Ok(value) => value,
                Err(error) => {
                    last_error = Some(format!(
                        "candidate node_id={} authentication failed: {error:?}",
                        beacon.node_id
                    ));
                    continue;
                }
            };

        if authenticated_id != beacon.node_id {
            last_error = Some(format!(
                "beacon/session identity mismatch: advertised={} authenticated={authenticated_id}",
                beacon.node_id
            ));
            continue;
        }

        println!(
            "authenticated discovered egress node_id={authenticated_id} addr={peer_addr}"
        );

        match print_peer_resolution(&mut session, &mut stream, hostname) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(format!(
                    "authenticated peer node_id={authenticated_id} could not serve request: {error}"
                ));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        "no authenticated egress peer discovered before timeout".to_owned()
    }))
}

fn serve_authenticated_connection(
    listener: &TcpListener,
    node_id: u64,
    key: &PeerKey,
) -> Result<(), String> {
    let mut replay_cache = NonceReplayCache::new(1024);

    loop {
        let (mut stream, peer_addr) = listener
            .accept()
            .map_err(|error| format!("accept peer: {error}"))?;

        let (peer_id, mut session) = match perform_server_handshake(
            &mut stream,
            node_id,
            key,
            &mut replay_cache,
        ) {
            Ok(value) => value,
            Err(error) => {
                eprintln!(
                    "rejected unauthenticated peer addr={peer_addr}: {error:?}"
                );
                continue;
            }
        };

        println!("authenticated peer node_id={peer_id} addr={peer_addr}");

        serve_one(&mut session, &mut stream, &SystemResolver)
            .map_err(|error| {
                format!("serve constrained egress request: {error:?}")
            })?;

        println!("request served");
        return Ok(());
    }
}

fn print_peer_resolution(
    session: &mut peer_session::SecureSession,
    stream: &mut TcpStream,
    hostname: &str,
) -> Result<(), String> {
    let addresses =
        resolve_via_peer(session, stream, 1, hostname)
            .map_err(|error| format!("peer resolution: {error:?}"))?;

    println!("resolved {hostname} through peer:");
    for address in addresses {
        println!("  {address}");
    }

    Ok(())
}

fn parse_node_id(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| "node_id must be an unsigned 64-bit integer".to_owned())
}

fn parse_u16(value: &str, name: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map_err(|_| format!("{name} must be an unsigned 16-bit integer"))
}

fn parse_u32(value: &str, name: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("{name} must be an unsigned 32-bit integer"))
}

fn parse_key(value: &str) -> Result<PeerKey, String> {
    if value.len() != 64 {
        return Err("PSK must be exactly 64 hexadecimal characters".to_owned());
    }

    let bytes = value.as_bytes();
    let mut key = [0_u8; 32];

    for index in 0..32 {
        let high = hex_nibble(bytes[index * 2])?;
        let low = hex_nibble(bytes[index * 2 + 1])?;
        key[index] = (high << 4) | low;
    }

    Ok(PeerKey::new(key))
}

fn hex_nibble(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err("PSK contains a non-hexadecimal character".to_owned()),
    }
}

fn usage() -> String {
    [
        "usage:",
        "  peer-egress-cli server <bind_addr> <node_id> <64_hex_psk>",
        "  peer-egress-cli client <peer_addr> <node_id> <64_hex_psk> <public_hostname>",
        "  peer-egress-cli advertise-server <bind_addr> <discovery_port> <node_id> <64_hex_psk> <advertised_bps>",
        "  peer-egress-cli discover-client <discovery_port> <node_id> <64_hex_psk> <public_hostname>",
        "",
        "examples:",
        "  peer-egress-cli advertise-server 0.0.0.0:0 45122 200 <psk> 1000000",
        "  peer-egress-cli discover-client 45122 100 <psk> example.com",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exact_hex_key() {
        assert!(parse_key(&"42".repeat(32)).is_ok());
        assert!(parse_key("42").is_err());
        assert!(parse_key(&"zz".repeat(32)).is_err());
    }

    #[test]
    fn parses_discovery_numbers() {
        assert_eq!(parse_u16("45122", "port").unwrap(), 45122);
        assert_eq!(parse_u32("82", "bps").unwrap(), 82);
        assert!(parse_u16("70000", "port").is_err());
    }
}
