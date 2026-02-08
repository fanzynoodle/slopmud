use bytes::Bytes;

use crate::ProtoError;
use crate::session::SessionId;

pub const REQ_ATTACH: u8 = 0x01;
pub const REQ_DETACH: u8 = 0x02;
pub const REQ_INPUT: u8 = 0x03;

pub const RESP_OUTPUT: u8 = 0x81;
pub const RESP_ERR: u8 = 0x82;

#[derive(Debug, Clone)]
pub enum ShardReq {
    /// Attach a session to the shard.
    ///
    /// Encoding:
    /// - type: `REQ_ATTACH` (1 byte)
    /// - session id: 16 bytes (u128 big-endian)
    /// - flags: 1 byte
    ///   - bit0: is_bot
    ///   - bit1: has_auth
    ///   - bit2: has_build (race+class+profile)
    /// - if has_auth:
    ///   - auth_len: u16 big-endian
    ///   - auth: auth_len bytes (opaque)
    /// - if has_build:
    ///   - race_len: u8
    ///   - race: race_len bytes (utf-8, typically lowercase token)
    ///   - class_len: u8
    ///   - class: class_len bytes (utf-8, typically lowercase token)
    ///   - sex_len: u8
    ///   - sex: sex_len bytes (utf-8, typically lowercase token)
    ///   - pronouns_len: u8
    ///   - pronouns: pronouns_len bytes (utf-8, typically lowercase token)
    /// - name: remaining bytes (utf-8)
    Attach {
        session: SessionId,
        is_bot: bool,
        auth: Option<Bytes>,
        race: Option<Bytes>,
        class: Option<Bytes>,
        sex: Option<Bytes>,
        pronouns: Option<Bytes>,
        name: Bytes,
    },
    Detach {
        session: SessionId,
    },
    Input {
        session: SessionId,
        line: Bytes,
    },
}

#[derive(Debug, Clone)]
pub enum ShardResp {
    Output { session: SessionId, line: Bytes },
    Err { session: SessionId, msg: Bytes },
}

pub fn parse_req(p: Bytes) -> Result<ShardReq, ProtoError> {
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
        REQ_ATTACH => {
            if p.len() < 1 + 16 + 1 {
                return Err(ProtoError::TooShort {
                    need: 1 + 16 + 1,
                    got: p.len(),
                });
            }
            let flags = p[1 + 16];
            let is_bot = (flags & 0x01) != 0;
            let has_auth = (flags & 0x02) != 0;
            let has_build = (flags & 0x04) != 0;
            let mut i = 1 + 16 + 1;
            let auth = if has_auth {
                if p.len() < i + 2 {
                    return Err(ProtoError::TooShort {
                        need: i + 2,
                        got: p.len(),
                    });
                }
                let len = u16::from_be_bytes([p[i], p[i + 1]]) as usize;
                i += 2;
                if p.len() < i + len {
                    return Err(ProtoError::TooShort {
                        need: i + len,
                        got: p.len(),
                    });
                }
                let a = p.slice(i..i + len);
                i += len;
                Some(a)
            } else {
                None
            };
            let (race, class, sex, pronouns) = if has_build {
                if p.len() < i + 1 {
                    return Err(ProtoError::TooShort {
                        need: i + 1,
                        got: p.len(),
                    });
                }
                let rlen = p[i] as usize;
                i += 1;
                if p.len() < i + rlen + 1 {
                    return Err(ProtoError::TooShort {
                        need: i + rlen + 1,
                        got: p.len(),
                    });
                }
                let race = p.slice(i..i + rlen);
                i += rlen;
                let clen = p[i] as usize;
                i += 1;
                if p.len() < i + clen + 1 {
                    return Err(ProtoError::TooShort {
                        need: i + clen + 1,
                        got: p.len(),
                    });
                }
                let class = p.slice(i..i + clen);
                i += clen;
                let slen = p[i] as usize;
                i += 1;
                if p.len() < i + slen + 1 {
                    return Err(ProtoError::TooShort {
                        need: i + slen + 1,
                        got: p.len(),
                    });
                }
                let sex = p.slice(i..i + slen);
                i += slen;
                let plen = p[i] as usize;
                i += 1;
                if p.len() < i + plen {
                    return Err(ProtoError::TooShort {
                        need: i + plen,
                        got: p.len(),
                    });
                }
                let pronouns = p.slice(i..i + plen);
                i += plen;
                (Some(race), Some(class), Some(sex), Some(pronouns))
            } else {
                (None, None, None, None)
            };
            Ok(ShardReq::Attach {
                session,
                is_bot,
                auth,
                race,
                class,
                sex,
                pronouns,
                name: p.slice(i..),
            })
        }
        REQ_DETACH => {
            if p.len() != 1 + 16 {
                return Err(ProtoError::Malformed("detach must be exactly 17 bytes"));
            }
            Ok(ShardReq::Detach { session })
        }
        REQ_INPUT => Ok(ShardReq::Input {
            session,
            line: p.slice(1 + 16..),
        }),
        _ => Err(ProtoError::UnknownType(t)),
    }
}

pub fn parse_resp(p: Bytes) -> Result<ShardResp, ProtoError> {
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
        RESP_OUTPUT => Ok(ShardResp::Output {
            session,
            line: p.slice(1 + 16..),
        }),
        RESP_ERR => Ok(ShardResp::Err {
            session,
            msg: p.slice(1 + 16..),
        }),
        _ => Err(ProtoError::UnknownType(t)),
    }
}
