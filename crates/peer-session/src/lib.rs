use chacha20poly1305::{
    aead::{Aead, Key, KeyInit as AeadKeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use hmac::{Hmac, KeyInit as HmacKeyInit, Mac};
use sha2::Sha256;
use std::collections::{HashSet, VecDeque};
use std::io::{self, Read, Write};

type HmacSha256 = Hmac<Sha256>;

const MAGIC: [u8; 4] = *b"SP3S";
const VERSION: u8 = 0;
const CLIENT_HELLO_KIND: u8 = 1;
const SERVER_HELLO_KIND: u8 = 2;
const HANDSHAKE_NONCE_LEN: usize = 24;
const TAG_LEN: usize = 32;
pub const HANDSHAKE_LEN: usize =
    4 + 1 + 1 + 8 + HANDSHAKE_NONCE_LEN + TAG_LEN;
pub const FRAME_HEADER_LEN: usize = 16;
pub const AEAD_TAG_LEN: usize = 16;
pub const MAX_PLAINTEXT_LEN: usize = u16::MAX as usize - AEAD_TAG_LEN;

#[derive(Debug)]
pub enum SessionError {
    Io(io::Error),
    Entropy(String),
    WrongLength,
    WrongMagic,
    WrongVersion(u8),
    WrongHandshakeKind(u8),
    AuthenticationFailed,
    ReplayDetected,
    PayloadTooLarge,
    UnexpectedCounter { expected: u64, got: u64 },
    CounterExhausted,
    Crypto,
}

impl From<io::Error> for SessionError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct PeerKey([u8; 32]);

impl PeerKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Drop for PeerKey {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientHello {
    pub node_id: u64,
    pub nonce: [u8; HANDSHAKE_NONCE_LEN],
    tag: [u8; TAG_LEN],
}

impl ClientHello {
    pub fn new(node_id: u64, key: &PeerKey) -> Result<Self, SessionError> {
        let mut nonce = [0_u8; HANDSHAKE_NONCE_LEN];
        getrandom::fill(&mut nonce)
            .map_err(|error| SessionError::Entropy(error.to_string()))?;

        Ok(Self::from_nonce(node_id, nonce, key))
    }

    pub fn from_nonce(
        node_id: u64,
        nonce: [u8; HANDSHAKE_NONCE_LEN],
        key: &PeerKey,
    ) -> Self {
        let tag = client_tag(node_id, &nonce, key);
        Self {
            node_id,
            nonce,
            tag,
        }
    }

    pub fn verify(&self, key: &PeerKey) -> Result<(), SessionError> {
        verify_client_tag(self.node_id, &self.nonce, &self.tag, key)
    }

    pub fn encode(&self) -> [u8; HANDSHAKE_LEN] {
        encode_handshake(
            CLIENT_HELLO_KIND,
            self.node_id,
            &self.nonce,
            &self.tag,
        )
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, SessionError> {
        let (kind, node_id, nonce, tag) = decode_handshake(bytes)?;
        if kind != CLIENT_HELLO_KIND {
            return Err(SessionError::WrongHandshakeKind(kind));
        }

        Ok(Self {
            node_id,
            nonce,
            tag,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerHello {
    pub node_id: u64,
    pub nonce: [u8; HANDSHAKE_NONCE_LEN],
    tag: [u8; TAG_LEN],
}

impl ServerHello {
    pub fn new(
        server_id: u64,
        client: &ClientHello,
        key: &PeerKey,
    ) -> Result<Self, SessionError> {
        client.verify(key)?;

        let mut nonce = [0_u8; HANDSHAKE_NONCE_LEN];
        getrandom::fill(&mut nonce)
            .map_err(|error| SessionError::Entropy(error.to_string()))?;

        Ok(Self::from_nonce(server_id, nonce, client, key))
    }

    pub fn from_nonce(
        server_id: u64,
        nonce: [u8; HANDSHAKE_NONCE_LEN],
        client: &ClientHello,
        key: &PeerKey,
    ) -> Self {
        let tag = server_tag(client, server_id, &nonce, key);
        Self {
            node_id: server_id,
            nonce,
            tag,
        }
    }

    pub fn verify(
        &self,
        client: &ClientHello,
        key: &PeerKey,
    ) -> Result<(), SessionError> {
        verify_server_tag(client, self.node_id, &self.nonce, &self.tag, key)
    }

    pub fn encode(&self) -> [u8; HANDSHAKE_LEN] {
        encode_handshake(
            SERVER_HELLO_KIND,
            self.node_id,
            &self.nonce,
            &self.tag,
        )
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, SessionError> {
        let (kind, node_id, nonce, tag) = decode_handshake(bytes)?;
        if kind != SERVER_HELLO_KIND {
            return Err(SessionError::WrongHandshakeKind(kind));
        }

        Ok(Self {
            node_id,
            nonce,
            tag,
        })
    }
}

#[derive(Debug)]
pub struct NonceReplayCache {
    capacity: usize,
    order: VecDeque<(u64, [u8; HANDSHAKE_NONCE_LEN])>,
    seen: HashSet<(u64, [u8; HANDSHAKE_NONCE_LEN])>,
}

impl NonceReplayCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::new(),
            seen: HashSet::new(),
        }
    }

    pub fn accept(
        &mut self,
        node_id: u64,
        nonce: [u8; HANDSHAKE_NONCE_LEN],
    ) -> bool {
        let key = (node_id, nonce);
        if self.seen.contains(&key) {
            return false;
        }

        self.seen.insert(key);
        self.order.push_back(key);

        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }

        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRole {
    Client,
    Server,
}

pub struct SecureSession {
    cipher: XChaCha20Poly1305,
    send_prefix: [u8; 16],
    recv_prefix: [u8; 16],
    send_counter: u64,
    recv_counter: u64,
}

impl SecureSession {
    pub fn from_handshake(
        role: SessionRole,
        key: &PeerKey,
        client: &ClientHello,
        server: &ServerHello,
    ) -> Result<Self, SessionError> {
        client.verify(key)?;
        server.verify(client, key)?;

        let session_key = derive_session_key(key, client, server);
        let c2s = derive_prefix(&session_key, b"SP3/c2s/v0");
        let s2c = derive_prefix(&session_key, b"SP3/s2c/v0");

        let key = Key::<XChaCha20Poly1305>::from(session_key);
        let cipher = XChaCha20Poly1305::new(&key);

        let (send_prefix, recv_prefix) = match role {
            SessionRole::Client => (c2s, s2c),
            SessionRole::Server => (s2c, c2s),
        };

        Ok(Self {
            cipher,
            send_prefix,
            recv_prefix,
            send_counter: 0,
            recv_counter: 0,
        })
    }

    pub fn seal(
        &mut self,
        kind: u8,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, SessionError> {
        if plaintext.len() > MAX_PLAINTEXT_LEN {
            return Err(SessionError::PayloadTooLarge);
        }

        let counter = self.send_counter;
        let nonce = nonce_for(self.send_prefix, counter);
        let aad = frame_aad(kind, counter);

        let ciphertext = self
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| SessionError::Crypto)?;

        let ciphertext_len =
            u16::try_from(ciphertext.len()).map_err(|_| SessionError::PayloadTooLarge)?;

        let mut out = Vec::with_capacity(FRAME_HEADER_LEN + ciphertext.len());
        out.extend_from_slice(&MAGIC);
        out.push(VERSION);
        out.push(kind);
        out.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&ciphertext_len.to_be_bytes());
        out.extend_from_slice(&ciphertext);

        self.send_counter = self
            .send_counter
            .checked_add(1)
            .ok_or(SessionError::CounterExhausted)?;

        Ok(out)
    }

    pub fn open(&mut self, frame: &[u8]) -> Result<(u8, Vec<u8>), SessionError> {
        if frame.len() < FRAME_HEADER_LEN {
            return Err(SessionError::WrongLength);
        }
        if frame[0..4] != MAGIC {
            return Err(SessionError::WrongMagic);
        }
        if frame[4] != VERSION {
            return Err(SessionError::WrongVersion(frame[4]));
        }

        let kind = frame[5];
        let counter = u64::from_be_bytes(
            frame[6..14]
                .try_into()
                .map_err(|_| SessionError::WrongLength)?,
        );
        let ciphertext_len = u16::from_be_bytes([frame[14], frame[15]]) as usize;

        if frame.len() != FRAME_HEADER_LEN + ciphertext_len {
            return Err(SessionError::WrongLength);
        }
        if counter != self.recv_counter {
            return Err(SessionError::UnexpectedCounter {
                expected: self.recv_counter,
                got: counter,
            });
        }

        let nonce = nonce_for(self.recv_prefix, counter);
        let aad = frame_aad(kind, counter);

        let plaintext = self
            .cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &frame[FRAME_HEADER_LEN..],
                    aad: &aad,
                },
            )
            .map_err(|_| SessionError::Crypto)?;

        self.recv_counter = self
            .recv_counter
            .checked_add(1)
            .ok_or(SessionError::CounterExhausted)?;

        Ok((kind, plaintext))
    }
}

pub fn perform_client_handshake<S: Read + Write>(
    stream: &mut S,
    client_id: u64,
    key: &PeerKey,
) -> Result<(u64, SecureSession), SessionError> {
    let client = ClientHello::new(client_id, key)?;
    stream.write_all(&client.encode())?;
    stream.flush()?;

    let mut bytes = [0_u8; HANDSHAKE_LEN];
    stream.read_exact(&mut bytes)?;
    let server = ServerHello::decode(&bytes)?;
    server.verify(&client, key)?;

    let session =
        SecureSession::from_handshake(SessionRole::Client, key, &client, &server)?;

    Ok((server.node_id, session))
}

pub fn perform_server_handshake<S: Read + Write>(
    stream: &mut S,
    server_id: u64,
    key: &PeerKey,
    replay_cache: &mut NonceReplayCache,
) -> Result<(u64, SecureSession), SessionError> {
    let mut bytes = [0_u8; HANDSHAKE_LEN];
    stream.read_exact(&mut bytes)?;

    let client = ClientHello::decode(&bytes)?;
    client.verify(key)?;

    if !replay_cache.accept(client.node_id, client.nonce) {
        return Err(SessionError::ReplayDetected);
    }

    let server = ServerHello::new(server_id, &client, key)?;
    stream.write_all(&server.encode())?;
    stream.flush()?;

    let session =
        SecureSession::from_handshake(SessionRole::Server, key, &client, &server)?;

    Ok((client.node_id, session))
}

fn encode_handshake(
    kind: u8,
    node_id: u64,
    nonce: &[u8; HANDSHAKE_NONCE_LEN],
    tag: &[u8; TAG_LEN],
) -> [u8; HANDSHAKE_LEN] {
    let mut out = [0_u8; HANDSHAKE_LEN];
    out[0..4].copy_from_slice(&MAGIC);
    out[4] = VERSION;
    out[5] = kind;
    out[6..14].copy_from_slice(&node_id.to_be_bytes());
    out[14..38].copy_from_slice(nonce);
    out[38..70].copy_from_slice(tag);
    out
}

fn decode_handshake(
    bytes: &[u8],
) -> Result<(u8, u64, [u8; 24], [u8; 32]), SessionError> {
    if bytes.len() != HANDSHAKE_LEN {
        return Err(SessionError::WrongLength);
    }
    if bytes[0..4] != MAGIC {
        return Err(SessionError::WrongMagic);
    }
    if bytes[4] != VERSION {
        return Err(SessionError::WrongVersion(bytes[4]));
    }

    let kind = bytes[5];
    let node_id = u64::from_be_bytes(
        bytes[6..14]
            .try_into()
            .map_err(|_| SessionError::WrongLength)?,
    );
    let nonce = bytes[14..38]
        .try_into()
        .map_err(|_| SessionError::WrongLength)?;
    let tag = bytes[38..70]
        .try_into()
        .map_err(|_| SessionError::WrongLength)?;

    Ok((kind, node_id, nonce, tag))
}

fn client_tag(
    node_id: u64,
    nonce: &[u8; HANDSHAKE_NONCE_LEN],
    key: &PeerKey,
) -> [u8; TAG_LEN] {
    let mut mac = new_mac(key.as_bytes());
    mac.update(b"SP3/client-hello/v0");
    mac.update(&node_id.to_be_bytes());
    mac.update(nonce);
    finalize_mac(mac)
}

fn verify_client_tag(
    node_id: u64,
    nonce: &[u8; HANDSHAKE_NONCE_LEN],
    tag: &[u8; TAG_LEN],
    key: &PeerKey,
) -> Result<(), SessionError> {
    let mut mac = new_mac(key.as_bytes());
    mac.update(b"SP3/client-hello/v0");
    mac.update(&node_id.to_be_bytes());
    mac.update(nonce);
    mac.verify_slice(tag)
        .map_err(|_| SessionError::AuthenticationFailed)
}

fn server_tag(
    client: &ClientHello,
    server_id: u64,
    server_nonce: &[u8; HANDSHAKE_NONCE_LEN],
    key: &PeerKey,
) -> [u8; TAG_LEN] {
    let mut mac = new_mac(key.as_bytes());
    mac.update(b"SP3/server-hello/v0");
    mac.update(&client.node_id.to_be_bytes());
    mac.update(&client.nonce);
    mac.update(&server_id.to_be_bytes());
    mac.update(server_nonce);
    finalize_mac(mac)
}

fn verify_server_tag(
    client: &ClientHello,
    server_id: u64,
    server_nonce: &[u8; HANDSHAKE_NONCE_LEN],
    tag: &[u8; TAG_LEN],
    key: &PeerKey,
) -> Result<(), SessionError> {
    let mut mac = new_mac(key.as_bytes());
    mac.update(b"SP3/server-hello/v0");
    mac.update(&client.node_id.to_be_bytes());
    mac.update(&client.nonce);
    mac.update(&server_id.to_be_bytes());
    mac.update(server_nonce);
    mac.verify_slice(tag)
        .map_err(|_| SessionError::AuthenticationFailed)
}

fn derive_session_key(
    key: &PeerKey,
    client: &ClientHello,
    server: &ServerHello,
) -> [u8; 32] {
    let mut mac = new_mac(key.as_bytes());
    mac.update(b"SP3/session-key/v0");
    mac.update(&client.node_id.to_be_bytes());
    mac.update(&client.nonce);
    mac.update(&server.node_id.to_be_bytes());
    mac.update(&server.nonce);
    finalize_mac(mac)
}

fn derive_prefix(key: &[u8; 32], label: &[u8]) -> [u8; 16] {
    let mut mac = new_mac(key);
    mac.update(label);
    let full = finalize_mac(mac);
    let mut prefix = [0_u8; 16];
    prefix.copy_from_slice(&full[..16]);
    prefix
}

fn new_mac(key: &[u8]) -> HmacSha256 {
    HmacSha256::new_from_slice(key)
        .expect("HMAC-SHA256 accepts keys of any length")
}

fn finalize_mac(mac: HmacSha256) -> [u8; 32] {
    let bytes = mac.finalize().into_bytes();
    let mut out = [0_u8; 32];
    out.copy_from_slice(&bytes);
    out
}

fn nonce_for(prefix: [u8; 16], counter: u64) -> XNonce {
    let mut bytes = [0_u8; 24];
    bytes[..16].copy_from_slice(&prefix);
    bytes[16..].copy_from_slice(&counter.to_be_bytes());
    XNonce::from(bytes)
}

fn frame_aad(kind: u8, counter: u64) -> [u8; 14] {
    let mut aad = [0_u8; 14];
    aad[0..4].copy_from_slice(&MAGIC);
    aad[4] = VERSION;
    aad[5] = kind;
    aad[6..14].copy_from_slice(&counter.to_be_bytes());
    aad
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> PeerKey {
        PeerKey::new([0x42; 32])
    }

    fn transcript() -> (ClientHello, ServerHello) {
        let key = key();
        let client = ClientHello::from_nonce(10, [0x11; 24], &key);
        let server = ServerHello::from_nonce(20, [0x22; 24], &client, &key);
        (client, server)
    }

    #[test]
    fn hello_roundtrip_and_authentication() {
        let key = key();
        let client = ClientHello::from_nonce(7, [3; 24], &key);
        let encoded = client.encode();
        let decoded = ClientHello::decode(&encoded).unwrap();

        assert_eq!(decoded, client);
        decoded.verify(&key).unwrap();
    }

    #[test]
    fn wrong_key_is_rejected() {
        let good = key();
        let bad = PeerKey::new([0x99; 32]);
        let client = ClientHello::from_nonce(7, [3; 24], &good);

        assert!(matches!(
            client.verify(&bad),
            Err(SessionError::AuthenticationFailed)
        ));
    }

    #[test]
    fn encrypted_frames_cross_between_roles() {
        let key = key();
        let (client_hello, server_hello) = transcript();

        let mut client = SecureSession::from_handshake(
            SessionRole::Client,
            &key,
            &client_hello,
            &server_hello,
        )
        .unwrap();

        let mut server = SecureSession::from_handshake(
            SessionRole::Server,
            &key,
            &client_hello,
            &server_hello,
        )
        .unwrap();

        let frame = client.seal(9, b"tiny-query").unwrap();
        let (kind, plaintext) = server.open(&frame).unwrap();

        assert_eq!(kind, 9);
        assert_eq!(plaintext, b"tiny-query");

        let reply = server.seal(10, b"tiny-result").unwrap();
        let (kind, plaintext) = client.open(&reply).unwrap();

        assert_eq!(kind, 10);
        assert_eq!(plaintext, b"tiny-result");
    }

    #[test]
    fn tampering_is_rejected() {
        let key = key();
        let (client_hello, server_hello) = transcript();

        let mut client = SecureSession::from_handshake(
            SessionRole::Client,
            &key,
            &client_hello,
            &server_hello,
        )
        .unwrap();

        let mut server = SecureSession::from_handshake(
            SessionRole::Server,
            &key,
            &client_hello,
            &server_hello,
        )
        .unwrap();

        let mut frame = client.seal(1, b"hello").unwrap();
        let last = frame.len() - 1;
        frame[last] ^= 0x01;

        assert!(matches!(
            server.open(&frame),
            Err(SessionError::Crypto)
        ));
    }

    #[test]
    fn replayed_frame_is_rejected_by_counter() {
        let key = key();
        let (client_hello, server_hello) = transcript();

        let mut client = SecureSession::from_handshake(
            SessionRole::Client,
            &key,
            &client_hello,
            &server_hello,
        )
        .unwrap();

        let mut server = SecureSession::from_handshake(
            SessionRole::Server,
            &key,
            &client_hello,
            &server_hello,
        )
        .unwrap();

        let frame = client.seal(1, b"one").unwrap();
        server.open(&frame).unwrap();

        assert!(matches!(
            server.open(&frame),
            Err(SessionError::UnexpectedCounter {
                expected: 1,
                got: 0
            })
        ));
    }

    #[test]
    fn handshake_replay_cache_rejects_duplicate_nonce() {
        let mut cache = NonceReplayCache::new(4);
        assert!(cache.accept(1, [7; 24]));
        assert!(!cache.accept(1, [7; 24]));
        assert!(cache.accept(2, [7; 24]));
    }
}
