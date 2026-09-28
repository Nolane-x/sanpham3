use jni::objects::{JByteArray, JObject};
use jni::sys::{jbyteArray, jint, jlong};
use jni::JNIEnv;
use peer_session::{
    frame_ciphertext_len_from_header, ClientHello, NonceReplayCache, PeerKey,
    SecureSession, ServerHello, SessionRole, FRAME_HEADER_LEN, HANDSHAKE_LEN,
};
use std::collections::HashMap;
use std::fmt;
use std::ptr;
use std::sync::{Mutex, OnceLock};

const HANDLE_PREFIX_LEN: usize = 8;

#[derive(Debug)]
enum BridgeError {
    InvalidKeyLength(usize),
    InvalidNodeId,
    InvalidHandle(u64),
    InvalidKind(i32),
    Session(String),
    Poisoned,
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKeyLength(length) => {
                write!(f, "peer key must be exactly 32 bytes, got {length}")
            }
            Self::InvalidNodeId => write!(f, "node_id must be non-negative"),
            Self::InvalidHandle(handle) => {
                write!(f, "unknown peer-session handle {handle}")
            }
            Self::InvalidKind(kind) => {
                write!(f, "frame kind must be in 0..=255, got {kind}")
            }
            Self::Session(detail) => write!(f, "peer-session error: {detail}"),
            Self::Poisoned => write!(f, "peer-session bridge state is poisoned"),
        }
    }
}

struct PendingClient {
    key: PeerKey,
    hello: ClientHello,
}

struct BridgeState {
    next_handle: u64,
    pending_clients: HashMap<u64, PendingClient>,
    sessions: HashMap<u64, SecureSession>,
    replay_cache: NonceReplayCache,
}

impl Default for BridgeState {
    fn default() -> Self {
        Self {
            next_handle: 1,
            pending_clients: HashMap::new(),
            sessions: HashMap::new(),
            replay_cache: NonceReplayCache::new(4096),
        }
    }
}

impl BridgeState {
    fn allocate_handle(&mut self) -> u64 {
        loop {
            let handle = self.next_handle.max(1);
            self.next_handle = self.next_handle.wrapping_add(1).max(1);

            if !self.pending_clients.contains_key(&handle)
                && !self.sessions.contains_key(&handle)
            {
                return handle;
            }
        }
    }

    fn client_begin(
        &mut self,
        node_id: u64,
        key_bytes: &[u8],
    ) -> Result<Vec<u8>, BridgeError> {
        let key = parse_key(key_bytes)?;
        let hello = ClientHello::new(node_id, &key)
            .map_err(|error| BridgeError::Session(format!("{error:?}")))?;
        let handle = self.allocate_handle();

        let mut package = Vec::with_capacity(HANDLE_PREFIX_LEN + HANDSHAKE_LEN);
        package.extend_from_slice(&handle.to_be_bytes());
        package.extend_from_slice(&hello.encode());

        self.pending_clients.insert(
            handle,
            PendingClient {
                key,
                hello,
            },
        );

        Ok(package)
    }

    fn server_accept(
        &mut self,
        server_id: u64,
        key_bytes: &[u8],
        client_hello_bytes: &[u8],
    ) -> Result<Vec<u8>, BridgeError> {
        let key = parse_key(key_bytes)?;
        let client = ClientHello::decode(client_hello_bytes)
            .map_err(|error| BridgeError::Session(format!("{error:?}")))?;
        client
            .verify(&key)
            .map_err(|error| BridgeError::Session(format!("{error:?}")))?;

        if !self.replay_cache.accept(client.node_id, client.nonce) {
            return Err(BridgeError::Session("ReplayDetected".to_owned()));
        }

        let server = ServerHello::new(server_id, &client, &key)
            .map_err(|error| BridgeError::Session(format!("{error:?}")))?;
        let session = SecureSession::from_handshake(
            SessionRole::Server,
            &key,
            &client,
            &server,
        )
        .map_err(|error| BridgeError::Session(format!("{error:?}")))?;
        let handle = self.allocate_handle();
        self.sessions.insert(handle, session);

        let mut package =
            Vec::with_capacity(HANDLE_PREFIX_LEN * 2 + HANDSHAKE_LEN);
        package.extend_from_slice(&handle.to_be_bytes());
        package.extend_from_slice(&client.node_id.to_be_bytes());
        package.extend_from_slice(&server.encode());

        Ok(package)
    }

    fn client_finish(
        &mut self,
        pending_handle: u64,
        server_hello_bytes: &[u8],
    ) -> Result<(u64, u64), BridgeError> {
        let pending = self
            .pending_clients
            .remove(&pending_handle)
            .ok_or(BridgeError::InvalidHandle(pending_handle))?;

        let server = ServerHello::decode(server_hello_bytes)
            .map_err(|error| BridgeError::Session(format!("{error:?}")))?;
        server
            .verify(&pending.hello, &pending.key)
            .map_err(|error| BridgeError::Session(format!("{error:?}")))?;

        let session = SecureSession::from_handshake(
            SessionRole::Client,
            &pending.key,
            &pending.hello,
            &server,
        )
        .map_err(|error| BridgeError::Session(format!("{error:?}")))?;

        let handle = self.allocate_handle();
        self.sessions.insert(handle, session);
        Ok((handle, server.node_id))
    }

