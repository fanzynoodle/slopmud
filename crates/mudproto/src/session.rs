#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub u128);

impl SessionId {
    pub const LEN: usize = 16;

    pub fn to_be_bytes(self) -> [u8; 16] {
        self.0.to_be_bytes()
    }

    pub fn from_be_bytes(b: [u8; 16]) -> Self {
        Self(u128::from_be_bytes(b))
    }

    pub fn short(self) -> u64 {
        // Good enough for logs/UI: XOR high/low halves.
        (self.0 as u64) ^ ((self.0 >> 64) as u64)
    }
}
