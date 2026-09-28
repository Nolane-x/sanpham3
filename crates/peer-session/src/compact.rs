use super::{
    nonce_for, SecureSession, SessionError, AEAD_TAG_LEN,
};
use chacha20poly1305::aead::{Aead, Payload};
use std::io::{Read, Write};

const COMPACT_AAD_MAGIC: [u8; 4] = *b"SP3C";
const COMPACT_VERSION: u8 = 0;

pub const COMPACT_FRAME_HEADER_LEN: usize = 3;
pub const MAX_COMPACT_PLAINTEXT_LEN: usize =
    u16::MAX as usize - AEAD_TAG_LEN;

impl SecureSession {
    /// Encrypts a frame for an ordered reliable byte stream.
    ///
    /// Unlike the general frame format, the counter is implicit in session
    /// state and magic/version are domain-separated through AEAD AAD instead
    /// of being repeated on the wire. Do not use this format on unordered
    /// datagrams.
    pub fn seal_compact(
        &mut self,
        kind: u8,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, SessionError> {
        if plaintext.len() > MAX_COMPACT_PLAINTEXT_LEN {
            return Err(SessionError::PayloadTooLarge);
        }

        let counter = self.send_counter;
        let nonce = nonce_for(self.send_prefix, counter);
        let aad = compact_frame_aad(kind, counter);

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
            u16::try_from(ciphertext.len())
                .map_err(|_| SessionError::PayloadTooLarge)?;

        let mut out =
            Vec::with_capacity(COMPACT_FRAME_HEADER_LEN + ciphertext.len());
        out.push(kind);
        out.extend_from_slice(&ciphertext_len.to_be_bytes());
        out.extend_from_slice(&ciphertext);

        self.send_counter = self
            .send_counter
            .checked_add(1)
            .ok_or(SessionError::CounterExhausted)?;

        Ok(out)
    }

    /// Opens a compact ordered-stream frame using the next expected counter.
    pub fn open_compact(
        &mut self,
        frame: &[u8],
    ) -> Result<(u8, Vec<u8>), SessionError> {
        if frame.len() < COMPACT_FRAME_HEADER_LEN {
            return Err(SessionError::WrongLength);
        }

        let kind = frame[0];
        let ciphertext_len =
            u16::from_be_bytes([frame[1], frame[2]]) as usize;

        if ciphertext_len < AEAD_TAG_LEN
            || frame.len() != COMPACT_FRAME_HEADER_LEN + ciphertext_len
        {
            return Err(SessionError::WrongLength);
        }

        let counter = self.recv_counter;
        let nonce = nonce_for(self.recv_prefix, counter);
        let aad = compact_frame_aad(kind, counter);

        let plaintext = self
            .cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &frame[COMPACT_FRAME_HEADER_LEN..],
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

    pub fn send_compact<W: Write>(
        &mut self,
        writer: &mut W,
        kind: u8,
        plaintext: &[u8],
    ) -> Result<(), SessionError> {
        let frame = self.seal_compact(kind, plaintext)?;
        writer.write_all(&frame)?;
        writer.flush()?;
        Ok(())
    }

    pub fn receive_compact<R: Read>(
        &mut self,
        reader: &mut R,
    ) -> Result<(u8, Vec<u8>), SessionError> {
        let mut header = [0_u8; COMPACT_FRAME_HEADER_LEN];
        reader.read_exact(&mut header)?;

        let ciphertext_len =
            u16::from_be_bytes([header[1], header[2]]) as usize;
        if ciphertext_len < AEAD_TAG_LEN {
            return Err(SessionError::WrongLength);
        }

        let mut frame =
            Vec::with_capacity(COMPACT_FRAME_HEADER_LEN + ciphertext_len);
        frame.extend_from_slice(&header);
        frame.resize(COMPACT_FRAME_HEADER_LEN + ciphertext_len, 0);
        reader.read_exact(&mut frame[COMPACT_FRAME_HEADER_LEN..])?;

        self.open_compact(&frame)
    }
}

fn compact_frame_aad(kind: u8, counter: u64) -> [u8; 14] {
    let mut aad = [0_u8; 14];
    aad[0..4].copy_from_slice(&COMPACT_AAD_MAGIC);
    aad[4] = COMPACT_VERSION;
    aad[5] = kind;
    aad[6..14].copy_from_slice(&counter.to_be_bytes());
    aad
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ClientHello, PeerKey, ServerHello, SessionRole,
    };

    fn sessions() -> (SecureSession, SecureSession) {
        let key = PeerKey::new([0x42; 32]);
        let client_hello =
            ClientHello::from_nonce(10, [0x11; 24], &key);
        let server_hello = ServerHello::from_nonce(
            20,
            [0x22; 24],
            &client_hello,
            &key,
        );

        let client = SecureSession::from_handshake(
            SessionRole::Client,
            &key,
            &client_hello,
            &server_hello,
        )
        .unwrap();
        let server = SecureSession::from_handshake(
            SessionRole::Server,
            &key,
            &client_hello,
            &server_hello,
        )
        .unwrap();

        (client, server)
    }

    #[test]
    fn compact_frame_has_three_byte_wire_header() {
        let (mut client, mut server) = sessions();
        let frame = client.seal_compact(7, b"tiny-query").unwrap();

        assert_eq!(
            frame.len(),
            COMPACT_FRAME_HEADER_LEN + AEAD_TAG_LEN + 10
        );

        let (kind, plaintext) = server.open_compact(&frame).unwrap();
        assert_eq!(kind, 7);
        assert_eq!(plaintext, b"tiny-query");
    }

    #[test]
    fn compact_frames_work_bidirectionally() {
        let (mut client, mut server) = sessions();

        let request = client.seal_compact(1, b"hello").unwrap();
        assert_eq!(
            server.open_compact(&request).unwrap(),
            (1, b"hello".to_vec())
        );

        let response = server.seal_compact(2, b"world").unwrap();
        assert_eq!(
            client.open_compact(&response).unwrap(),
            (2, b"world".to_vec())
        );
    }

    #[test]
    fn compact_tampering_is_rejected() {
        let (mut client, mut server) = sessions();
        let mut frame = client.seal_compact(1, b"hello").unwrap();

        let last = frame.len() - 1;
        frame[last] ^= 1;

        assert!(matches!(
            server.open_compact(&frame),
            Err(SessionError::Crypto)
        ));
    }

    #[test]
    fn compact_replay_fails_under_implicit_counter() {
        let (mut client, mut server) = sessions();
        let frame = client.seal_compact(1, b"hello").unwrap();

        server.open_compact(&frame).unwrap();

        assert!(matches!(
            server.open_compact(&frame),
            Err(SessionError::Crypto)
        ));
    }
}
