//! `slopio`: tiny "NIH" zero-copy-ish IO helpers.
//!
//! This crate intentionally avoids tokio-util's codecs and implements just what we need:
//! - CRLF/LF line framing without copying (`BytesMut::split_to(..).freeze()`),
//! - Length-prefixed frame IO (u32 BE) with the ability to write payload in parts (no concatenation).

pub mod frame;
pub mod line;
pub mod telnet;