    fn seal(
        &mut self,
        handle: u64,
        kind: u8,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, BridgeError> {
        let session = self
            .sessions
            .get_mut(&handle)
            .ok_or(BridgeError::InvalidHandle(handle))?;

        session
            .seal(kind, plaintext)
            .map_err(|error| BridgeError::Session(format!("{error:?}")))
    }

    fn open(
        &mut self,
        handle: u64,
        frame: &[u8],
    ) -> Result<Vec<u8>, BridgeError> {
        let session = self
            .sessions
            .get_mut(&handle)
            .ok_or(BridgeError::InvalidHandle(handle))?;
        let (kind, plaintext) = session
            .open(frame)
            .map_err(|error| BridgeError::Session(format!("{error:?}")))?;

        let mut result = Vec::with_capacity(1 + plaintext.len());
        result.push(kind);
        result.extend_from_slice(&plaintext);
        Ok(result)
    }

    fn close(&mut self, handle: u64) -> bool {
        let pending = self.pending_clients.remove(&handle).is_some();
        let session = self.sessions.remove(&handle).is_some();
        pending || session
    }
}

fn parse_key(bytes: &[u8]) -> Result<PeerKey, BridgeError> {
    let key: [u8; 32] = bytes
        .try_into()
        .map_err(|_| BridgeError::InvalidKeyLength(bytes.len()))?;
    Ok(PeerKey::new(key))
}

fn state() -> &'static Mutex<BridgeState> {
    static STATE: OnceLock<Mutex<BridgeState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(BridgeState::default()))
}

fn with_state<T>(
    operation: impl FnOnce(&mut BridgeState) -> Result<T, BridgeError>,
) -> Result<T, BridgeError> {
    let mut guard = state().lock().map_err(|_| BridgeError::Poisoned)?;
    operation(&mut guard)
}

fn node_id(value: jlong) -> Result<u64, BridgeError> {
    u64::try_from(value).map_err(|_| BridgeError::InvalidNodeId)
}

fn handle_id(value: jlong) -> Result<u64, BridgeError> {
    u64::try_from(value).map_err(|_| BridgeError::InvalidHandle(0))
}

fn frame_kind(value: jint) -> Result<u8, BridgeError> {
    u8::try_from(value).map_err(|_| BridgeError::InvalidKind(value))
}

#[cfg(test)]
fn package_handle(bytes: &[u8]) -> u64 {
    let prefix: [u8; HANDLE_PREFIX_LEN] = bytes[..HANDLE_PREFIX_LEN]
        .try_into()
        .expect("test package must contain handle prefix");
    u64::from_be_bytes(prefix)
}

fn jni_bytes(
    env: &mut JNIEnv<'_>,
    bytes: Result<Vec<u8>, BridgeError>,
) -> jbyteArray {
    match bytes {
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
                "java/lang/IllegalStateException",
                error.to_string(),
            );
            ptr::null_mut()
        }
    }
}

fn java_bytes(
    env: &JNIEnv<'_>,
    bytes: &JByteArray<'_>,
) -> Result<Vec<u8>, BridgeError> {
    env.convert_byte_array(bytes)
        .map_err(|error| BridgeError::Session(error.to_string()))
}

