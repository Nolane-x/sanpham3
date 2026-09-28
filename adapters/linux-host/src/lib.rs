use connectivity_core::{Capability, PermissionState, PlatformScanner, Transport};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct LinuxScanner {
    sys_class_net: PathBuf,
}

impl LinuxScanner {
    pub fn new() -> Self {
        Self {
            sys_class_net: PathBuf::from("/sys/class/net"),
        }
    }

    #[cfg(test)]
    fn with_sys_class_net(path: impl Into<PathBuf>) -> Self {
        Self {
            sys_class_net: path.into(),
        }
    }

    fn classify_interface(path: &Path, name: &str) -> Transport {
        if name == "lo" {
            return Transport::Other;
        }

        if path.join("wireless").exists() {
            return Transport::Wifi;
        }

        if name.starts_with("wwan")
            || name.starts_with("rmnet")
            || name.starts_with("ccmni")
        {
            return Transport::Cellular;
        }

        if name.starts_with("tun")
            || name.starts_with("tap")
            || name.starts_with("wg")
            || name.starts_with("tailscale")
        {
            return Transport::Tunnel;
        }

        Transport::Ethernet
    }

    fn read_operstate(path: &Path) -> Option<String> {
        fs::read_to_string(path.join("operstate"))
            .ok()
            .map(|value| value.trim().to_owned())
    }

    fn interface_capability(path: &Path, name: String) -> Capability {
        let transport = Self::classify_interface(path, &name);
        let operstate = Self::read_operstate(path);
        let available = matches!(
            operstate.as_deref(),
            Some("up") | Some("unknown") | Some("dormant")
        );

        Capability {
            name: format!("net:{name}"),
            interface: Some(name),
            transport,
            available,
            permission: PermissionState::Granted,
            can_scan: transport == Transport::Wifi,
            can_connect: transport != Transport::Other,
            can_advertise: false,
            can_relay: transport != Transport::Other,
            can_bind_socket: transport != Transport::Other,
            constraints: operstate
                .map(|state| vec![format!("operstate={state}")])
                .unwrap_or_default(),
        }
    }
}

impl PlatformScanner for LinuxScanner {
    fn platform_name(&self) -> &'static str {
        "linux"
    }

    fn inventory(&mut self) -> Vec<Capability> {
        let Ok(entries) = fs::read_dir(&self.sys_class_net) else {
            return vec![Capability {
                name: "linux-net-inventory".to_owned(),
                interface: None,
                transport: Transport::Other,
                available: false,
                permission: PermissionState::Unsupported,
                can_scan: false,
                can_connect: false,
                can_advertise: false,
                can_relay: false,
                can_bind_socket: false,
                constraints: vec![format!(
                    "cannot_read={}",
                    self.sys_class_net.display()
                )],
            }];
        };

        let mut capabilities = entries
            .filter_map(Result::ok)
            .map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                Self::interface_capability(&entry.path(), name)
            })
            .collect::<Vec<_>>();

        capabilities.sort_by(|left, right| left.name.cmp(&right.name));
        capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "sanpham3-linux-scanner-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn inventories_wifi_and_tunnel_interfaces() {
        let root = temp_root();

        let wifi = root.join("wlan0");
        create_dir_all(wifi.join("wireless")).unwrap();
        write(wifi.join("operstate"), "up\n").unwrap();

        let tunnel = root.join("wg0");
        create_dir_all(&tunnel).unwrap();
        write(tunnel.join("operstate"), "unknown\n").unwrap();

        let mut scanner = LinuxScanner::with_sys_class_net(&root);
        let items = scanner.inventory();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].transport, Transport::Wifi);
        assert!(items[0].available);
        assert_eq!(items[1].transport, Transport::Tunnel);
        assert!(items[1].can_bind_socket);

        fs::remove_dir_all(root).unwrap();
    }
}
