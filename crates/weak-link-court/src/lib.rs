use peer_egress::{
    decode_request, decode_response, encode_request, encode_response,
    handle_request, ResolveRequest, ResolveResponse, ResolveStatus, Resolver,
    KIND_RESOLVE_REQUEST, KIND_RESOLVE_RESPONSE,
};
use peer_session::{
    ClientHello, PeerKey, SecureSession, ServerHello, SessionRole,
};
use std::collections::HashMap;
use std::time::Duration;
use urt_core::{decode_exact, encode_exact, sha256, DecodeBudget, Digest32, ExactStrategy};

const LOSS_SCALE: u32 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeriodicOutage {
    /// Full up+down cycle.
    pub period: Duration,
    /// Down time at the end of each cycle.
    pub down_for: Duration,
}

impl PeriodicOutage {
    pub fn validate(self) -> Result<(), CourtError> {
        if self.period.is_zero() || self.down_for >= self.period {
            return Err(CourtError::InvalidProfile(
                "outage requires 0 < down_for < period",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeakLinkProfile {
    pub bitrate_bps: u64,
    pub chunk_bytes: usize,
    pub one_way_latency: Duration,
    /// Deterministic lower-layer attempt loss in parts per million.
    ///
    /// Lost chunks are retransmitted by the virtual reliable carrier. This
    /// models wire cost and completion time without corrupting the byte stream.
    pub loss_ppm: u32,
    pub outage: Option<PeriodicOutage>,
}

impl WeakLinkProfile {
    pub fn validate(self) -> Result<(), CourtError> {
        if self.bitrate_bps == 0 {
            return Err(CourtError::InvalidProfile(
                "bitrate_bps must be greater than zero",
            ));
        }
        if self.chunk_bytes == 0 {
            return Err(CourtError::InvalidProfile(
                "chunk_bytes must be greater than zero",
            ));
        }
        if self.loss_ppm >= LOSS_SCALE {
            return Err(CourtError::InvalidProfile(
                "loss_ppm must be below 1_000_000",
            ));
        }
        if let Some(outage) = self.outage {
            outage.validate()?;
        }
        Ok(())
    }

    pub fn ladder(bitrate_bps: u64) -> Self {
        Self {
            bitrate_bps,
            chunk_bytes: 16,
            one_way_latency: Duration::from_millis(250),
            loss_ppm: 0,
            outage: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkAccounting {
    pub elapsed: Duration,
    pub logical_messages: u64,
    pub delivered_chunks: u64,
    pub lost_chunk_attempts: u64,
    pub delivered_bytes: u64,
    pub attempted_bytes: u64,
    pub retransmitted_bytes: u64,
    pub outage_wait: Duration,
    pub serialization_time: Duration,
    pub propagation_time: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourtResult {
    pub profile: WeakLinkProfile,
    pub request_id: u32,
    pub hostname: String,
    pub response: ResolveResponse,
    pub request_payload_bytes: usize,
    pub response_payload_bytes: usize,
    pub useful_payload_bytes: usize,
    pub handshake_wire_bytes: usize,
    pub encrypted_request_frame_bytes: usize,
    pub encrypted_response_frame_bytes: usize,
    pub accounting: LinkAccounting,
}

impl CourtResult {
    pub fn succeeded(&self) -> bool {
        self.response.status == ResolveStatus::Ok
            && !self.response.addresses.is_empty()
    }

    pub fn useful_efficiency_ppm(&self) -> u32 {
        if self.accounting.attempted_bytes == 0 {
            return 0;
        }

        ((self.useful_payload_bytes as u128 * LOSS_SCALE as u128)
            / self.accounting.attempted_bytes as u128)
            .min(LOSS_SCALE as u128) as u32
    }
}

#[derive(Debug)]
pub enum CourtError {
    InvalidProfile(&'static str),
    Protocol(String),
    Session(String),
    UnexpectedFrameKind(u8),
    ArithmeticOverflow,
    Urt(String),
}

pub struct VirtualReliableLink {
    profile: WeakLinkProfile,
    elapsed_ns: u128,
    logical_messages: u64,
    delivered_chunks: u64,
    lost_chunk_attempts: u64,
    delivered_bytes: u64,
    attempted_bytes: u64,
    retransmitted_bytes: u64,
    outage_wait_ns: u128,
    serialization_ns: u128,
    propagation_ns: u128,
    loss_accumulator: u32,
}

impl VirtualReliableLink {
    pub fn new(profile: WeakLinkProfile) -> Result<Self, CourtError> {
        profile.validate()?;

        Ok(Self {
            profile,
            elapsed_ns: 0,
            logical_messages: 0,
            delivered_chunks: 0,
            lost_chunk_attempts: 0,
            delivered_bytes: 0,
            attempted_bytes: 0,
            retransmitted_bytes: 0,
            outage_wait_ns: 0,
            serialization_ns: 0,
            propagation_ns: 0,
            loss_accumulator: 0,
        })
    }

    pub fn transmit(&mut self, bytes: &[u8]) -> Result<Vec<u8>, CourtError> {
        self.logical_messages = self.logical_messages.saturating_add(1);
        let mut delivered = Vec::with_capacity(bytes.len());

        for chunk in bytes.chunks(self.profile.chunk_bytes) {
            loop {
                self.account_attempt(chunk.len())?;

                self.loss_accumulator = self
                    .loss_accumulator
                    .saturating_add(self.profile.loss_ppm);

                if self.loss_accumulator >= LOSS_SCALE {
                    self.loss_accumulator -= LOSS_SCALE;
                    self.lost_chunk_attempts =
                        self.lost_chunk_attempts.saturating_add(1);
                    self.retransmitted_bytes = self
                        .retransmitted_bytes
                        .saturating_add(chunk.len() as u64);
                    continue;
                }

                delivered.extend_from_slice(chunk);
                self.delivered_chunks = self.delivered_chunks.saturating_add(1);
                self.delivered_bytes = self
                    .delivered_bytes
                    .saturating_add(chunk.len() as u64);
                break;
            }
        }

        Ok(delivered)
    }

    pub fn accounting(&self) -> Result<LinkAccounting, CourtError> {
        Ok(LinkAccounting {
            elapsed: duration_from_ns(self.elapsed_ns)?,
            logical_messages: self.logical_messages,
            delivered_chunks: self.delivered_chunks,
            lost_chunk_attempts: self.lost_chunk_attempts,
            delivered_bytes: self.delivered_bytes,
            attempted_bytes: self.attempted_bytes,
            retransmitted_bytes: self.retransmitted_bytes,
            outage_wait: duration_from_ns(self.outage_wait_ns)?,
            serialization_time: duration_from_ns(self.serialization_ns)?,
            propagation_time: duration_from_ns(self.propagation_ns)?,
        })
    }

    fn account_attempt(&mut self, bytes: usize) -> Result<(), CourtError> {
        let bits = (bytes as u128)
            .checked_mul(8)
            .ok_or(CourtError::ArithmeticOverflow)?;
        let serialization_ns = bits
            .checked_mul(1_000_000_000)
            .ok_or(CourtError::ArithmeticOverflow)?
            .div_ceil(self.profile.bitrate_bps as u128);
        let latency_ns = self.profile.one_way_latency.as_nanos();

        self.consume_serialization(serialization_ns)?;

        self.propagation_ns = self
            .propagation_ns
            .checked_add(latency_ns)
            .ok_or(CourtError::ArithmeticOverflow)?;
        self.elapsed_ns = self
            .elapsed_ns
            .checked_add(latency_ns)
            .ok_or(CourtError::ArithmeticOverflow)?;
        self.attempted_bytes = self
            .attempted_bytes
            .saturating_add(bytes as u64);

        Ok(())
    }

    fn consume_serialization(
        &mut self,
        mut remaining_ns: u128,
    ) -> Result<(), CourtError> {
        while remaining_ns > 0 {
            let Some(outage) = self.profile.outage else {
                self.serialization_ns = self
                    .serialization_ns
                    .checked_add(remaining_ns)
                    .ok_or(CourtError::ArithmeticOverflow)?;
                self.elapsed_ns = self
                    .elapsed_ns
                    .checked_add(remaining_ns)
                    .ok_or(CourtError::ArithmeticOverflow)?;
                return Ok(());
            };

            let period_ns = outage.period.as_nanos();
            let down_ns = outage.down_for.as_nanos();
            let up_ns = period_ns
                .checked_sub(down_ns)
                .ok_or(CourtError::ArithmeticOverflow)?;
            let phase = self.elapsed_ns % period_ns;

            if phase >= up_ns {
                let wait_ns = period_ns - phase;
                self.elapsed_ns = self
                    .elapsed_ns
                    .checked_add(wait_ns)
                    .ok_or(CourtError::ArithmeticOverflow)?;
                self.outage_wait_ns = self
                    .outage_wait_ns
                    .checked_add(wait_ns)
                    .ok_or(CourtError::ArithmeticOverflow)?;
                continue;
            }

            let usable_ns = up_ns - phase;
            let step_ns = remaining_ns.min(usable_ns);

            self.serialization_ns = self
                .serialization_ns
                .checked_add(step_ns)
                .ok_or(CourtError::ArithmeticOverflow)?;
            self.elapsed_ns = self
                .elapsed_ns
                .checked_add(step_ns)
                .ok_or(CourtError::ArithmeticOverflow)?;
            remaining_ns -= step_ns;
        }

        Ok(())
    }
}

pub fn standard_ladder() -> [WeakLinkProfile; 4] {
    [
        WeakLinkProfile::ladder(1_000),
        WeakLinkProfile::ladder(100),
        WeakLinkProfile::ladder(30),
        WeakLinkProfile::ladder(10),
    ]
}

pub fn run_resolve_court<R: Resolver>(
    profile: WeakLinkProfile,
    resolver: &R,
    hostname: &str,
) -> Result<CourtResult, CourtError> {
    let mut link = VirtualReliableLink::new(profile)?;
    let key = PeerKey::new([0x47; 32]);

    // Phase 1: authenticated handshake travels through the constrained link.
    let client_hello = ClientHello::from_nonce(100, [0x11; 24], &key);
    let client_hello_bytes = client_hello.encode();
    let client_wire = link.transmit(&client_hello_bytes)?;
    let server_seen = ClientHello::decode(&client_wire)
        .map_err(|error| CourtError::Session(format!("{error:?}")))?;
    server_seen
        .verify(&key)
        .map_err(|error| CourtError::Session(format!("{error:?}")))?;

    let server_hello =
        ServerHello::from_nonce(200, [0x22; 24], &server_seen, &key);
    let server_hello_bytes = server_hello.encode();
    let server_wire = link.transmit(&server_hello_bytes)?;
    let client_seen = ServerHello::decode(&server_wire)
        .map_err(|error| CourtError::Session(format!("{error:?}")))?;
    client_seen
        .verify(&client_hello, &key)
        .map_err(|error| CourtError::Session(format!("{error:?}")))?;

    let mut client_session = SecureSession::from_handshake(
        SessionRole::Client,
        &key,
        &client_hello,
        &client_seen,
    )
    .map_err(|error| CourtError::Session(format!("{error:?}")))?;
    let mut server_session = SecureSession::from_handshake(
        SessionRole::Server,
        &key,
        &server_seen,
        &server_hello,
    )
    .map_err(|error| CourtError::Session(format!("{error:?}")))?;

    // Phase 2: a compact Internet task crosses as a real encrypted frame.
    let request = ResolveRequest {
        request_id: 501,
        relays_remaining: 1,
        hostname: hostname.to_owned(),
    };
    let request_payload = encode_request(&request)
        .map_err(|error| CourtError::Protocol(format!("{error:?}")))?;
    let request_frame = client_session
        .seal(KIND_RESOLVE_REQUEST, &request_payload)
        .map_err(|error| CourtError::Session(format!("{error:?}")))?;
    let request_frame_bytes = request_frame.len();
    let request_wire = link.transmit(&request_frame)?;

    let (request_kind, request_plaintext) = server_session
        .open(&request_wire)
        .map_err(|error| CourtError::Session(format!("{error:?}")))?;
    if request_kind != KIND_RESOLVE_REQUEST {
        return Err(CourtError::UnexpectedFrameKind(request_kind));
    }

    let decoded_request = decode_request(&request_plaintext)
        .map_err(|error| CourtError::Protocol(format!("{error:?}")))?;
    let response = handle_request(decoded_request, resolver);
    let response_payload = encode_response(&response)
        .map_err(|error| CourtError::Protocol(format!("{error:?}")))?;
    let response_frame = server_session
        .seal(KIND_RESOLVE_RESPONSE, &response_payload)
        .map_err(|error| CourtError::Session(format!("{error:?}")))?;
    let response_frame_bytes = response_frame.len();
    let response_wire = link.transmit(&response_frame)?;

    let (response_kind, response_plaintext) = client_session
        .open(&response_wire)
        .map_err(|error| CourtError::Session(format!("{error:?}")))?;
    if response_kind != KIND_RESOLVE_RESPONSE {
        return Err(CourtError::UnexpectedFrameKind(response_kind));
    }
    let final_response = decode_response(&response_plaintext)
        .map_err(|error| CourtError::Protocol(format!("{error:?}")))?;

    let request_payload_bytes = request_payload.len();
    let response_payload_bytes = response_payload.len();

    Ok(CourtResult {
        profile,
        request_id: request.request_id,
        hostname: request.hostname,
        response: final_response,
        request_payload_bytes,
        response_payload_bytes,
        useful_payload_bytes: request_payload_bytes + response_payload_bytes,
        handshake_wire_bytes:
            client_hello_bytes.len() + server_hello_bytes.len(),
        encrypted_request_frame_bytes: request_frame_bytes,
        encrypted_response_frame_bytes: response_frame_bytes,
        accounting: link.accounting()?,
    })
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrtExactCourtResult {
    pub profile: WeakLinkProfile,
    pub strategy: ExactStrategy,
    pub original_bytes: u64,
    pub network_bytes: u64,
    pub shared_state_bytes: u64,
    pub exact_hash: Digest32,
    pub accounting: LinkAccounting,
}

/// Sends one exact URT packet through the same constrained-link simulator used
/// by the G7 weak-link court, then reconstructs and verifies the bytes at the
/// receiver. Shared base state is explicitly accounted for.
pub fn run_urt_exact_court(
    profile: WeakLinkProfile,
    input: &[u8],
    known_base: Option<&[u8]>,
) -> Result<UrtExactCourtResult, CourtError> {
    let encoded = encode_exact(input, known_base);
    let wire = encoded.packet.to_bytes();
    let mut link = VirtualReliableLink::new(profile)?;
    let delivered = link.transmit(&wire)?;

    let mut cache = HashMap::<Digest32, Vec<u8>>::new();
    if let Some(base) = known_base {
        cache.insert(sha256(base), base.to_vec());
    }

    let decoded = decode_exact(
        &delivered,
        &cache,
        DecodeBudget::permissive_for(input.len() as u64),
    )
    .map_err(|error| CourtError::Urt(error.to_string()))?;

    if decoded != input {
        return Err(CourtError::Urt(
            "exact URT court reconstructed different bytes".to_owned(),
        ));
    }

    Ok(UrtExactCourtResult {
        profile,
        strategy: encoded.report.strategy,
        original_bytes: encoded.report.original_bytes,
        network_bytes: encoded.report.network_bytes,
        shared_state_bytes: encoded.report.shared_state_bytes,
        exact_hash: sha256(&decoded),
        accounting: link.accounting()?,
    })
}

fn duration_from_ns(value: u128) -> Result<Duration, CourtError> {
    let seconds = value / 1_000_000_000;
    let nanos = (value % 1_000_000_000) as u32;
    let seconds =
        u64::try_from(seconds).map_err(|_| CourtError::ArithmeticOverflow)?;
    Ok(Duration::new(seconds, nanos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::net::IpAddr;

    struct FixedResolver;

    impl Resolver for FixedResolver {
        fn resolve(&self, _hostname: &str) -> io::Result<Vec<IpAddr>> {
            Ok(vec![
                "10.0.0.7".parse().unwrap(),
                "8.8.8.8".parse().unwrap(),
            ])
        }
    }

    #[test]
    fn urt_zstandard_text_survives_hundred_bps_virtual_link() {
        let mut input = Vec::new();
        for index in 0..2_000_u32 {
            let line = format!(
                "node={index} route=peer-egress status=degraded freshness=fresh_remote\n"
            );
            input.extend_from_slice(line.as_bytes());
        }

        let result = run_urt_exact_court(
            WeakLinkProfile::ladder(100),
            &input,
            None,
        )
        .unwrap();

        assert_eq!(result.strategy, ExactStrategy::Zstandard);
        assert_eq!(result.exact_hash, sha256(&input));
        assert!(result.network_bytes < result.original_bytes / 3);
    }

    #[test]
    fn urt_exact_payload_survives_ten_bps_virtual_link() {
        let input = b"QWER".repeat(64 * 1024);
        let result = run_urt_exact_court(
            WeakLinkProfile::ladder(10),
            &input,
            None,
        )
        .unwrap();

        assert_eq!(result.strategy, ExactStrategy::RepeatPattern);
        assert_eq!(result.exact_hash, sha256(&input));
        assert!(result.network_bytes < 128);
        assert!(result.accounting.delivered_bytes < 128);
        assert!(result.accounting.elapsed < Duration::from_secs(120));
    }

    #[test]
    fn urt_delta_court_keeps_shared_state_accounting_explicit() {
        let mut base = vec![0x11_u8; 128 * 1024];
        for (index, byte) in base.iter_mut().enumerate() {
            *byte = (index % 251) as u8;
        }
        let mut changed = base.clone();
        changed[64_000..64_016].copy_from_slice(&[0xE1; 16]);

        let result = run_urt_exact_court(
            WeakLinkProfile::ladder(100),
            &changed,
            Some(&base),
        )
        .unwrap();

        assert_eq!(result.strategy, ExactStrategy::BaseDelta);
        assert_eq!(result.shared_state_bytes, base.len() as u64);
        assert!(result.network_bytes < 256);
        assert_eq!(result.exact_hash, sha256(&changed));
    }

    #[test]
    fn standard_ladder_completes_useful_task_at_every_tier() {
        let mut previous_elapsed = Duration::ZERO;

        for profile in standard_ladder() {
            let result =
                run_resolve_court(profile, &FixedResolver, "example.com")
                    .unwrap();

            assert!(result.succeeded());
            assert_eq!(
                result.response.addresses,
                vec!["8.8.8.8".parse::<IpAddr>().unwrap()]
            );
            assert!(result.accounting.elapsed > previous_elapsed);
            assert!(result.accounting.delivered_bytes > 0);
            assert!(result.useful_efficiency_ppm() > 0);

            previous_elapsed = result.accounting.elapsed;
        }
    }

    #[test]
    fn ten_bps_is_slow_but_still_completes_in_virtual_time() {
        let result = run_resolve_court(
            WeakLinkProfile::ladder(10),
            &FixedResolver,
            "example.com",
        )
        .unwrap();

        assert!(result.succeeded());
        assert_eq!(result.handshake_wire_bytes, 140);
        assert_eq!(result.encrypted_request_frame_bytes, 49);
        assert_eq!(result.encrypted_response_frame_bytes, 43);
        assert_eq!(result.accounting.delivered_bytes, 232);
        assert!(result.useful_efficiency_ppm() >= 120_000);
        assert!(result.accounting.elapsed > Duration::from_secs(180));
        assert!(result.accounting.elapsed <= Duration::from_secs(190));
    }

    #[test]
    fn one_bps_experimental_extension_eventually_completes() {
        let result = run_resolve_court(
            WeakLinkProfile::ladder(1),
            &FixedResolver,
            "example.com",
        )
        .unwrap();

        assert!(result.succeeded());
        assert!(result.accounting.elapsed > Duration::from_secs(1_000));
        assert!(result.accounting.elapsed < Duration::from_secs(4_000));
    }

    #[test]
    fn loss_and_periodic_outage_increase_wire_cost_but_preserve_result() {
        let baseline = run_resolve_court(
            WeakLinkProfile::ladder(100),
            &FixedResolver,
            "example.com",
        )
        .unwrap();

        let harsh = run_resolve_court(
            WeakLinkProfile {
                bitrate_bps: 100,
                chunk_bytes: 8,
                one_way_latency: Duration::from_millis(300),
                loss_ppm: 200_000,
                outage: Some(PeriodicOutage {
                    period: Duration::from_secs(15),
                    down_for: Duration::from_secs(4),
                }),
            },
            &FixedResolver,
            "example.com",
        )
        .unwrap();

        assert!(harsh.succeeded());
        assert!(harsh.accounting.elapsed > baseline.accounting.elapsed);
        assert!(harsh.accounting.lost_chunk_attempts > 0);
        assert!(harsh.accounting.retransmitted_bytes > 0);
        assert!(harsh.accounting.outage_wait > Duration::ZERO);
    }

    #[test]
    fn outage_pauses_serialization_even_when_it_starts_mid_chunk() {
        let mut link = VirtualReliableLink::new(WeakLinkProfile {
            bitrate_bps: 8,
            chunk_bytes: 2,
            one_way_latency: Duration::ZERO,
            loss_ppm: 0,
            outage: Some(PeriodicOutage {
                period: Duration::from_secs(3),
                down_for: Duration::from_secs(1),
            }),
        })
        .unwrap();

        let delivered = link.transmit(&[1, 2, 3, 4]).unwrap();
        assert_eq!(delivered, vec![1, 2, 3, 4]);

        let accounting = link.accounting().unwrap();
        assert_eq!(accounting.serialization_time, Duration::from_secs(4));
        assert_eq!(accounting.outage_wait, Duration::from_secs(1));
        assert_eq!(accounting.elapsed, Duration::from_secs(5));
    }

    #[test]
    fn invalid_profiles_are_rejected() {
        let invalid = WeakLinkProfile {
            bitrate_bps: 0,
            chunk_bytes: 16,
            one_way_latency: Duration::ZERO,
            loss_ppm: 0,
            outage: None,
        };
        assert!(matches!(
            VirtualReliableLink::new(invalid),
            Err(CourtError::InvalidProfile(_))
        ));

        let impossible_loss = WeakLinkProfile {
            bitrate_bps: 10,
            chunk_bytes: 16,
            one_way_latency: Duration::ZERO,
            loss_ppm: 1_000_000,
            outage: None,
        };
        assert!(matches!(
            VirtualReliableLink::new(impossible_loss),
            Err(CourtError::InvalidProfile(_))
        ));
    }
}
