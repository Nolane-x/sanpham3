use peer_session::{AEAD_TAG_LEN, FRAME_HEADER_LEN, HANDSHAKE_LEN};
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkRate {
    bits_per_second: u64,
}

impl LinkRate {
    pub fn new(bits_per_second: u64) -> Result<Self, WeakLinkError> {
        if bits_per_second == 0 {
            return Err(WeakLinkError::ZeroBitrate);
        }
        Ok(Self { bits_per_second })
    }

    pub fn bits_per_second(self) -> u64 {
        self.bits_per_second
    }

    pub fn duration_for_bytes(self, bytes: usize) -> Duration {
        if bytes == 0 {
            return Duration::ZERO;
        }

        let bits = (bytes as u128) * 8;
        let nanos = bits
            .saturating_mul(1_000_000_000)
            .div_ceil(u128::from(self.bits_per_second));

        let nanos = nanos.min(u128::from(u64::MAX)) as u64;
        Duration::from_nanos(nanos)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    FreshHandshake,
    ExistingSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeBudget {
    pub bitrate_bps: u64,
    pub mode: SessionMode,
    pub request_plaintext_bytes: usize,
    pub response_plaintext_bytes: usize,
    pub handshake_wire_bytes: usize,
    pub request_wire_bytes: usize,
    pub response_wire_bytes: usize,
    pub total_wire_bytes: usize,
    pub ideal_serialization_time: Duration,
}

pub fn estimate_exchange(
    rate: LinkRate,
    mode: SessionMode,
    request_plaintext_bytes: usize,
    response_plaintext_bytes: usize,
) -> ExchangeBudget {
    let handshake_wire_bytes = match mode {
        SessionMode::FreshHandshake => HANDSHAKE_LEN * 2,
        SessionMode::ExistingSession => 0,
    };

    let encrypted_overhead = FRAME_HEADER_LEN + AEAD_TAG_LEN;
    let request_wire_bytes = encrypted_overhead + request_plaintext_bytes;
    let response_wire_bytes = encrypted_overhead + response_plaintext_bytes;
    let total_wire_bytes =
        handshake_wire_bytes + request_wire_bytes + response_wire_bytes;

    ExchangeBudget {
        bitrate_bps: rate.bits_per_second(),
        mode,
        request_plaintext_bytes,
        response_plaintext_bytes,
        handshake_wire_bytes,
        request_wire_bytes,
        response_wire_bytes,
        total_wire_bytes,
        ideal_serialization_time: rate.duration_for_bytes(total_wire_bytes),
    }
}

#[derive(Debug)]
pub enum WeakLinkError {
    ZeroBitrate,
    ZeroChunkSize,
}

pub struct RateLimitedWriter<W> {
    inner: W,
    rate: LinkRate,
    max_chunk_bytes: usize,
}

impl<W> RateLimitedWriter<W> {
    pub fn new(
        inner: W,
        rate: LinkRate,
        max_chunk_bytes: usize,
    ) -> Result<Self, WeakLinkError> {
        if max_chunk_bytes == 0 {
            return Err(WeakLinkError::ZeroChunkSize);
        }

        Ok(Self {
            inner,
            rate,
            max_chunk_bytes,
        })
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for RateLimitedWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }

        let count = buffer.len().min(self.max_chunk_bytes);
        let delay = self.rate.duration_for_bytes(count);
        if !delay.is_zero() {
            thread::sleep(delay);
        }

        self.inner.write(&buffer[..count])
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_duration_for_common_g7_rates() {
        let one_byte = 1_usize;

        assert_eq!(
            LinkRate::new(1_000)
                .unwrap()
                .duration_for_bytes(one_byte),
            Duration::from_millis(8),
        );
        assert_eq!(
            LinkRate::new(100)
                .unwrap()
                .duration_for_bytes(one_byte),
            Duration::from_millis(80),
        );
        assert_eq!(
            LinkRate::new(10)
                .unwrap()
                .duration_for_bytes(one_byte),
            Duration::from_millis(800),
        );
    }

    #[test]
    fn fresh_handshake_cost_is_visible() {
        let rate = LinkRate::new(10).unwrap();
        let budget = estimate_exchange(
            rate,
            SessionMode::FreshHandshake,
            10,
            20,
        );

        assert_eq!(budget.handshake_wire_bytes, HANDSHAKE_LEN * 2);
        assert_eq!(
            budget.request_wire_bytes,
            FRAME_HEADER_LEN + AEAD_TAG_LEN + 10,
        );
        assert_eq!(
            budget.response_wire_bytes,
            FRAME_HEADER_LEN + AEAD_TAG_LEN + 20,
        );
        assert!(
            budget.ideal_serialization_time > Duration::from_secs(100),
        );
    }

    #[test]
    fn existing_session_removes_handshake_wire_cost() {
        let rate = LinkRate::new(30).unwrap();
        let fresh = estimate_exchange(
            rate,
            SessionMode::FreshHandshake,
            12,
            20,
        );
        let existing = estimate_exchange(
            rate,
            SessionMode::ExistingSession,
            12,
            20,
        );

        assert_eq!(existing.handshake_wire_bytes, 0);
        assert!(
            existing.ideal_serialization_time
                < fresh.ideal_serialization_time,
        );
    }

    #[test]
    fn writer_rejects_zero_chunk_size() {
        let rate = LinkRate::new(1_000).unwrap();
        assert!(matches!(
            RateLimitedWriter::new(Vec::<u8>::new(), rate, 0),
            Err(WeakLinkError::ZeroChunkSize)
        ));
    }
}
