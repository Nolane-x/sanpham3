use peer_session::{
    perform_client_handshake, perform_server_handshake, NonceReplayCache,
    PeerKey, SecureSession,
};
use std::env;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

const KIND_PAIR_CHALLENGE: u8 = 0x50;
const KIND_PAIR_ACK: u8 = 0x51;
const CHALLENGE_MAGIC: [u8; 4] = *b"G8P0";
const CHALLENGE_NONCE_LEN: usize = 32;
const CHALLENGE_LEN: usize = CHALLENGE_MAGIC.len() + CHALLENGE_NONCE_LEN;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PairEvidence {
    local_node: u64,
    peer_node: u64,
    challenge: [u8; CHALLENGE_LEN],
}

fn main() {
    if let Err(error) = run() {
        eprintln!("g8-pair-cli: {error}");
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
    let node_id = parse_u64(&args[3], "node_id")?;
    let key = parse_key(&args[4])?;

    let listener = TcpListener::bind(bind_addr)
        .map_err(|error| format!("bind {bind_addr}: {error}"))?;
    let local_addr = listener
        .local_addr()
        .map_err(|error| format!("read local address: {error}"))?;

    println!("G8_PAIR_LISTEN addr={local_addr} node={node_id}");

    let (mut stream, peer_addr) = listener
        .accept()
        .map_err(|error| format!("accept peer: {error}"))?;

    let mut replay_cache = NonceReplayCache::new(1024);
    let (peer_node, mut session) = perform_server_handshake(
        &mut stream,
        node_id,
        &key,
        &mut replay_cache,
    )
    .map_err(|error| format!("secure handshake: {error:?}"))?;

    let evidence = serve_pair_challenge(
        &mut session,
        &mut stream,
        node_id,
        peer_node,
    )?;

    println!(
        "G8_PAIR_PASS role=server local_node={} peer_node={} peer_addr={} challenge={}",
        evidence.local_node,
        evidence.peer_node,
        peer_addr,
        hex(&evidence.challenge),
    );

    Ok(())
}

fn run_client(args: &[String]) -> Result<(), String> {
    if args.len() != 5 {
        return Err(usage());
    }

    let peer_addr = &args[2];
    let node_id = parse_u64(&args[3], "node_id")?;
    let key = parse_key(&args[4])?;

    let mut stream = TcpStream::connect(peer_addr)
        .map_err(|error| format!("connect {peer_addr}: {error}"))?;

    let (peer_node, mut session) =
        perform_client_handshake(&mut stream, node_id, &key)
            .map_err(|error| format!("secure handshake: {error:?}"))?;

    let evidence = send_pair_challenge(
        &mut session,
        &mut stream,
        node_id,
        peer_node,
    )?;

    println!(
        "G8_PAIR_PASS role=client local_node={} peer_node={} peer_addr={} challenge={}",
        evidence.local_node,
        evidence.peer_node,
        peer_addr,
        hex(&evidence.challenge),
    );

    Ok(())
}

fn send_pair_challenge<S: Read + Write>(
    session: &mut SecureSession,
    stream: &mut S,
    local_node: u64,
    peer_node: u64,
) -> Result<PairEvidence, String> {
    let challenge = new_challenge()?;

    session
        .send(stream, KIND_PAIR_CHALLENGE, &challenge)
        .map_err(|error| format!("send encrypted challenge: {error:?}"))?;

    let (kind, payload) = session
        .receive(stream)
        .map_err(|error| format!("receive encrypted ack: {error:?}"))?;

    if kind != KIND_PAIR_ACK {
        return Err(format!(
            "unexpected encrypted response kind {kind}, expected {KIND_PAIR_ACK}"
        ));
    }

    validate_challenge(&payload)?;
    if payload.as_slice() != challenge {
        return Err("pair ACK challenge does not match request".to_owned());
    }

    Ok(PairEvidence {
        local_node,
        peer_node,
        challenge,
    })
}

fn serve_pair_challenge<S: Read + Write>(
    session: &mut SecureSession,
    stream: &mut S,
    local_node: u64,
    peer_node: u64,
) -> Result<PairEvidence, String> {
    let (kind, payload) = session
        .receive(stream)
        .map_err(|error| format!("receive encrypted challenge: {error:?}"))?;

    if kind != KIND_PAIR_CHALLENGE {
        return Err(format!(
            "unexpected encrypted request kind {kind}, expected {KIND_PAIR_CHALLENGE}"
        ));
    }

    let challenge = validate_challenge(&payload)?;

    session
        .send(stream, KIND_PAIR_ACK, &challenge)
        .map_err(|error| format!("send encrypted ack: {error:?}"))?;

    Ok(PairEvidence {
        local_node,
        peer_node,
        challenge,
    })
}

fn new_challenge() -> Result<[u8; CHALLENGE_LEN], String> {
    let mut challenge = [0_u8; CHALLENGE_LEN];
    challenge[..CHALLENGE_MAGIC.len()].copy_from_slice(&CHALLENGE_MAGIC);

    getrandom::fill(&mut challenge[CHALLENGE_MAGIC.len()..])
        .map_err(|error| format!("generate challenge entropy: {error}"))?;

    Ok(challenge)
}

fn validate_challenge(
    payload: &[u8],
) -> Result<[u8; CHALLENGE_LEN], String> {
    if payload.len() != CHALLENGE_LEN {
        return Err(format!(
            "invalid pair challenge length {}, expected {CHALLENGE_LEN}",
            payload.len()
        ));
    }
    if payload[..CHALLENGE_MAGIC.len()] != CHALLENGE_MAGIC {
        return Err("invalid pair challenge magic".to_owned());
    }

    payload
        .try_into()
        .map_err(|_| "invalid pair challenge".to_owned())
}

fn parse_u64(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an unsigned 64-bit integer"))
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

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }

    out
}

fn usage() -> String {
    [
        "usage:",
        "  g8-pair-cli server <bind_addr> <node_id> <64_hex_psk>",
        "  g8-pair-cli client <peer_addr> <node_id> <64_hex_psk>",
        "",
        "A PASS means:",
        "  - the peer-session handshake authenticated the remote project node ID;",
        "  - an encrypted challenge crossed client -> server;",
        "  - the identical encrypted ACK crossed server -> client.",
        "",
        "This is pair-protocol evidence, not proof of a specific RF transport.",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn challenge_roundtrip_validates() {
        let challenge = new_challenge().unwrap();
        assert_eq!(validate_challenge(&challenge).unwrap(), challenge);

        let mut bad = challenge;
        bad[0] ^= 0x01;
        assert!(validate_challenge(&bad).is_err());
    }

    #[test]
    fn full_loopback_pair_court_passes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let key = PeerKey::new([0x55; 32]);
            let mut replay = NonceReplayCache::new(16);

            let (peer_node, mut session) = perform_server_handshake(
                &mut stream,
                200,
                &key,
                &mut replay,
            )
            .unwrap();

            serve_pair_challenge(
                &mut session,
                &mut stream,
                200,
                peer_node,
            )
            .unwrap()
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let key = PeerKey::new([0x55; 32]);
        let (peer_node, mut session) =
            perform_client_handshake(&mut stream, 100, &key).unwrap();

        let client = send_pair_challenge(
            &mut session,
            &mut stream,
            100,
            peer_node,
        )
        .unwrap();

        let server = server.join().unwrap();

        assert_eq!(client.peer_node, 200);
        assert_eq!(server.peer_node, 100);
        assert_eq!(client.challenge, server.challenge);
    }

    #[test]
    fn hex_encoding_is_stable() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
    }
}
