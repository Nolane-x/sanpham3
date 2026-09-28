use peer_egress::Resolver;
use std::io;
use std::net::IpAddr;
use std::time::Duration;
use weak_link_court::{
    run_nearzero_resolve_court, run_resolve_court, standard_ladder,
    PeriodicOutage, WeakLinkProfile,
};

struct FixedResolver;

impl Resolver for FixedResolver {
    fn resolve(&self, _hostname: &str) -> io::Result<Vec<IpAddr>> {
        Ok(vec![
            "10.0.0.5".parse().unwrap(),
            "8.8.8.8".parse().unwrap(),
        ])
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("weak-link-court-cli: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().collect::<Vec<_>>();

    match args.get(1).map(String::as_str) {
        None | Some("ladder") => run_ladder(),
        Some("compare") => run_compare(&args),
        Some("profile") => run_profile(&args),
        _ => Err(usage()),
    }
}

fn run_ladder() -> Result<(), String> {
    for profile in standard_ladder() {
        print_result(profile)?;
    }
    Ok(())
}

fn run_compare(args: &[String]) -> Result<(), String> {
    if args.len() > 3 {
        return Err(usage());
    }

    let bitrate_bps = match args.get(2) {
        Some(value) => parse_u64(value, "bitrate_bps")?,
        None => 10,
    };
    let profile = WeakLinkProfile::ladder(bitrate_bps);

    let baseline =
        run_resolve_court(profile, &FixedResolver, "example.com")
            .map_err(|error| format!("{error:?}"))?;
    let nearzero =
        run_nearzero_resolve_court(profile, &FixedResolver, "example.com")
            .map_err(|error| format!("{error:?}"))?;

    if baseline.response != nearzero.response {
        return Err("v0 and Near-Zero v1 produced different results".to_owned());
    }

    let baseline_bytes = baseline.accounting.delivered_bytes;
    let nearzero_bytes = nearzero.accounting.delivered_bytes;
    let saved_bytes = baseline_bytes.saturating_sub(nearzero_bytes);

    let saved_wire_ppm = if baseline_bytes == 0 {
        0
    } else {
        ((u128::from(saved_bytes) * 1_000_000)
            / u128::from(baseline_bytes)) as u64
    };

    let baseline_ms = baseline.accounting.elapsed.as_millis();
    let nearzero_ms = nearzero.accounting.elapsed.as_millis();
    let saved_ms = baseline_ms.saturating_sub(nearzero_ms);

    println!(
        "COMPARE bps={} same_result=true baseline_bytes={} nearzero_bytes={} saved_bytes={} saved_wire_ppm={}",
        bitrate_bps,
        baseline_bytes,
        nearzero_bytes,
        saved_bytes,
        saved_wire_ppm,
    );
    println!(
        "COMPARE_TIME baseline_ms={} nearzero_ms={} saved_ms={}",
        baseline_ms,
        nearzero_ms,
        saved_ms,
    );
    println!(
        "COMPARE_SECURITY baseline_handshake={} nearzero_handshake={} baseline_request_frame={} nearzero_request_frame={} baseline_response_frame={} nearzero_response_frame={}",
        baseline.handshake_wire_bytes,
        nearzero.handshake_wire_bytes,
        baseline.encrypted_request_frame_bytes,
        nearzero.encrypted_request_frame_bytes,
        baseline.encrypted_response_frame_bytes,
        nearzero.encrypted_response_frame_bytes,
    );

    Ok(())
}

fn run_profile(args: &[String]) -> Result<(), String> {
    if args.len() != 8 {
        return Err(usage());
    }

    let bitrate_bps = parse_u64(&args[2], "bitrate_bps")?;
    let loss_ppm = parse_u32(&args[3], "loss_ppm")?;
    let latency_ms = parse_u64(&args[4], "latency_ms")?;
    let chunk_bytes = parse_usize(&args[5], "chunk_bytes")?;
    let outage_period_ms = parse_u64(&args[6], "outage_period_ms")?;
    let outage_down_ms = parse_u64(&args[7], "outage_down_ms")?;

    let outage = if outage_period_ms == 0 && outage_down_ms == 0 {
        None
    } else {
        Some(PeriodicOutage {
            period: Duration::from_millis(outage_period_ms),
            down_for: Duration::from_millis(outage_down_ms),
        })
    };

    print_result(WeakLinkProfile {
        bitrate_bps,
        chunk_bytes,
        one_way_latency: Duration::from_millis(latency_ms),
        loss_ppm,
        outage,
    })
}

fn print_result(profile: WeakLinkProfile) -> Result<(), String> {
    let result =
        run_resolve_court(profile, &FixedResolver, "example.com")
            .map_err(|error| format!("{error:?}"))?;

    println!(
        "PROFILE bps={} loss_ppm={} latency_ms={} chunk_bytes={} outage={}",
        profile.bitrate_bps,
        profile.loss_ppm,
        profile.one_way_latency.as_millis(),
        profile.chunk_bytes,
        profile
            .outage
            .map(|value| format!(
                "{}ms/{}ms",
                value.down_for.as_millis(),
                value.period.as_millis(),
            ))
            .unwrap_or_else(|| "-".to_owned()),
    );
    println!(
        "RESULT success={} elapsed_ms={} addresses={:?}",
        result.succeeded(),
        result.accounting.elapsed.as_millis(),
        result.response.addresses,
    );
    println!(
        "WIRE logical_messages={} delivered_chunks={} lost_attempts={} delivered_bytes={} attempted_bytes={} retransmitted_bytes={}",
        result.accounting.logical_messages,
        result.accounting.delivered_chunks,
        result.accounting.lost_chunk_attempts,
        result.accounting.delivered_bytes,
        result.accounting.attempted_bytes,
        result.accounting.retransmitted_bytes,
    );
    println!(
        "TIME serialization_ms={} propagation_ms={} outage_wait_ms={}",
        result.accounting.serialization_time.as_millis(),
        result.accounting.propagation_time.as_millis(),
        result.accounting.outage_wait.as_millis(),
    );
    println!(
        "PAYLOAD request={} response={} handshake_wire={} encrypted_request={} encrypted_response={} useful_efficiency_ppm={}",
        result.request_payload_bytes,
        result.response_payload_bytes,
        result.handshake_wire_bytes,
        result.encrypted_request_frame_bytes,
        result.encrypted_response_frame_bytes,
        result.useful_efficiency_ppm(),
    );
    println!();

    Ok(())
}

fn parse_u64(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an unsigned integer"))
}

fn parse_u32(value: &str, name: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("{name} must be an unsigned integer"))
}

fn parse_usize(value: &str, name: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a positive integer"))
}

fn usage() -> String {
    [
        "usage:",
        "  weak-link-court-cli ladder",
        "  weak-link-court-cli compare [bitrate_bps]",
        "  weak-link-court-cli profile <bitrate_bps> <loss_ppm> <latency_ms> <chunk_bytes> <outage_period_ms> <outage_down_ms>",
        "",
        "examples:",
        "  weak-link-court-cli ladder",
        "  weak-link-court-cli compare 10",
        "  weak-link-court-cli compare 1",
        "  weak-link-court-cli profile 100 200000 300 8 15000 4000",
        "",
        "This is a virtual-time software court, not physical RF evidence.",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_mode_preserves_result_and_saves_wire_bytes() {
        let profile = WeakLinkProfile::ladder(10);
        let baseline =
            run_resolve_court(profile, &FixedResolver, "example.com").unwrap();
        let nearzero =
            run_nearzero_resolve_court(profile, &FixedResolver, "example.com")
                .unwrap();

        assert_eq!(baseline.response, nearzero.response);
        assert_eq!(baseline.accounting.delivered_bytes, 232);
        assert_eq!(nearzero.accounting.delivered_bytes, 142);
    }

    #[test]
    fn custom_profile_parser_accepts_zero_outage() {
        assert_eq!(parse_u64("100", "bps").unwrap(), 100);
        assert_eq!(parse_u32("200000", "loss").unwrap(), 200_000);
        assert_eq!(parse_usize("8", "chunk").unwrap(), 8);
    }
}
