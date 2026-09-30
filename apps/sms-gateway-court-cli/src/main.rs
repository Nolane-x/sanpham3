use peer_egress::{
    decode_request, decode_response, encode_request, encode_response,
    ResolveRequest, ResolveResponse, ResolveStatus,
};
use sms_capsule::{
    segment_capsule, AcceptOutcome, SmsAssembler, SmsSegment, SmsSendPolicy,
};
use std::net::{IpAddr, Ipv6Addr};
use tiny_capsule::{Capsule, CapsuleError, CapsuleKey, ReplayGuard};

const SMS_MAGIC: u32 = u32::from_be_bytes(*b"SP3M");
type SmsCapsule = Capsule<SMS_MAGIC>;
type SmsReplayGuard = ReplayGuard<SMS_MAGIC>;

fn main() {
    let hostname = format!("edge-{}.example.com", "a".repeat(50));
    let request = ResolveRequest {
        request_id: 0x5152_5354,
        relays_remaining: 1,
        hostname,
    };
    let request_payload =
        encode_request(&request).expect("encode resolve request");

    let key = CapsuleKey::new([0x5D; 32]);
    let request_capsule = SmsCapsule {
        sender_id: 4004,
        sequence: 21,
        payload: request_payload.clone(),
    };
    let request_wire =
        request_capsule.seal(&key).expect("seal SP3M request");
    assert_eq!(&request_wire[..4], b"SP3M");

    let send_policy = SmsSendPolicy {
        user_consented: true,
        subscription_available: true,
        roaming: false,
        allow_roaming: false,
        max_segments: 4,
    };

    let (gateway_wire, request_segments) =
        simulate_sms_transfer(&request_wire, send_policy);
    let mut gateway_replay = SmsReplayGuard::new(32);
    let gateway_capsule = gateway_replay
        .open_and_accept(&gateway_wire, &key)
        .expect("authenticate request capsule");
    let gateway_request =
        decode_request(&gateway_capsule.payload).expect("decode request");
    assert_eq!(gateway_request, request);

    assert_eq!(
        gateway_replay
            .open_and_accept(&gateway_wire, &key)
            .unwrap_err(),
        CapsuleError::ReplayDetected,
    );

    let response = ResolveResponse {
        request_id: request.request_id,
        status: ResolveStatus::Ok,
        addresses: (1..=5)
            .map(|tail| {
                IpAddr::V6(Ipv6Addr::new(
                    0x2001, 0x0db8, 0, 0, 0, 0, 0, tail,
                ))
            })
            .collect(),
    };
    let response_payload =
        encode_response(&response).expect("encode resolve response");
    let response_capsule = SmsCapsule {
        sender_id: 9009,
        sequence: 22,
        payload: response_payload.clone(),
    };
    let response_wire =
        response_capsule.seal(&key).expect("seal SP3M response");

    let (client_wire, response_segments) =
        simulate_sms_transfer(&response_wire, send_policy);
    let mut client_replay = SmsReplayGuard::new(32);
    let client_capsule = client_replay
        .open_and_accept(&client_wire, &key)
        .expect("authenticate response capsule");
    let client_response =
        decode_response(&client_capsule.payload).expect("decode response");
    assert_eq!(client_response, response);

    println!(
        "F6_SMS_GATEWAY_PASS hostname_len={} request_payload_bytes={} request_capsule_bytes={} request_segments={} response_payload_bytes={} response_capsule_bytes={} response_segments={} addresses={} cost_bearing=true user_consented=true replay_rejected=true",
        request.hostname.len(),
        request_payload.len(),
        request_wire.len(),
        request_segments,
        response_payload.len(),
        response_wire.len(),
        response_segments,
        response.addresses.len(),
    );
}

fn simulate_sms_transfer(
    capsule_wire: &[u8],
    policy: SmsSendPolicy,
) -> (Vec<u8>, usize) {
    let segments = segment_capsule(capsule_wire).expect("segment capsule");
    policy
        .authorize(segments.len())
        .expect("authorize segment count");

    let segment_count = segments.len();
    let mut encoded = segments
        .iter()
        .map(|segment| segment.encode().expect("encode SMS segment"))
        .collect::<Vec<_>>();

    encoded.reverse();
    if let Some(first) = encoded.first().cloned() {
        encoded.insert(1.min(encoded.len()), first);
    }

    let mut assembler = SmsAssembler::conservative();
    let mut completed = None;
    let mut duplicate_seen = false;

    for wire in encoded {
        let segment = SmsSegment::decode(&wire).expect("decode SMS segment");
        match assembler.accept(segment).expect("accept SMS segment") {
            AcceptOutcome::Incomplete => {}
            AcceptOutcome::Duplicate => duplicate_seen = true,
            AcceptOutcome::Complete(bytes) => {
                assert!(completed.is_none());
                completed = Some(bytes);
            }
        }
    }

    assert!(duplicate_seen, "court must exercise duplicate delivery");
    (
        completed.expect("SMS transfer must complete"),
        segment_count,
    )
}
