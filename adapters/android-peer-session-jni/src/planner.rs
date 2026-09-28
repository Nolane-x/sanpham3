use connectivity_core::{
    default_energy_cost, plan_recovery, ConnectivityGraph, LinkObservation,
    LinkState, NodeProfile, PlanReason, Reachability, RecoveryPathKind,
    RecoveryPlan, RecoveryTask, TrafficClass, Transport,
};
use jni::objects::{JByteArray, JObject};
use jni::sys::jbyteArray;
use jni::JNIEnv;
use std::fmt;
use std::ptr;
use std::time::Duration;

const REQUEST_MAGIC: [u8; 4] = *b"SP3P";
const RESPONSE_MAGIC: [u8; 4] = *b"SP3R";
const VERSION: u8 = 0;
const REQUEST_HEADER_LEN: usize = 27;
const PATH_LEN: usize = 32;
const RESPONSE_LEN: usize = 52;
const MAX_PATHS: usize = 256;
const LOCAL_NODE: u64 = 1;
const FIRST_PATH_NODE: u64 = 10_000;
const NO_PATH_INDEX: u16 = u16::MAX;

#[derive(Debug)]
enum PlannerError {
    WrongLength,
    WrongMagic,
    WrongVersion(u8),
    TooManyPaths(usize),
    InvalidTrafficClass(u8),
    InvalidFlags(u8),
    InvalidRoundTrips,
    InvalidPathKind(u8),
    InvalidTransport(u8),
    InvalidState(u8),
    InvalidBoolean(u8),
    InvalidLoss(u32),
    InvalidJvmUnsigned(&'static str, u64),
    PathNodeOverflow,
    Jni(String),
}

impl fmt::Display for PlannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength => write!(f, "invalid Android planner request length"),
            Self::WrongMagic => write!(f, "invalid Android planner request magic"),
            Self::WrongVersion(version) => {
                write!(f, "unsupported Android planner version {version}")
            }
            Self::TooManyPaths(count) => {
                write!(f, "Android planner path count {count} exceeds {MAX_PATHS}")
            }
            Self::InvalidTrafficClass(value) => {
                write!(f, "invalid Android planner traffic class {value}")
            }
            Self::InvalidFlags(value) => {
                write!(f, "invalid Android planner flags 0x{value:02x}")
            }
            Self::InvalidRoundTrips => {
                write!(f, "estimated round trips must be greater than zero")
            }
            Self::InvalidPathKind(value) => {
                write!(f, "invalid Android planner path kind {value}")
            }
            Self::InvalidTransport(value) => {
                write!(f, "invalid Android planner transport {value}")
            }
            Self::InvalidState(value) => {
                write!(f, "invalid Android planner link state {value}")
            }
            Self::InvalidBoolean(value) => {
                write!(f, "invalid Android planner boolean {value}")
            }
            Self::InvalidLoss(value) => {
                write!(f, "invalid Android planner loss_ppm {value}")
            }
            Self::InvalidJvmUnsigned(name, value) => {
                write!(
                    f,
                    "Android planner {name} {value} exceeds non-negative JVM Long range"
                )
            }
            Self::PathNodeOverflow => write!(f, "Android planner path node overflow"),
            Self::Jni(detail) => write!(f, "Android planner JNI error: {detail}"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum PlanningPathKind {
    DirectInternet,
    PeerEgress,
}

#[derive(Debug, Clone)]
struct PlanningPath {
    kind: PlanningPathKind,
    external_id: u64,
    transport: Transport,
    state: LinkState,
    bitrate_bps: u64,
    loss_ppm: u32,
    rtt: Duration,
    metered: bool,
}

#[derive(Debug)]
struct PlannerRequest {
    task: RecoveryTask,
    paths: Vec<PlanningPath>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], PlannerError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(PlannerError::WrongLength)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(PlannerError::WrongLength)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, PlannerError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, PlannerError> {
        Ok(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| PlannerError::WrongLength)?,
        ))
    }

    fn u32(&mut self) -> Result<u32, PlannerError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| PlannerError::WrongLength)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, PlannerError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| PlannerError::WrongLength)?,
        ))
    }
}

