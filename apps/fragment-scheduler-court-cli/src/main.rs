use fragment_scheduler::{
    choose_energy_efficient_path, schedule_contact, ContactBudget,
    EnergyPathMeasurement, EnergyTransferCandidate, PendingTransfer,
};
use fragment_transport::{
    fragment_for_wire_budget, FragmentAssembler, FragmentKey,
};
use std::collections::HashMap;
use std::time::Duration;
use urt_core::{decode_exact, encode_exact, DecodeBudget, Digest32};

fn main() {
    let key = FragmentKey::new([0x71; 32]);

    let mut urgent_logical = Vec::new();
    for index in 0..1_200_u32 {
        urgent_logical.extend_from_slice(
            format!(
                "{{\"seq\":{index},\"fresh_remote\":true,\"kind\":\"recovery-result\"}}\n"
            )
            .as_bytes(),
        );
    }
    let urgent_urt = encode_exact(&urgent_logical, None);
    let urgent_wire = urgent_urt.packet.to_bytes();

    let mut background_state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut background_logical = vec![0_u8; 96 * 1024];
    for byte in &mut background_logical {
        background_state ^= background_state << 13;
        background_state ^= background_state >> 7;
        background_state ^= background_state << 17;
        *byte = background_state as u8;
    }
    let background_urt = encode_exact(&background_logical, None);
    assert!(background_urt.packet.to_bytes().len() > 90 * 1024);
    let background_wire = background_urt.packet.to_bytes();

    let urgent = PendingTransfer::from_bytes(
        &urgent_wire,
        220,
        &key,
        Duration::from_secs(8),
        10,
    )
    .expect("urgent transfer");
    let urgent_id = urgent.transfer_id;

    let background = PendingTransfer::from_bytes(
        &background_wire,
        220,
        &key,
        Duration::from_secs(120),
        100,
    )
    .expect("background transfer");
    let background_id = background.transfer_id;

    // Deliberately put high-priority background first. The scheduler should
    // still finish the freshness-constrained URT result in the early contact.
    let mut transfers = vec![background, urgent];

    let early = schedule_contact(
        &mut transfers,
        ContactBudget {
            starts_at: Duration::ZERO,
            duration: Duration::from_secs(6),
            bitrate_bps: 4_000,
            max_wire_bytes: None,
        },
    )
    .expect("early contact schedule");

    assert!(early.completed_transfers.contains(&urgent_id));
    assert!(!early.completed_transfers.contains(&background_id));

    let late = schedule_contact(
        &mut transfers,
        ContactBudget {
            starts_at: Duration::from_secs(20),
            duration: Duration::from_secs(100),
            bitrate_bps: 20_000,
            max_wire_bytes: None,
        },
    )
    .expect("late contact schedule");
    assert!(late.completed_transfers.contains(&background_id));

    let mut urgent_assembler = FragmentAssembler::new(2 * 1024 * 1024);
    let mut background_assembler = FragmentAssembler::new(2 * 1024 * 1024);

    for scheduled in early.fragments.iter().chain(late.fragments.iter()) {
        if scheduled.transfer_id == urgent_id {
            urgent_assembler
                .accept_wire(&scheduled.wire, &key)
                .expect("urgent fragment");
        } else if scheduled.transfer_id == background_id {
            background_assembler
                .accept_wire(&scheduled.wire, &key)
                .expect("background fragment");
        }
    }

    let urgent_reconstructed = urgent_assembler
        .reconstruct()
        .expect("urgent URT reconstruction");
    let cache = HashMap::<Digest32, Vec<u8>>::new();
    let urgent_output = decode_exact(
        &urgent_reconstructed,
        &cache,
        DecodeBudget::permissive_for(urgent_logical.len() as u64),
    )
    .expect("urgent URT exact decode");
    assert_eq!(urgent_output, urgent_logical);

    assert!(background_assembler.is_complete());

    println!(
        "F4_DEADLINE_SCHEDULER_PASS urgent_logical_bytes={} urgent_urt_bytes={} early_used_bytes={} early_capacity_bytes={} background_completed_late=true urgent_strategy={:?}",
        urgent_logical.len(),
        urgent_wire.len(),
        early.used_bytes,
        early.capacity_bytes,
        urgent_urt.report.strategy,
    );

    // Energy court: objective consumes externally supplied measurements rather
    // than inventing carrier energy. Expected wire bytes include the real SP3F
    // overhead generated for this URT object.
    let energy_wires = fragment_for_wire_budget(
        &urgent_wire,
        220,
        &key,
    )
    .expect("energy wire accounting");
    let expected_wire_bytes = energy_wires
        .iter()
        .map(|wire| wire.len() as u64)
        .sum::<u64>();
    let useful_bits = (urgent_wire.len() as u64).saturating_mul(8);

    let candidates = vec![
        EnergyTransferCandidate {
            measurement: EnergyPathMeasurement {
                path_id: "wifi-fast".to_owned(),
                setup_time: Duration::from_millis(20),
                setup_microjoules: 4_000,
                active_microwatts: 2_000_000,
                bitrate_bps: 10_000_000,
            },
            useful_bits,
            expected_wire_bytes,
        },
        EnergyTransferCandidate {
            measurement: EnergyPathMeasurement {
                path_id: "ble-efficient".to_owned(),
                setup_time: Duration::from_millis(500),
                setup_microjoules: 500,
                active_microwatts: 10_000,
                bitrate_bps: 100_000,
            },
            useful_bits,
            expected_wire_bytes,
        },
    ];

    let efficient = choose_energy_efficient_path(
        &candidates,
        Duration::from_secs(5),
    )
    .expect("energy efficient candidate");
    assert_eq!(efficient.path_id, "ble-efficient");

    let tight = choose_energy_efficient_path(
        &candidates,
        Duration::from_millis(200),
    )
    .expect("deadline-constrained energy candidate");
    assert_eq!(tight.path_id, "wifi-fast");

    let (energy_numerator, useful_denominator) =
        efficient.energy_per_useful_bit_ratio();
    println!(
        "F4_ENERGY_OBJECTIVE_PASS useful_bits={} expected_wire_bytes={} selected={} estimated_energy_uj={} ratio_uj_over_useful_bit={}/{} completion_ms={} tight_deadline_selected={}",
        useful_bits,
        expected_wire_bytes,
        efficient.path_id,
        efficient.estimated_energy_microjoules,
        energy_numerator,
        useful_denominator,
        efficient.completion_time.as_millis(),
        tight.path_id,
    );
}
