use bytes::Bytes;

use crate::ProtoError;
use crate::session::SessionId;

pub const REQ_ATTACH: u8 = 0x01;
pub const REQ_DETACH: u8 = 0x02;
pub const REQ_INPUT: u8 = 0x03;
pub const REQ_INPUT_BLOB: u8 = 0x04;
pub const REQ_INPUT_IDEMPOTENT: u8 = 0x05;

pub const RESP_OUTPUT: u8 = 0x81;
pub const RESP_ERR: u8 = 0x82;
pub const RESP_OUTPUT_BLOB: u8 = 0x83;

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
    ///   - bit3: quiet reattach (suppress login banner)
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
        quiet: bool,
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
        command_id: Option<u64>,
        line: Bytes,
    },
    InputBlob {
        session: SessionId,
        command: Bytes,
        path: Bytes,
        len: u64,
    },
}

#[derive(Debug, Clone)]
pub enum ShardResp {
    Output {
        session: SessionId,
        line: Bytes,
    },
    Err {
        session: SessionId,
        msg: Bytes,
    },
    OutputBlob {
        session: SessionId,
        prefix: Bytes,
        path: Bytes,
        len: u64,
        suffix: Bytes,
    },
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
            let quiet = (flags & 0x08) != 0;
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
                quiet,
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
            command_id: None,
            line: p.slice(1 + 16..),
        }),
        REQ_INPUT_IDEMPOTENT => {
            let i = 1 + 16;
            if p.len() < i + 8 {
                return Err(ProtoError::TooShort {
                    need: i + 8,
                    got: p.len(),
                });
            }
            let command_id = u64::from_be_bytes([
                p[i],
                p[i + 1],
                p[i + 2],
                p[i + 3],
                p[i + 4],
                p[i + 5],
                p[i + 6],
                p[i + 7],
            ]);
            Ok(ShardReq::Input {
                session,
                command_id: Some(command_id),
                line: p.slice(i + 8..),
            })
        }
        REQ_INPUT_BLOB => {
            let mut i = 1 + 16;
            if p.len() < i + 1 {
                return Err(ProtoError::TooShort {
                    need: i + 1,
                    got: p.len(),
                });
            }
            let clen = p[i] as usize;
            i += 1;
            if p.len() < i + clen + 2 {
                return Err(ProtoError::TooShort {
                    need: i + clen + 2,
                    got: p.len(),
                });
            }
            let command = p.slice(i..i + clen);
            i += clen;
            let plen = u16::from_be_bytes([p[i], p[i + 1]]) as usize;
            i += 2;
            if p.len() < i + plen + 8 {
                return Err(ProtoError::TooShort {
                    need: i + plen + 8,
                    got: p.len(),
                });
            }
            let path = p.slice(i..i + plen);
            i += plen;
            let len = u64::from_be_bytes([
                p[i],
                p[i + 1],
                p[i + 2],
                p[i + 3],
                p[i + 4],
                p[i + 5],
                p[i + 6],
                p[i + 7],
            ]);
            i += 8;
            if p.len() != i {
                return Err(ProtoError::Malformed("input blob has trailing bytes"));
            }
            Ok(ShardReq::InputBlob {
                session,
                command,
                path,
                len,
            })
        }
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
        RESP_OUTPUT_BLOB => {
            let mut i = 1 + 16;
            if p.len() < i + 2 {
                return Err(ProtoError::TooShort {
                    need: i + 2,
                    got: p.len(),
                });
            }
            let prefix_len = u16::from_be_bytes([p[i], p[i + 1]]) as usize;
            i += 2;
            if p.len() < i + prefix_len + 2 {
                return Err(ProtoError::TooShort {
                    need: i + prefix_len + 2,
                    got: p.len(),
                });
            }
            let prefix = p.slice(i..i + prefix_len);
            i += prefix_len;
            let path_len = u16::from_be_bytes([p[i], p[i + 1]]) as usize;
            i += 2;
            if p.len() < i + path_len + 8 + 2 {
                return Err(ProtoError::TooShort {
                    need: i + path_len + 8 + 2,
                    got: p.len(),
                });
            }
            let path = p.slice(i..i + path_len);
            i += path_len;
            let len = u64::from_be_bytes([
                p[i],
                p[i + 1],
                p[i + 2],
                p[i + 3],
                p[i + 4],
                p[i + 5],
                p[i + 6],
                p[i + 7],
            ]);
            i += 8;
            let suffix_len = u16::from_be_bytes([p[i], p[i + 1]]) as usize;
            i += 2;
            if p.len() < i + suffix_len {
                return Err(ProtoError::TooShort {
                    need: i + suffix_len,
                    got: p.len(),
                });
            }
            let suffix = p.slice(i..i + suffix_len);
            i += suffix_len;
            if p.len() != i {
                return Err(ProtoError::Malformed("output blob has trailing bytes"));
            }
            Ok(ShardResp::OutputBlob {
                session,
                prefix,
                path,
                len,
                suffix,
            })
        }
        _ => Err(ProtoError::UnknownType(t)),
    }
}