fn decode_request(bytes: &[u8]) -> Result<PlannerRequest, PlannerError> {
    if bytes.len() < REQUEST_HEADER_LEN {
        return Err(PlannerError::WrongLength);
    }

    let mut cursor = Cursor::new(bytes);
    if cursor.take(4)? != REQUEST_MAGIC {
        return Err(PlannerError::WrongMagic);
    }

    let version = cursor.u8()?;
    if version != VERSION {
        return Err(PlannerError::WrongVersion(version));
    }

    let path_count = usize::from(cursor.u16()?);
    if path_count > MAX_PATHS {
        return Err(PlannerError::TooManyPaths(path_count));
    }

    let class = match cursor.u8()? {
        0 => TrafficClass::Critical,
        1 => TrafficClass::TinySemantic,
        2 => TrafficClass::Interactive,
        3 => TrafficClass::Bulk,
        value => return Err(PlannerError::InvalidTrafficClass(value)),
    };

    let flags = cursor.u8()?;
    if flags & !0b0000_0011 != 0 {
        return Err(PlannerError::InvalidFlags(flags));
    }

    let round_trips = cursor.u16()?;
    if round_trips == 0 {
        return Err(PlannerError::InvalidRoundTrips);
    }

    let wire_bytes = cursor.u64()?;
    let max_wait_ms = cursor.u64()?;

    let expected_len = REQUEST_HEADER_LEN
        .checked_add(
            path_count
                .checked_mul(PATH_LEN)
                .ok_or(PlannerError::WrongLength)?,
        )
        .ok_or(PlannerError::WrongLength)?;
    if bytes.len() != expected_len {
        return Err(PlannerError::WrongLength);
    }

    let mut paths = Vec::with_capacity(path_count);
    for _ in 0..path_count {
        let kind = match cursor.u8()? {
            0 => PlanningPathKind::DirectInternet,
            1 => PlanningPathKind::PeerEgress,
            value => return Err(PlannerError::InvalidPathKind(value)),
        };
        let transport = decode_transport(cursor.u8()?)?;
        let state = match cursor.u8()? {
            0 => LinkState::Up,
            1 => LinkState::Intermittent,
            2 => LinkState::Down,
            value => return Err(PlannerError::InvalidState(value)),
        };
        let metered = decode_bool(cursor.u8()?)?;
        let external_id = jvm_unsigned("external_id", cursor.u64()?)?;
        let bitrate_bps = jvm_unsigned("bitrate_bps", cursor.u64()?)?.max(1);
        let loss_ppm = cursor.u32()?;
        if loss_ppm > 1_000_000 {
            return Err(PlannerError::InvalidLoss(loss_ppm));
        }
        let rtt_ms = jvm_unsigned("rtt_ms", cursor.u64()?)?;

        paths.push(PlanningPath {
            kind,
            external_id,
            transport,
            state,
            bitrate_bps,
            loss_ppm,
            rtt: Duration::from_millis(rtt_ms),
            metered,
        });
    }

    let mut task = RecoveryTask::new(class, wire_bytes);
    task.estimated_round_trips = round_trips;
    task.allow_metered = flags & 0b0000_0001 != 0;
    task.allow_delay_tolerant = flags & 0b0000_0010 != 0;
    task.max_live_wait = if max_wait_ms == u64::MAX {
        None
    } else {
        Some(Duration::from_millis(max_wait_ms))
    };

    Ok(PlannerRequest { task, paths })
}

