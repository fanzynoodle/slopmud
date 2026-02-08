//! Telnet IAC parsing.
//!
//! This is intentionally minimal: it strips IAC sequences from the byte stream
//! and (optionally) generates "refuse everything" negotiation replies:
//! - `IAC DO <opt>`   => `IAC WONT <opt>`
//! - `IAC WILL <opt>` => `IAC DONT <opt>`
//!
//! It also strips subnegotiation blocks: `IAC SB ... IAC SE`.

#[derive(Debug, Default)]
pub struct IacParser {
    state: State,
    /// If true, emit default refusal replies for DO/WILL.
    refuse_negotiation: bool,
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Data,
    Iac,
    Negotiate {
        cmd: u8,
    },
    Subneg {
        opt: Option<u8>,
        iac_seen: bool,
        // Keep any subnegotiation payload we might want later (currently unused).
        _buf: Vec<u8>,
    },
}

impl IacParser {
    pub fn new() -> Self {
        Self {
            state: State::Data,
            refuse_negotiation: true,
        }
    }

    pub fn refuse_negotiation(mut self, on: bool) -> Self {
        self.refuse_negotiation = on;
        self
    }

    /// Parse a chunk of bytes, returning `(data, replies)`:
    /// - `data`: the stream with IAC sequences removed
    /// - `replies`: bytes to write back to the telnet peer (may be empty)
    pub fn parse(&mut self, chunk: &[u8]) -> (Vec<u8>, Vec<u8>) {
        const IAC: u8 = 255;
        const DONT: u8 = 254;
        const DO: u8 = 253;
        const WONT: u8 = 252;
        const WILL: u8 = 251;
        const SB: u8 = 250;
        const SE: u8 = 240;

        let mut out = Vec::with_capacity(chunk.len());
        let mut replies = Vec::new();

        for b in chunk {
            match &mut self.state {
                State::Data => {
                    if *b == IAC {
                        self.state = State::Iac;
                    } else {
                        out.push(*b);
                    }
                }
                State::Iac => {
                    match *b {
                        // Escaped 0xff => literal 0xff.
                        IAC => {
                            out.push(IAC);
                            self.state = State::Data;
                        }
                        // Negotiation commands are 3 bytes: IAC <cmd> <opt>
                        DO | DONT | WILL | WONT => {
                            self.state = State::Negotiate { cmd: *b };
                        }
                        // Subnegotiation: IAC SB <opt> ... IAC SE
                        SB => {
                            self.state = State::Subneg {
                                opt: None,
                                iac_seen: false,
                                _buf: Vec::new(),
                            };
                        }
                        // Other 2-byte IAC commands (NOP, GA, etc.) - ignore.
                        _ => {
                            self.state = State::Data;
                        }
                    }
                }
                State::Negotiate { cmd } => {
                    let opt = *b;
                    if self.refuse_negotiation {
                        match *cmd {
                            // "Please do X" => "No thanks".
                            DO => replies.extend_from_slice(&[IAC, WONT, opt]),
                            // "I will do X" => "Please don't".
                            WILL => replies.extend_from_slice(&[IAC, DONT, opt]),
                            _ => {}
                        }
                    }
                    self.state = State::Data;
                }
                State::Subneg {
                    opt,
                    iac_seen,
                    _buf,
                } => {
                    if opt.is_none() {
                        *opt = Some(*b);
                        continue;
                    }

                    if *iac_seen {
                        // Only SE matters; IAC IAC is escaped literal IAC.
                        if *b == SE {
                            self.state = State::Data;
                        } else if *b == IAC {
                            _buf.push(IAC);
                            *iac_seen = false;
                        } else {
                            // Unknown IAC within SB; ignore.
                            *iac_seen = false;
                        }
                        continue;
                    }

                    if *b == IAC {
                        *iac_seen = true;
                        continue;
                    }

                    _buf.push(*b);
                }
            }
        }

        (out, replies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_plain_data() {
        let mut p = IacParser::new();
        let (d, r) = p.parse(b"hello\n");
        assert_eq!(d, b"hello\n");
        assert!(r.is_empty());
    }

    #[test]
    fn decodes_escaped_iac() {
        let mut p = IacParser::new();
        let (d, r) = p.parse(&[255, 255, b'a']);
        assert_eq!(d, vec![255, b'a']);
        assert!(r.is_empty());
    }

    #[test]
    fn refuses_do_and_will() {
        let mut p = IacParser::new();
        let (d, r) = p.parse(&[255, 253, 1, 255, 251, 3, b'x']); // IAC DO 1, IAC WILL 3, then x
        assert_eq!(d, vec![b'x']);
        assert_eq!(r, vec![255, 252, 1, 255, 254, 3]); // WONT 1, DONT 3
    }

    #[test]
    fn handles_split_negotiation_across_calls() {
        let mut p = IacParser::new();
        let (d1, r1) = p.parse(&[255, 253]); // IAC DO (incomplete)
        assert!(d1.is_empty());
        assert!(r1.is_empty());

        let (d2, r2) = p.parse(&[7, b'z']);
        assert_eq!(d2, vec![b'z']);
        assert_eq!(r2, vec![255, 252, 7]);
    }

    #[test]
    fn strips_subnegotiation() {
        let mut p = IacParser::new();
        let bytes = [b'a', 255, 250, 24, b'x', b'y', 255, 240, b'b']; // a IAC SB 24 x y IAC SE b
        let (d, r) = p.parse(&bytes);
        assert_eq!(d, vec![b'a', b'b']);
        assert!(r.is_empty());
    }
}
