use connectivity_core::{Bundle, BundlePriority, DtnQueue};
use peer_egress::{
    decode_response, encode_response, ResolveResponse, ResolveStatus,
};
use peer_session::{
    perform_client_handshake, perform_server_handshake, NonceReplayCache,
    PeerKey,
};
use recovery_runtime::{
    dispatch_next_resolve, enqueue_resolve, load_queue, offer_next_bundle,
    receive_one_bundle, save_queue, DispatchOutcome,
};
use std::env;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

fn main() {
    if let Err(error) = run() {
        eprintln!("recovery-lab-cli: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().collect::<Vec<_>>();

    match args.get(1).map(String::as_str) {
        Some("enqueue") => enqueue(&args),
        Some("custody-receive") => custody_receive(&args),
        Some("custody-send") => custody_send(&args),
        Some("dispatch") => dispatch(&args),
        Some("show-result") => show_result(&args),
        _ => Err(usage()),
    }
}

fn enqueue(args: &[String]) -> Result<(), String> {
    if args.len() != 8 {
        return Err(usage());
    }

    let spool = PathBuf::from(&args[2]);
    let bundle_id = parse_u64(&args[3], "bundle_id")?;
    let request_id = parse_u32(&args[4], "request_id")?;
    let hostname = &args[5];
    let priority = parse_priority(&args[6])?;
    let ttl_secs = parse_u64(&args[7], "ttl_secs")?;

    let mono = Instant::now();
    let wall = SystemTime::now();
    let mut queue = open_queue(&spool, mono, wall)?;

    enqueue_resolve(
        &mut queue,
        bundle_id,
        request_id,
        hostname,
        priority,
        Duration::from_secs(ttl_secs),
        mono,
    )
    .map_err(|error| format!("enqueue request: {error:?}"))?;

    let count = save_queue(&spool, &queue, mono, wall)
        .map_err(|error| format!("persist queue: {error:?}"))?;

    println!(
        "queued bundle_id={bundle_id} request_id={request_id} hostname={hostname} priority={priority:?} ttl_secs={ttl_secs} persisted={count}"
    );
    Ok(())
}

fn custody_receive(args: &[String]) -> Result<(), String> {
    if args.len() != 6 {
        return Err(usage());
    }

    let bind_addr = &args[2];
    let node_id = parse_u64(&args[3], "node_id")?;
    let key = parse_key(&args[4])?;
    let spool = PathBuf::from(&args[5]);

    let mono = Instant::now();
    let wall = SystemTime::now();
    let mut queue = open_queue(&spool, mono, wall)?;

    let listener = TcpListener::bind(bind_addr)
        .map_err(|error| format!("bind {bind_addr}: {error}"))?;
    println!("waiting for DTN custody transfer on {}", listener.local_addr().map_err(|e| e.to_string())?);

    let (mut stream, peer_addr) = listener
        .accept()
        .map_err(|error| format!("accept peer: {error}"))?;

    let mut replay = NonceReplayCache::new(1024);
    let (peer_id, mut session) = perform_server_handshake(
        &mut stream,
        node_id,
        &key,
        &mut replay,
    )
    .map_err(|error| format!("secure handshake: {error:?}"))?;

    let (bundle_id, status) = receive_one_bundle(
        &mut queue,
        &mut session,
        &mut stream,
        Instant::now(),
    )
    .map_err(|error| format!("receive custody: {error:?}"))?;

    save_queue(&spool, &queue, Instant::now(), SystemTime::now())
        .map_err(|error| format!("persist received custody: {error:?}"))?;

    println!(
        "custody peer_id={peer_id} addr={peer_addr} bundle_id={bundle_id} status={status:?} queue_len={}",
        queue.len()
    );
    Ok(())
}

fn custody_send(args: &[String]) -> Result<(), String> {
    if args.len() != 6 {
        return Err(usage());
    }

    let peer_addr = &args[2];
    let node_id = parse_u64(&args[3], "node_id")?;
    let key = parse_key(&args[4])?;
    let spool = PathBuf::from(&args[5]);

    let mono = Instant::now();
    let wall = SystemTime::now();
    let mut queue = open_queue(&spool, mono, wall)?;

    let mut stream = TcpStream::connect(peer_addr)
        .map_err(|error| format!("connect {peer_addr}: {error}"))?;
    let (peer_id, mut session) =
        perform_client_handshake(&mut stream, node_id, &key)
            .map_err(|error| format!("secure handshake: {error:?}"))?;

    let outcome = offer_next_bundle(
        &mut queue,
        &mut session,
        &mut stream,
        Instant::now(),
    );

    save_queue(&spool, &queue, Instant::now(), SystemTime::now())
        .map_err(|error| format!("persist sender queue: {error:?}"))?;

    let outcome =
        outcome.map_err(|error| format!("offer custody: {error:?}"))?;

    println!(
        "authenticated peer_id={peer_id} outcome={outcome:?} remaining={}",
        queue.len()
    );
    Ok(())
}

fn dispatch(args: &[String]) -> Result<(), String> {
    if args.len() != 9 {
        return Err(usage());
    }

    let peer_addr = &args[2];
    let node_id = parse_u64(&args[3], "node_id")?;
    let key = parse_key(&args[4])?;
    let request_spool = PathBuf::from(&args[5]);
    let return_spool = PathBuf::from(&args[6]);
    let return_bundle_id = parse_u64(&args[7], "return_bundle_id")?;
    let return_ttl_secs = parse_u64(&args[8], "return_ttl_secs")?;

    let mono = Instant::now();
    let wall = SystemTime::now();
    let mut request_queue = open_queue(&request_spool, mono, wall)?;

    let mut stream = TcpStream::connect(peer_addr)
        .map_err(|error| format!("connect egress {peer_addr}: {error}"))?;
    let (peer_id, mut session) =
        perform_client_handshake(&mut stream, node_id, &key)
            .map_err(|error| format!("secure handshake: {error:?}"))?;

    let outcome = dispatch_next_resolve(
        &mut request_queue,
        &mut session,
        &mut stream,
        Instant::now(),
    );

    save_queue(
        &request_spool,
        &request_queue,
        Instant::now(),
        SystemTime::now(),
    )
    .map_err(|error| format!("persist request queue: {error:?}"))?;

    let outcome =
        outcome.map_err(|error| format!("dispatch queued request: {error:?}"))?;

    let response = match outcome {
        DispatchOutcome::Empty => {
            println!("request queue is empty");
            return Ok(());
        }
        DispatchOutcome::Delivered {
            request_id,
            addresses,
            ..
        } => ResolveResponse {
            request_id,
            status: ResolveStatus::Ok,
            addresses,
        },
        DispatchOutcome::RemoteRejected {
            request_id,
            status,
            ..
        } => ResolveResponse {
            request_id,
            status,
            addresses: Vec::new(),
        },
    };

    let payload = encode_response(&response)
        .map_err(|error| format!("encode return result: {error:?}"))?;

    let mono = Instant::now();
    let wall = SystemTime::now();
    let mut return_queue = open_queue(&return_spool, mono, wall)?;

    let inserted = return_queue.push_unique(Bundle {
        id: return_bundle_id,
        priority: BundlePriority::Urgent,
        created_at: mono,
        ttl: Duration::from_secs(return_ttl_secs),
        payload,
        attempts: 0,
    });

    if !inserted {
        return Err(format!(
            "return bundle_id={return_bundle_id} already exists"
        ));
    }

    save_queue(&return_spool, &return_queue, mono, wall)
        .map_err(|error| format!("persist return queue: {error:?}"))?;

    println!(
        "egress peer_id={peer_id} result queued for return bundle_id={return_bundle_id} request_id={} status={:?}",
        response.request_id, response.status
    );
    Ok(())
}

fn show_result(args: &[String]) -> Result<(), String> {
    if args.len() != 3 {
        return Err(usage());
    }

    let spool = PathBuf::from(&args[2]);
    let queue = open_queue(&spool, Instant::now(), SystemTime::now())?;

    if queue.is_empty() {
        println!("result spool is empty");
        return Ok(());
    }

    for bundle in queue.iter() {
        match decode_response(&bundle.payload) {
            Ok(response) => {
                println!(
                    "bundle_id={} request_id={} status={:?} attempts={}",
                    bundle.id,
                    response.request_id,
                    response.status,
                    bundle.attempts,
                );
                for address in response.addresses {
                    println!("  {address}");
                }
            }
            Err(error) => {
                println!(
                    "bundle_id={} is not a resolve response: {error:?}",
                    bundle.id
                );
            }
        }
    }

    Ok(())
}

fn open_queue(
    path: &Path,
    mono: Instant,
    wall: SystemTime,
) -> Result<DtnQueue, String> {
    match fs::metadata(path) {
        Ok(_) => load_queue(path, mono, wall)
            .map_err(|error| format!("load spool {}: {error:?}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(DtnQueue::new())
        }
        Err(error) => Err(format!(
            "inspect spool {}: {error}",
            path.display()
        )),
    }
}

fn parse_priority(value: &str) -> Result<BundlePriority, String> {
    match value {
        "bulk" => Ok(BundlePriority::Bulk),
        "normal" => Ok(BundlePriority::Normal),
        "urgent" => Ok(BundlePriority::Urgent),
        _ => Err("priority must be bulk, normal, or urgent".to_owned()),
    }
}

fn parse_u64(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an unsigned 64-bit integer"))
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
        "physical DTN recovery lab:",
        "",
        "  enqueue <spool> <bundle_id> <request_id> <hostname> <bulk|normal|urgent> <ttl_secs>",
        "  custody-receive <bind_addr> <node_id> <64_hex_psk> <spool>",
        "  custody-send <peer_addr> <node_id> <64_hex_psk> <spool>",
        "  dispatch <egress_addr> <node_id> <64_hex_psk> <request_spool> <return_spool> <return_bundle_id> <return_ttl_secs>",
        "  show-result <spool>",
        "",
        "A -> B request:",
        "  A: enqueue a.spool 1001 501 example.com urgent 3600",
        "  B: custody-receive 0.0.0.0:45130 200 <psk> b-request.spool",
        "  A: custody-send <B_IP>:45130 100 <psk> a.spool",
        "",
        "B -> C egress:",
        "  C: use peer-egress-cli server 0.0.0.0:45123 300 <psk>",
        "  B: dispatch <C_IP>:45123 200 <psk> b-request.spool b-return.spool 2001 3600",
        "",
        "B -> A return:",
        "  A: custody-receive 0.0.0.0:45131 100 <psk> a-result.spool",
        "  B: custody-send <A_IP>:45131 200 <psk> b-return.spool",
        "  A: show-result a-result.spool",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_priority_and_key() {
        assert!(matches!(
            parse_priority("urgent"),
            Ok(BundlePriority::Urgent)
        ));
        assert!(parse_priority("other").is_err());
        assert!(parse_key(&"42".repeat(32)).is_ok());
        assert!(parse_key("42").is_err());
    }
}
