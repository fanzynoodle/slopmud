use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use sbc_core::IpPrefix;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterBan {
    pub name_lc: String,
    pub created_unix: u64,
    pub created_by: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpBan {
    pub cidr: String,
    pub created_unix: u64,
    pub created_by: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BanListFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub updated_unix: u64,
    #[serde(default)]
    pub character_bans: Vec<CharacterBan>,
    #[serde(default)]
    pub ip_bans: Vec<IpBan>,
}

#[derive(Debug)]
pub struct BanState {
    path: PathBuf,
    chars: HashMap<String, CharacterBan>,
    ips: Vec<(IpPrefix, IpBan)>,
    updated_unix: u64,
}

impl BanState {
    pub fn load(path: PathBuf) -> Self {
        let mut st = Self {
            path,
            chars: HashMap::new(),
            ips: Vec::new(),
            updated_unix: 0,
        };
        let _ = st.reload();
        st
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn updated_unix(&self) -> u64 {
        self.updated_unix
    }

    pub fn snapshot_file(&self) -> BanListFile {
        let mut character_bans = self.chars.values().cloned().collect::<Vec<_>>();
        character_bans.sort_by(|a, b| a.name_lc.cmp(&b.name_lc));

        let mut ip_bans = self.ips.iter().map(|(_, b)| b.clone()).collect::<Vec<_>>();
        ip_bans.sort_by(|a, b| a.cidr.cmp(&b.cidr));

        BanListFile {
            version: 1,
            updated_unix: self.updated_unix,
            character_bans,
            ip_bans,
        }
    }

    pub fn reload(&mut self) -> anyhow::Result<()> {
        let s = match std::fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.chars.clear();
                self.ips.clear();
                self.updated_unix = 0;
                return Ok(());
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "failed to read bans file {:?}: {e}",
                    self.path
                ));
            }
        };

        let file: BanListFile = serde_json::from_str(&s)
            .map_err(|e| anyhow::anyhow!("failed to parse bans file {:?}: {e}", self.path))?;

        let mut chars = HashMap::new();
        for b in file.character_bans {
            if b.name_lc.trim().is_empty() {
                continue;
            }
            chars.insert(b.name_lc.clone(), b);
        }

        let mut ips = Vec::new();
        for b in file.ip_bans {
            let cidr = b.cidr.trim();
            if cidr.is_empty() {
                continue;
            }
            if let Ok(pfx) = parse_ip_prefix(cidr) {
                ips.push((pfx, b));
            }
        }

        self.updated_unix = file.updated_unix;
        self.chars = chars;
        self.ips = ips;
        Ok(())
    }

    pub fn is_char_banned(&self, name: &str) -> Option<&CharacterBan> {
        let k = name.trim().to_ascii_lowercase();
        if k.is_empty() {
            return None;
        }
        self.chars.get(&k)
    }

    pub fn is_ip_banned(&self, ip: IpAddr) -> Option<&IpBan> {
        for (pfx, b) in &self.ips {
            if pfx.contains_ip(ip) {
                return Some(b);
            }
        }
        None
    }

    pub fn upsert_char_ban(
        &mut self,
        name: &str,
        created_unix: u64,
        created_by: String,
        reason: String,
    ) -> anyhow::Result<bool> {
        let name_lc = name.trim().to_ascii_lowercase();
        if name_lc.is_empty() {
            anyhow::bail!("empty character name");
        }

        let rec = CharacterBan {
            name_lc: name_lc.clone(),
            created_unix,
            created_by,
            reason,
        };
        let changed = match self.chars.get(&name_lc) {
            Some(prev) => {
                prev.created_unix != rec.created_unix
                    || prev.created_by != rec.created_by
                    || prev.reason != rec.reason
            }
            None => true,
        };

        self.chars.insert(name_lc, rec);
        self.updated_unix = created_unix.max(self.updated_unix);
        self.save()?;
        Ok(changed)
    }

    pub fn upsert_ip_ban(
        &mut self,
        cidr: &str,
        created_unix: u64,
        created_by: String,
        reason: String,
    ) -> anyhow::Result<(bool, IpPrefix)> {
        let cidr = cidr.trim();
        if cidr.is_empty() {
            anyhow::bail!("empty cidr");
        }
        let pfx = parse_ip_prefix(cidr)?;
        let cidr_norm = pfx.to_cidr_string();

        let rec = IpBan {
            cidr: cidr_norm.clone(),
            created_unix,
            created_by,
            reason,
        };

        // Replace existing entry if present.
        let mut changed = true;
        let mut out = Vec::with_capacity(self.ips.len() + 1);
        for (p, b) in self.ips.drain(..) {
            if p == pfx {
                changed = b.created_unix != rec.created_unix
                    || b.created_by != rec.created_by
                    || b.reason != rec.reason;
                continue;
            }
            out.push((p, b));
        }
        out.push((pfx.clone(), rec));
        self.ips = out;
        self.updated_unix = created_unix.max(self.updated_unix);
        self.save()?;
        Ok((changed, pfx))
    }

    fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("failed to create bans dir {:?}: {e}", parent))?;
        }

        let file = self.snapshot_file();
        let s = serde_json::to_string_pretty(&file)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, s)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

fn parse_ip_prefix(s: &str) -> anyhow::Result<IpPrefix> {
    // Accept plain IP as /32 or /128.
    if !s.contains('/') {
        let ip: IpAddr = s
            .parse()
            .map_err(|e| anyhow::anyhow!("bad ip {s:?}: {e}"))?;
        let plen = if ip.is_ipv4() { 32 } else { 128 };
        return IpPrefix::new(ip, plen);
    }
    IpPrefix::parse_cidr(s)
}