pub fn build_input_blob_body(command: &[u8], path: &[u8], len: u64) -> Result<Vec<u8>, ProtoError> {
    let clen: u8 = command
        .len()
        .try_into()
        .map_err(|_| ProtoError::Malformed("input blob command too long"))?;
    let plen: u16 = path
        .len()
        .try_into()
        .map_err(|_| ProtoError::Malformed("input blob path too long"))?;
    let mut out = Vec::with_capacity(1 + command.len() + 2 + path.len() + 8);
    out.push(clen);
    out.extend_from_slice(command);
    out.extend_from_slice(&plen.to_be_bytes());
    out.extend_from_slice(path);
    out.extend_from_slice(&len.to_be_bytes());
    Ok(out)
}

pub fn build_input_idempotent_body(command_id: u64, line: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + line.len());
    out.extend_from_slice(&command_id.to_be_bytes());
    out.extend_from_slice(line);
    out
}

pub fn build_output_blob_body(
    prefix: &[u8],
    path: &[u8],
    len: u64,
    suffix: &[u8],
) -> Result<Vec<u8>, ProtoError> {
    let prefix_len: u16 = prefix
        .len()
        .try_into()
        .map_err(|_| ProtoError::Malformed("output blob prefix too long"))?;
    let path_len: u16 = path
        .len()
        .try_into()
        .map_err(|_| ProtoError::Malformed("output blob path too long"))?;
    let suffix_len: u16 = suffix
        .len()
        .try_into()
        .map_err(|_| ProtoError::Malformed("output blob suffix too long"))?;
    let mut out = Vec::with_capacity(2 + prefix.len() + 2 + path.len() + 8 + 2 + suffix.len());
    out.extend_from_slice(&prefix_len.to_be_bytes());
    out.extend_from_slice(prefix);
    out.extend_from_slice(&path_len.to_be_bytes());
    out.extend_from_slice(path);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&suffix_len.to_be_bytes());
    out.extend_from_slice(suffix);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_blob_round_trips_without_payload_copy() {
        let session = SessionId(42);
        let body = build_input_blob_body(b"say", b"/tmp/blob.bin", 5_000_000_000).unwrap();
        let mut frame = Vec::with_capacity(1 + SessionId::LEN + body.len());
        frame.push(REQ_INPUT_BLOB);
        frame.extend_from_slice(&session.to_be_bytes());
        frame.extend_from_slice(&body);

        match parse_req(Bytes::from(frame)).unwrap() {
            ShardReq::InputBlob {
                session: got_session,
                command,
                path,
                len,
            } => {
                assert_eq!(got_session, session);
                assert_eq!(command.as_ref(), b"say");
                assert_eq!(path.as_ref(), b"/tmp/blob.bin");
                assert_eq!(len, 5_000_000_000);
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    #[test]
    fn input_idempotent_round_trips() {
        let session = SessionId(99);
        let body = build_input_idempotent_body(123, b"quest set a b");
        let mut frame = Vec::with_capacity(1 + SessionId::LEN + body.len());
        frame.push(REQ_INPUT_IDEMPOTENT);
        frame.extend_from_slice(&session.to_be_bytes());
        frame.extend_from_slice(&body);

        match parse_req(Bytes::from(frame)).unwrap() {
            ShardReq::Input {
                session: got_session,
                command_id,
                line,
            } => {
                assert_eq!(got_session, session);
                assert_eq!(command_id, Some(123));
                assert_eq!(line.as_ref(), b"quest set a b");
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    #[test]
    fn output_blob_round_trips_without_payload_copy() {
        let session = SessionId(7);
        let body =
            build_output_blob_body(b"ari: ", b"/tmp/blob.bin", 3_500_000_000, b"\r\n").unwrap();
        let mut frame = Vec::with_capacity(1 + SessionId::LEN + body.len());
        frame.push(RESP_OUTPUT_BLOB);
        frame.extend_from_slice(&session.to_be_bytes());
        frame.extend_from_slice(&body);

        match parse_resp(Bytes::from(frame)).unwrap() {
            ShardResp::OutputBlob {
                session: got_session,
                prefix,
                path,
                len,
                suffix,
            } => {
                assert_eq!(got_session, session);
                assert_eq!(prefix.as_ref(), b"ari: ");
                assert_eq!(path.as_ref(), b"/tmp/blob.bin");
                assert_eq!(len, 3_500_000_000);
                assert_eq!(suffix.as_ref(), b"\r\n");
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }
}
