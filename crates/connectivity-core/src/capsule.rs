#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CapsuleKind {
    Ping = 0,
    Query = 1,
    Result = 2,
    Capability = 3,
    RouteHint = 4,
    Ack = 5,
}

impl TryFrom<u8> for CapsuleKind {
    type Error = CapsuleError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Ping),
            1 => Ok(Self::Query),
            2 => Ok(Self::Result),
            3 => Ok(Self::Capability),
            4 => Ok(Self::RouteHint),
            5 => Ok(Self::Ack),
            _ => Err(CapsuleError::UnknownKind(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCapsule {
    pub kind: CapsuleKind,
    pub flags: u8,
    pub request_id: u32,
    pub ttl_hops: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapsuleError {
    TooShort,
    WrongVersion(u8),
    UnknownKind(u8),
    PayloadTooLarge,
    LengthMismatch,
}

impl SemanticCapsule {
    pub const VERSION: u8 = 0;
    pub const HEADER_LEN: usize = 10;

    /// Deterministic v0 wire format with 10 bytes of header overhead.
    pub fn encode(&self) -> Result<Vec<u8>, CapsuleError> {
        if self.payload.len() > u16::MAX as usize {
            return Err(CapsuleError::PayloadTooLarge);
        }

        let mut out =
            Vec::with_capacity(Self::HEADER_LEN + self.payload.len());

        out.push((Self::VERSION << 4) | (self.kind as u8 & 0x0f));
        out.push(self.flags);
        out.extend_from_slice(&self.request_id.to_be_bytes());
        out.push(self.ttl_hops);
        out.push(0); // reserved for a future reliability class
        out.extend_from_slice(&(self.payload.len() as u16).to_be_bytes());
        out.extend_from_slice(&self.payload);

        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CapsuleError> {
        if bytes.len() < Self::HEADER_LEN {
            return Err(CapsuleError::TooShort);
        }

        let version = bytes[0] >> 4;
        if version != Self::VERSION {
            return Err(CapsuleError::WrongVersion(version));
        }

        let kind = CapsuleKind::try_from(bytes[0] & 0x0f)?;
        let flags = bytes[1];
        let request_id =
            u32::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
        let ttl_hops = bytes[6];
        let payload_len = u16::from_be_bytes([bytes[8], bytes[9]]) as usize;

        if bytes.len() != Self::HEADER_LEN + payload_len {
            return Err(CapsuleError::LengthMismatch);
        }

        Ok(Self {
            kind,
            flags,
            request_id,
            ttl_hops,
            payload: bytes[Self::HEADER_LEN..].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capsule_roundtrip() {
        let capsule = SemanticCapsule {
            kind: CapsuleKind::Query,
            flags: 1,
            request_id: 42,
            ttl_hops: 8,
            payload: b"weather:hp".to_vec(),
        };

        let encoded = capsule.encode().unwrap();
        assert_eq!(encoded.len(), SemanticCapsule::HEADER_LEN + 10);
        assert_eq!(SemanticCapsule::decode(&encoded).unwrap(), capsule);
    }
}
