use hmac::Mac;
use super::{
    finalize_mac, new_mac, PeerKey, SecureSession, SessionError, SessionRole,
};
use std::collections::{HashSet, VecDeque};
use std::io::{Read, Write};
use subtle::ConstantTimeEq;

const RESUME_MAGIC: [u8; 4] = *b"SP3R";
const RESUME_VERSION: u8 = 0;
const RESUME_CLIENT_KIND: u8 = 1;
const RESUME_SERVER_KIND: u8 = 2;
const RESUME_KEY_ID_LEN: usize = 8;
const RESUME_NONCE_LEN: usize = 12;
const RESUME_TAG_LEN: usize = 16;

pub const RESUME_CLIENT_HELLO_LEN: usize =
    4 + 1 + 1 + RESUME_KEY_ID_LEN + RESUME_NONCE_LEN + RESUME_TAG_LEN;
pub const RESUME_SERVER_HELLO_LEN: usize =
    4 + 1 + 1 + RESUME_NONCE_LEN + RESUME_TAG_LEN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeClientHello {
    pub key_id: [u8; RESUME_KEY_ID_LEN],
    pub nonce: [u8; RESUME_NONCE_LEN],
    tag: [u8; RESUME_TAG_LEN],
}

impl ResumeClientHello {
    pub fn new(
        client_id: u64,
        server_id: u64,
        key: &PeerKey,
    ) -> Result<Self, SessionError> {
        let mut nonce = [0_u8; RESUME_NONCE_LEN];
        getrandom::fill(&mut nonce)
            .map_err(|error| SessionError::Entropy(error.to_string()))?;

        Ok(Self::from_nonce(client_id, server_id, nonce, key))
    }

    pub fn from_nonce(
        client_id: u64,
        server_id: u64,
        nonce: [u8; RESUME_NONCE_LEN],
        key: &PeerKey,
    ) -> Self {
        let key_id = resume_key_id(client_id, server_id, key);
        let tag = resume_client_tag(
            client_id,
            server_id,
            &key_id,
            &nonce,
            key,
        );

        Self { key_id, nonce, tag }
    }

    pub fn verify(
        &self,
        client_id: u64,
        server_id: u64,
        key: &PeerKey,
    ) -> Result<(), SessionError> {
        let expected_key_id = resume_key_id(client_id, server_id, key);
        if !constant_time_equal(&expected_key_id, &self.key_id) {
            return Err(SessionError::AuthenticationFailed);
        }

        let expected = resume_client_tag(
            client_id,
            server_id,
            &self.key_id,
            &self.nonce,
            key,
        );

        if constant_time_equal(&expected, &self.tag) {
            Ok(())
        } else {
            Err(SessionError::AuthenticationFailed)
        }
    }

