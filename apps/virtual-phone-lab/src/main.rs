use carrier_frontier::{
    rank_candidates, CarrierKind, CarrierProfile, DeviceCapabilities,
    InformationTask, Platform,
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

fn broad_android_profile() -> DeviceCapabilities {
    DeviceCapabilities {
        platform: Platform::Android,
        wifi_direct: true,
        wifi_aware: true,
        bluetooth_le: true,
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
        "",
        "This is a simulation/research tool. It does not convert simulated",
        "carrier success into physical evidence.",
    ]
    .join("\n")
}
