#[cfg(any(target_os = "linux", target_os = "windows"))]
use connectivity_core::{Capability, PlatformScanner};

fn main() {
    #[cfg(target_os = "linux")]
    run_linux();

    #[cfg(target_os = "windows")]
    run_windows();

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    println!("host-probe-cli: supported hosts are Linux and Windows");
}

#[cfg(target_os = "linux")]
fn run_linux() {
    let mut scanner = linux_host::LinuxScanner::new();

    println!("platform={}", scanner.platform_name());

    let capabilities = scanner.inventory();
    print_capabilities(&capabilities);

    println!();
    println!("recovery_probe:");

    let https_targets = match configured_https_targets() {
        Ok(targets) => targets,
        Err(error) => {
            eprintln!("invalid HTTPS probe configuration: {error}");
            std::process::exit(2);
        }
    };

    match linux_host::LinuxRecoveryProbe::new()
        .https_targets(https_targets)
        .run(&capabilities)
    {
        Ok(snapshot) => {
            for record in snapshot.ledger.records() {
                println!(
                    "PROBE id={} kind={:?} status={:?} detail={}",
                    record.id,
                    record.kind,
                    record.status,
                    record.detail.as_deref().unwrap_or("-"),
                );
            }

            println!(
                "SUMMARY pending={} information_path={} local_only={}",
                snapshot.ledger.pending_count(),
                snapshot.ledger.any_information_path(),
                snapshot.ledger.can_declare_local_only(),
            );
        }
        Err(error) => {
            eprintln!("recovery probe failed: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(target_os = "windows")]
fn run_windows() {
    let mut scanner = windows_host::WindowsScanner::new();

    println!("platform={}", scanner.platform_name());

    let capabilities = scanner.inventory();
    print_capabilities(&capabilities);

    println!();
    println!("recovery_probe:");

    let https_targets = match configured_https_targets() {
        Ok(targets) => targets,
        Err(error) => {
            eprintln!("invalid HTTPS probe configuration: {error}");
            std::process::exit(2);
        }
    };

    match windows_host::WindowsRecoveryProbe::new()
        .https_targets(https_targets)
        .run()
    {
        Ok(snapshot) => {
            for adapter in &snapshot.adapters {
                println!(
                    "PATH name={} transport={:?} available={} gateway={} ipv4_metric={} ipv6_metric={} tx_bps={} rx_bps={} unicast={:?} dns={:?}",
                    adapter.name,
                    adapter.transport,
                    adapter.available,
                    adapter.has_gateway,
                    adapter.ipv4_metric,
                    adapter.ipv6_metric,
                    adapter.tx_bps,
                    adapter.rx_bps,
                    adapter.unicast,
                    adapter.dns_servers,
                );
            }

            for record in snapshot.ledger.records() {
                println!(
                    "PROBE id={} kind={:?} status={:?} detail={}",
                    record.id,
                    record.kind,
                    record.status,
                    record.detail.as_deref().unwrap_or("-"),
                );
            }

            println!(
                "SUMMARY pending={} information_path={} local_only={}",
                snapshot.ledger.pending_count(),
                snapshot.ledger.any_information_path(),
                snapshot.ledger.can_declare_local_only(),
            );
        }
        Err(error) => {
            eprintln!("Windows recovery probe failed: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn print_capabilities(capabilities: &[Capability]) {
    for capability in capabilities {
        println!(
            "CAP {} interface={:?} transport={:?} available={} scan={} connect={} relay={} bind_socket={} constraints={:?}",
            capability.name,
            capability.interface,
            capability.transport,
            capability.available,
            capability.can_scan,
            capability.can_connect,
            capability.can_relay,
            capability.can_bind_socket,
            capability.constraints,
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn configured_https_targets() -> Result<Vec<path_probe::HttpsProbeTarget>, String> {
    let address = std::env::var("SP3_HTTPS_PROBE_ADDR").ok();
    let server_name = std::env::var("SP3_HTTPS_PROBE_NAME").ok();

    match (address, server_name) {
        (None, None) => Ok(Vec::new()),
        (Some(_), None) | (None, Some(_)) => Err(
            "SP3_HTTPS_PROBE_ADDR and SP3_HTTPS_PROBE_NAME must be set together"
                .to_owned(),
        ),
        (Some(address), Some(server_name)) => {
            let address = address.parse().map_err(|_| {
                "SP3_HTTPS_PROBE_ADDR must be a literal IP:port".to_owned()
            })?;
            let path = std::env::var("SP3_HTTPS_PROBE_PATH")
                .unwrap_or_else(|_| "/".to_owned());
            let max_response_bytes = std::env::var(
                "SP3_HTTPS_PROBE_MAX_BYTES",
            )
            .ok()
            .map(|value| {
                value.parse::<usize>().map_err(|_| {
                    "SP3_HTTPS_PROBE_MAX_BYTES must be a positive integer"
                        .to_owned()
                })
            })
            .transpose()?
            .unwrap_or(1024);

            path_probe::HttpsProbeTarget::new(
                address,
                server_name,
                path,
                max_response_bytes,
            )
            .map(|target| vec![target])
            .map_err(|error| error.to_string())
        }
    }
}
