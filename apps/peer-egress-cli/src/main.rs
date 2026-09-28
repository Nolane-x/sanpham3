use peer_egress::{resolve_via_peer, serve_one, SystemResolver};
use peer_session::{
    perform_client_handshake, perform_server_handshake, NonceReplayCache,
    PeerKey,
};
use std::env;
use std::net::{TcpListener, TcpStream};

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

    let (mut stream, peer_addr) = listener
        .accept()
        .map_err(|error| format!("accept peer: {error}"))?;

    let mut replay_cache = NonceReplayCache::new(1024);
    let (peer_id, mut session) = perform_server_handshake(
        &mut stream,
        node_id,
        &key,
        &mut replay_cache,
    )
    .map_err(|error| format!("secure handshake: {error:?}"))?;

    println!("authenticated peer node_id={peer_id} addr={peer_addr}");

    serve_one(&mut session, &mut stream, &SystemResolver)
        .map_err(|error| format!("serve constrained egress request: {error:?}"))?;

    println!("request served");
    Ok(())
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

    let addresses =
        resolve_via_peer(&mut session, &mut stream, 1, hostname)
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
        "",
        "example:",
        "  peer-egress-cli server 0.0.0.0:45123 200 <psk>",
        "  peer-egress-cli client 192.168.1.10:45123 100 <psk> example.com",
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
}
