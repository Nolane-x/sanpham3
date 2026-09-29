use carrier_frontier::{
    rank_candidates, CarrierKind, CarrierProfile, DeviceCapabilities,
    AndroidHardwareProfile, AndroidPermissionProfile, ContactWindow,
    InformationTask, Platform, VirtualAndroidPhone, scavenge_across_contacts,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("virtual-phone-lab: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        None | Some("frontier") => frontier_sweep(),
        Some("zero-carrier") => zero_carrier_demo(),
        Some("android-minimal") => android_minimal_sweep(),
        Some("scavenge") => scavenge_demo(),
        Some("android-matrix") => android_matrix(),
        _ => Err(usage()),
    }
}

fn frontier_sweep() -> Result<(), String> {
    let device = broad_android_profile();
    let task = InformationTask::tiny_fresh_query();
    let profiles = frontier_profiles();

    println!(
        "VIRTUAL_PHONE platform={:?} peer_egress=true fresh_required=true task_bits={}",
        device.platform,
        task.total_bits(),
    );

    for outcome in rank_candidates(
        &profiles,
        &device,
        &task,
        true,
        true,
    ) {
        println!(
            "CARRIER kind={:?} feasible={} freshness={:?} completion_ms={} reason={}",
            outcome.carrier,
            outcome.feasible,
            outcome.freshness,
            outcome
                .completion_time
                .map(|value| value.as_millis().to_string())
                .unwrap_or_else(|| "-".to_owned()),
            outcome.reason,
        );
    }

    Ok(())
}

fn zero_carrier_demo() -> Result<(), String> {
    let device = broad_android_profile();

    let fresh = InformationTask::tiny_fresh_query();
    let fresh_outcome = carrier_frontier::simulate_single_carrier(
        &CarrierProfile::baseline(CarrierKind::None),
        &device,
        &fresh,
        false,
        true,
    );

    println!(
        "ZERO_CARRIER fresh_required=true feasible={} freshness={:?} reason={}",
        fresh_outcome.feasible,
        fresh_outcome.freshness,
        fresh_outcome.reason,
    );

    let cached = InformationTask {
        require_fresh_remote: false,
        allow_generated: false,
        ..fresh
    };
    let cached_outcome = carrier_frontier::simulate_single_carrier(
        &CarrierProfile::baseline(CarrierKind::None),
        &device,
        &cached,
        false,
        true,
    );

    println!(
        "ZERO_CARRIER fresh_required=false cached=true feasible={} freshness={:?} reason={}",
        cached_outcome.feasible,
        cached_outcome.freshness,
        cached_outcome.reason,
    );

    Ok(())
}

fn scavenge_demo() -> Result<(), String> {
    let device = broad_android_profile();
    let task = InformationTask {
        request_bits: 80,
        response_bits: 240,
        require_fresh_remote: true,
        tolerate_delay: true,
        allow_generated: false,
    };

    let windows = vec![
        ContactWindow {
            carrier: CarrierProfile {
                nominal_bps: 10,
                setup_latency: std::time::Duration::ZERO,
                ..CarrierProfile::baseline(
                    CarrierKind::VibrationSurface,
                )
            },
            duration: std::time::Duration::from_secs(8),
            peer_has_internet_egress: true,
        },
        ContactWindow {
            carrier: CarrierProfile {
                nominal_bps: 20,
                setup_latency: std::time::Duration::ZERO,
                ..CarrierProfile::baseline(
                    CarrierKind::AcousticNearUltrasonic,
                )
            },
            duration: std::time::Duration::from_secs(6),
            peer_has_internet_egress: true,
        },
        ContactWindow {
            carrier: CarrierProfile {
                nominal_bps: 60,
                setup_latency: std::time::Duration::ZERO,
                ..CarrierProfile::baseline(
                    CarrierKind::OpticalScreenCamera,
                )
            },
            duration: std::time::Duration::from_secs(2),
            peer_has_internet_egress: true,
        },
    ];

    let outcome = scavenge_across_contacts(
        &device,
        &task,
        &windows,
    );

    println!(
        "SCAVENGE feasible={} delivered_bits={} required_bits={} completed_ms={} freshness={:?} reason={}",
        outcome.feasible,
        outcome.delivered_bits,
        outcome.required_bits,
        outcome
            .completed_at
            .map(|value| value.as_millis().to_string())
            .unwrap_or_else(|| "-".to_owned()),
        outcome.freshness,
        outcome.reason,
    );

    for (index, step) in outcome.steps.iter().enumerate() {
        println!(
            "STEP index={} carrier={:?} window_ms={} delivered_bits={} cumulative_bits={}",
            index + 1,
            step.carrier,
            step.window.as_millis(),
            step.delivered_bits,
            step.cumulative_bits,
        );
    }

    Ok(())
}

