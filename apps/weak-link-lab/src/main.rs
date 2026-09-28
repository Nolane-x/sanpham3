use std::env;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use weak_link::{
    estimate_exchange, LinkRate, RateLimitedWriter, SessionMode,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("weak-link-lab: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().collect::<Vec<_>>();

    match args.get(1).map(String::as_str) {
        Some("budget") => budget(&args),
        Some("ladder") => ladder(&args),
        Some("proxy") => proxy(&args),
        _ => Err(usage()),
    }
}

fn budget(args: &[String]) -> Result<(), String> {
    if args.len() != 6 {
        return Err(usage());
    }

    let bps = parse_u64(&args[2], "bps")?;
    let request_bytes = parse_usize(&args[3], "request_bytes")?;
    let response_bytes = parse_usize(&args[4], "response_bytes")?;
    let mode = parse_mode(&args[5])?;

    print_budget(
        LinkRate::new(bps).map_err(|error| format!("{error:?}"))?,
        mode,
        request_bytes,
        response_bytes,
    );
    Ok(())
}

fn ladder(args: &[String]) -> Result<(), String> {
    if args.len() != 5 {
        return Err(usage());
    }

    let request_bytes = parse_usize(&args[2], "request_bytes")?;
    let response_bytes = parse_usize(&args[3], "response_bytes")?;
    let mode = parse_mode(&args[4])?;

    for bps in [1_000_u64, 100, 30, 10] {
        print_budget(
            LinkRate::new(bps).expect("fixed ladder rate"),
            mode,
            request_bytes,
            response_bytes,
        );
    }

    Ok(())
}

fn proxy(args: &[String]) -> Result<(), String> {
    if args.len() != 8 {
        return Err(usage());
    }

    let listen = parse_socket(&args[2], "listen")?;
    let target = parse_socket(&args[3], "target")?;
    let client_to_target_bps = parse_u64(&args[4], "client_to_target_bps")?;
    let target_to_client_bps = parse_u64(&args[5], "target_to_client_bps")?;
    let chunk_bytes = parse_usize(&args[6], "chunk_bytes")?;
    let connections = parse_usize(&args[7], "connections")?;

    let uplink = LinkRate::new(client_to_target_bps)
        .map_err(|error| format!("{error:?}"))?;
    let downlink = LinkRate::new(target_to_client_bps)
        .map_err(|error| format!("{error:?}"))?;

    let listener = TcpListener::bind(listen)
        .map_err(|error| format!("bind {listen}: {error}"))?;

    println!(
        "weak-link proxy listening={} target={} uplink_bps={} downlink_bps={} chunk_bytes={} connections={}",
        listener.local_addr().map_err(|error| error.to_string())?,
        target,
        client_to_target_bps,
        target_to_client_bps,
        chunk_bytes,
        connections,
    );

    for index in 0..connections {
        let (client, peer) = listener
            .accept()
            .map_err(|error| format!("accept connection: {error}"))?;

        println!("connection={} client={peer}", index + 1);
        let upstream = TcpStream::connect(target)
            .map_err(|error| format!("connect target {target}: {error}"))?;

        relay_connection(
            client,
            upstream,
            uplink,
            downlink,
            chunk_bytes,
        )
        .map_err(|error| format!("relay connection: {error}"))?;
    }

    Ok(())
}

fn relay_connection(
    client: TcpStream,
    upstream: TcpStream,
    uplink: LinkRate,
    downlink: LinkRate,
    chunk_bytes: usize,
) -> io::Result<()> {
    let mut client_reader = client.try_clone()?;
    let client_writer = client;

    let mut upstream_reader = upstream.try_clone()?;
    let upstream_writer = upstream;

    let to_upstream = thread::spawn(move || -> io::Result<u64> {
        let mut writer =
            RateLimitedWriter::new(upstream_writer, uplink, chunk_bytes)
                .map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("{error:?}"),
                    )
                })?;

        io::copy(&mut client_reader, &mut writer)
    });

    let to_client = thread::spawn(move || -> io::Result<u64> {
        let mut writer =
            RateLimitedWriter::new(client_writer, downlink, chunk_bytes)
                .map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("{error:?}"),
                    )
                })?;

        io::copy(&mut upstream_reader, &mut writer)
    });

    let uplink_bytes = to_upstream
        .join()
        .map_err(|_| io::Error::other("uplink relay thread panicked"))??;
    let downlink_bytes = to_client
        .join()
        .map_err(|_| io::Error::other("downlink relay thread panicked"))??;

    println!(
        "relay complete uplink_bytes={uplink_bytes} downlink_bytes={downlink_bytes}"
    );

    Ok(())
}

fn print_budget(
    rate: LinkRate,
    mode: SessionMode,
    request_bytes: usize,
    response_bytes: usize,
) {
    let budget = estimate_exchange(
        rate,
        mode,
        request_bytes,
        response_bytes,
    );

    println!(
        "BUDGET bps={} mode={:?} request_plain={} response_plain={} handshake_wire={} request_wire={} response_wire={} total_wire={} ideal_seconds={:.3}",
        budget.bitrate_bps,
        budget.mode,
        budget.request_plaintext_bytes,
        budget.response_plaintext_bytes,
        budget.handshake_wire_bytes,
        budget.request_wire_bytes,
        budget.response_wire_bytes,
        budget.total_wire_bytes,
        budget.ideal_serialization_time.as_secs_f64(),
    );
}

fn parse_mode(value: &str) -> Result<SessionMode, String> {
    match value {
        "fresh" => Ok(SessionMode::FreshHandshake),
        "existing" => Ok(SessionMode::ExistingSession),
        _ => Err("mode must be 'fresh' or 'existing'".to_owned()),
    }
}

fn parse_socket(value: &str, name: &str) -> Result<SocketAddr, String> {
    value
        .parse::<SocketAddr>()
        .map_err(|_| format!("{name} must be a literal IP:port"))
}

fn parse_u64(value: &str, name: &str) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an unsigned integer"))?;

    if parsed == 0 {
        return Err(format!("{name} must be greater than zero"));
    }

    Ok(parsed)
}

fn parse_usize(value: &str, name: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a positive integer"))?;

    if parsed == 0 {
        return Err(format!("{name} must be greater than zero"));
    }

    Ok(parsed)
}

fn usage() -> String {
    [
        "usage:",
        "  weak-link-lab budget <bps> <request_bytes> <response_bytes> <fresh|existing>",
        "  weak-link-lab ladder <request_bytes> <response_bytes> <fresh|existing>",
        "  weak-link-lab proxy <listen_ip:port> <target_ip:port> <client_to_target_bps> <target_to_client_bps> <chunk_bytes> <connections>",
        "",
        "examples:",
        "  weak-link-lab ladder 12 24 fresh",
        "  weak-link-lab ladder 12 24 existing",
        "  weak-link-lab proxy 127.0.0.1:45140 127.0.0.1:45123 30 30 1 1",
        "",
        "For precise 10/30 bit/s experiments use chunk_bytes=1.",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes_and_positive_numbers() {
        assert_eq!(parse_mode("fresh").unwrap(), SessionMode::FreshHandshake);
        assert_eq!(
            parse_mode("existing").unwrap(),
            SessionMode::ExistingSession,
        );
        assert!(parse_mode("other").is_err());
        assert_eq!(parse_u64("10", "bps").unwrap(), 10);
        assert!(parse_u64("0", "bps").is_err());
    }
}
