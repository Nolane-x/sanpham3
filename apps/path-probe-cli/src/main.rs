use path_probe::{
    run_series, summarize, tcp_connect, tcp_connect_device, tiny_https_head,
    tiny_https_head_device, AttemptMeasurement, AttemptSample,
    ProbeSeriesSummary,
};
use std::env;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        eprintln!("path-probe-cli: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().collect::<Vec<_>>();

    match args.get(1).map(String::as_str) {
        Some("tcp-source") => tcp_source(&args),
        Some("tcp-interface") => tcp_interface(&args),
        Some("https-source") => https_source(&args),
        Some("https-interface") => https_interface(&args),
        _ => Err(usage()),
    }
}

fn tcp_source(args: &[String]) -> Result<(), String> {
    if args.len() != 7 {
        return Err(usage());
    }

    let source = parse_source_ip(&args[2])?;
    let destination = parse_socket(&args[3], "destination")?;
    let attempts = parse_usize(&args[4], "attempts")?;
    let timeout = Duration::from_millis(parse_u64(&args[5], "timeout_ms")?);
    let pause = Duration::from_millis(parse_u64(&args[6], "pause_ms")?);

    let samples = run_series(attempts, pause, || {
        let result = tcp_connect(source, destination, timeout)?;
        Ok(AttemptMeasurement {
            elapsed: result.elapsed,
            useful_bytes: 0,
        })
    });

    print_samples(&samples);
    print_summary(&summarize(&samples));
    Ok(())
}

fn tcp_interface(args: &[String]) -> Result<(), String> {
    if args.len() != 7 {
        return Err(usage());
    }

    let interface = &args[2];
    let destination = parse_socket(&args[3], "destination")?;
    let attempts = parse_usize(&args[4], "attempts")?;
    let timeout = Duration::from_millis(parse_u64(&args[5], "timeout_ms")?);
    let pause = Duration::from_millis(parse_u64(&args[6], "pause_ms")?);

    let samples = run_series(attempts, pause, || {
        let result = tcp_connect_device(interface, destination, timeout)?;
        Ok(AttemptMeasurement {
            elapsed: result.elapsed,
            useful_bytes: 0,
        })
    });

    print_samples(&samples);
    print_summary(&summarize(&samples));
    Ok(())
}

fn https_source(args: &[String]) -> Result<(), String> {
    if args.len() != 10 {
        return Err(usage());
    }

    let source = parse_source_ip(&args[2])?;
    let destination = parse_socket(&args[3], "destination")?;
    let server_name = &args[4];
    let path = &args[5];
    let attempts = parse_usize(&args[6], "attempts")?;
    let timeout = Duration::from_millis(parse_u64(&args[7], "timeout_ms")?);
    let pause = Duration::from_millis(parse_u64(&args[8], "pause_ms")?);
    let max_bytes = parse_usize(&args[9], "max_response_bytes")?;

    let samples = run_series(attempts, pause, || {
        let result = tiny_https_head(
            source,
            destination,
            server_name,
            path,
            timeout,
            max_bytes,
        )?;

        Ok(AttemptMeasurement {
            elapsed: result.total_elapsed,
            useful_bytes: result.response_bytes,
        })
    });

    print_samples(&samples);
    print_summary(&summarize(&samples));
    Ok(())
}

fn https_interface(args: &[String]) -> Result<(), String> {
    if args.len() != 10 {
        return Err(usage());
    }

    let interface = &args[2];
    let destination = parse_socket(&args[3], "destination")?;
    let server_name = &args[4];
    let path = &args[5];
    let attempts = parse_usize(&args[6], "attempts")?;
    let timeout = Duration::from_millis(parse_u64(&args[7], "timeout_ms")?);
    let pause = Duration::from_millis(parse_u64(&args[8], "pause_ms")?);
    let max_bytes = parse_usize(&args[9], "max_response_bytes")?;

    let samples = run_series(attempts, pause, || {
        let result = tiny_https_head_device(
            interface,
            destination,
            server_name,
            path,
            timeout,
            max_bytes,
        )?;

        Ok(AttemptMeasurement {
            elapsed: result.total_elapsed,
            useful_bytes: result.response_bytes,
        })
    });

    print_samples(&samples);
    print_summary(&summarize(&samples));
    Ok(())
}

fn print_samples(samples: &[AttemptSample]) {
    for (index, sample) in samples.iter().enumerate() {
        println!(
            "ATTEMPT index={} success={} elapsed_ms={} useful_bytes={} detail={}",
            index + 1,
            sample.success,
            sample.elapsed.as_millis(),
            sample.useful_bytes,
            sample.detail.as_deref().unwrap_or("-"),
        );
    }
}

fn print_summary(summary: &ProbeSeriesSummary) {
    println!(
        "SUMMARY attempts={} successes={} failures={} loss_ppm={} min_ms={} median_ms={} p95_ms={} useful_bytes={} useful_bps={} longest_failure_run={} transitions={} intermittent={}",
        summary.attempts,
        summary.successes,
        summary.failures,
        summary.loss_ppm,
        duration_ms(summary.min_rtt),
        duration_ms(summary.median_rtt),
        duration_ms(summary.p95_rtt),
        summary.total_useful_bytes,
        summary.observed_useful_bitrate_bps,
        summary.longest_failure_run,
        summary.state_transitions,
        summary.intermittent,
    );
}

fn duration_ms(value: Option<Duration>) -> String {
    value
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|| "-".to_owned())
}

fn parse_source_ip(value: &str) -> Result<Option<IpAddr>, String> {
    if value == "any" {
        return Ok(None);
    }

    value
        .parse::<IpAddr>()
        .map(Some)
        .map_err(|_| "source must be 'any' or a literal IP address".to_owned())
}

fn parse_socket(value: &str, name: &str) -> Result<SocketAddr, String> {
    value
        .parse::<SocketAddr>()
        .map_err(|_| format!("{name} must be a literal IP:port socket address"))
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

fn parse_u64(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an unsigned integer"))
}

fn usage() -> String {
    [
        "usage:",
        "  path-probe-cli tcp-source <source_ip|any> <target_ip:port> <attempts> <timeout_ms> <pause_ms>",
        "  path-probe-cli tcp-interface <interface> <target_ip:port> <attempts> <timeout_ms> <pause_ms>",
        "  path-probe-cli https-source <source_ip|any> <target_ip:443> <server_name> <path> <attempts> <timeout_ms> <pause_ms> <max_response_bytes>",
        "  path-probe-cli https-interface <interface> <target_ip:443> <server_name> <path> <attempts> <timeout_ms> <pause_ms> <max_response_bytes>",
        "",
        "notes:",
        "  tcp-interface / https-interface use Linux SO_BINDTODEVICE semantics.",
        "  source modes bind to an explicit local IP and work on Windows/Linux.",
        "  target must be a literal IP:port so DNS is not silently performed through another path.",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_source_and_positive_attempts() {
        assert_eq!(parse_source_ip("any").unwrap(), None);
        assert_eq!(
            parse_source_ip("127.0.0.1").unwrap(),
            Some("127.0.0.1".parse().unwrap())
        );
        assert!(parse_source_ip("localhost").is_err());
        assert_eq!(parse_usize("3", "attempts").unwrap(), 3);
        assert!(parse_usize("0", "attempts").is_err());
    }
}
