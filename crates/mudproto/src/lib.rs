//! `mudproto`: tiny binary protocols shared by slopmud services.
//!
//! All messages are carried inside a `slopio::frame` length-prefixed frame:
//! - outer framing: `u32` big-endian byte length + payload
//! - payload: first byte is a message type; remaining bytes are type-specific
//!
//! This crate is intentionally "zero-copy" on parse: decoders return `bytes::Bytes`
//! slices that reference the original frame payload.

pub mod chat;
pub mod session;
pub mod shard;

#[derive(Debug, Clone)]
pub enum ProtoError {
    TooShort { need: usize, got: usize },
    UnknownType(u8),
    Malformed(&'static str),
}

impl std::fmt::Display for ProtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtoError::TooShort { need, got } => {
                write!(f, "message too short: need {need}, got {got}")
            }
            ProtoError::UnknownType(t) => write!(f, "unknown message type: 0x{t:02x}"),
            ProtoError::Malformed(s) => write!(f, "malformed message: {s}"),
        }
    }
}

impl std::error::Error for ProtoError {}