fn decode_transport(value: u8) -> Result<Transport, PlannerError> {
    match value {
        0 => Ok(Transport::Wifi),
        1 => Ok(Transport::Cellular),
        2 => Ok(Transport::Ethernet),
        3 => Ok(Transport::Tunnel),
        4 => Ok(Transport::BluetoothLe),
        5 => Ok(Transport::WifiAware),
        6 => Ok(Transport::Satellite),
        7 | 8 => Ok(Transport::Other),
        other => Err(PlannerError::InvalidTransport(other)),
    }
}

fn jvm_unsigned(
    name: &'static str,
    value: u64,
) -> Result<u64, PlannerError> {
    if value > i64::MAX as u64 {
        return Err(PlannerError::InvalidJvmUnsigned(name, value));
    }
    Ok(value)
}

fn decode_bool(value: u8) -> Result<bool, PlannerError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(PlannerError::InvalidBoolean(other)),
    }
}

fn plan(request: &PlannerRequest) -> Result<Vec<u8>, PlannerError> {
    let mut graph = ConnectivityGraph::new();
    graph.upsert_node(NodeProfile::local(LOCAL_NODE));

    for (index, path) in request.paths.iter().enumerate() {
        let index_u64 = u64::try_from(index).map_err(|_| PlannerError::PathNodeOverflow)?;
        let node = FIRST_PATH_NODE
            .checked_add(index_u64)
            .ok_or(PlannerError::PathNodeOverflow)?;

        graph.upsert_node(NodeProfile::egress(node, path.metered));
        graph.observe_link(LinkObservation {
            from: LOCAL_NODE,
            to: node,
            transport: path.transport,
            reachability: match path.kind {
                PlanningPathKind::DirectInternet => Reachability::Internet,
                PlanningPathKind::PeerEgress => Reachability::PeerOnly,
            },
            state: path.state,
            estimated_bitrate_bps: path.bitrate_bps,
            loss_ppm: path.loss_ppm,
            rtt: path.rtt,
            energy_cost: default_energy_cost(path.transport),
            metered: path.metered,
            last_success_age: Duration::ZERO,
        });
    }

    let recovery = plan_recovery(&graph, LOCAL_NODE, &request.task);
    encode_response(&recovery, &request.paths)
}

fn encode_response(
    recovery: &RecoveryPlan,
    paths: &[PlanningPath],
) -> Result<Vec<u8>, PlannerError> {
    let mut out = Vec::with_capacity(RESPONSE_LEN);
    out.extend_from_slice(&RESPONSE_MAGIC);
    out.push(VERSION);

    match recovery {
        RecoveryPlan::Live(plan) => {
            out.push(0);
            out.push(match plan.mode {
                connectivity_core::DeliveryMode::Full => 0,
                connectivity_core::DeliveryMode::Compact => 1,
                connectivity_core::DeliveryMode::Semantic => 2,
                connectivity_core::DeliveryMode::TinySemantic => 3,
                connectivity_core::DeliveryMode::Emergency => 4,
            });
            out.push(match plan.path_kind {
                RecoveryPathKind::LocalEgress => 0,
                RecoveryPathKind::DirectInternet => 1,
                RecoveryPathKind::PeerEgress => 2,
            });
            out.push(0);
            out.push(u8::from(plan.experimental));

            let selected_node = plan.route_nodes.last().copied().unwrap_or(LOCAL_NODE);
            let selected_index = if selected_node >= FIRST_PATH_NODE {
                let raw = selected_node - FIRST_PATH_NODE;
                usize::try_from(raw)
                    .ok()
                    .filter(|index| *index < paths.len())
            } else {
                None
            };

            let path_index = selected_index
                .and_then(|index| u16::try_from(index).ok())
                .unwrap_or(NO_PATH_INDEX);
            let external_id = selected_index
                .and_then(|index| paths.get(index))
                .map(|path| path.external_id)
                .unwrap_or(0);

            out.extend_from_slice(&path_index.to_be_bytes());
            out.extend_from_slice(&external_id.to_be_bytes());
            out.extend_from_slice(
                &jvm_u64(plan.effective_bps).to_be_bytes(),
            );
            out.extend_from_slice(
                &duration_millis_jvm(plan.expected_completion).to_be_bytes(),
            );
            out.extend_from_slice(&plan.worst_loss_ppm.to_be_bytes());
            out.extend_from_slice(&duration_millis_jvm(plan.total_rtt).to_be_bytes());
            out.extend_from_slice(&plan.intermittent_hops.to_be_bytes());
            out.extend_from_slice(&plan.metered_hops.to_be_bytes());
        }
        RecoveryPlan::DelayTolerant { reason, .. } => {
            encode_non_live(&mut out, 1, reason);
        }
        RecoveryPlan::LocalOnly { reason } => {
            encode_non_live(&mut out, 2, reason);
        }
    }

    if out.len() != RESPONSE_LEN {
        return Err(PlannerError::WrongLength);
    }
    Ok(out)
}

