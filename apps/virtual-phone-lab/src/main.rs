use carrier_frontier::{
    choose_failure_diverse_carriers, primary_failure_domain, rank_candidates,
    scavenge_across_contacts, AndroidHardwareProfile, AndroidPermissionProfile,
    CarrierKind, CarrierProfile, ContactWindow, DesktopHardwareProfile,
    DesktopProjectAdapters, DeviceCapabilities, FailureDomain, FailureScenario,
    InformationTask, LinuxPrivilegeProfile, Platform, VirtualAndroidPhone,
    VirtualLinuxDevice, VirtualWindowsDevice, WindowsPermissionProfile,
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
        Some("desktop-matrix") => desktop_matrix(),
        Some("acoustic-synthetic") => acoustic_synthetic(),
        Some("optical-synthetic") => optical_synthetic(),
        Some("vibration-synthetic") => vibration_synthetic(),
        Some("continuity") => continuity_demo(),
        Some("failure-domains") => failure_domain_demo(),
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

fn failure_domain_demo() -> Result<(), String> {
    let mut device = broad_android_profile();
    device.ble_l2cap_coc = true;

    let profiles = vec![
        CarrierProfile::baseline(CarrierKind::WifiDirect),
        CarrierProfile::baseline(CarrierKind::WifiAware),
        CarrierProfile::baseline(CarrierKind::BluetoothLeGatt),
        CarrierProfile::baseline(CarrierKind::BluetoothLeL2cap),
        CarrierProfile::baseline(CarrierKind::BluetoothRfcomm),
        CarrierProfile::baseline(CarrierKind::NfcHce),
        CarrierProfile::baseline(CarrierKind::OpticalScreenCamera),
    ];

    let selection =
        choose_failure_diverse_carriers(&profiles, &device, 4);

    println!(
        "F4_FAILURE_DIVERSITY selected={:?} distinct_domains={}",
        selection.carriers,
        selection.distinct_failure_domains,
    );

    for carrier in &selection.carriers {
        println!(
            "F4_FAILURE_DOMAIN carrier={carrier:?} domain={:?}",
            primary_failure_domain(*carrier),
        );
    }

    let bluetooth_failure = FailureScenario {
        failed_domains: vec![FailureDomain::BluetoothRadio],
    };
    println!(
        "F4_CORRELATED_FAILURE failed={:?} gatt_survives={} l2cap_survives={} rfcomm_survives={} wifi_survives={} optical_survives={}",
        FailureDomain::BluetoothRadio,
        bluetooth_failure.carrier_survives(CarrierKind::BluetoothLeGatt),
        bluetooth_failure.carrier_survives(CarrierKind::BluetoothLeL2cap),
        bluetooth_failure.carrier_survives(CarrierKind::BluetoothRfcomm),
        bluetooth_failure.carrier_survives(CarrierKind::WifiDirect),
        bluetooth_failure.carrier_survives(CarrierKind::OpticalScreenCamera),
    );

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

fn continuity_demo() -> Result<(), String> {
    use std::time::Duration;
    use zero_carrier_continuity::{
        CachedObject, ContinuityContract, ContinuityFreshness,
        ContinuityMiss, ContinuityStore, ServiceTwinRecipe,
        SourceReceipt, run_service_twin,
    };

    let mut store = ContinuityStore::new();
    let weather = b"temp=30;humidity=70";
    store
        .insert_verified(CachedObject {
            key: "weather.raw".to_owned(),
            bytes: weather.to_vec(),
            receipt: SourceReceipt::for_bytes(
                "weather-source",
                10_000,
                weather,
                "last successful remote observation",
            ),
            valid_for: Duration::from_secs(300),
        })
        .map_err(str::to_owned)?;

    let current_required =
        store.resolve_zero_carrier::<fn(&str) -> Vec<u8>>(
            "weather.raw",
            20_000,
            ContinuityContract {
                require_current_remote_observation: true,
                max_cache_age: Some(Duration::from_secs(60)),
                allow_generated: true,
            },
            None,
        );

    if current_required != Err(ContinuityMiss::CurrentRemoteRequired) {
        return Err(
            "zero-carrier continuity incorrectly satisfied a current-remote contract"
                .to_owned(),
        );
    }

    let cached = store
        .resolve_zero_carrier::<fn(&str) -> Vec<u8>>(
            "weather.raw",
            20_000,
            ContinuityContract::cached_ok(Duration::from_secs(60)),
            None,
        )
        .map_err(|error| format!("{error:?}"))?;

    if cached.freshness != ContinuityFreshness::CachedRemote {
        return Err("cached continuity answer lost freshness typing".to_owned());
    }

    let twin = run_service_twin(
        &store,
        &ServiceTwinRecipe {
            recipe_id: "weather-card-v1".to_owned(),
            required_keys: vec!["weather.raw".to_owned()],
        },
        20_000,
        Duration::from_secs(60),
        |inputs| {
            format!(
                "offline-card:{}",
                String::from_utf8_lossy(inputs[0].1),
            )
            .into_bytes()
        },
    )
    .map_err(|error| format!("{error:?}"))?;

    println!(
        "CONTINUITY current_remote=false cached_label={:?} age_ms={} receipt_source={} twin_label={:?} twin_inputs={} twin_bytes={}",
        cached.freshness,
        cached.age.unwrap_or_default().as_millis(),
        cached
            .source_receipt
            .as_ref()
            .map(|receipt| receipt.source_id.as_str())
            .unwrap_or("-"),
        twin.freshness,
        twin.inputs.len(),
        String::from_utf8_lossy(&twin.bytes),
    );

    Ok(())
}

fn optical_synthetic() -> Result<(), String> {
    let bits = vec![
        1, 0, 1, 1, 0, 0, 1, 0,
        0, 1, 1, 0, 1, 0, 0, 1,
    ];
    let repetition =
        signal_frontier::OpticalRepetitionConfig::robust_default();
    let grid = signal_frontier::OpticalGridConfig::camera_baseline();
    let perspective =
        signal_frontier::OpticalPerspective::mild_keystone();

    let symbols = signal_frontier::encode_optical_repetition(
        &bits,
        repetition,
    )
    .map_err(|error| format!("{error:?}"))?;
    let rendered = signal_frontier::render_optical_cells(
        &symbols,
        grid,
    )
    .map_err(|error| format!("{error:?}"))?;
    let warped = signal_frontier::warp_optical_perspective(
        &rendered,
        perspective,
    )
    .map_err(|error| format!("{error:?}"))?;
    let banded =
        signal_frontier::apply_optical_rolling_shutter_banding(
            &warped,
            signal_frontier::OpticalRollingShutterBanding::phone_pwm_baseline(),
        )
        .map_err(|error| format!("{error:?}"))?;
    let blurred = signal_frontier::apply_optical_box_blur(
        &banded,
        1,
    )
    .map_err(|error| format!("{error:?}"))?;
    let photographed = signal_frontier::apply_optical_photometric(
        &blurred,
        signal_frontier::OpticalPhotometric::phone_camera_baseline(),
    )
    .map_err(|error| format!("{error:?}"))?;
    let sampled = signal_frontier::decode_optical_cells(
        &photographed,
        symbols.len(),
        grid,
        Some(perspective),
    )
    .map_err(|error| format!("{error:?}"))?;
    let decoded = signal_frontier::decode_optical_repetition(
        &sampled,
        repetition,
    )
    .map_err(|error| format!("{error:?}"))?;
    let errors = signal_frontier::bit_error_count(&bits, &decoded);
    let erasures = sampled.iter().filter(|symbol| symbol.is_none()).count();

    println!(
        "OPTICAL_SYNTHETIC bits={} symbols={} frame={}x{} erasures={} errors={} perspective=true rolling_shutter=true blur_radius=1 exposure_gamma=true",
        bits.len(),
        symbols.len(),
        photographed.width,
        photographed.height,
        erasures,
        errors,
    );

    if errors != 0 {
        return Err(
            "synthetic optical raster payload did not roundtrip".to_owned(),
        );
    }
    Ok(())
}

fn vibration_synthetic() -> Result<(), String> {
    let bits = vec![
        1, 0, 1, 1, 0, 0, 1, 0,
        0, 1, 1, 0, 1, 0, 0, 1,
    ];
    let config = signal_frontier::VibrationOokConfig::surface_2_5bps();
    let encoded = signal_frontier::encode_vibration_ook(
        &bits,
        config,
    )
    .map_err(|error| format!("{error:?}"))?;
    let resonant = signal_frontier::apply_mechanical_impulse_response(
        &encoded,
        &[
            signal_frontier::MechanicalImpulseTap {
                delay_samples: 0,
                gain: 0.90,
            },
            signal_frontier::MechanicalImpulseTap {
                delay_samples: 3,
                gain: 0.10,
            },
            signal_frontier::MechanicalImpulseTap {
                delay_samples: 8,
                gain: -0.03,
            },
        ],
    )
    .map_err(|error| format!("{error:?}"))?;
    let impaired = signal_frontier::apply_profiled_mechanical_channel(
        &resonant,
        signal_frontier::MechanicalChannel {
            gain: 0.62,
            white_noise_amplitude: 0.025,
        },
        signal_frontier::VibrationMountProfile::flat_table(),
        99,
    )
    .map_err(|error| format!("{error:?}"))?;
    let decoded = signal_frontier::decode_vibration_ook(
        &impaired,
        config,
    )
    .map_err(|error| format!("{error:?}"))?;
    let errors = signal_frontier::bit_error_count(&bits, &decoded);

    println!(
        "VIBRATION_SYNTHETIC bits={} samples={} errors={} resonance=true mount=flat_table",
        bits.len(),
        impaired.len(),
        errors,
    );

    if errors != 0 {
        return Err("synthetic vibration payload did not roundtrip".to_owned());
    }
    Ok(())
}

fn acoustic_synthetic() -> Result<(), String> {
    let config = signal_frontier::AcousticFskConfig::near_ultrasonic_50bps();
    let bits = vec![
        1, 0, 1, 1, 0, 0, 1, 0,
        0, 1, 1, 0, 1, 0, 0, 1,
    ];

    let clean = signal_frontier::encode_fsk(&bits, config)
        .map_err(|error| format!("{error:?}"))?;
    let multipath = signal_frontier::apply_acoustic_impulse_response(
        &clean,
        &[
            signal_frontier::AcousticImpulseTap {
                delay_samples: 0,
                gain: 0.72,
            },
            signal_frontier::AcousticImpulseTap {
                delay_samples: 7,
                gain: 0.16,
            },
            signal_frontier::AcousticImpulseTap {
                delay_samples: 19,
                gain: -0.07,
            },
        ],
    )
    .map_err(|error| format!("{error:?}"))?;
    let drifted =
        signal_frontier::apply_clock_drift_resampling(&multipath, 80)
            .map_err(|error| format!("{error:?}"))?;
    let agc = signal_frontier::apply_acoustic_agc(
        &drifted,
        signal_frontier::AcousticAgcConfig::phone_baseline(),
    )
    .map_err(|error| format!("{error:?}"))?;
    let nonlinear = signal_frontier::apply_acoustic_nonlinearity(
        &agc,
        signal_frontier::AcousticNonlinearity::phone_baseline(),
    )
    .map_err(|error| format!("{error:?}"))?;
    let impaired = signal_frontier::apply_acoustic_channel(
        &nonlinear,
        signal_frontier::AcousticChannel {
            gain: 0.90,
            white_noise_amplitude: 0.025,
            clip_level: 0.95,
        },
        42,
    )
    .map_err(|error| format!("{error:?}"))?;
    let decoded = signal_frontier::decode_fsk(&impaired, config)
        .map_err(|error| format!("{error:?}"))?;

    let errors = signal_frontier::bit_error_count(&bits, &decoded.bits);

    println!(
        "ACOUSTIC_SYNTHETIC bits={} samples={} errors={} min_confidence={:.4} multipath=true drift_ppm=80 agc=true nonlinear=true",
        bits.len(),
        impaired.len(),
        errors,
        decoded.minimum_confidence,
    );

    if errors != 0 {
        return Err("synthetic acoustic reference payload did not roundtrip".to_owned());
    }

    Ok(())
}

fn desktop_matrix() -> Result<(), String> {
    let hardware = DesktopHardwareProfile::broad_laptop();

    let windows = VirtualWindowsDevice {
        hardware,
        permissions: WindowsPermissionProfile::all_granted(),
        adapters: DesktopProjectAdapters::current_windows(),
    }
    .capabilities();

    println!(
        "DESKTOP platform=Windows profile=current-project wifi_direct={} ble={} rfcomm={} acoustic={} optical={} usb_peer={} external_interface={}",
        windows.wifi_direct,
        windows.bluetooth_le,
        windows.bluetooth_classic,
        windows.microphone && windows.speaker,
        windows.camera && windows.screen,
        windows.usb,
        windows.external_os_interface,
    );

    let linux = VirtualLinuxDevice {
        hardware,
        privileges: LinuxPrivilegeProfile::all_granted(),
        adapters: DesktopProjectAdapters::current_linux(),
    }
    .capabilities();

    println!(
        "DESKTOP platform=Linux profile=current-project wifi_direct={} ble={} rfcomm={} acoustic={} optical={} usb_peer={} external_interface={}",
        linux.wifi_direct,
        linux.bluetooth_le,
        linux.bluetooth_classic,
        linux.microphone && linux.speaker,
        linux.camera && linux.screen,
        linux.usb,
        linux.external_os_interface,
    );

    let mut research_adapters = DesktopProjectAdapters::current_windows();
    research_adapters.acoustic = true;
    research_adapters.optical = true;
    research_adapters.usb_peer = true;

    let mut denied = WindowsPermissionProfile::all_granted();
    denied.audio_capture = false;
    denied.usb_device_access = false;

    let gated = VirtualWindowsDevice {
        hardware,
        permissions: denied,
        adapters: research_adapters,
    }
    .capabilities();

    println!(
        "DESKTOP platform=Windows profile=adapter-present-access-denied acoustic={} optical={} usb_peer={} external_interface={}",
        gated.microphone && gated.speaker,
        gated.camera && gated.screen,
        gated.usb,
        gated.external_os_interface,
    );

    if windows.wifi_direct
        || windows.bluetooth_le
        || windows.usb
        || linux.wifi_direct
        || linux.bluetooth_le
        || linux.usb
        || gated.microphone
        || gated.usb
    {
        return Err(
            "desktop capability model promoted an unimplemented or denied carrier"
                .to_owned(),
        );
    }

    if !windows.external_os_interface
        || !linux.external_os_interface
        || !(gated.camera && gated.screen)
    {
        return Err(
            "desktop capability model blocked a supported modeled capability"
                .to_owned(),
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
        external_os_interface: false,
    }
}

fn frontier_profiles() -> Vec<CarrierProfile> {
    [
        CarrierKind::InternetIp,
        CarrierKind::WifiDirect,
        CarrierKind::WifiAware,
        CarrierKind::BluetoothLeAdvertisement,
        CarrierKind::BluetoothLeGatt,
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
        CarrierKind::ExternalOsInterface,
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
        "  virtual-phone-lab desktop-matrix",
        "  virtual-phone-lab acoustic-synthetic",
        "  virtual-phone-lab optical-synthetic",
        "  virtual-phone-lab vibration-synthetic",
        "  virtual-phone-lab continuity",
        "  virtual-phone-lab failure-domains",
        "",
        "This is a simulation/research tool. It does not convert simulated",
        "carrier success into physical evidence.",
    ]
    .join("\n")
}
