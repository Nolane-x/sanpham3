use connectivity_core::PlatformScanner;

fn main() {
    #[cfg(target_os = "linux")]
    {
        let mut scanner = linux_host::LinuxScanner::new();

        println!("platform={}", scanner.platform_name());

        for capability in scanner.inventory() {
            println!(
                "{} interface={:?} transport={:?} available={} scan={} connect={} relay={} bind_socket={} constraints={:?}",
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

    #[cfg(not(target_os = "linux"))]
    {
        println!("host-probe-cli: Linux scanner is not active on this OS yet");
    }
}
