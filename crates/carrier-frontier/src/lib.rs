use std::cmp::Ordering;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Android,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierKind {
    InternetIp,
    WifiDirect,
    WifiAware,
    BluetoothLeAdvertisement,
    BluetoothLeGatt,
    BluetoothLeL2cap,
    BluetoothRfcomm,
    LocalOnlyHotspot,
    CellularSms,
    NfcHce,
    AcousticNearUltrasonic,
    OpticalScreenCamera,
    VibrationSurface,
    MagneticSensor,
    UsbLocal,
    ExternalOsInterface,
    PhysicalDataMule,
    LocalCacheTwin,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InformationFreshness {
    FreshRemote,
    DelayedRemote,
    CachedRemote,
    LocallyGenerated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceClass {
    ProductionApi,
    ResearchDemonstrated,
    ResearchHardwareOnly,
    LocalOnly,
    ImpossibleFreshRemote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Directionality {
    OneWay,
    HalfDuplex,
    FullDuplex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FailureDomain {
    InternetPath,
    WifiRadio,
    BluetoothRadio,
    CellularModem,
    NfcController,
    AudioTransducers,
    DisplayCamera,
    HapticsSensors,
    MagnetometerPath,
    UsbPath,
    ExternalInterface,
    HumanMobility,
    LocalState,
    NoChannel,
}

pub fn primary_failure_domain(kind: CarrierKind) -> FailureDomain {
    match kind {
        CarrierKind::InternetIp => FailureDomain::InternetPath,
        CarrierKind::WifiDirect
        | CarrierKind::WifiAware
        | CarrierKind::LocalOnlyHotspot => FailureDomain::WifiRadio,
        CarrierKind::BluetoothLeAdvertisement
        | CarrierKind::BluetoothLeGatt
        | CarrierKind::BluetoothLeL2cap
        | CarrierKind::BluetoothRfcomm => FailureDomain::BluetoothRadio,
        CarrierKind::CellularSms => FailureDomain::CellularModem,
        CarrierKind::NfcHce => FailureDomain::NfcController,
        CarrierKind::AcousticNearUltrasonic => FailureDomain::AudioTransducers,
        CarrierKind::OpticalScreenCamera => FailureDomain::DisplayCamera,
        CarrierKind::VibrationSurface => FailureDomain::HapticsSensors,
        CarrierKind::MagneticSensor => FailureDomain::MagnetometerPath,
        CarrierKind::UsbLocal => FailureDomain::UsbPath,
        CarrierKind::ExternalOsInterface => FailureDomain::ExternalInterface,
        CarrierKind::PhysicalDataMule => FailureDomain::HumanMobility,
        CarrierKind::LocalCacheTwin => FailureDomain::LocalState,
        CarrierKind::None => FailureDomain::NoChannel,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureScenario {
    pub failed_domains: Vec<FailureDomain>,
}

impl FailureScenario {
    pub fn carrier_survives(&self, kind: CarrierKind) -> bool {
        !self
            .failed_domains
            .contains(&primary_failure_domain(kind))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedundancySelection {
    pub carriers: Vec<CarrierKind>,
    pub distinct_failure_domains: usize,
}

/// Greedy deterministic baseline for spreading redundant fragments/parity
/// across physical failure domains.
///
/// The first carrier is the fastest supported candidate. Subsequent picks
/// prefer a previously unused primary failure domain before considering
/// nominal bitrate. This avoids counting BLE GATT + BLE L2CAP + RFCOMM as
/// three independent failure paths when all depend on the same Bluetooth
/// radio/controller.
pub fn choose_failure_diverse_carriers(
    profiles: &[CarrierProfile],
    device: &DeviceCapabilities,
    copies: usize,
) -> RedundancySelection {
    if copies == 0 {
        return RedundancySelection {
            carriers: Vec::new(),
            distinct_failure_domains: 0,
        };
    }

    let mut candidates = profiles
        .iter()
        .filter(|profile| {
            profile.supported_by(device)
                && profile.kind != CarrierKind::None
                && profile.kind != CarrierKind::LocalCacheTwin
                && profile.nominal_bps > 0
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        right
            .nominal_bps
            .cmp(&left.nominal_bps)
            .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
    });

    let mut selected = Vec::new();
    let mut domains = Vec::new();

    while selected.len() < copies && !candidates.is_empty() {
        let best_index = candidates
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| {
                let left_seen =
                    domains.contains(&primary_failure_domain(left.kind));
                let right_seen =
                    domains.contains(&primary_failure_domain(right.kind));

                left_seen
                    .cmp(&right_seen)
                    .then_with(|| right.nominal_bps.cmp(&left.nominal_bps))
                    .then_with(|| {
                        format!("{:?}", left.kind)
                            .cmp(&format!("{:?}", right.kind))
                    })
            })
            .map(|(index, _)| index)
            .expect("non-empty candidates");

        let profile = candidates.remove(best_index);
        let domain = primary_failure_domain(profile.kind);
        selected.push(profile.kind);
        if !domains.contains(&domain) {
            domains.push(domain);
        }
    }

    RedundancySelection {
        carriers: selected,
        distinct_failure_domains: domains.len(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceCapabilities {
    pub platform: Platform,
    pub wifi_direct: bool,
    pub wifi_aware: bool,
    pub bluetooth_le: bool,
    pub ble_l2cap_coc: bool,
    pub bluetooth_classic: bool,
    pub local_only_hotspot: bool,
    pub telephony_messaging: bool,
    pub nfc_hce_or_reader: bool,
    pub microphone: bool,
    pub speaker: bool,
    pub camera: bool,
    pub screen: bool,
    pub vibrator: bool,
    pub accelerometer: bool,
    pub magnetometer: bool,
    pub usb: bool,
    /// Optional external device already attached and exposed through a normal
    /// OS/driver/API interface. This is never a product requirement.
    pub external_os_interface: bool,
}

impl DeviceCapabilities {
    pub fn conservative_android() -> Self {
        Self {
            platform: Platform::Android,
            wifi_direct: true,
            wifi_aware: false,
            bluetooth_le: true,
            ble_l2cap_coc: false,
            bluetooth_classic: true,
            local_only_hotspot: true,
            telephony_messaging: false,
            nfc_hce_or_reader: false,
            microphone: true,
            speaker: true,
            camera: true,
            screen: true,
            vibrator: true,
            accelerometer: true,
            magnetometer: true,
            usb: true,
            external_os_interface: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidHardwareProfile {
    pub wifi_direct: bool,
    pub wifi_aware: bool,
    pub bluetooth_le: bool,
    pub bluetooth_classic: bool,
    pub nfc_hce_or_reader: bool,
    pub telephony_messaging: bool,
    pub microphone: bool,
    pub speaker: bool,
    pub camera: bool,
    pub screen: bool,
    pub vibrator: bool,
    pub accelerometer: bool,
    pub magnetometer: bool,
    pub usb: bool,
    pub external_os_interface: bool,
}

impl AndroidHardwareProfile {
    pub fn broad_phone() -> Self {
        Self {
            wifi_direct: true,
            wifi_aware: true,
            bluetooth_le: true,
            bluetooth_classic: true,
            nfc_hce_or_reader: true,
            telephony_messaging: true,
            microphone: true,
            speaker: true,
            camera: true,
            screen: true,
            vibrator: true,
            accelerometer: true,
            magnetometer: true,
            usb: true,
            external_os_interface: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidPermissionProfile {
    pub legacy_location: bool,
    pub nearby_wifi_devices: bool,
    pub bluetooth_scan: bool,
    pub bluetooth_connect: bool,
    pub bluetooth_advertise: bool,
    pub send_sms: bool,
}

impl AndroidPermissionProfile {
    pub fn all_granted() -> Self {
        Self {
            legacy_location: true,
            nearby_wifi_devices: true,
            bluetooth_scan: true,
            bluetooth_connect: true,
            bluetooth_advertise: true,
            send_sms: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidLocalNetworkAccess {
    /// Android 17 enforcement does not apply to this app/OS combination.
    LegacyUnrestricted,
    /// Android 17+ broad LAN permission is granted.
    AccessLocalNetworkGranted,
    /// Android 16 opt-in restriction is active and NEARBY_WIFI_DEVICES is
    /// granted for the compatibility test phase.
    Android16OptInGranted,
    /// A privacy-preserving system picker selected a specific device/path.
    /// This is intentionally not equivalent to arbitrary LAN permission.
    SystemMediatedSelectedDevice,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidLocalNetworkPolicy {
    pub os_api_level: u16,
    pub target_sdk: u16,
    pub access_local_network_granted: bool,
    pub system_mediated_selected_device: bool,
    pub android16_restrict_local_network_opt_in: bool,
    pub nearby_wifi_devices_granted: bool,
}

impl AndroidLocalNetworkPolicy {
    pub fn decision(self) -> AndroidLocalNetworkAccess {
        if self.os_api_level >= 37 && self.target_sdk >= 37 {
            if self.access_local_network_granted {
                return AndroidLocalNetworkAccess::AccessLocalNetworkGranted;
            }
            if self.system_mediated_selected_device {
                return AndroidLocalNetworkAccess::SystemMediatedSelectedDevice;
            }
            return AndroidLocalNetworkAccess::Denied;
        }

        if self.os_api_level == 36
            && self.android16_restrict_local_network_opt_in
        {
            if self.nearby_wifi_devices_granted {
                return AndroidLocalNetworkAccess::Android16OptInGranted;
            }
            if self.system_mediated_selected_device {
                return AndroidLocalNetworkAccess::SystemMediatedSelectedDevice;
            }
            return AndroidLocalNetworkAccess::Denied;
        }

        AndroidLocalNetworkAccess::LegacyUnrestricted
    }

    pub fn allows_arbitrary_lan(self) -> bool {
        matches!(
            self.decision(),
            AndroidLocalNetworkAccess::LegacyUnrestricted
                | AndroidLocalNetworkAccess::AccessLocalNetworkGranted
                | AndroidLocalNetworkAccess::Android16OptInGranted
        )
    }

    pub fn allows_selected_device_lan(self) -> bool {
        !matches!(self.decision(), AndroidLocalNetworkAccess::Denied)
    }

    /// Android local-network protection gates LAN traffic, not ordinary
    /// Internet traffic. This remains true even when LAN access is denied.
    pub fn allows_internet(self) -> bool {
        true
    }

    /// Applies the current sanpham3 architecture boundary.
    ///
    /// Current Wi-Fi Direct/Aware/Local-Only Hotspot peer paths eventually
    /// use app-owned LAN sockets. Until sanpham3 integrates a system-mediated
    /// picker path, selected-device-only access is insufficient for these
    /// arbitrary peer socket carriers.
    pub fn apply_to_project_capabilities(
        self,
        mut capabilities: DeviceCapabilities,
    ) -> DeviceCapabilities {
        if !self.allows_arbitrary_lan() {
            capabilities.wifi_direct = false;
            capabilities.wifi_aware = false;
            capabilities.local_only_hotspot = false;
        }
        capabilities
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidOemQuirkProfile {
    pub background_wifi_peer_unreliable: bool,
    pub background_bluetooth_unreliable: bool,
    pub local_hotspot_unreliable: bool,
    pub deep_doze_peer_radios_unreliable: bool,
}

impl AndroidOemQuirkProfile {
    pub fn reference() -> Self {
        Self {
            background_wifi_peer_unreliable: false,
            background_bluetooth_unreliable: false,
            local_hotspot_unreliable: false,
            deep_doze_peer_radios_unreliable: false,
        }
    }

    pub fn aggressive_background() -> Self {
        Self {
            background_wifi_peer_unreliable: true,
            background_bluetooth_unreliable: true,
            local_hotspot_unreliable: true,
            deep_doze_peer_radios_unreliable: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidExecutionProfile {
    pub app_foreground: bool,
    pub background_restricted: bool,
    pub deep_doze: bool,
    pub battery_saver: bool,
}

impl AndroidExecutionProfile {
    pub fn foreground() -> Self {
        Self {
            app_foreground: true,
            background_restricted: false,
            deep_doze: false,
            battery_saver: false,
        }
    }

    pub fn restricted_background() -> Self {
        Self {
            app_foreground: false,
            background_restricted: true,
            deep_doze: false,
            battery_saver: false,
        }
    }

    pub fn deep_doze() -> Self {
        Self {
            app_foreground: false,
            background_restricted: true,
            deep_doze: true,
            battery_saver: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AndroidThermalLevel {
    Nominal,
    Warm,
    Hot,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidResourceState {
    pub battery_percent: u8,
    pub charging: bool,
    pub thermal: AndroidThermalLevel,
}

impl AndroidResourceState {
    pub fn nominal() -> Self {
        Self {
            battery_percent: 100,
            charging: false,
            thermal: AndroidThermalLevel::Nominal,
        }
    }

    pub fn validate(self) -> Result<(), &'static str> {
        if self.battery_percent > 100 {
            return Err("battery_percent must be <= 100");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AndroidResourceMode {
    Normal,
    Conserve,
    Critical,
}

pub fn android_resource_mode(
    execution: AndroidExecutionProfile,
    resources: AndroidResourceState,
) -> Result<AndroidResourceMode, &'static str> {
    resources.validate()?;

    if resources.thermal == AndroidThermalLevel::Critical
        || (!resources.charging && resources.battery_percent <= 3)
    {
        return Ok(AndroidResourceMode::Critical);
    }

    if resources.thermal >= AndroidThermalLevel::Hot
        || execution.battery_saver
        || (!resources.charging && resources.battery_percent <= 10)
    {
        return Ok(AndroidResourceMode::Conserve);
    }

    Ok(AndroidResourceMode::Normal)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualAndroidPhone {
    pub api_level: u16,
    pub hardware: AndroidHardwareProfile,
    pub permissions: AndroidPermissionProfile,
}

impl VirtualAndroidPhone {
    pub fn capabilities_for_execution(
        &self,
        execution: AndroidExecutionProfile,
        quirks: AndroidOemQuirkProfile,
        resources: AndroidResourceState,
    ) -> Result<DeviceCapabilities, &'static str> {
        let mut caps = self.capabilities();

        if !execution.app_foreground && execution.background_restricted {
            caps.wifi_direct = false;
            caps.wifi_aware = false;
            caps.local_only_hotspot = false;
        }

        if !execution.app_foreground
            && quirks.background_wifi_peer_unreliable
        {
            caps.wifi_direct = false;
            caps.wifi_aware = false;
            caps.local_only_hotspot = false;
        }

        if !execution.app_foreground
            && quirks.background_bluetooth_unreliable
        {
            caps.bluetooth_le = false;
            caps.ble_l2cap_coc = false;
            caps.bluetooth_classic = false;
        }

        if quirks.local_hotspot_unreliable {
            caps.local_only_hotspot = false;
        }

        if execution.deep_doze
            && quirks.deep_doze_peer_radios_unreliable
        {
            caps.wifi_direct = false;
            caps.wifi_aware = false;
            caps.local_only_hotspot = false;
            caps.bluetooth_le = false;
            caps.ble_l2cap_coc = false;
            caps.bluetooth_classic = false;
        }

        match android_resource_mode(execution, resources)? {
            AndroidResourceMode::Normal => {}
            AndroidResourceMode::Conserve => {
                // Conservative policy: nontraditional active transducers are
                // suppressed first. This is a project policy default, not a
                // measured OEM energy claim.
                caps.microphone = false;
                caps.camera = false;
                caps.vibrator = false;
            }
            AndroidResourceMode::Critical => {
                caps.wifi_direct = false;
                caps.wifi_aware = false;
                caps.local_only_hotspot = false;
                caps.ble_l2cap_coc = false;
                caps.bluetooth_classic = false;
                caps.microphone = false;
                caps.camera = false;
                caps.vibrator = false;
            }
        }

        Ok(caps)
    }

    pub fn capabilities(&self) -> DeviceCapabilities {
        let wifi_permission = if self.api_level >= 33 {
            self.permissions.nearby_wifi_devices
        } else {
            self.permissions.legacy_location
        };
        let bluetooth_scan_permission = if self.api_level >= 31 {
            self.permissions.bluetooth_scan
        } else {
            self.permissions.legacy_location
        };
        let bluetooth_connect_permission = if self.api_level >= 31 {
            self.permissions.bluetooth_connect
        } else {
            true
        };

        DeviceCapabilities {
            platform: Platform::Android,
            wifi_direct: self.hardware.wifi_direct && wifi_permission,
            wifi_aware: self.api_level >= 26
                && self.hardware.wifi_aware
                && wifi_permission,
            bluetooth_le: self.hardware.bluetooth_le
                && bluetooth_scan_permission,
            ble_l2cap_coc: self.api_level >= 29
                && self.hardware.bluetooth_le
                && bluetooth_connect_permission,
            bluetooth_classic: self.hardware.bluetooth_classic
                && bluetooth_connect_permission,
            local_only_hotspot: self.api_level >= 26
                && self.hardware.wifi_direct
                && wifi_permission,
            telephony_messaging: self.hardware.telephony_messaging
                && self.permissions.send_sms,
            nfc_hce_or_reader: self.api_level >= 19
                && self.hardware.nfc_hce_or_reader,
            microphone: self.hardware.microphone,
            speaker: self.hardware.speaker,
            camera: self.hardware.camera,
            screen: self.hardware.screen,
            vibrator: self.hardware.vibrator,
            accelerometer: self.hardware.accelerometer,
            magnetometer: self.hardware.magnetometer,
            usb: self.hardware.usb,
            external_os_interface: self.hardware.external_os_interface,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopHardwareProfile {
    pub wifi_direct: bool,
    pub wifi_aware: bool,
    pub bluetooth_le: bool,
    pub bluetooth_classic: bool,
    pub telephony_messaging: bool,
    pub nfc: bool,
    pub microphone: bool,
    pub speaker: bool,
    pub camera: bool,
    pub screen: bool,
    pub vibrator: bool,
    pub accelerometer: bool,
    pub magnetometer: bool,
    pub usb: bool,
    pub external_os_interface: bool,
}

impl DesktopHardwareProfile {
    pub fn broad_laptop() -> Self {
        Self {
            wifi_direct: true,
            wifi_aware: false,
            bluetooth_le: true,
            bluetooth_classic: true,
            telephony_messaging: false,
            nfc: false,
            microphone: true,
            speaker: true,
            camera: true,
            screen: true,
            vibrator: false,
            accelerometer: false,
            magnetometer: false,
            usb: true,
            external_os_interface: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopProjectAdapters {
    pub wifi_direct: bool,
    pub wifi_aware: bool,
    pub bluetooth_le: bool,
    pub ble_l2cap_coc: bool,
    pub bluetooth_classic: bool,
    pub local_only_hotspot: bool,
    pub telephony_messaging: bool,
    pub nfc_hce_or_reader: bool,
    pub acoustic: bool,
    pub optical: bool,
    pub vibration: bool,
    pub magnetic: bool,
    pub usb_peer: bool,
    pub external_os_interface: bool,
}

impl DesktopProjectAdapters {
    /// Conservative adapter inventory for the currently merged Windows host.
    ///
    /// The Windows adapter can inventory/bind/probe ordinary OS network
    /// interfaces, including an already-present external network interface.
    /// No dedicated Windows peer-radio/audio/optical/USB-peer carrier adapter
    /// is merged yet.
    pub fn current_windows() -> Self {
        Self {
            wifi_direct: false,
            wifi_aware: false,
            bluetooth_le: false,
            ble_l2cap_coc: false,
            bluetooth_classic: false,
            local_only_hotspot: false,
            telephony_messaging: false,
            nfc_hce_or_reader: false,
            acoustic: false,
            optical: false,
            vibration: false,
            magnetic: false,
            usb_peer: false,
            external_os_interface: true,
        }
    }

    /// Conservative adapter inventory for the currently merged Linux host.
    ///
    /// Linux currently inventories/routes/binds ordinary interfaces. Special
    /// peer-radio/audio/optical/USB-peer carriers stay disabled until project
    /// adapters exist, even if the machine hardware could theoretically do it.
    pub fn current_linux() -> Self {
        Self {
            external_os_interface: true,
            ..Self::current_windows()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsPermissionProfile {
    pub wifi_peer_access: bool,
    pub bluetooth_access: bool,
    pub audio_capture: bool,
    pub audio_playback: bool,
    pub camera_access: bool,
    pub display_output: bool,
    pub sensor_access: bool,
    pub usb_device_access: bool,
    pub external_interface_access: bool,
}

impl WindowsPermissionProfile {
    pub fn all_granted() -> Self {
        Self {
            wifi_peer_access: true,
            bluetooth_access: true,
            audio_capture: true,
            audio_playback: true,
            camera_access: true,
            display_output: true,
            sensor_access: true,
            usb_device_access: true,
            external_interface_access: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxPrivilegeProfile {
    pub wifi_peer_control: bool,
    pub bluetooth_user_access: bool,
    pub audio_capture: bool,
    pub audio_playback: bool,
    pub video_capture: bool,
    pub display_output: bool,
    pub sensor_access: bool,
    pub usb_device_access: bool,
    pub external_interface_access: bool,
}

impl LinuxPrivilegeProfile {
    pub fn all_granted() -> Self {
        Self {
            wifi_peer_control: true,
            bluetooth_user_access: true,
            audio_capture: true,
            audio_playback: true,
            video_capture: true,
            display_output: true,
            sensor_access: true,
            usb_device_access: true,
            external_interface_access: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualWindowsDevice {
    pub hardware: DesktopHardwareProfile,
    pub permissions: WindowsPermissionProfile,
    pub adapters: DesktopProjectAdapters,
}

impl VirtualWindowsDevice {
    pub fn capabilities(&self) -> DeviceCapabilities {
        DeviceCapabilities {
            platform: Platform::Windows,
            wifi_direct: self.hardware.wifi_direct
                && self.permissions.wifi_peer_access
                && self.adapters.wifi_direct,
            wifi_aware: self.hardware.wifi_aware
                && self.permissions.wifi_peer_access
                && self.adapters.wifi_aware,
            bluetooth_le: self.hardware.bluetooth_le
                && self.permissions.bluetooth_access
                && self.adapters.bluetooth_le,
            ble_l2cap_coc: self.hardware.bluetooth_le
                && self.permissions.bluetooth_access
                && self.adapters.ble_l2cap_coc,
            bluetooth_classic: self.hardware.bluetooth_classic
                && self.permissions.bluetooth_access
                && self.adapters.bluetooth_classic,
            local_only_hotspot: self.hardware.wifi_direct
                && self.permissions.wifi_peer_access
                && self.adapters.local_only_hotspot,
            telephony_messaging: self.hardware.telephony_messaging
                && self.adapters.telephony_messaging,
            nfc_hce_or_reader: self.hardware.nfc
                && self.adapters.nfc_hce_or_reader,
            microphone: self.hardware.microphone
                && self.permissions.audio_capture
                && self.adapters.acoustic,
            speaker: self.hardware.speaker
                && self.permissions.audio_playback
                && self.adapters.acoustic,
            camera: self.hardware.camera
                && self.permissions.camera_access
                && self.adapters.optical,
            screen: self.hardware.screen
                && self.permissions.display_output
                && self.adapters.optical,
            vibrator: self.hardware.vibrator
                && self.permissions.sensor_access
                && self.adapters.vibration,
            accelerometer: self.hardware.accelerometer
                && self.permissions.sensor_access
                && self.adapters.vibration,
            magnetometer: self.hardware.magnetometer
                && self.permissions.sensor_access
                && self.adapters.magnetic,
            usb: self.hardware.usb
                && self.permissions.usb_device_access
                && self.adapters.usb_peer,
            external_os_interface: self.hardware.external_os_interface
                && self.permissions.external_interface_access
                && self.adapters.external_os_interface,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualLinuxDevice {
    pub hardware: DesktopHardwareProfile,
    pub privileges: LinuxPrivilegeProfile,
    pub adapters: DesktopProjectAdapters,
}

impl VirtualLinuxDevice {
    pub fn capabilities(&self) -> DeviceCapabilities {
        DeviceCapabilities {
            platform: Platform::Linux,
            wifi_direct: self.hardware.wifi_direct
                && self.privileges.wifi_peer_control
                && self.adapters.wifi_direct,
            wifi_aware: self.hardware.wifi_aware
                && self.privileges.wifi_peer_control
                && self.adapters.wifi_aware,
            bluetooth_le: self.hardware.bluetooth_le
                && self.privileges.bluetooth_user_access
                && self.adapters.bluetooth_le,
            ble_l2cap_coc: self.hardware.bluetooth_le
                && self.privileges.bluetooth_user_access
                && self.adapters.ble_l2cap_coc,
            bluetooth_classic: self.hardware.bluetooth_classic
                && self.privileges.bluetooth_user_access
                && self.adapters.bluetooth_classic,
            local_only_hotspot: self.hardware.wifi_direct
                && self.privileges.wifi_peer_control
                && self.adapters.local_only_hotspot,
            telephony_messaging: self.hardware.telephony_messaging
                && self.adapters.telephony_messaging,
            nfc_hce_or_reader: self.hardware.nfc
                && self.adapters.nfc_hce_or_reader,
            microphone: self.hardware.microphone
                && self.privileges.audio_capture
                && self.adapters.acoustic,
            speaker: self.hardware.speaker
                && self.privileges.audio_playback
                && self.adapters.acoustic,
            camera: self.hardware.camera
                && self.privileges.video_capture
                && self.adapters.optical,
            screen: self.hardware.screen
                && self.privileges.display_output
                && self.adapters.optical,
            vibrator: self.hardware.vibrator
                && self.privileges.sensor_access
                && self.adapters.vibration,
            accelerometer: self.hardware.accelerometer
                && self.privileges.sensor_access
                && self.adapters.vibration,
            magnetometer: self.hardware.magnetometer
                && self.privileges.sensor_access
                && self.adapters.magnetic,
            usb: self.hardware.usb
                && self.privileges.usb_device_access
                && self.adapters.usb_peer,
            external_os_interface: self.hardware.external_os_interface
                && self.privileges.external_interface_access
                && self.adapters.external_os_interface,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierProfile {
    pub kind: CarrierKind,
    pub evidence: EvidenceClass,
    pub directionality: Directionality,
    pub nominal_bps: u64,
    pub setup_latency: Duration,
    pub one_way_latency: Duration,
    pub max_payload_bytes: Option<u64>,
    pub requires_line_of_sight: bool,
    pub requires_surface_contact: bool,
    pub requires_existing_internet_egress: bool,
    pub requires_extra_hardware: bool,
    pub app_only_candidate: bool,
    pub fresh_remote_capable: bool,
    pub notes: &'static str,
}

impl CarrierProfile {
    pub fn baseline(kind: CarrierKind) -> Self {
        match kind {
            CarrierKind::InternetIp => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::FullDuplex,
                nominal_bps: 1_000_000,
                setup_latency: Duration::from_millis(100),
                one_way_latency: Duration::from_millis(50),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: true,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: true,
                notes: "Ordinary IP path; may be weak/intermittent in real use.",
            },
            CarrierKind::WifiDirect => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::FullDuplex,
                nominal_bps: 5_000_000,
                setup_latency: Duration::from_secs(2),
                one_way_latency: Duration::from_millis(20),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Peer carrier; fresh Internet requires some peer to have egress.",
            },
            CarrierKind::WifiAware => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::FullDuplex,
                nominal_bps: 5_000_000,
                setup_latency: Duration::from_secs(1),
                one_way_latency: Duration::from_millis(20),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Peer carrier; Android hardware/availability varies.",
            },
            CarrierKind::BluetoothLeAdvertisement => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::OneWay,
                nominal_bps: 10,
                setup_latency: Duration::from_millis(100),
                one_way_latency: Duration::from_millis(50),
                max_payload_bytes: Some(12),
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Tiny broadcast/control fallback using current sanpham3 service-data budget.",
            },
            CarrierKind::BluetoothLeGatt => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::HalfDuplex,
                nominal_bps: 1_000,
                setup_latency: Duration::from_secs(2),
                one_way_latency: Duration::from_millis(80),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Conservative GATT fallback model; measured project goodput still required.",
            },
            CarrierKind::BluetoothLeL2cap => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::FullDuplex,
                nominal_bps: 100_000,
                setup_latency: Duration::from_secs(2),
                one_way_latency: Duration::from_millis(50),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Current Android tiny-stream peer carrier in sanpham3.",
            },
            CarrierKind::BluetoothRfcomm => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::FullDuplex,
                nominal_bps: 200_000,
                setup_latency: Duration::from_secs(2),
                one_way_latency: Duration::from_millis(50),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Bluetooth Classic RFCOMM stream candidate; pairing/permissions apply.",
            },
            CarrierKind::LocalOnlyHotspot => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::FullDuplex,
                nominal_bps: 5_000_000,
                setup_latency: Duration::from_secs(3),
                one_way_latency: Duration::from_millis(20),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Android local-only hotspot creates a peer LAN with no Internet by itself.",
            },
            CarrierKind::CellularSms => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::HalfDuplex,
                nominal_bps: 1,
                setup_latency: Duration::from_secs(5),
                one_way_latency: Duration::from_secs(5),
                max_payload_bytes: Some(120),
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Fallback when packet data is unavailable but carrier messaging still works; 120-byte project data-message budget is conservative prototype accounting, not a universal carrier guarantee; permission, subscription, consent, cost and carrier policy apply.",
            },
            CarrierKind::NfcHce => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::HalfDuplex,
                nominal_bps: 106_000,
                setup_latency: Duration::from_millis(200),
                one_way_latency: Duration::from_millis(50),
                max_payload_bytes: Some(1024),
                requires_line_of_sight: false,
                requires_surface_contact: true,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Android reader/HCE APDU exchange; very short range.",
            },
            CarrierKind::AcousticNearUltrasonic => Self {
                kind,
                evidence: EvidenceClass::ResearchDemonstrated,
                directionality: Directionality::HalfDuplex,
                nominal_bps: 4_000,
                setup_latency: Duration::from_millis(500),
                one_way_latency: Duration::from_millis(20),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Uses built-in speaker/microphone; rate is research-derived, not guaranteed.",
            },
            CarrierKind::OpticalScreenCamera => Self {
                kind,
                evidence: EvidenceClass::ResearchDemonstrated,
                directionality: Directionality::HalfDuplex,
                nominal_bps: 100,
                setup_latency: Duration::from_secs(1),
                one_way_latency: Duration::from_millis(50),
                max_payload_bytes: None,
                requires_line_of_sight: true,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Screen-camera optical carrier; conservative placeholder until measured in-project.",
            },
            CarrierKind::VibrationSurface => Self {
                kind,
                evidence: EvidenceClass::ResearchDemonstrated,
                directionality: Directionality::HalfDuplex,
                nominal_bps: 2,
                setup_latency: Duration::from_secs(1),
                one_way_latency: Duration::from_millis(100),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: true,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Vibrator-to-accelerometer carrier over shared surface; experimental.",
            },
            CarrierKind::MagneticSensor => Self {
                kind,
                evidence: EvidenceClass::ResearchDemonstrated,
                directionality: Directionality::OneWay,
                nominal_bps: 1,
                setup_latency: Duration::from_secs(2),
                one_way_latency: Duration::from_millis(100),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: false,
                fresh_remote_capable: false,
                notes: "Air-gap research reference; current evidence is not a phone-to-phone product carrier.",
            },
            CarrierKind::UsbLocal => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::FullDuplex,
                nominal_bps: 10_000_000,
                setup_latency: Duration::from_millis(500),
                one_way_latency: Duration::from_millis(5),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: true,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Uses already-connected USB path; app itself requires no custom radio.",
            },
            CarrierKind::ExternalOsInterface => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::FullDuplex,
                nominal_bps: 10_000_000,
                setup_latency: Duration::from_millis(500),
                one_way_latency: Duration::from_millis(10),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: true,
                requires_extra_hardware: true,
                app_only_candidate: false,
                fresh_remote_capable: true,
                notes: "Optional user-owned hardware already exposed by the OS (for example an attached network/tether interface). It may be scavenged when present but never becomes a product requirement or an app-only proof.",
            },
            CarrierKind::PhysicalDataMule => Self {
                kind,
                evidence: EvidenceClass::ProductionApi,
                directionality: Directionality::HalfDuplex,
                nominal_bps: 1,
                setup_latency: Duration::from_secs(60),
                one_way_latency: Duration::from_secs(60),
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: true,
                notes: "Store-carry-forward: information arrives when a device later meets an egress.",
            },
            CarrierKind::LocalCacheTwin => Self {
                kind,
                evidence: EvidenceClass::LocalOnly,
                directionality: Directionality::FullDuplex,
                nominal_bps: u64::MAX,
                setup_latency: Duration::ZERO,
                one_way_latency: Duration::ZERO,
                max_payload_bytes: None,
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "Local cache/service twin. Useful with no carrier, but never fresh remote truth.",
            },
            CarrierKind::None => Self {
                kind,
                evidence: EvidenceClass::ImpossibleFreshRemote,
                directionality: Directionality::OneWay,
                nominal_bps: 0,
                setup_latency: Duration::ZERO,
                one_way_latency: Duration::ZERO,
                max_payload_bytes: Some(0),
                requires_line_of_sight: false,
                requires_surface_contact: false,
                requires_existing_internet_egress: false,
                requires_extra_hardware: false,
                app_only_candidate: true,
                fresh_remote_capable: false,
                notes: "No physical/information path: fresh remote information is impossible.",
            },
        }
    }

    pub fn eligible_for_core_app_only_claim(&self) -> bool {
        self.app_only_candidate && !self.requires_extra_hardware
    }

    pub fn supported_by(&self, device: &DeviceCapabilities) -> bool {
        match self.kind {
            CarrierKind::InternetIp => true,
            CarrierKind::WifiDirect => device.wifi_direct,
            CarrierKind::WifiAware => device.wifi_aware,
            CarrierKind::BluetoothLeAdvertisement
            | CarrierKind::BluetoothLeGatt => device.bluetooth_le,
            CarrierKind::BluetoothLeL2cap => device.ble_l2cap_coc,
            CarrierKind::BluetoothRfcomm => device.bluetooth_classic,
            CarrierKind::LocalOnlyHotspot => device.local_only_hotspot,
            CarrierKind::CellularSms => device.telephony_messaging,
            CarrierKind::NfcHce => device.nfc_hce_or_reader,
            CarrierKind::AcousticNearUltrasonic => device.microphone && device.speaker,
            CarrierKind::OpticalScreenCamera => device.camera && device.screen,
            CarrierKind::VibrationSurface => device.vibrator && device.accelerometer,
            CarrierKind::MagneticSensor => device.magnetometer,
            CarrierKind::UsbLocal => device.usb,
            CarrierKind::ExternalOsInterface => device.external_os_interface,
            CarrierKind::PhysicalDataMule | CarrierKind::LocalCacheTwin | CarrierKind::None => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InformationTask {
    pub request_bits: u64,
    pub response_bits: u64,
    pub require_fresh_remote: bool,
    pub tolerate_delay: bool,
    pub allow_generated: bool,
}

impl InformationTask {
    pub fn tiny_fresh_query() -> Self {
        Self {
            request_bits: 128,
            response_bits: 512,
            require_fresh_remote: true,
            tolerate_delay: true,
            allow_generated: false,
        }
    }

    pub fn total_bits(&self) -> u64 {
        self.request_bits.saturating_add(self.response_bits)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactWindow {
    pub carrier: CarrierProfile,
    pub duration: Duration,
    pub peer_has_internet_egress: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScavengeStep {
    pub carrier: CarrierKind,
    pub window: Duration,
    pub delivered_bits: u64,
    pub cumulative_bits: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScavengeOutcome {
    pub feasible: bool,
    pub completed_at: Option<Duration>,
    pub delivered_bits: u64,
    pub required_bits: u64,
    pub freshness: Option<InformationFreshness>,
    pub steps: Vec<ScavengeStep>,
    pub reason: &'static str,
}

/// Models app-layer fragment accumulation across intermittent contacts.
///
/// This is intentionally not transparent TCP striping. Each window simply
/// contributes authenticated application fragments to the same task until the
/// minimum information product is complete.
pub fn scavenge_across_contacts(
    device: &DeviceCapabilities,
    task: &InformationTask,
    windows: &[ContactWindow],
) -> ScavengeOutcome {
    let required_bits = task.total_bits();
    let mut delivered_bits = 0_u64;
    let mut elapsed = Duration::ZERO;
    let mut steps = Vec::new();
    let mut any_remote_egress = false;

    for window in windows {
        elapsed = elapsed.saturating_add(window.carrier.setup_latency);

        if !window.carrier.supported_by(device)
            || window.carrier.kind == CarrierKind::None
            || window.carrier.kind == CarrierKind::LocalCacheTwin
            || window.carrier.nominal_bps == 0
        {
            elapsed = elapsed.saturating_add(window.duration);
            steps.push(ScavengeStep {
                carrier: window.carrier.kind,
                window: window.duration,
                delivered_bits: 0,
                cumulative_bits: delivered_bits,
            });
            continue;
        }

        any_remote_egress |= window.carrier.fresh_remote_capable
            || window.peer_has_internet_egress;

        let capacity = (u128::from(window.carrier.nominal_bps)
            .saturating_mul(window.duration.as_nanos())
            / 1_000_000_000_u128)
            .min(u128::from(u64::MAX)) as u64;

        let remaining = required_bits.saturating_sub(delivered_bits);
        let delivered = capacity.min(remaining);
        delivered_bits = delivered_bits.saturating_add(delivered);

        steps.push(ScavengeStep {
            carrier: window.carrier.kind,
            window: window.duration,
            delivered_bits: delivered,
            cumulative_bits: delivered_bits,
        });

        if delivered_bits >= required_bits {
            let freshness = if task.require_fresh_remote {
                if !any_remote_egress {
                    return ScavengeOutcome {
                        feasible: false,
                        completed_at: None,
                        delivered_bits,
                        required_bits,
                        freshness: None,
                        steps,
                        reason:
                            "bits accumulated, but no contributing contact had Internet egress",
                    };
                }
                InformationFreshness::FreshRemote
            } else {
                InformationFreshness::DelayedRemote
            };

            return ScavengeOutcome {
                feasible: true,
                completed_at: Some(elapsed.saturating_add(window.duration)),
                delivered_bits,
                required_bits,
                freshness: Some(freshness),
                steps,
                reason: "task completed by app-layer fragment accumulation",
            };
        }

        elapsed = elapsed.saturating_add(window.duration);
    }

    ScavengeOutcome {
        feasible: false,
        completed_at: None,
        delivered_bits,
        required_bits,
        freshness: None,
        steps,
        reason: "contact windows ended before enough task bits were accumulated",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationOutcome {
    pub carrier: CarrierKind,
    pub feasible: bool,
    pub freshness: Option<InformationFreshness>,
    pub completion_time: Option<Duration>,
    pub reason: &'static str,
}

pub fn simulate_single_carrier(
    carrier: &CarrierProfile,
    device: &DeviceCapabilities,
    task: &InformationTask,
    peer_has_internet_egress: bool,
    cached_answer_available: bool,
) -> SimulationOutcome {
    if !carrier.supported_by(device) {
        return SimulationOutcome {
            carrier: carrier.kind,
            feasible: false,
            freshness: None,
            completion_time: None,
            reason: "device/API capability unavailable",
        };
    }

    if carrier.kind == CarrierKind::None {
        return no_carrier_outcome(task, cached_answer_available);
    }

    if carrier.kind == CarrierKind::LocalCacheTwin {
        if cached_answer_available {
            return SimulationOutcome {
                carrier: carrier.kind,
                feasible: !task.require_fresh_remote,
                freshness: Some(InformationFreshness::CachedRemote),
                completion_time: Some(Duration::ZERO),
                reason: if task.require_fresh_remote {
                    "cache exists but cannot satisfy fresh-remote requirement"
                } else {
                    "served from local cache/service twin"
                },
            };
        }

        return SimulationOutcome {
            carrier: carrier.kind,
            feasible: task.allow_generated && !task.require_fresh_remote,
            freshness: task
                .allow_generated
                .then_some(InformationFreshness::LocallyGenerated),
            completion_time: task.allow_generated.then_some(Duration::ZERO),
            reason: if task.allow_generated {
                "locally generated answer; not remote freshness"
            } else {
                "no cached answer and generation is disallowed"
            },
        };
    }

    let peer_or_carrier_reaches_internet =
        carrier.fresh_remote_capable || peer_has_internet_egress;

    if task.require_fresh_remote && !peer_or_carrier_reaches_internet {
        return SimulationOutcome {
            carrier: carrier.kind,
            feasible: false,
            freshness: None,
            completion_time: None,
            reason: "carrier can move bits locally but no endpoint has Internet egress",
        };
    }

    if carrier.kind == CarrierKind::PhysicalDataMule && !task.tolerate_delay {
        return SimulationOutcome {
            carrier: carrier.kind,
            feasible: false,
            freshness: None,
            completion_time: None,
            reason: "task forbids store-carry-forward delay",
        };
    }

    if carrier.nominal_bps == 0 {
        return SimulationOutcome {
            carrier: carrier.kind,
            feasible: false,
            freshness: None,
            completion_time: None,
            reason: "carrier has zero modeled capacity",
        };
    }

    if let Some(limit) = carrier.max_payload_bytes {
        let task_bytes = task.total_bits().div_ceil(8);
        if task_bytes > limit {
            return SimulationOutcome {
                carrier: carrier.kind,
                feasible: false,
                freshness: None,
                completion_time: None,
                reason: "task exceeds modeled carrier payload limit",
            };
        }
    }

    let serialization = if carrier.nominal_bps == u64::MAX {
        Duration::ZERO
    } else {
        duration_from_fraction(task.total_bits(), carrier.nominal_bps)
    };

    let completion = carrier
        .setup_latency
        .saturating_add(carrier.one_way_latency.saturating_mul(2))
        .saturating_add(serialization);

    let freshness = if carrier.kind == CarrierKind::PhysicalDataMule {
        InformationFreshness::DelayedRemote
    } else if task.require_fresh_remote {
        InformationFreshness::FreshRemote
    } else {
        InformationFreshness::DelayedRemote
    };

    SimulationOutcome {
        carrier: carrier.kind,
        feasible: true,
        freshness: Some(freshness),
        completion_time: Some(completion),
        reason: "modeled carrier can complete task under stated assumptions",
    }
}

pub fn rank_candidates(
    profiles: &[CarrierProfile],
    device: &DeviceCapabilities,
    task: &InformationTask,
    peer_has_internet_egress: bool,
    cached_answer_available: bool,
) -> Vec<SimulationOutcome> {
    let mut outcomes = profiles
        .iter()
        .map(|profile| {
            simulate_single_carrier(
                profile,
                device,
                task,
                peer_has_internet_egress,
                cached_answer_available,
            )
        })
        .collect::<Vec<_>>();

    outcomes.sort_by(|left, right| match (left.feasible, right.feasible) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => left
            .completion_time
            .cmp(&right.completion_time)
            .then_with(|| format!("{:?}", left.carrier).cmp(&format!("{:?}", right.carrier))),
    });

    outcomes
}

fn no_carrier_outcome(
    task: &InformationTask,
    cached_answer_available: bool,
) -> SimulationOutcome {
    if task.require_fresh_remote {
        return SimulationOutcome {
            carrier: CarrierKind::None,
            feasible: false,
            freshness: None,
            completion_time: None,
            reason: "fresh remote information cannot cross a zero-information channel",
        };
    }

    if cached_answer_available {
        return SimulationOutcome {
            carrier: CarrierKind::None,
            feasible: true,
            freshness: Some(InformationFreshness::CachedRemote),
            completion_time: Some(Duration::ZERO),
            reason: "no carrier; only previously cached information is available",
        };
    }

    if task.allow_generated {
        return SimulationOutcome {
            carrier: CarrierKind::None,
            feasible: true,
            freshness: Some(InformationFreshness::LocallyGenerated),
            completion_time: Some(Duration::ZERO),
            reason: "no carrier; locally generated information only",
        };
    }

    SimulationOutcome {
        carrier: CarrierKind::None,
        feasible: false,
        freshness: None,
        completion_time: None,
        reason: "no carrier and no admissible local information source",
    }
}

fn duration_from_fraction(bits: u64, bps: u64) -> Duration {
    let nanos = u128::from(bits)
        .saturating_mul(1_000_000_000)
        .div_ceil(u128::from(bps.max(1)));
    let secs = (nanos / 1_000_000_000).min(u128::from(u64::MAX)) as u64;
    Duration::new(secs, (nanos % 1_000_000_000) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_android_phone_models_api_and_permission_gates() {
        let hardware = AndroidHardwareProfile::broad_phone();
        let permissions = AndroidPermissionProfile::all_granted();

        let api28 = VirtualAndroidPhone {
            api_level: 28,
            hardware,
            permissions,
        }
        .capabilities();
        assert!(api28.bluetooth_le);
        assert!(!api28.ble_l2cap_coc);
        assert!(api28.wifi_aware);

        let api29 = VirtualAndroidPhone {
            api_level: 29,
            hardware,
            permissions,
        }
        .capabilities();
        assert!(api29.ble_l2cap_coc);

        let mut denied = permissions;
        denied.nearby_wifi_devices = false;
        let api33 = VirtualAndroidPhone {
            api_level: 33,
            hardware,
            permissions: denied,
        }
        .capabilities();
        assert!(!api33.wifi_direct);
        assert!(!api33.wifi_aware);
        assert!(!api33.local_only_hotspot);
    }

    #[test]
    fn android17_target37_denies_arbitrary_lan_without_permission() {
        let policy = AndroidLocalNetworkPolicy {
            os_api_level: 37,
            target_sdk: 37,
            access_local_network_granted: false,
            system_mediated_selected_device: false,
            android16_restrict_local_network_opt_in: false,
            nearby_wifi_devices_granted: true,
        };

        assert_eq!(
            policy.decision(),
            AndroidLocalNetworkAccess::Denied,
        );
        assert!(!policy.allows_arbitrary_lan());
        assert!(!policy.allows_selected_device_lan());
        assert!(policy.allows_internet());

        let caps = policy.apply_to_project_capabilities(
            VirtualAndroidPhone {
                api_level: 37,
                hardware: AndroidHardwareProfile::broad_phone(),
                permissions: AndroidPermissionProfile::all_granted(),
            }
            .capabilities(),
        );
        assert!(!caps.wifi_direct);
        assert!(!caps.wifi_aware);
        assert!(!caps.local_only_hotspot);
        assert!(caps.bluetooth_le);
    }

    #[test]
    fn android17_permission_restores_arbitrary_lan() {
        let policy = AndroidLocalNetworkPolicy {
            os_api_level: 37,
            target_sdk: 37,
            access_local_network_granted: true,
            system_mediated_selected_device: false,
            android16_restrict_local_network_opt_in: false,
            nearby_wifi_devices_granted: true,
        };

        assert_eq!(
            policy.decision(),
            AndroidLocalNetworkAccess::AccessLocalNetworkGranted,
        );
        assert!(policy.allows_arbitrary_lan());
        assert!(policy.allows_selected_device_lan());
    }

    #[test]
    fn system_picker_does_not_become_broad_lan_permission() {
        let policy = AndroidLocalNetworkPolicy {
            os_api_level: 37,
            target_sdk: 37,
            access_local_network_granted: false,
            system_mediated_selected_device: true,
            android16_restrict_local_network_opt_in: false,
            nearby_wifi_devices_granted: true,
        };

        assert_eq!(
            policy.decision(),
            AndroidLocalNetworkAccess::SystemMediatedSelectedDevice,
        );
        assert!(!policy.allows_arbitrary_lan());
        assert!(policy.allows_selected_device_lan());

        let caps = policy.apply_to_project_capabilities(
            VirtualAndroidPhone {
                api_level: 37,
                hardware: AndroidHardwareProfile::broad_phone(),
                permissions: AndroidPermissionProfile::all_granted(),
            }
            .capabilities(),
        );
        assert!(!caps.wifi_direct);
        assert!(!caps.wifi_aware);
        assert!(!caps.local_only_hotspot);
    }

    #[test]
    fn target36_on_android17_keeps_legacy_lan_access() {
        let policy = AndroidLocalNetworkPolicy {
            os_api_level: 37,
            target_sdk: 36,
            access_local_network_granted: false,
            system_mediated_selected_device: false,
            android16_restrict_local_network_opt_in: false,
            nearby_wifi_devices_granted: false,
        };

        assert_eq!(
            policy.decision(),
            AndroidLocalNetworkAccess::LegacyUnrestricted,
        );
        assert!(policy.allows_arbitrary_lan());
    }

    #[test]
    fn android16_opt_in_uses_nearby_wifi_compatibility_gate() {
        let denied = AndroidLocalNetworkPolicy {
            os_api_level: 36,
            target_sdk: 36,
            access_local_network_granted: false,
            system_mediated_selected_device: false,
            android16_restrict_local_network_opt_in: true,
            nearby_wifi_devices_granted: false,
        };
        assert_eq!(denied.decision(), AndroidLocalNetworkAccess::Denied);
        assert!(!denied.allows_arbitrary_lan());
        assert!(denied.allows_internet());

        let allowed = AndroidLocalNetworkPolicy {
            nearby_wifi_devices_granted: true,
            ..denied
        };
        assert_eq!(
            allowed.decision(),
            AndroidLocalNetworkAccess::Android16OptInGranted,
        );
        assert!(allowed.allows_arbitrary_lan());
    }

    #[test]
    fn android_execution_constraints_only_remove_capabilities() {
        let phone = VirtualAndroidPhone {
            api_level: 36,
            hardware: AndroidHardwareProfile::broad_phone(),
            permissions: AndroidPermissionProfile::all_granted(),
        };
        let base = phone.capabilities();

        let constrained = phone
            .capabilities_for_execution(
                AndroidExecutionProfile::restricted_background(),
                AndroidOemQuirkProfile::aggressive_background(),
                AndroidResourceState {
                    battery_percent: 8,
                    charging: false,
                    thermal: AndroidThermalLevel::Hot,
                },
            )
            .unwrap();

        assert!(!constrained.wifi_direct);
        assert!(!constrained.wifi_aware);
        assert!(!constrained.bluetooth_le);
        assert!(!constrained.bluetooth_classic);
        assert!(!constrained.microphone);
        assert!(!constrained.camera);
        assert!(!constrained.vibrator);

        assert!(!constrained.wifi_direct || base.wifi_direct);
        assert!(!constrained.bluetooth_le || base.bluetooth_le);
        assert!(!constrained.microphone || base.microphone);
        assert!(!constrained.camera || base.camera);
        assert!(!constrained.usb || base.usb);
    }

    #[test]
    fn clean_foreground_profile_preserves_base_capabilities() {
        let phone = VirtualAndroidPhone {
            api_level: 36,
            hardware: AndroidHardwareProfile::broad_phone(),
            permissions: AndroidPermissionProfile::all_granted(),
        };
        let base = phone.capabilities();
        let effective = phone
            .capabilities_for_execution(
                AndroidExecutionProfile::foreground(),
                AndroidOemQuirkProfile::reference(),
                AndroidResourceState::nominal(),
            )
            .unwrap();

        assert_eq!(effective, base);
    }

    #[test]
    fn critical_resource_mode_blocks_high_cost_carriers_conservatively() {
        let phone = VirtualAndroidPhone {
            api_level: 36,
            hardware: AndroidHardwareProfile::broad_phone(),
            permissions: AndroidPermissionProfile::all_granted(),
        };

        let critical = phone
            .capabilities_for_execution(
                AndroidExecutionProfile::foreground(),
                AndroidOemQuirkProfile::reference(),
                AndroidResourceState {
                    battery_percent: 2,
                    charging: false,
                    thermal: AndroidThermalLevel::Nominal,
                },
            )
            .unwrap();

        assert!(!critical.wifi_direct);
        assert!(!critical.ble_l2cap_coc);
        assert!(!critical.bluetooth_classic);
        assert!(!CarrierProfile::baseline(
            CarrierKind::AcousticNearUltrasonic,
        )
        .supported_by(&critical));
        assert!(!CarrierProfile::baseline(
            CarrierKind::OpticalScreenCamera,
        )
        .supported_by(&critical));
        assert!(!CarrierProfile::baseline(
            CarrierKind::VibrationSurface,
        )
        .supported_by(&critical));
        assert!(critical.usb);
    }

    #[test]
    fn charging_prevents_low_battery_alone_from_forcing_conserve_mode() {
        let mode = android_resource_mode(
            AndroidExecutionProfile::foreground(),
            AndroidResourceState {
                battery_percent: 2,
                charging: true,
                thermal: AndroidThermalLevel::Nominal,
            },
        )
        .unwrap();
        assert_eq!(mode, AndroidResourceMode::Normal);
    }

    #[test]
    fn battery_percent_above_100_is_rejected() {
        assert!(android_resource_mode(
            AndroidExecutionProfile::foreground(),
            AndroidResourceState {
                battery_percent: 101,
                charging: false,
                thermal: AndroidThermalLevel::Nominal,
            },
        )
        .is_err());
    }

    #[test]
    fn sms_fallback_requires_permission_and_telephony() {
        let hardware = AndroidHardwareProfile::broad_phone();
        let mut permissions = AndroidPermissionProfile::all_granted();
        permissions.send_sms = false;

        let denied = VirtualAndroidPhone {
            api_level: 36,
            hardware,
            permissions,
        }
        .capabilities();
        assert!(!denied.telephony_messaging);

        let allowed = VirtualAndroidPhone {
            api_level: 36,
            hardware,
            permissions: AndroidPermissionProfile::all_granted(),
        }
        .capabilities();
        assert!(allowed.telephony_messaging);
    }

    #[test]
    fn current_windows_profile_does_not_invent_unimplemented_carriers() {
        let caps = VirtualWindowsDevice {
            hardware: DesktopHardwareProfile::broad_laptop(),
            permissions: WindowsPermissionProfile::all_granted(),
            adapters: DesktopProjectAdapters::current_windows(),
        }
        .capabilities();

        assert_eq!(caps.platform, Platform::Windows);
        assert!(caps.external_os_interface);
        assert!(!caps.wifi_direct);
        assert!(!caps.bluetooth_le);
        assert!(!caps.bluetooth_classic);
        assert!(!caps.microphone);
        assert!(!caps.camera);
        assert!(!caps.usb);
    }

    #[test]
    fn windows_permission_and_project_adapter_both_gate_carriers() {
        let mut adapters = DesktopProjectAdapters::current_windows();
        adapters.acoustic = true;
        adapters.optical = true;
        adapters.usb_peer = true;

        let mut permissions = WindowsPermissionProfile::all_granted();
        permissions.audio_capture = false;

        let caps = VirtualWindowsDevice {
            hardware: DesktopHardwareProfile::broad_laptop(),
            permissions,
            adapters,
        }
        .capabilities();

        assert!(!caps.microphone);
        assert!(caps.speaker);
        assert!(caps.camera);
        assert!(caps.screen);
        assert!(caps.usb);
        assert!(!CarrierProfile::baseline(
            CarrierKind::AcousticNearUltrasonic,
        )
        .supported_by(&caps));
        assert!(CarrierProfile::baseline(
            CarrierKind::OpticalScreenCamera,
        )
        .supported_by(&caps));
    }

    #[test]
    fn current_linux_profile_is_conservative_and_privilege_aware() {
        let current = VirtualLinuxDevice {
            hardware: DesktopHardwareProfile::broad_laptop(),
            privileges: LinuxPrivilegeProfile::all_granted(),
            adapters: DesktopProjectAdapters::current_linux(),
        }
        .capabilities();

        assert_eq!(current.platform, Platform::Linux);
        assert!(current.external_os_interface);
        assert!(!current.bluetooth_le);
        assert!(!current.usb);

        let mut adapters = DesktopProjectAdapters::current_linux();
        adapters.usb_peer = true;
        let mut privileges = LinuxPrivilegeProfile::all_granted();
        privileges.usb_device_access = false;

        let denied = VirtualLinuxDevice {
            hardware: DesktopHardwareProfile::broad_laptop(),
            privileges,
            adapters,
        }
        .capabilities();
        assert!(!denied.usb);
    }

    #[test]
    fn bluetooth_carriers_share_one_correlated_failure_domain() {
        let scenario = FailureScenario {
            failed_domains: vec![FailureDomain::BluetoothRadio],
        };

        assert!(!scenario.carrier_survives(CarrierKind::BluetoothLeGatt));
        assert!(!scenario.carrier_survives(CarrierKind::BluetoothLeL2cap));
        assert!(!scenario.carrier_survives(CarrierKind::BluetoothRfcomm));
        assert!(scenario.carrier_survives(CarrierKind::WifiDirect));
        assert!(scenario.carrier_survives(CarrierKind::OpticalScreenCamera));
    }

    #[test]
    fn wifi_peer_carriers_are_not_counted_as_independent_radios() {
        assert_eq!(
            primary_failure_domain(CarrierKind::WifiDirect),
            FailureDomain::WifiRadio,
        );
        assert_eq!(
            primary_failure_domain(CarrierKind::WifiAware),
            FailureDomain::WifiRadio,
        );
        assert_eq!(
            primary_failure_domain(CarrierKind::LocalOnlyHotspot),
            FailureDomain::WifiRadio,
        );
    }

    #[test]
    fn redundancy_planner_prefers_distinct_failure_domains() {
        let mut device = DeviceCapabilities::conservative_android();
        device.ble_l2cap_coc = true;
        device.nfc_hce_or_reader = true;

        let profiles = [
            CarrierProfile {
                nominal_bps: 50_000,
                ..CarrierProfile::baseline(CarrierKind::BluetoothLeL2cap)
            },
            CarrierProfile {
                nominal_bps: 40_000,
                ..CarrierProfile::baseline(CarrierKind::BluetoothRfcomm)
            },
            CarrierProfile {
                nominal_bps: 30_000,
                ..CarrierProfile::baseline(CarrierKind::BluetoothLeGatt)
            },
            CarrierProfile {
                nominal_bps: 20_000,
                ..CarrierProfile::baseline(CarrierKind::WifiDirect)
            },
            CarrierProfile {
                nominal_bps: 5_000,
                ..CarrierProfile::baseline(CarrierKind::NfcHce)
            },
        ];

        let selected =
            choose_failure_diverse_carriers(&profiles, &device, 3);

        assert_eq!(selected.carriers[0], CarrierKind::BluetoothLeL2cap);
        assert!(selected.carriers.contains(&CarrierKind::WifiDirect));
        assert!(selected.carriers.contains(&CarrierKind::NfcHce));
        assert_eq!(selected.distinct_failure_domains, 3);
    }

    #[test]
    fn redundancy_planner_reuses_domain_only_after_diverse_options_exhausted() {
        let mut device = DeviceCapabilities::conservative_android();
        device.ble_l2cap_coc = true;

        let profiles = [
            CarrierProfile::baseline(CarrierKind::BluetoothLeL2cap),
            CarrierProfile::baseline(CarrierKind::BluetoothLeGatt),
            CarrierProfile::baseline(CarrierKind::BluetoothRfcomm),
        ];

        let selected =
            choose_failure_diverse_carriers(&profiles, &device, 3);

        assert_eq!(selected.carriers.len(), 3);
        assert_eq!(selected.distinct_failure_domains, 1);
    }

    #[test]
    fn optional_external_os_interface_can_help_without_redefining_core_product() {
        let device = DeviceCapabilities {
            external_os_interface: true,
            ..DeviceCapabilities::conservative_android()
        };
        let carrier = CarrierProfile::baseline(CarrierKind::ExternalOsInterface);
        let outcome = simulate_single_carrier(
            &carrier,
            &device,
            &InformationTask::tiny_fresh_query(),
            false,
            false,
        );

        assert!(outcome.feasible);
        assert!(carrier.requires_extra_hardware);
        assert!(!carrier.eligible_for_core_app_only_claim());
    }

    #[test]
    fn zero_carrier_cannot_produce_fresh_remote_information() {
        let device = DeviceCapabilities::conservative_android();
        let task = InformationTask::tiny_fresh_query();

        let outcome = simulate_single_carrier(
            &CarrierProfile::baseline(CarrierKind::None),
            &device,
            &task,
            false,
            true,
        );

        assert!(!outcome.feasible);
        assert_eq!(outcome.freshness, None);
    }

    #[test]
    fn cache_twin_can_serve_only_nonfresh_contract() {
        let device = DeviceCapabilities::conservative_android();
        let task = InformationTask {
            require_fresh_remote: false,
            allow_generated: false,
            ..InformationTask::tiny_fresh_query()
        };

        let outcome = simulate_single_carrier(
            &CarrierProfile::baseline(CarrierKind::LocalCacheTwin),
            &device,
            &task,
            false,
            true,
        );

        assert!(outcome.feasible);
        assert_eq!(
            outcome.freshness,
            Some(InformationFreshness::CachedRemote)
        );
    }

    #[test]
    fn acoustic_carrier_becomes_useful_if_peer_has_egress() {
        let device = DeviceCapabilities::conservative_android();
        let task = InformationTask::tiny_fresh_query();

        let no_egress = simulate_single_carrier(
            &CarrierProfile::baseline(
                CarrierKind::AcousticNearUltrasonic,
            ),
            &device,
            &task,
            false,
            false,
        );
        assert!(!no_egress.feasible);

        let with_egress = simulate_single_carrier(
            &CarrierProfile::baseline(
                CarrierKind::AcousticNearUltrasonic,
            ),
            &device,
            &task,
            true,
            false,
        );
        assert!(with_egress.feasible);
        assert_eq!(
            with_egress.freshness,
            Some(InformationFreshness::FreshRemote)
        );
    }

    #[test]
    fn unsupported_hardware_is_rejected() {
        let device = DeviceCapabilities {
            nfc_hce_or_reader: false,
            ..DeviceCapabilities::conservative_android()
        };

        let outcome = simulate_single_carrier(
            &CarrierProfile::baseline(CarrierKind::NfcHce),
            &device,
            &InformationTask::tiny_fresh_query(),
            true,
            false,
        );

        assert!(!outcome.feasible);
    }

    #[test]
    fn delayed_data_mule_respects_delay_contract() {
        let device = DeviceCapabilities::conservative_android();
        let mut task = InformationTask::tiny_fresh_query();
        task.tolerate_delay = false;

        let blocked = simulate_single_carrier(
            &CarrierProfile::baseline(CarrierKind::PhysicalDataMule),
            &device,
            &task,
            true,
            false,
        );
        assert!(!blocked.feasible);

        task.tolerate_delay = true;
        let allowed = simulate_single_carrier(
            &CarrierProfile::baseline(CarrierKind::PhysicalDataMule),
            &device,
            &task,
            true,
            false,
        );
        assert!(allowed.feasible);
        assert_eq!(
            allowed.freshness,
            Some(InformationFreshness::DelayedRemote)
        );
    }

    #[test]
    fn fragments_can_accumulate_across_different_contacts() {
        let mut device = DeviceCapabilities::conservative_android();
        device.ble_l2cap_coc = true;
        let task = InformationTask {
            request_bits: 40,
            response_bits: 80,
            require_fresh_remote: true,
            tolerate_delay: true,
            allow_generated: false,
        };

        let windows = vec![
            ContactWindow {
                carrier: CarrierProfile {
                    nominal_bps: 10,
                    setup_latency: Duration::ZERO,
                    ..CarrierProfile::baseline(
                        CarrierKind::AcousticNearUltrasonic,
                    )
                },
                duration: Duration::from_secs(4),
                peer_has_internet_egress: true,
            },
            ContactWindow {
                carrier: CarrierProfile {
                    nominal_bps: 20,
                    setup_latency: Duration::ZERO,
                    ..CarrierProfile::baseline(
                        CarrierKind::BluetoothLeL2cap,
                    )
                },
                duration: Duration::from_secs(4),
                peer_has_internet_egress: true,
            },
        ];

        let outcome = scavenge_across_contacts(&device, &task, &windows);
        assert!(outcome.feasible);
        assert_eq!(outcome.delivered_bits, 120);
        assert_eq!(outcome.steps.len(), 2);
        assert_eq!(outcome.steps[0].delivered_bits, 40);
        assert_eq!(outcome.steps[1].delivered_bits, 80);
    }

    #[test]
    fn accumulated_bits_without_any_egress_do_not_become_fresh_internet() {
        let device = DeviceCapabilities::conservative_android();
        let task = InformationTask {
            request_bits: 8,
            response_bits: 8,
            require_fresh_remote: true,
            tolerate_delay: true,
            allow_generated: false,
        };

        let windows = vec![ContactWindow {
            carrier: CarrierProfile {
                nominal_bps: 100,
                setup_latency: Duration::ZERO,
                ..CarrierProfile::baseline(
                    CarrierKind::AcousticNearUltrasonic,
                )
            },
            duration: Duration::from_secs(1),
            peer_has_internet_egress: false,
        }];

        let outcome = scavenge_across_contacts(&device, &task, &windows);
        assert!(!outcome.feasible);
        assert_eq!(outcome.delivered_bits, 16);
        assert_eq!(outcome.freshness, None);
    }

    #[test]
    fn ranking_keeps_impossible_candidates_behind_feasible_ones() {
        let mut device = DeviceCapabilities::conservative_android();
        device.ble_l2cap_coc = true;
        let task = InformationTask::tiny_fresh_query();
        let profiles = [
            CarrierProfile::baseline(CarrierKind::None),
            CarrierProfile::baseline(CarrierKind::AcousticNearUltrasonic),
            CarrierProfile::baseline(CarrierKind::BluetoothLeL2cap),
        ];

        let ranked = rank_candidates(
            &profiles,
            &device,
            &task,
            true,
            false,
        );

        assert!(ranked[0].feasible);
        assert!(ranked[1].feasible);
        assert!(!ranked[2].feasible);
    }
}
