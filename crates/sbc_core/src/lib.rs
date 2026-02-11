use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum IpFamily {
    V4,
    V6,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct IpPrefix {
    pub addr: IpAddr,
    pub prefix_len: u8,
}

impl IpPrefix {
    pub fn family(&self) -> IpFamily {
        match self.addr {
            IpAddr::V4(_) => IpFamily::V4,
            IpAddr::V6(_) => IpFamily::V6,
        }
    }

    pub fn new(addr: IpAddr, prefix_len: u8) -> anyhow::Result<Self> {
        let max = match addr {
            IpAddr::V4(_) => 32u8,
            IpAddr::V6(_) => 128u8,
        };
        if prefix_len > max {
            anyhow::bail!("invalid prefix len {prefix_len} for {addr}");
        }

        let addr = match addr {
            IpAddr::V4(a) => IpAddr::V4(mask_v4(a, prefix_len)),
            IpAddr::V6(a) => IpAddr::V6(mask_v6(a, prefix_len)),
        };
        Ok(Self { addr, prefix_len })
    }

    pub fn parse_cidr(s: &str) -> anyhow::Result<Self> {
        let s = s.trim();
        let (ip_s, plen_s) = s
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("missing /prefix in {s:?}"))?;
        let addr: IpAddr = ip_s
            .trim()
            .parse()
            .map_err(|e| anyhow::anyhow!("bad ip in {s:?}: {e}"))?;
        let prefix_len: u8 = plen_s
            .trim()
            .parse()
            .map_err(|e| anyhow::anyhow!("bad prefix len in {s:?}: {e}"))?;
        Self::new(addr, prefix_len)
    }

    pub fn to_cidr_string(&self) -> String {
        format!("{}/{}", self.addr, self.prefix_len)
    }

    pub fn contains_ip(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(a), IpAddr::V4(b)) => {
                let a = u32::from(a);
                let b = u32::from(b);
                let mask = v4_mask(self.prefix_len);
                (a & mask) == (b & mask)
            }
            (IpAddr::V6(a), IpAddr::V6(b)) => {
                let a = u128::from(a);
                let b = u128::from(b);
                let mask = v6_mask(self.prefix_len);
                (a & mask) == (b & mask)
            }
            _ => false,
        }
    }

    pub fn contains_prefix(&self, other: &IpPrefix) -> bool {
        if self.family() != other.family() {
            return false;
        }
        if self.prefix_len > other.prefix_len {
            return false;
        }
        self.contains_ip(other.addr)
    }
}

impl fmt::Display for IpPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix_len)
    }
}

fn v4_mask(prefix_len: u8) -> u32 {
    if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32u32 - prefix_len as u32)
    }
}

fn v6_mask(prefix_len: u8) -> u128 {
    if prefix_len == 0 {
        0
    } else {
        u128::MAX << (128u32 - prefix_len as u32)
    }
}

fn mask_v4(addr: Ipv4Addr, prefix_len: u8) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(addr) & v4_mask(prefix_len))
}

fn mask_v6(addr: Ipv6Addr, prefix_len: u8) -> Ipv6Addr {
    Ipv6Addr::from(u128::from(addr) & v6_mask(prefix_len))
}

#[derive(Clone, Debug)]
pub struct ExemptPrefixes {
    pub prefixes: Vec<IpPrefix>,
}

impl ExemptPrefixes {
    pub fn empty() -> Self {
        Self {
            prefixes: Vec::new(),
        }
    }

    pub fn contains_ip(&self, ip: IpAddr) -> bool {
        self.prefixes.iter().any(|p| p.contains_ip(ip))
    }

    pub fn contains_prefix(&self, pfx: &IpPrefix) -> bool {
        self.prefixes.iter().any(|p| p.contains_prefix(pfx))
    }

    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let s = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read exempt prefixes {:?}: {e}", path))?;
        let mut out = Vec::new();
        for (i, raw) in s.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let p = IpPrefix::parse_cidr(line)
                .map_err(|e| anyhow::anyhow!("bad CIDR at {}:{}: {e}", path.display(), i + 1))?;
            out.push(p);
        }
        Ok(Self { prefixes: out })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BanEntry {
    pub ban_id: String,
    pub key: IpPrefix,
    pub created_at_unix: u64,
    pub created_by: String,
    pub reason: String,
    pub expires_at_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LegalHoldEntry {
    pub name_lc: String,
    pub created_at_unix: u64,
    pub created_by: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnforcementStatus {
    pub node_id: String,
    pub dns_name: String,
    pub dns_enabled: bool,
    #[serde(default)]
    pub dns_last_error: Option<String>,
    pub backend: String,
    pub backend_attached: bool,
    pub enforcement_mode: String, // enforcing | fail_open
    pub reported_at_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BanApplyResult {
    pub node_id: String,
    pub ban_id: String,
    pub op: String,     // upsert | delete
    pub result: String, // ok | err | skipped
    #[serde(default)]
    pub error: Option<String>,
    pub reported_at_unix: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscribeMode {
    Tail,
    Snapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventsReq {
    Subscribe { mode: SubscribeMode },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdminReq {
    UpsertBan {
        key: String, // CIDR string
        ttl_s: u64,
        created_by: String,
        reason: String,
    },
    DeleteBan {
        ban_id: String,
    },
    UpsertLegalHold {
        name: String,
        created_by: String,
        reason: String,
    },
    DeleteLegalHold {
        name: String,
    },
    ReportEnforcementStatus {
        status: EnforcementStatus,
    },
    ReportBanApplyResult {
        report: BanApplyResult,
    },
    GetState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdminResp {
    Ok {
        index: u64,
    },
    OkBan {
        index: u64,
        entry: BanEntry,
    },
    OkLegalHold {
        index: u64,
        entry: LegalHoldEntry,
    },
    OkState {
        index: u64,
        bans: Vec<BanEntry>,
        holds: Vec<LegalHoldEntry>,
    },
    Err {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Snapshot {
        bans: Vec<BanEntry>,
        holds: Vec<LegalHoldEntry>,
    },
    BanUpserted {
        entry: BanEntry,
    },
    BanDeleted {
        ban_id: String,
    },
    LegalHoldUpserted {
        entry: LegalHoldEntry,
    },
    LegalHoldDeleted {
        name_lc: String,
    },
    EnforcementStatus {
        status: EnforcementStatus,
    },
    BanApplyResult {
        report: BanApplyResult,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub index: u64,
    pub event: Event,
}
