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

    match linux_host::LinuxRecoveryProbe::new().run(&capabilities) {
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

    match windows_host::WindowsRecoveryProbe::new().run() {
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