fn android_minimal_sweep() -> Result<(), String> {
    let device = DeviceCapabilities::conservative_android();
    let task = InformationTask::tiny_fresh_query();

    println!(
        "VIRTUAL_PHONE platform={:?} profile=conservative peer_egress=true",
        device.platform,
    );

    for outcome in rank_candidates(
        &frontier_profiles(),
        &device,
        &task,
        true,
        false,
    ) {
        println!(
            "CARRIER kind={:?} feasible={} completion_ms={} reason={}",
            outcome.carrier,
            outcome.feasible,
            outcome
                .completion_time
                .map(|value| value.as_millis().to_string())
                .unwrap_or_else(|| "-".to_owned()),
            outcome.reason,
        );
    }

    Ok(())
}

fn android_matrix() -> Result<(), String> {
    let hardware = AndroidHardwareProfile::broad_phone();
    let all = AndroidPermissionProfile::all_granted();

    for api_level in [26_u16, 28, 29, 31, 33, 36, 37] {
        let phone = VirtualAndroidPhone {
            api_level,
            hardware,
            permissions: all,
        };
        let caps = phone.capabilities();

        println!(
            "ANDROID api={} wifi_direct={} wifi_aware={} ble={} ble_l2cap={} rfcomm={} hotspot={} sms={} nfc={}",
            api_level,
            caps.wifi_direct,
            caps.wifi_aware,
            caps.bluetooth_le,
            caps.ble_l2cap_coc,
            caps.bluetooth_classic,
            caps.local_only_hotspot,
            caps.telephony_messaging,
            caps.nfc_hce_or_reader,
        );
    }

    let mut denied = all;
    denied.nearby_wifi_devices = false;
    denied.bluetooth_scan = false;
    denied.bluetooth_connect = false;
    denied.send_sms = false;

    let restricted = VirtualAndroidPhone {
        api_level: 36,
        hardware,
        permissions: denied,
    }
    .capabilities();

    println!(
        "ANDROID api=36 profile=permission-denied wifi_direct={} wifi_aware={} ble={} ble_l2cap={} rfcomm={} sms={}",
        restricted.wifi_direct,
        restricted.wifi_aware,
        restricted.bluetooth_le,
        restricted.ble_l2cap_coc,
        restricted.bluetooth_classic,
        restricted.telephony_messaging,
    );

    Ok(())
}

fn broad_android_profile() -> DeviceCapabilities {
    DeviceCapabilities {
        platform: Platform::Android,
        wifi_direct: true,
        wifi_aware: true,
        bluetooth_le: true,
        ble_l2cap_coc: true,
        bluetooth_classic: true,
        local_only_hotspot: true,
        telephony_messaging: true,
        nfc_hce_or_reader: true,
        microphone: true,
        speaker: true,
        camera: true,
        screen: true,
        vibrator: true,
        accelerometer: true,
        magnetometer: true,
        usb: true,
    }
}

fn frontier_profiles() -> Vec<CarrierProfile> {
    [
        CarrierKind::InternetIp,
        CarrierKind::WifiDirect,
        CarrierKind::WifiAware,
        CarrierKind::BluetoothLeL2cap,
        CarrierKind::BluetoothRfcomm,
        CarrierKind::LocalOnlyHotspot,
        CarrierKind::CellularSms,
        CarrierKind::NfcHce,
        CarrierKind::AcousticNearUltrasonic,
        CarrierKind::OpticalScreenCamera,
        CarrierKind::VibrationSurface,
        CarrierKind::MagneticSensor,
        CarrierKind::UsbLocal,
        CarrierKind::PhysicalDataMule,
        CarrierKind::LocalCacheTwin,
        CarrierKind::None,
    ]
    .into_iter()
    .map(CarrierProfile::baseline)
    .collect()
}

fn usage() -> String {
    [
        "usage:",
        "  virtual-phone-lab frontier",
        "  virtual-phone-lab zero-carrier",
        "  virtual-phone-lab android-minimal",
        "  virtual-phone-lab scavenge",
        "  virtual-phone-lab android-matrix",
        "",
        "This is a simulation/research tool. It does not convert simulated",
        "carrier success into physical evidence.",
    ]
    .join("\n")
}