    pub fn encode(&self) -> [u8; RESUME_CLIENT_HELLO_LEN] {
        let mut out = [0_u8; RESUME_CLIENT_HELLO_LEN];
        out[0..4].copy_from_slice(&RESUME_MAGIC);
        out[4] = RESUME_VERSION;
        out[5] = RESUME_CLIENT_KIND;
        out[6..14].copy_from_slice(&self.key_id);
        out[14..26].copy_from_slice(&self.nonce);
        out[26..42].copy_from_slice(&self.tag);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, SessionError> {
        if bytes.len() != RESUME_CLIENT_HELLO_LEN {
            return Err(SessionError::WrongLength);
        }
        verify_resume_prefix(bytes, RESUME_CLIENT_KIND)?;

        Ok(Self {
            key_id: bytes[6..14]
                .try_into()
                .map_err(|_| SessionError::WrongLength)?,
            nonce: bytes[14..26]
                .try_into()
                .map_err(|_| SessionError::WrongLength)?,
            tag: bytes[26..42]
                .try_into()
                .map_err(|_| SessionError::WrongLength)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeServerHello {
    pub nonce: [u8; RESUME_NONCE_LEN],
    tag: [u8; RESUME_TAG_LEN],
}

impl ResumeServerHello {
    pub fn new(
        client_id: u64,
        server_id: u64,
        client: &ResumeClientHello,
        key: &PeerKey,
    ) -> Result<Self, SessionError> {
        client.verify(client_id, server_id, key)?;

        let mut nonce = [0_u8; RESUME_NONCE_LEN];
        getrandom::fill(&mut nonce)
            .map_err(|error| SessionError::Entropy(error.to_string()))?;

        Ok(Self::from_nonce(
            client_id,
            server_id,
            nonce,
            client,
            key,
        ))
    }

    pub fn from_nonce(
        client_id: u64,
        server_id: u64,
        nonce: [u8; RESUME_NONCE_LEN],
        client: &ResumeClientHello,
        key: &PeerKey,
    ) -> Self {
        let tag = resume_server_tag(
            client_id,
            server_id,
            client,
            &nonce,
            key,
        );
        Self { nonce, tag }
    }

    pub fn verify(
        &self,
        client_id: u64,
        server_id: u64,
        client: &ResumeClientHello,
        key: &PeerKey,
    ) -> Result<(), SessionError> {
        client.verify(client_id, server_id, key)?;

        let expected = resume_server_tag(
            client_id,
            server_id,
            client,
            &self.nonce,
            key,
        );

        if constant_time_equal(&expected, &self.tag) {
            Ok(())
        } else {
            Err(SessionError::AuthenticationFailed)
        }
    }

    pub fn encode(&self) -> [u8; RESUME_SERVER_HELLO_LEN] {
        let mut out = [0_u8; RESUME_SERVER_HELLO_LEN];
        out[0..4].copy_from_slice(&RESUME_MAGIC);
        out[4] = RESUME_VERSION;
        out[5] = RESUME_SERVER_KIND;
        out[6..18].copy_from_slice(&self.nonce);
        out[18..34].copy_from_slice(&self.tag);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, SessionError> {
        if bytes.len() != RESUME_SERVER_HELLO_LEN {
            return Err(SessionError::WrongLength);
        }
        verify_resume_prefix(bytes, RESUME_SERVER_KIND)?;

        Ok(Self {
            nonce: bytes[6..18]
                .try_into()
                .map_err(|_| SessionError::WrongLength)?,
            tag: bytes[18..34]
                .try_into()
                .map_err(|_| SessionError::WrongLength)?,
        })
    }
}

#[derive(Debug)]
pub struct ResumeReplayCache {
    capacity: usize,
    order: VecDeque<(u64, [u8; RESUME_NONCE_LEN])>,
    seen: HashSet<(u64, [u8; RESUME_NONCE_LEN])>,
}

impl ResumeReplayCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::new(),
            seen: HashSet::new(),
        }
    }

    pub fn accept(
        &mut self,
        client_id: u64,
        nonce: [u8; RESUME_NONCE_LEN],
    ) -> bool {
        let item = (client_id, nonce);
        if self.seen.contains(&item) {
            return false;
        }

        self.seen.insert(item);
        self.order.push_back(item);

        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }

        true
    }
}

pub fn resume_key_id(
    client_id: u64,
    server_id: u64,
    key: &PeerKey,
) -> [u8; RESUME_KEY_ID_LEN] {
    let mut mac = new_mac(key.as_bytes());
    mac.update(b"SP3/resume-key-id/v1");
    mac.update(&client_id.to_be_bytes());
    mac.update(&server_id.to_be_bytes());

    let full = finalize_mac(mac);
    let mut key_id = [0_u8; RESUME_KEY_ID_LEN];
    key_id.copy_from_slice(&full[..RESUME_KEY_ID_LEN]);
    key_id
}

pub fn perform_resume_client_handshake<S: Read + Write>(
    stream: &mut S,
    client_id: u64,
    server_id: u64,
    key: &PeerKey,
) -> Result<SecureSession, SessionError> {
    let client = ResumeClientHello::new(client_id, server_id, key)?;
    stream.write_all(&client.encode())?;
    stream.flush()?;

    let mut bytes = [0_u8; RESUME_SERVER_HELLO_LEN];
    stream.read_exact(&mut bytes)?;
    let server = ResumeServerHello::decode(&bytes)?;
    server.verify(client_id, server_id, &client, key)?;

    SecureSession::from_resume(
        SessionRole::Client,
        client_id,
        server_id,
        key,
        &client,
        &server,
    )
}

pub fn perform_resume_server_handshake<S: Read + Write>(
    stream: &mut S,
    client_id: u64,
    server_id: u64,
    key: &PeerKey,
    replay_cache: &mut ResumeReplayCache,
) -> Result<SecureSession, SessionError> {
    let mut bytes = [0_u8; RESUME_CLIENT_HELLO_LEN];
    stream.read_exact(&mut bytes)?;

    let client = ResumeClientHello::decode(&bytes)?;
    client.verify(client_id, server_id, key)?;

    if !replay_cache.accept(client_id, client.nonce) {
        return Err(SessionError::ReplayDetected);
    }

    let server = ResumeServerHello::new(
        client_id,
        server_id,
        &client,
        key,
    )?;
    stream.write_all(&server.encode())?;
    stream.flush()?;

    SecureSession::from_resume(
        SessionRole::Server,
        client_id,
        server_id,
        key,
        &client,
        &server,
    )
}

impl SecureSession {
    pub fn from_resume(
        role: SessionRole,
        client_id: u64,
        server_id: u64,
        key: &PeerKey,
        client: &ResumeClientHello,
        server: &ResumeServerHello,
    ) -> Result<Self, SessionError> {
        client.verify(client_id, server_id, key)?;
        server.verify(client_id, server_id, client, key)?;

        let session_key = derive_resume_session_key(
            client_id,
            server_id,
            key,
            client,
            server,
        );

        Ok(Self::from_session_key(role, session_key))
    }
}

fn verify_resume_prefix(
    bytes: &[u8],
    expected_kind: u8,
) -> Result<(), SessionError> {
    if bytes[0..4] != RESUME_MAGIC {
        return Err(SessionError::WrongMagic);
    }
    if bytes[4] != RESUME_VERSION {
        return Err(SessionError::WrongVersion(bytes[4]));
    }
    if bytes[5] != expected_kind {
        return Err(SessionError::WrongHandshakeKind(bytes[5]));
    }
    Ok(())
}

fn resume_client_tag(
    client_id: u64,
    server_id: u64,
    key_id: &[u8; RESUME_KEY_ID_LEN],
    nonce: &[u8; RESUME_NONCE_LEN],
    key: &PeerKey,
) -> [u8; RESUME_TAG_LEN] {
    let mut mac = new_mac(key.as_bytes());
    mac.update(b"SP3/resume-client/v1");
    mac.update(&client_id.to_be_bytes());
    mac.update(&server_id.to_be_bytes());
    mac.update(key_id);
    mac.update(nonce);
    truncated_mac(mac)
}

fn resume_server_tag(
    client_id: u64,
    server_id: u64,
    client: &ResumeClientHello,
    server_nonce: &[u8; RESUME_NONCE_LEN],
    key: &PeerKey,
) -> [u8; RESUME_TAG_LEN] {
    let mut mac = new_mac(key.as_bytes());
    mac.update(b"SP3/resume-server/v1");
    mac.update(&client_id.to_be_bytes());
    mac.update(&server_id.to_be_bytes());
    mac.update(&client.key_id);
    mac.update(&client.nonce);
    mac.update(server_nonce);
    truncated_mac(mac)
}

fn derive_resume_session_key(
    client_id: u64,
    server_id: u64,
    key: &PeerKey,
    client: &ResumeClientHello,
    server: &ResumeServerHello,
) -> [u8; 32] {
    let mut mac = new_mac(key.as_bytes());
    mac.update(b"SP3/resume-session-key/v1");
    mac.update(&client_id.to_be_bytes());
    mac.update(&server_id.to_be_bytes());
    mac.update(&client.key_id);
    mac.update(&client.nonce);
    mac.update(&server.nonce);
    finalize_mac(mac)
}

fn truncated_mac(mac: super::HmacSha256) -> [u8; RESUME_TAG_LEN] {
    let full = finalize_mac(mac);
    let mut tag = [0_u8; RESUME_TAG_LEN];
    tag.copy_from_slice(&full[..RESUME_TAG_LEN]);
    tag
}

fn constant_time_equal<const N: usize>(
    left: &[u8; N],
    right: &[u8; N],
) -> bool {
    bool::from(left.ct_eq(right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn key() -> PeerKey {
        PeerKey::new([0x5A; 32])
    }

    fn transcript() -> (ResumeClientHello, ResumeServerHello) {
        let key = key();
        let client =
            ResumeClientHello::from_nonce(10, 20, [0x11; 12], &key);
        let server = ResumeServerHello::from_nonce(
            10,
            20,
            [0x22; 12],
            &client,
            &key,
        );
        (client, server)
    }

    #[test]
    fn resume_wire_lengths_are_compact_and_exact() {
        let (client, server) = transcript();

        assert_eq!(client.encode().len(), 42);
        assert_eq!(server.encode().len(), 34);
        assert_eq!(RESUME_CLIENT_HELLO_LEN, 42);
        assert_eq!(RESUME_SERVER_HELLO_LEN, 34);

        assert_eq!(
            ResumeClientHello::decode(&client.encode()).unwrap(),
            client
        );
        assert_eq!(
            ResumeServerHello::decode(&server.encode()).unwrap(),
            server
        );
    }

    #[test]
    fn resume_authentication_binds_peer_ids_and_key() {
        let good = key();
        let bad = PeerKey::new([0x99; 32]);
        let client =
            ResumeClientHello::from_nonce(10, 20, [0x11; 12], &good);

        client.verify(10, 20, &good).unwrap();
        assert!(matches!(
            client.verify(10, 21, &good),
            Err(SessionError::AuthenticationFailed)
        ));
        assert!(matches!(
            client.verify(10, 20, &bad),
            Err(SessionError::AuthenticationFailed)
        ));
    }

    #[test]
    fn resume_session_crosses_compact_encrypted_frames() {
        let key = key();
        let (client_hello, server_hello) = transcript();

        let mut client = SecureSession::from_resume(
            SessionRole::Client,
            10,
            20,
            &key,
            &client_hello,
            &server_hello,
        )
        .unwrap();
        let mut server = SecureSession::from_resume(
            SessionRole::Server,
            10,
            20,
            &key,
            &client_hello,
            &server_hello,
        )
        .unwrap();

        let request = client.seal_compact(7, b"query").unwrap();
        let (kind, payload) = server.open_compact(&request).unwrap();
        assert_eq!(kind, 7);
        assert_eq!(payload, b"query");

        let response = server.seal_compact(8, b"result").unwrap();
        let (kind, payload) = client.open_compact(&response).unwrap();
        assert_eq!(kind, 8);
        assert_eq!(payload, b"result");
    }

    #[test]
    fn resume_replay_cache_rejects_duplicate_nonce() {
        let mut cache = ResumeReplayCache::new(4);
        assert!(cache.accept(10, [7; 12]));
        assert!(!cache.accept(10, [7; 12]));
        assert!(cache.accept(11, [7; 12]));
    }

    #[test]
    fn real_tcp_resume_handshake_and_compact_exchange() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let key = PeerKey::new([0xA7; 32]);
            let mut replay = ResumeReplayCache::new(16);

            let mut session = perform_resume_server_handshake(
                &mut stream,
                100,
                200,
                &key,
                &mut replay,
            )
            .unwrap();

            let (kind, payload) =
                session.receive_compact(&mut stream).unwrap();
            assert_eq!(kind, 1);
            assert_eq!(payload, b"tiny");

            session
                .send_compact(&mut stream, 2, b"ok")
                .unwrap();
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let key = PeerKey::new([0xA7; 32]);

        let mut session = perform_resume_client_handshake(
            &mut stream,
            100,
            200,
            &key,
        )
        .unwrap();

        session
            .send_compact(&mut stream, 1, b"tiny")
            .unwrap();
        let (kind, payload) =
            session.receive_compact(&mut stream).unwrap();

        assert_eq!(kind, 2);
        assert_eq!(payload, b"ok");

        server.join().unwrap();
    }
}
