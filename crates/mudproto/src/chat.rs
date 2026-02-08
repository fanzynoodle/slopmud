use bytes::Bytes;

use crate::ProtoError;
use crate::session::SessionId;

pub const REQ_JOIN: u8 = 0x01;
pub const REQ_LEAVE: u8 = 0x02;
pub const REQ_SAY: u8 = 0x03;

pub const EVT_LINE: u8 = 0x81;
pub const EVT_ERR: u8 = 0x82;

#[derive(Debug, Clone)]
pub enum ChatReq {
    Join { session: SessionId, name: Bytes },
    Leave { session: SessionId },
    Say { session: SessionId, msg: Bytes },
}

#[derive(Debug, Clone)]
pub enum ChatEvent {
    Line { line: Bytes },
    Err { msg: Bytes },
}

pub fn parse_req(p: Bytes) -> Result<ChatReq, ProtoError> {
    if p.len() < 1 + SessionId::LEN {
        return Err(ProtoError::TooShort {
            need: 1 + SessionId::LEN,
            got: p.len(),
        });
    }

    let t = p[0];
    let mut sid = [0u8; 16];
    sid.copy_from_slice(&p[1..1 + 16]);
    let session = SessionId::from_be_bytes(sid);

    match t {
        REQ_JOIN => Ok(ChatReq::Join {
            session,
            name: p.slice(1 + 16..),
        }),
        REQ_LEAVE => {
            if p.len() != 1 + 16 {
                return Err(ProtoError::Malformed("leave must be exactly 17 bytes"));
            }
            Ok(ChatReq::Leave { session })
        }
        REQ_SAY => Ok(ChatReq::Say {
            session,
            msg: p.slice(1 + 16..),
        }),
        _ => Err(ProtoError::UnknownType(t)),
    }
}

pub fn parse_event(p: Bytes) -> Result<ChatEvent, ProtoError> {
    if p.is_empty() {
        return Err(ProtoError::TooShort { need: 1, got: 0 });
    }

    let t = p[0];
    match t {
        EVT_LINE => Ok(ChatEvent::Line { line: p.slice(1..) }),
        EVT_ERR => Ok(ChatEvent::Err { msg: p.slice(1..) }),
        _ => Err(ProtoError::UnknownType(t)),
    }
}
