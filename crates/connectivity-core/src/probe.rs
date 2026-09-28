use crate::model::Transport;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
    RequiresUserAction,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct Capability {
    pub name: String,
    pub interface: Option<String>,
    pub transport: Transport,
    pub available: bool,
    pub permission: PermissionState,
    pub can_scan: bool,
    pub can_connect: bool,
    pub can_advertise: bool,
    pub can_relay: bool,
    pub can_bind_socket: bool,
    pub constraints: Vec<String>,
}

/// Contract implemented by Android, Windows, and Linux capability scanners.
///
/// A scanner reports evidence about the current machine. The core must not infer
/// access to a radio/API merely because the operating system can support it.
pub trait PlatformScanner {
    fn platform_name(&self) -> &'static str;
    fn inventory(&mut self) -> Vec<Capability>;
}