fn encode_non_live(out: &mut Vec<u8>, kind: u8, reason: &PlanReason) {
    out.push(kind);
    out.push(u8::MAX);
    out.push(u8::MAX);
    out.push(reason_code(*reason));
    out.push(0);
    out.extend_from_slice(&NO_PATH_INDEX.to_be_bytes());
    out.extend_from_slice(&0_u64.to_be_bytes());
    out.extend_from_slice(&0_u64.to_be_bytes());
    out.extend_from_slice(&0_u64.to_be_bytes());
    out.extend_from_slice(&0_u32.to_be_bytes());
    out.extend_from_slice(&0_u64.to_be_bytes());
    out.extend_from_slice(&0_u16.to_be_bytes());
    out.extend_from_slice(&0_u16.to_be_bytes());
}

fn reason_code(reason: PlanReason) -> u8 {
    match reason {
        PlanReason::BestLivePath => 0,
        PlanReason::NoLiveEgress => 1,
        PlanReason::MeteredPathDisallowed => 2,
        PlanReason::TrafficTooHeavyForPath => 3,
        PlanReason::LiveWaitBudgetExceeded => 4,
    }
}

fn jvm_u64(value: u64) -> u64 {
    value.min(i64::MAX as u64)
}

fn duration_millis_jvm(duration: Duration) -> u64 {
    duration
        .as_millis()
        .min(i64::MAX as u128) as u64
}

fn java_bytes(
    env: &JNIEnv<'_>,
    bytes: &JByteArray<'_>,
) -> Result<Vec<u8>, PlannerError> {
    env.convert_byte_array(bytes)
        .map_err(|error| PlannerError::Jni(error.to_string()))
}

