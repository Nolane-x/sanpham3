use fragment_transport::{
    fragment_for_wire_budget, sha256, AcceptOutcome, FragmentAssembler,
    FragmentKey,
};

fn main() {
    let mut state = 0x91ab_5512_13cc_f00d_u64;
    let mut input = vec![0_u8; 32 * 1024];
    for byte in &mut input {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }

    let key = FragmentKey::new([0x47; 32]);
    let mut wires =
        fragment_for_wire_budget(&input, 220, &key).expect("fragment transfer");

    // Simulate intermittent contacts: reverse delivery order, duplicate one
    // fragment, and still require exact reconstruction.
    wires.reverse();
    let duplicate = wires[2].clone();
    wires.insert(7, duplicate);

    let mut assembler = FragmentAssembler::new(64 * 1024);
    let mut duplicates = 0_u64;
    let mut wire_bytes = 0_u64;

    for wire in &wires {
        wire_bytes += wire.len() as u64;
        if assembler.accept_wire(wire, &key).expect("authenticated fragment")
            == AcceptOutcome::Duplicate
        {
            duplicates += 1;
        }
    }

    let output = assembler.reconstruct().expect("exact reconstruction");
    assert_eq!(output, input);
    assert_eq!(sha256(&output), sha256(&input));

    println!(
        "F4_FRAGMENT_PASS original_bytes={} delivered_wire_bytes={} fragments={} duplicates={} digest={:02x?}",
        input.len(),
        wire_bytes,
        assembler.fragment_count(),
        duplicates,
        &sha256(&output)[..8],
    );
}