#[no_mangle]
pub extern "system" fn Java_dev_nolane_sanpham3_androidhost_AndroidPeerSessionNative_handshakeLength(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jint {
    HANDSHAKE_LEN as jint
}

#[no_mangle]
pub extern "system" fn Java_dev_nolane_sanpham3_androidhost_AndroidPeerSessionNative_frameHeaderLength(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jint {
    FRAME_HEADER_LEN as jint
}

#[no_mangle]
pub extern "system" fn Java_dev_nolane_sanpham3_androidhost_AndroidPeerSessionNative_clientBegin(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    node_id_value: jlong,
    key: JByteArray<'_>,
) -> jbyteArray {
    let result = (|| {
        let node_id = node_id(node_id_value)?;
        let key = java_bytes(&env, &key)?;
        with_state(|state| state.client_begin(node_id, &key))
    })();

    jni_bytes(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_dev_nolane_sanpham3_androidhost_AndroidPeerSessionNative_serverAccept(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    node_id_value: jlong,
    key: JByteArray<'_>,
    client_hello: JByteArray<'_>,
) -> jbyteArray {
    let result = (|| {
        let node_id = node_id(node_id_value)?;
        let key = java_bytes(&env, &key)?;
        let client_hello = java_bytes(&env, &client_hello)?;
        with_state(|state| {
            state.server_accept(node_id, &key, &client_hello)
        })
    })();

    jni_bytes(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_dev_nolane_sanpham3_androidhost_AndroidPeerSessionNative_clientFinish(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    pending_handle: jlong,
    server_hello: JByteArray<'_>,
) -> jbyteArray {
    let result = (|| {
        let pending_handle = handle_id(pending_handle)?;
        let server_hello = java_bytes(&env, &server_hello)?;
        let (session_handle, peer_id) = with_state(|state| {
            state.client_finish(pending_handle, &server_hello)
        })?;

        let mut package = Vec::with_capacity(HANDLE_PREFIX_LEN * 2);
        package.extend_from_slice(&session_handle.to_be_bytes());
        package.extend_from_slice(&peer_id.to_be_bytes());
        Ok(package)
    })();

    jni_bytes(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_dev_nolane_sanpham3_androidhost_AndroidPeerSessionNative_seal(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    kind: jint,
    plaintext: JByteArray<'_>,
) -> jbyteArray {
    let result = (|| {
        let handle = handle_id(handle)?;
        let kind = frame_kind(kind)?;
        let plaintext = java_bytes(&env, &plaintext)?;
        with_state(|state| state.seal(handle, kind, &plaintext))
    })();

    jni_bytes(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_dev_nolane_sanpham3_androidhost_AndroidPeerSessionNative_open(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    frame: JByteArray<'_>,
) -> jbyteArray {
    let result = (|| {
        let handle = handle_id(handle)?;
        let frame = java_bytes(&env, &frame)?;
        with_state(|state| state.open(handle, &frame))
    })();

    jni_bytes(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_dev_nolane_sanpham3_androidhost_AndroidPeerSessionNative_frameCiphertextLength(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    header: JByteArray<'_>,
) -> jint {
    let result = (|| {
        let header = java_bytes(&env, &header)?;
        frame_ciphertext_len_from_header(&header)
            .map_err(|error| BridgeError::Session(format!("{error:?}")))
    })();

    match result {
        Ok(length) => length as jint,
        Err(error) => {
            let _ = env.throw_new(
                "java/lang/IllegalStateException",
                error.to_string(),
            );
            -1
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_dev_nolane_sanpham3_androidhost_AndroidPeerSessionNative_closeHandle(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    let result = (|| {
        let handle = handle_id(handle)?;
        with_state(|state| {
            state.close(handle);
            Ok(())
        })
    })();

    if let Err(error) = result {
        let _ = env.throw_new(
            "java/lang/IllegalStateException",
            error.to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] {
        [0x42; 32]
    }

    #[test]
    fn bridge_client_and_server_interoperate_using_real_peer_session() {
        let mut client_bridge = BridgeState::default();
        let mut server_bridge = BridgeState::default();

        let client_package = client_bridge.client_begin(100, &key()).unwrap();
        let pending_handle = package_handle(&client_package);
        let client_hello = &client_package[HANDLE_PREFIX_LEN..];

        let server_package = server_bridge
            .server_accept(200, &key(), client_hello)
            .unwrap();
        let server_handle = package_handle(&server_package);
        let server_peer_id = u64::from_be_bytes(
            server_package[8..16].try_into().unwrap(),
        );
        let server_hello = &server_package[HANDLE_PREFIX_LEN * 2..];

        let (client_handle, client_peer_id) = client_bridge
            .client_finish(pending_handle, server_hello)
            .unwrap();

        assert_eq!(server_peer_id, 100);
        assert_eq!(client_peer_id, 200);

        let frame = client_bridge
            .seal(client_handle, 7, b"android-to-rust")
            .unwrap();
        let opened = server_bridge.open(server_handle, &frame).unwrap();

        assert_eq!(opened[0], 7);
        assert_eq!(&opened[1..], b"android-to-rust");

        let reply = server_bridge
            .seal(server_handle, 8, b"rust-to-android")
            .unwrap();
        let opened = client_bridge.open(client_handle, &reply).unwrap();

        assert_eq!(opened[0], 8);
        assert_eq!(&opened[1..], b"rust-to-android");
    }

    #[test]
    fn replayed_client_hello_is_rejected_on_server_bridge() {
        let mut client_bridge = BridgeState::default();
        let mut server_bridge = BridgeState::default();

        let package = client_bridge.client_begin(100, &key()).unwrap();
        let client_hello = &package[HANDLE_PREFIX_LEN..];

        server_bridge
            .server_accept(200, &key(), client_hello)
            .unwrap();

        let replay = server_bridge.server_accept(200, &key(), client_hello);
        assert!(matches!(replay, Err(BridgeError::Session(_))));
    }

    #[test]
    fn frame_length_helper_matches_sealed_frame() {
        let mut client_bridge = BridgeState::default();
        let mut server_bridge = BridgeState::default();

        let client_package = client_bridge.client_begin(1, &key()).unwrap();
        let pending_handle = package_handle(&client_package);
        let server_package = server_bridge
            .server_accept(
                2,
                &key(),
                &client_package[HANDLE_PREFIX_LEN..],
            )
            .unwrap();
        let (client_handle, peer_id) = client_bridge
            .client_finish(
                pending_handle,
                &server_package[HANDLE_PREFIX_LEN * 2..],
            )
            .unwrap();
        assert_eq!(peer_id, 2);

        let frame = client_bridge.seal(client_handle, 3, b"hello").unwrap();
        let length =
            frame_ciphertext_len_from_header(&frame[..FRAME_HEADER_LEN])
                .unwrap();

        assert_eq!(length, frame.len() - FRAME_HEADER_LEN);
    }
}