fn jni_bytes(
    env: &mut JNIEnv<'_>,
    result: Result<Vec<u8>, PlannerError>,
) -> jbyteArray {
    match result {
        Ok(bytes) => match env.byte_array_from_slice(&bytes) {
            Ok(array) => array.into_raw(),
            Err(error) => {
                let _ = env.throw_new(
                    "java/lang/IllegalStateException",
                    error.to_string(),
                );
                ptr::null_mut()
            }
        },
        Err(error) => {
            let _ = env.throw_new(
                "java/lang/IllegalArgumentException",
                error.to_string(),
            );
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_dev_nolane_sanpham3_androidhost_AndroidRecoveryPlannerNative_plan(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    request: JByteArray<'_>,
) -> jbyteArray {
    let result = (|| {
        let bytes = java_bytes(&env, &request)?;
        let request = decode_request(&bytes)?;
        plan(&request)
    })();

    jni_bytes(&mut env, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_path(
        out: &mut Vec<u8>,
        kind: u8,
        transport: u8,
        state: u8,
        metered: bool,
        external_id: u64,
        bitrate: u64,
        loss_ppm: u32,
        rtt_ms: u64,
    ) {
        out.push(kind);
        out.push(transport);
        out.push(state);
        out.push(u8::from(metered));
        out.extend_from_slice(&external_id.to_be_bytes());
        out.extend_from_slice(&bitrate.to_be_bytes());
        out.extend_from_slice(&loss_ppm.to_be_bytes());
        out.extend_from_slice(&rtt_ms.to_be_bytes());
    }

    fn request(
        class: u8,
        flags: u8,
        round_trips: u16,
        wire_bytes: u64,
        max_wait_ms: u64,
        paths: impl FnOnce(&mut Vec<u8>),
        path_count: u16,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&REQUEST_MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&path_count.to_be_bytes());
        out.push(class);
        out.push(flags);
        out.extend_from_slice(&round_trips.to_be_bytes());
        out.extend_from_slice(&wire_bytes.to_be_bytes());
        out.extend_from_slice(&max_wait_ms.to_be_bytes());
        paths(&mut out);
        out
    }

    #[test]
    fn dead_direct_path_is_skipped_for_peer_rescue() {
        let bytes = request(
            1,
            0b11,
            2,
            232,
            u64::MAX,
            |out| {
                push_path(out, 0, 0, 2, false, 11, 50_000_000, 1_000_000, 2_000);
                push_path(out, 1, 0, 0, false, 300, 100, 100_000, 180);
            },
            2,
        );

        let request = decode_request(&bytes).unwrap();
        let response = plan(&request).unwrap();

        assert_eq!(response.len(), RESPONSE_LEN);
        assert_eq!(&response[..4], &RESPONSE_MAGIC);
        assert_eq!(response[5], 0);
        assert_eq!(response[6], 3);
        assert_eq!(response[7], 2);
        assert_eq!(u16::from_be_bytes([response[10], response[11]]), 1);
        assert_eq!(
            u64::from_be_bytes(response[12..20].try_into().unwrap()),
            300
        );
        assert_eq!(
            u64::from_be_bytes(response[20..28].try_into().unwrap()),
            90
        );
    }

    #[test]
    fn disallowed_metered_path_returns_dtn() {
        let bytes = request(
            3,
            0b10,
            2,
            1_000_000,
            u64::MAX,
            |out| {
                push_path(out, 0, 1, 0, true, 77, 5_000_000, 0, 50);
            },
            1,
        );

        let request = decode_request(&bytes).unwrap();
        let response = plan(&request).unwrap();

        assert_eq!(response[5], 1);
        assert_eq!(response[8], 2);
        assert_eq!(
            u16::from_be_bytes([response[10], response[11]]),
            NO_PATH_INDEX
        );
    }

    #[test]
    fn no_paths_without_dtn_returns_local_only() {
        let bytes = request(0, 0, 1, 32, u64::MAX, |_| {}, 0);
        let request = decode_request(&bytes).unwrap();
        let response = plan(&request).unwrap();

        assert_eq!(response[5], 2);
        assert_eq!(response[8], 1);
    }

    #[test]
    fn rejects_unknown_flags_and_trailing_bytes() {
        let bad_flags = request(0, 0x80, 1, 32, u64::MAX, |_| {}, 0);
        assert!(matches!(
            decode_request(&bad_flags),
            Err(PlannerError::InvalidFlags(0x80))
        ));

        let mut trailing = request(0, 0, 1, 32, u64::MAX, |_| {}, 0);
        trailing.push(0);
        assert!(matches!(
            decode_request(&trailing),
            Err(PlannerError::WrongLength)
        ));
    }

    #[test]
    fn rejects_more_than_max_paths_before_parsing_entries() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&REQUEST_MAGIC);
        bytes.push(VERSION);
        bytes.extend_from_slice(&((MAX_PATHS as u16) + 1).to_be_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 1]);
        bytes.extend_from_slice(&0_u64.to_be_bytes());
        bytes.extend_from_slice(&u64::MAX.to_be_bytes());

        assert!(matches!(
            decode_request(&bytes),
            Err(PlannerError::TooManyPaths(_))
        ));
    }
}
