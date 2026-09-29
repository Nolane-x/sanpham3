use fragment_transport::{
    fragment_for_wire_budget, fragment_with_xor_parity,
    rateless_symbol_for_wire_budget, sha256, AcceptOutcome, FragmentAssembler,
    FragmentKey, FragmentProvenance, ParityRecoveryOutcome,
    ProvenanceAssembler, RatelessDecoder, RatelessSymbolEnvelope,
};
use std::collections::HashMap;
use urt_core::{decode_exact, encode_exact, DecodeBudget, Digest32};

fn main() {
    let mut input = Vec::new();
    for index in 0..12_000_u32 {
        let line = format!(
            "{{\"seq\":{index},\"mode\":\"recovery\",\"carrier\":\"intermittent\",\"fresh\":true}}\n"
        );
        input.extend_from_slice(line.as_bytes());
    }

    let urt = encode_exact(&input, None);
    let urt_wire = urt.packet.to_bytes();
    assert!(urt_wire.len() < input.len());

    let key = FragmentKey::new([0x47; 32]);
    let transfer = fragment_with_xor_parity(
        &urt_wire,
        220,
        4,
        &key,
    )
    .expect("erasure transfer");

    // Three synthetic contact windows. Every stripe deliberately loses its
    // second data shard. Parity arrives in a later contact and must recover
    // the missing exact bytes. Some data fragments are duplicated and order
    // is intentionally scrambled across windows.
    let mut contact_a = Vec::new();
    let mut contact_b = Vec::new();
    for (index, wire) in transfer.data_wires.iter().enumerate() {
        if index % transfer.stripe_width == 1 {
            continue;
        }
        if index % 2 == 0 {
            contact_a.push(wire.clone());
        } else {
            contact_b.push(wire.clone());
        }
    }
    contact_a.reverse();
    contact_b.reverse();

    if let Some(duplicate) = contact_a.get(2).cloned() {
        contact_b.push(duplicate);
    }

    let mut assembler = FragmentAssembler::new(4 * 1024 * 1024);
    let mut duplicates = 0_u64;
    let mut delivered_wire_bytes = 0_u64;

    for window in [&contact_a, &contact_b] {
        for wire in window {
            delivered_wire_bytes += wire.len() as u64;
            if assembler
                .accept_wire(wire, &key)
                .expect("authenticated SP3F fragment")
                == AcceptOutcome::Duplicate
            {
                duplicates += 1;
            }
        }
    }

    let mut recovered = 0_u64;
    for parity in &transfer.parity_wires {
        delivered_wire_bytes += parity.len() as u64;
        match assembler
            .recover_with_parity_wire(parity, &key)
            .expect("authenticated SP3E parity")
        {
            ParityRecoveryOutcome::Recovered { .. } => recovered += 1,
            ParityRecoveryOutcome::NotNeeded => {}
            ParityRecoveryOutcome::Insufficient { missing_shards } => {
                panic!("stripe still missing {missing_shards} shards");
            }
        }
    }

    let reconstructed_urt = assembler
        .reconstruct()
        .expect("exact URT wire reconstruction");
    assert_eq!(reconstructed_urt, urt_wire);
    assert_eq!(sha256(&reconstructed_urt), sha256(&urt_wire));

    let cache = HashMap::<Digest32, Vec<u8>>::new();
    let output = decode_exact(
        &reconstructed_urt,
        &cache,
        DecodeBudget::permissive_for(input.len() as u64),
    )
    .expect("exact URT decode");
    assert_eq!(output, input);

    println!(
        "F4_ERASURE_URT_PASS logical_bytes={} urt_bytes={} delivered_wire_bytes={} data_fragments={} parity_fragments={} recovered_shards={} duplicates={} urt_strategy={:?} digest={:02x?}",
        input.len(),
        urt_wire.len(),
        delivered_wire_bytes,
        transfer.data_wires.len(),
        transfer.parity_wires.len(),
        recovered,
        duplicates,
        urt.report.strategy,
        &sha256(&output)[..8],
    );

    // Independent provenance court: fragments for the same exact URT object
    // arrive through multiple source/carrier identities. An exact duplicate
    // is observed through a fourth source and must enrich provenance without
    // changing the reconstructed bytes.
    let provenance_wires =
        fragment_for_wire_budget(&urt_wire, 240, &key)
            .expect("provenance fragments");
    let mut provenance_assembler =
        ProvenanceAssembler::new(4 * 1024 * 1024);

    for (index, wire) in provenance_wires.iter().enumerate().rev() {
        let (source_id, carrier) = match index % 3 {
            0 => ("peer-a", "ble-gatt"),
            1 => ("peer-b", "wifi-direct"),
            _ => ("peer-c", "acoustic"),
        };
        provenance_assembler
            .accept_wire_from(
                wire,
                &key,
                FragmentProvenance {
                    source_id: source_id.to_owned(),
                    carrier: carrier.to_owned(),
                    observed_at_ms: 50_000 + index as u64,
                },
            )
            .expect("provenanced fragment");
    }

    assert_eq!(
        provenance_assembler
            .accept_wire_from(
                &provenance_wires[0],
                &key,
                FragmentProvenance {
                    source_id: "peer-d".to_owned(),
                    carrier: "nfc".to_owned(),
                    observed_at_ms: 99_000,
                },
            )
            .expect("duplicate provenance"),
        AcceptOutcome::Duplicate,
    );

    let (provenance_output, summary) = provenance_assembler
        .reconstruct_with_summary()
        .expect("provenance reconstruction");
    assert_eq!(provenance_output, urt_wire);
    assert_eq!(summary.unique_sources.len(), 4);
    assert_eq!(summary.fragment_offsets_with_multiple_sources, 1);
    assert!(summary.unique_carriers.contains(&"acoustic".to_owned()));
    assert!(summary.unique_carriers.contains(&"nfc".to_owned()));

    println!(
        "F4_PROVENANCE_PASS urt_bytes={} fragments={} unique_sources={} unique_carriers={} multi_source_offsets={} first_observed_ms={} last_observed_ms={}",
        urt_wire.len(),
        summary.fragments_tracked,
        summary.unique_sources.len(),
        summary.unique_carriers.len(),
        summary.fragment_offsets_with_multiple_sources,
        summary.first_observed_at_ms,
        summary.last_observed_at_ms,
    );

    // Rateless court: send systematic symbols with deliberate multi-shard
    // loss, then keep generating repair symbol IDs until the decoder reaches
    // full rank. Some repair symbols are also lost.
    let first_rateless =
        rateless_symbol_for_wire_budget(&urt_wire, 220, 0, &key)
            .expect("first rateless symbol");
    let first_envelope =
        RatelessSymbolEnvelope::open(&first_rateless, &key)
            .expect("rateless metadata");
    let source_count = first_envelope.source_count as usize;
    let mut rateless = RatelessDecoder::new(4 * 1024 * 1024);
    let mut generated_symbols = 0_u64;
    let mut delivered_symbols = 0_u64;
    let mut lost_symbols = 0_u64;

    for symbol_id in 0..source_count as u64 {
        generated_symbols += 1;
        if symbol_id % 4 == 0 {
            lost_symbols += 1;
            continue;
        }
        let wire = rateless_symbol_for_wire_budget(
            &urt_wire,
            220,
            symbol_id,
            &key,
        )
        .expect("systematic rateless symbol");
        rateless
            .accept_wire(&wire, &key)
            .expect("systematic rateless receive");
        delivered_symbols += 1;
    }
    assert!(!rateless.is_decodable());

    let mut symbol_id = source_count as u64;
    let repair_limit = source_count as u64 * 8;
    while !rateless.is_decodable() && symbol_id < repair_limit {
        generated_symbols += 1;
        if symbol_id % 7 == 0 {
            lost_symbols += 1;
            symbol_id += 1;
            continue;
        }

        let wire = rateless_symbol_for_wire_budget(
            &urt_wire,
            220,
            symbol_id,
            &key,
        )
        .expect("repair rateless symbol");
        rateless
            .accept_wire(&wire, &key)
            .expect("repair rateless receive");
        delivered_symbols += 1;
        symbol_id += 1;
    }

    assert!(rateless.is_decodable());
    assert_eq!(rateless.rank(), source_count);
    let rateless_urt = rateless
        .reconstruct()
        .expect("rateless URT reconstruction");
    assert_eq!(rateless_urt, urt_wire);

    let rateless_output = decode_exact(
        &rateless_urt,
        &cache,
        DecodeBudget::permissive_for(input.len() as u64),
    )
    .expect("rateless URT exact decode");
    assert_eq!(rateless_output, input);

    println!(
        "F4_RATELESS_PASS logical_bytes={} urt_bytes={} source_shards={} generated_symbols={} delivered_symbols={} lost_symbols={} final_rank={} digest={:02x?}",
        input.len(),
        urt_wire.len(),
        source_count,
        generated_symbols,
        delivered_symbols,
        lost_symbols,
        rateless.rank(),
        &sha256(&rateless_output)[..8],
    );
}
