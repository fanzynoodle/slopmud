use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;

const RAFT_BULK_RPC_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RaftEnvelope<E> {
    pub index: u64,
    #[serde(default)]
    pub term: u64,
    pub ms: u64,
    pub entry: E,
}

#[derive(Clone, Debug)]
pub struct ConsensusPeer {
    pub node_id: String,
    pub addr: SocketAddr,
}

#[derive(Clone, Debug)]
pub struct ConsensusConfig {
    pub node_id: String,
    pub bind: Option<SocketAddr>,
    pub peers: Vec<ConsensusPeer>,
    pub election_timeout_ms: u64,
    pub heartbeat_ms: u64,
}

impl ConsensusConfig {
    pub fn disabled(node_id: String) -> Self {
        Self {
            node_id,
            bind: None,
            peers: Vec::new(),
            election_timeout_ms: 0,
            heartbeat_ms: 0,
        }
    }

    pub fn enabled(&self) -> bool {
        self.bind.is_some() && !self.peers.is_empty()
    }

    fn majority(&self) -> usize {
        ((self.peers.len() + 1) / 2) + 1
    }
}

#[derive(Clone)]
pub struct RaftLog<E> {
    inner: Arc<Mutex<RaftLogInner<E>>>,
    consensus: Option<Arc<Consensus<E>>>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct PersistentState {
    #[serde(default)]
    current_term: u64,
    #[serde(default)]
    voted_for: Option<String>,
    #[serde(default)]
    commit_index: u64,
}

impl Default for PersistentState {
    fn default() -> Self {
        Self {
            current_term: 0,
            voted_for: None,
            commit_index: 0,
        }
    }
}

#[derive(Clone, Debug)]
struct RaftLogInner<E> {
    path: PathBuf,
    state_path: PathBuf,
    entries: Vec<RaftEnvelope<E>>,
    next_log_index: u64,
    next_apply_index: u64,
    commit_index: u64,
    current_term: u64,
    voted_for: Option<String>,
    recent: VecDeque<String>,
    recent_cap: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Follower,
    Candidate,
    Leader,
}

#[derive(Clone, Debug)]
struct RuntimeState {
    role: Role,
    leader_id: Option<String>,
    last_leader_seen: Instant,
    last_quorum_at: Option<Instant>,
    replication_latency: ReplicationLatencyStats,
}

const REPL_LATENCY_BUCKETS_MS: [u64; 11] = [1, 5, 10, 25, 50, 100, 250, 500, 1_000, 2_000, 5_000];

#[derive(Clone, Debug)]
struct ReplicationLatencyStats {
    total: u64,
    ok: u64,
    err: u64,
    sum_ms: u128,
    min_ms: u64,
    max_ms: u64,
    buckets: [u64; REPL_LATENCY_BUCKETS_MS.len() + 1],
}

impl Default for ReplicationLatencyStats {
    fn default() -> Self {
        Self {
            total: 0,
            ok: 0,
            err: 0,
            sum_ms: 0,
            min_ms: u64::MAX,
            max_ms: 0,
            buckets: [0; REPL_LATENCY_BUCKETS_MS.len() + 1],
        }
    }
}

impl ReplicationLatencyStats {
    fn record(&mut self, latency: Duration, ok: bool) {
        let ms = latency.as_millis().min(u128::from(u64::MAX)) as u64;
        self.total = self.total.saturating_add(1);
        if ok {
            self.ok = self.ok.saturating_add(1);
        } else {
            self.err = self.err.saturating_add(1);
        }
        self.sum_ms = self.sum_ms.saturating_add(u128::from(ms));
        self.min_ms = self.min_ms.min(ms);
        self.max_ms = self.max_ms.max(ms);
        let bucket = REPL_LATENCY_BUCKETS_MS
            .iter()
            .position(|upper| ms <= *upper)
            .unwrap_or(REPL_LATENCY_BUCKETS_MS.len());
        self.buckets[bucket] = self.buckets[bucket].saturating_add(1);
    }

    fn percentile_upper_ms(&self, percentile: u64) -> Option<String> {
        if self.total == 0 {
            return None;
        }
        let target = self.total.saturating_mul(percentile).div_ceil(100).max(1);
        let mut seen = 0u64;
        for (i, n) in self.buckets.iter().enumerate() {
            seen = seen.saturating_add(*n);
            if seen >= target {
                if i < REPL_LATENCY_BUCKETS_MS.len() {
                    return Some(REPL_LATENCY_BUCKETS_MS[i].to_string());
                }
                return Some(format!(
                    ">{}",
                    REPL_LATENCY_BUCKETS_MS.last().copied().unwrap_or(0)
                ));
            }
        }
        Some(self.max_ms.to_string())
    }

    fn status_lines(&self) -> Vec<String> {
        if self.total == 0 {
            return vec![" - replication_latency_ms: count=0".to_string()];
        }

        let avg = self.sum_ms / u128::from(self.total.max(1));
        let mut lines = Vec::new();
        lines.push(format!(
            " - replication_latency_ms: count={} ok={} err={} min={} avg={} p50={} p95={} p99={} max={}",
            self.total,
            self.ok,
            self.err,
            self.min_ms,
            avg,
            self.percentile_upper_ms(50).unwrap_or_else(|| "-".to_string()),
            self.percentile_upper_ms(95).unwrap_or_else(|| "-".to_string()),
            self.percentile_upper_ms(99).unwrap_or_else(|| "-".to_string()),
            self.max_ms,
        ));
        let mut parts = Vec::new();
        for (i, n) in self.buckets.iter().enumerate() {
            if i < REPL_LATENCY_BUCKETS_MS.len() {
                parts.push(format!("<={}ms:{}", REPL_LATENCY_BUCKETS_MS[i], n));
            } else {
                parts.push(format!(">{}ms:{}", REPL_LATENCY_BUCKETS_MS[i - 1], n));
            }
        }
        lines.push(format!(
            " - replication_latency_buckets: {}",
            parts.join(" ")
        ));
        lines
    }
}

struct Consensus<E> {
    cfg: ConsensusConfig,
    inner: Arc<Mutex<RaftLogInner<E>>>,
    runtime: Mutex<RuntimeState>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t")]
enum RaftRpc<E> {
    RequestVote {
        term: u64,
        candidate_id: String,
        last_log_index: u64,
        last_log_term: u64,
    },
    VoteResp {
        term: u64,
        vote_granted: bool,
    },
    AppendEntries {
        term: u64,
        leader_id: String,
        prev_log_index: u64,
        prev_log_term: u64,
        entries: Vec<RaftEnvelope<E>>,
        leader_commit: u64,
    },
    AppendResp {
        term: u64,
        success: bool,
        match_index: u64,
    },
    StatusReq,
    StatusResp {
        term: u64,
        node_id: String,
        role: String,
        leader_id: Option<String>,
        commit_index: u64,
        last_index: u64,
        last_log_term: u64,
        quorum_recent: bool,
    },
    TransferLeaderReq {
        target_id: Option<String>,
    },
    TransferLeaderResp {
        term: u64,
        accepted: bool,
        leader_id: Option<String>,
        target_id: Option<String>,
        reason: String,
    },
    TimeoutNow {
        term: u64,
        leader_id: String,
    },
    TimeoutNowResp {
        term: u64,
        started: bool,
        leader_id: Option<String>,
        reason: String,
    },
}

impl<E> RaftLog<E>
where
    E: serde::Serialize + DeserializeOwned + Clone + Send + 'static,
{
    pub fn open(path: PathBuf) -> anyhow::Result<(Self, Vec<RaftEnvelope<E>>)> {
        Self::open_with_consensus(path, ConsensusConfig::disabled("single".to_string()))
    }

    pub fn open_with_consensus(
        path: PathBuf,
        consensus_cfg: ConsensusConfig,
    ) -> anyhow::Result<(Self, Vec<RaftEnvelope<E>>)>
    where
        E: Send + 'static,
    {
        let mut inner = RaftLogInner::new(path);
        inner.load_replay()?;
        let loaded_state = inner.load_state()?;
        if !consensus_cfg.enabled() || !loaded_state {
            inner.commit_index = inner.last_index();
        }
        inner.commit_index = inner.commit_index.min(inner.last_index());
        inner.next_apply_index = inner.commit_index.saturating_add(1).max(1);
        let replay = inner.committed_entries();

        let inner = Arc::new(Mutex::new(inner));
        let consensus = if consensus_cfg.enabled() {
            Some(Consensus::start(inner.clone(), consensus_cfg)?)
        } else {
            None
        };

        Ok((Self { inner, consensus }, replay))
    }

    pub fn path(&self) -> PathBuf {
        self.inner
            .lock()
            .expect("raft log mutex poisoned")
            .path
            .clone()
    }

    pub fn next_index(&self) -> u64 {
        self.inner
            .lock()
            .expect("raft log mutex poisoned")
            .next_log_index
    }

    pub fn consensus_enabled(&self) -> bool {
        self.consensus.is_some()
    }

    pub fn accepts_client_writes(&self) -> bool {
        self.consensus
            .as_ref()
            .map(|c| c.accepts_client_writes())
            .unwrap_or(true)
    }

    pub fn status_lines(&self) -> Vec<String> {
        let inner = self.inner.lock().expect("raft log mutex poisoned");
        let mut out = vec![
            format!(" - path: {}", inner.path.display()),
            format!(" - next_index: {}", inner.next_log_index),
            format!(" - commit_index: {}", inner.commit_index),
            format!(
                " - last_applied: {}",
                inner.next_apply_index.saturating_sub(1)
            ),
            format!(" - term: {}", inner.current_term),
            format!(
                " - voted_for: {}",
                inner.voted_for.as_deref().unwrap_or("-")
            ),
        ];
        drop(inner);

        if let Some(c) = &self.consensus {
            let rt = c.runtime.lock().expect("raft runtime mutex poisoned");
            out.push(format!(" - node_id: {}", c.cfg.node_id));
            if let Some(bind) = c.cfg.bind {
                out.push(format!(" - rpc_bind: {bind}"));
            }
            out.push(format!(
                " - peers: {}",
                c.cfg
                    .peers
                    .iter()
                    .map(|p| format!("{}@{}", p.node_id, p.addr))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            out.push(format!(" - role: {:?}", rt.role));
            out.push(format!(
                " - leader: {}",
                rt.leader_id.as_deref().unwrap_or("-")
            ));
            out.push(format!(
                " - quorum_recent: {}",
                rt.role == Role::Leader
                    && rt.last_quorum_at.is_some_and(|t| t.elapsed()
                        <= Duration::from_millis(
                            c.cfg.election_timeout_ms.saturating_mul(3).max(1)
                        ))
            ));
            out.extend(rt.replication_latency.status_lines());
        } else {
            out.push(" - mode: single-node".to_string());
        }
        out
    }

    pub fn recent_lines(&self, n: usize) -> Vec<String> {
        let inner = self.inner.lock().expect("raft log mutex poisoned");
        let n = n.max(1).min(inner.recent.len());
        inner
            .recent
            .iter()
            .rev()
            .take(n)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn append(&mut self, ms: u64, entry: E) -> anyhow::Result<RaftEnvelope<E>>
    where
        E: Send + 'static,
    {
        if let Some(consensus) = &self.consensus {
            return consensus.append(ms, entry);
        }

        let mut inner = self.inner.lock().expect("raft log mutex poisoned");
        let term = inner.current_term;
        let env = inner.append_new(term, ms, entry, true)?;
        Ok(env)
    }

    pub fn poll_replay(&mut self) -> anyhow::Result<Vec<RaftEnvelope<E>>> {
        let mut inner = self.inner.lock().expect("raft log mutex poisoned");
        let mut out = Vec::new();
        let mut max_index = inner.next_apply_index.saturating_sub(1);
        for env in inner.entries.iter() {
            if env.index < inner.next_apply_index {
                continue;
            }
            if env.index > inner.commit_index {
                break;
            }
            max_index = max_index.max(env.index);
            out.push(env.clone());
        }
        inner.next_apply_index = max_index.saturating_add(1).max(inner.next_apply_index);
        Ok(out)
    }
}

impl<E> RaftLogInner<E>
where
    E: serde::Serialize + DeserializeOwned + Clone,
{
    fn new(path: PathBuf) -> Self {
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        Self {
            path,
            state_path,
            entries: Vec::new(),
            next_log_index: 1,
            next_apply_index: 1,
            commit_index: 0,
            current_term: 0,
            voted_for: None,
            recent: VecDeque::new(),
            recent_cap: 200,
        }
    }

    fn last_index(&self) -> u64 {
        self.entries.last().map(|e| e.index).unwrap_or(0)
    }

    fn last_term(&self) -> u64 {
        self.entries.last().map(|e| e.term).unwrap_or(0)
    }

    fn term_at(&self, index: u64) -> Option<u64> {
        if index == 0 {
            return Some(0);
        }
        self.entries
            .iter()
            .find(|e| e.index == index)
            .map(|e| e.term)
    }

    fn all_entries(&self) -> Vec<RaftEnvelope<E>> {
        self.entries.clone()
    }

    fn committed_entries(&self) -> Vec<RaftEnvelope<E>> {
        self.entries
            .iter()
            .take_while(|env| env.index <= self.commit_index)
            .cloned()
            .collect()
    }

    fn append_new(
        &mut self,
        term: u64,
        ms: u64,
        entry: E,
        mark_applied: bool,
    ) -> anyhow::Result<RaftEnvelope<E>> {
        let env = RaftEnvelope {
            index: self.next_log_index,
            term,
            ms,
            entry,
        };
        self.next_log_index = self.next_log_index.saturating_add(1);
        self.append_env_to_disk(&env)?;
        self.entries.push(env.clone());
        if mark_applied {
            self.mark_applied_through(env.index)?;
        }
        Ok(env)
    }

    fn set_commit_index(&mut self, index: u64) -> anyhow::Result<u64> {
        let index = index.min(self.last_index());
        if index > self.commit_index {
            self.commit_index = index;
            self.save_state()?;
        }
        Ok(self.commit_index)
    }

    fn mark_applied_through(&mut self, index: u64) -> anyhow::Result<()> {
        self.set_commit_index(index)?;
        self.next_apply_index = self.next_apply_index.max(index.saturating_add(1));
        Ok(())
    }

    fn append_replicated(&mut self, env: RaftEnvelope<E>) -> anyhow::Result<()> {
        if env.index == 0 {
            return Err(anyhow::anyhow!("raft entry index 0 is invalid"));
        }
        if let Some(pos) = self.entries.iter().position(|e| e.index == env.index) {
            if self.entries[pos].term == env.term {
                return Ok(());
            }
            self.entries.truncate(pos);
            self.next_log_index = env.index;
            self.commit_index = self.commit_index.min(self.last_index());
            self.next_apply_index = self.next_apply_index.min(self.next_log_index);
            self.rewrite_log_file()?;
        }
        if env.index != self.last_index().saturating_add(1) {
            return Err(anyhow::anyhow!(
                "raft append gap: entry={} last={}",
                env.index,
                self.last_index()
            ));
        }
        self.append_env_to_disk(&env)?;
        self.next_log_index = env.index.saturating_add(1);
        self.entries.push(env);
        Ok(())
    }

    fn truncate_uncommitted_from(&mut self, index: u64) -> anyhow::Result<()> {
        if index == 0 || index <= self.commit_index {
            return Ok(());
        }
        let Some(pos) = self.entries.iter().position(|e| e.index >= index) else {
            return Ok(());
        };
        self.entries.truncate(pos);
        self.next_log_index = index;
        self.next_apply_index = self.next_apply_index.min(self.next_log_index);
        self.rewrite_log_file()
    }

    fn truncate_uncommitted_after(&mut self, index: u64) -> anyhow::Result<()> {
        if index < self.commit_index {
            return Err(anyhow::anyhow!(
                "raft cannot truncate committed suffix: index={} commit={}",
                index,
                self.commit_index
            ));
        }
        let keep = self.entries.iter().take_while(|e| e.index <= index).count();
        if keep == self.entries.len() {
            return Ok(());
        }
        self.entries.truncate(keep);
        self.next_log_index = self.last_index().saturating_add(1).max(1);
        self.next_apply_index = self.next_apply_index.min(self.next_log_index);
        self.rewrite_log_file()
    }

    fn append_env_to_disk(&mut self, env: &RaftEnvelope<E>) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }

        let line = serde_json::to_string(env)?;
        {
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            f.write_all(line.as_bytes())?;
            f.write_all(b"\n")?;
            f.flush()?;
        }
        self.push_recent(line);
        Ok(())
    }

    fn rewrite_log_file(&mut self) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }

        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        self.recent.clear();
        for env in self.entries.clone() {
            let line = serde_json::to_string(&env)?;
            f.write_all(line.as_bytes())?;
            f.write_all(b"\n")?;
            self.push_recent(line);
        }
        f.flush()?;
        self.next_log_index = self.last_index().saturating_add(1).max(1);
        self.commit_index = self.commit_index.min(self.last_index());
        self.next_apply_index = self.next_apply_index.min(self.next_log_index);
        Ok(())
    }

    fn push_recent(&mut self, line: String) {
        while self.recent.len() >= self.recent_cap {
            self.recent.pop_front();
        }
        self.recent.push_back(line);
    }

    fn load_replay(&mut self) -> anyhow::Result<()> {
        let f = match std::fs::File::open(&self.path) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let rd = BufReader::new(f);

        let mut max_index = 0u64;
        for (lineno, line) in rd.lines().enumerate() {
            let line = line?;
            let raw = line.trim();
            if raw.is_empty() {
                continue;
            }
            let env: RaftEnvelope<E> = match serde_json::from_str(raw) {
                Ok(v) => v,
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "raft log parse error at {}:{}: {}",
                        self.path.display(),
                        lineno + 1,
                        e
                    ));
                }
            };
            max_index = max_index.max(env.index);
            self.push_recent(raw.to_string());
            self.entries.push(env);
        }
        self.next_log_index = max_index.saturating_add(1).max(1);
        Ok(())
    }

    fn load_state(&mut self) -> anyhow::Result<bool> {
        let raw = match std::fs::read_to_string(&self.state_path) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        let state: PersistentState = serde_json::from_str(&raw)?;
        self.current_term = state.current_term;
        self.voted_for = state.voted_for;
        self.commit_index = state.commit_index.min(self.last_index());
        Ok(true)
    }

    fn save_state(&self) -> anyhow::Result<()> {
        if let Some(dir) = self.state_path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let state = PersistentState {
            current_term: self.current_term,
            voted_for: self.voted_for.clone(),
            commit_index: self.commit_index,
        };
        std::fs::write(&self.state_path, serde_json::to_vec(&state)?)?;
        Ok(())
    }

    fn set_term_vote(&mut self, term: u64, voted_for: Option<String>) -> anyhow::Result<()> {
        self.current_term = term;
        self.voted_for = voted_for;
        self.save_state()
    }
}

impl<E> Consensus<E>
where
    E: serde::Serialize + DeserializeOwned + Clone + Send + 'static,
{
    fn start(
        inner: Arc<Mutex<RaftLogInner<E>>>,
        mut cfg: ConsensusConfig,
    ) -> anyhow::Result<Arc<Self>> {
        cfg.election_timeout_ms = cfg.election_timeout_ms.max(150);
        cfg.heartbeat_ms = cfg.heartbeat_ms.clamp(25, cfg.election_timeout_ms / 2);
        let bind = cfg.bind.expect("consensus enabled requires bind");
        let listener = TcpListener::bind(bind)?;
        listener.set_nonblocking(false)?;

        let now = Instant::now();
        let consensus = Arc::new(Self {
            cfg,
            inner,
            runtime: Mutex::new(RuntimeState {
                role: Role::Follower,
                leader_id: None,
                last_leader_seen: now,
                last_quorum_at: None,
                replication_latency: ReplicationLatencyStats::default(),
            }),
        });

        {
            let c = consensus.clone();
            thread::Builder::new()
                .name(format!("raft-rpc-{}", c.cfg.node_id))
                .spawn(move || c.rpc_listener(listener))?;
        }
        {
            let c = consensus.clone();
            thread::Builder::new()
                .name(format!("raft-election-{}", c.cfg.node_id))
                .spawn(move || c.election_loop())?;
        }

        Ok(consensus)
    }

    fn accepts_client_writes(&self) -> bool {
        self.runtime
            .lock()
            .expect("raft runtime mutex poisoned")
            .role
            == Role::Leader
    }

    fn record_replication_latency(&self, started: Instant, ok: bool) {
        let mut rt = self.runtime.lock().expect("raft runtime mutex poisoned");
        rt.replication_latency.record(started.elapsed(), ok);
    }

    fn append(&self, ms: u64, entry: E) -> anyhow::Result<RaftEnvelope<E>> {
        if !self.accepts_client_writes() {
            return Err(anyhow::anyhow!("raft not leader"));
        }

        let (term, prev_log_index, prev_log_term, env) = {
            let mut inner = self.inner.lock().expect("raft log mutex poisoned");
            let term = inner.current_term.max(1);
            let prev_log_index = inner.last_index();
            let prev_log_term = inner.last_term();
            let env = inner.append_new(term, ms, entry, false)?;
            (term, prev_log_index, prev_log_term, env)
        };

        if self.replicate_entry(term, prev_log_index, prev_log_term, env.clone()) {
            self.inner
                .lock()
                .expect("raft log mutex poisoned")
                .mark_applied_through(env.index)?;
            self.send_heartbeats();
            return Ok(env);
        }

        if let Err(err) = self
            .inner
            .lock()
            .expect("raft log mutex poisoned")
            .truncate_uncommitted_from(env.index)
        {
            tracing::warn!(err = %err, index = env.index, "raft failed append rollback failed");
        }

        Err(anyhow::anyhow!("raft quorum unavailable"))
    }

    fn rpc_listener(self: Arc<Self>, listener: TcpListener) {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let c = self.clone();
                    let _ = thread::Builder::new()
                        .name(format!("raft-rpc-conn-{}", c.cfg.node_id))
                        .spawn(move || {
                            if let Err(err) = c.handle_stream(stream) {
                                tracing::warn!(err = %err, "raft rpc connection failed");
                            }
                        });
                }
                Err(err) => {
                    tracing::warn!(err = %err, "raft rpc accept failed");
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }

    fn handle_stream(&self, stream: TcpStream) -> anyhow::Result<()> {
        stream.set_read_timeout(Some(RAFT_BULK_RPC_TIMEOUT))?;
        stream.set_write_timeout(Some(RAFT_BULK_RPC_TIMEOUT))?;
        let mut rd = BufReader::new(stream.try_clone()?);
        let mut line = String::new();
        rd.read_line(&mut line)?;
        let req: RaftRpc<E> = serde_json::from_str(line.trim())?;
        let resp = match req {
            RaftRpc::RequestVote {
                term,
                candidate_id,
                last_log_index,
                last_log_term,
            } => self.handle_request_vote(term, candidate_id, last_log_index, last_log_term),
            RaftRpc::AppendEntries {
                term,
                leader_id,
                prev_log_index,
                prev_log_term,
                entries,
                leader_commit,
            } => self.handle_append_entries(
                term,
                leader_id,
                prev_log_index,
                prev_log_term,
                entries,
                leader_commit,
            ),
            RaftRpc::StatusReq => self.handle_status(),
            RaftRpc::TransferLeaderReq { target_id } => self.handle_transfer_leader(target_id),
            RaftRpc::TimeoutNow { term, leader_id } => self.handle_timeout_now(term, leader_id),
            RaftRpc::VoteResp { .. }
            | RaftRpc::AppendResp { .. }
            | RaftRpc::StatusResp { .. }
            | RaftRpc::TransferLeaderResp { .. }
            | RaftRpc::TimeoutNowResp { .. } => {
                return Err(anyhow::anyhow!("unexpected raft rpc response"));
            }
        };

        let mut wr = stream;
        serde_json::to_writer(&mut wr, &resp)?;
        wr.write_all(b"\n")?;
        wr.flush()?;
        Ok(())
    }

    fn handle_request_vote(
        &self,
        term: u64,
        candidate_id: String,
        last_log_index: u64,
        last_log_term: u64,
    ) -> RaftRpc<E> {
        let mut inner = self.inner.lock().expect("raft log mutex poisoned");
        if term < inner.current_term {
            return RaftRpc::VoteResp {
                term: inner.current_term,
                vote_granted: false,
            };
        }

        if term > inner.current_term {
            if let Err(err) = inner.set_term_vote(term, None) {
                tracing::warn!(err = %err, "raft term persist failed");
            }
            self.step_down_runtime_preserving_timeout(None);
        }

        let up_to_date = last_log_term > inner.last_term()
            || (last_log_term == inner.last_term() && last_log_index >= inner.last_index());
        let vote_available = inner.voted_for.as_ref().is_none_or(|v| v == &candidate_id);
        let granted = vote_available && up_to_date;
        if granted {
            if let Err(err) = inner.set_term_vote(term, Some(candidate_id.clone())) {
                tracing::warn!(err = %err, "raft vote persist failed");
            }
            self.step_down_runtime(None);
        }

        RaftRpc::VoteResp {
            term: inner.current_term,
            vote_granted: granted,
        }
    }

    fn handle_append_entries(
        &self,
        term: u64,
        leader_id: String,
        prev_log_index: u64,
        prev_log_term: u64,
        entries: Vec<RaftEnvelope<E>>,
        leader_commit: u64,
    ) -> RaftRpc<E> {
        let mut inner = self.inner.lock().expect("raft log mutex poisoned");
        if term < inner.current_term {
            return RaftRpc::AppendResp {
                term: inner.current_term,
                success: false,
                match_index: inner.last_index(),
            };
        }

        if term > inner.current_term {
            if let Err(err) = inner.set_term_vote(term, None) {
                tracing::warn!(err = %err, "raft term persist failed");
            }
        }
        self.step_down_runtime(Some(leader_id));

        if prev_log_index > 0 && inner.term_at(prev_log_index) != Some(prev_log_term) {
            return RaftRpc::AppendResp {
                term: inner.current_term,
                success: false,
                match_index: inner.last_index(),
            };
        }

        let full_log_replace = prev_log_index == 0 && !entries.is_empty();
        let mut match_index = prev_log_index;
        for env in entries {
            match_index = env.index;
            if let Err(err) = inner.append_replicated(env) {
                tracing::warn!(err = %err, "raft replicated append failed");
                return RaftRpc::AppendResp {
                    term: inner.current_term,
                    success: false,
                    match_index: inner.last_index(),
                };
            }
        }
        if full_log_replace {
            if let Err(err) = inner.truncate_uncommitted_after(match_index) {
                tracing::warn!(err = %err, "raft replicated suffix truncate failed");
                return RaftRpc::AppendResp {
                    term: inner.current_term,
                    success: false,
                    match_index: inner.last_index(),
                };
            }
        }
        if let Err(err) = inner.set_commit_index(leader_commit) {
            tracing::warn!(err = %err, "raft commit index persist failed");
            return RaftRpc::AppendResp {
                term: inner.current_term,
                success: false,
                match_index: inner.last_index(),
            };
        }

        RaftRpc::AppendResp {
            term: inner.current_term,
            success: true,
            match_index,
        }
    }

    fn handle_status(&self) -> RaftRpc<E> {
        let (term, commit_index, last_index, last_log_term) = {
            let inner = self.inner.lock().expect("raft log mutex poisoned");
            (
                inner.current_term,
                inner.commit_index,
                inner.last_index(),
                inner.last_term(),
            )
        };
        let rt = self.runtime.lock().expect("raft runtime mutex poisoned");
        let quorum_recent = rt.role == Role::Leader
            && rt.last_quorum_at.is_some_and(|t| {
                t.elapsed()
                    <= Duration::from_millis(self.cfg.election_timeout_ms.saturating_mul(3).max(1))
            });
        RaftRpc::StatusResp {
            term,
            node_id: self.cfg.node_id.clone(),
            role: format!("{:?}", rt.role),
            leader_id: rt.leader_id.clone(),
            commit_index,
            last_index,
            last_log_term,
            quorum_recent,
        }
    }

    fn handle_transfer_leader(&self, target_id: Option<String>) -> RaftRpc<E> {
        let term = {
            let inner = self.inner.lock().expect("raft log mutex poisoned");
            inner.current_term
        };
        let current_leader = {
            let rt = self.runtime.lock().expect("raft runtime mutex poisoned");
            if rt.role != Role::Leader {
                return RaftRpc::TransferLeaderResp {
                    term,
                    accepted: false,
                    leader_id: rt.leader_id.clone(),
                    target_id,
                    reason: "node is not leader".to_string(),
                };
            }
            rt.leader_id.clone()
        };

        let peer = match target_id.as_deref() {
            Some(id) => match self.cfg.peers.iter().find(|p| p.node_id == id).cloned() {
                Some(peer) => peer,
                None => {
                    return RaftRpc::TransferLeaderResp {
                        term,
                        accepted: false,
                        leader_id: current_leader,
                        target_id,
                        reason: "transfer target is not a configured peer".to_string(),
                    };
                }
            },
            None => match self.cfg.peers.first().cloned() {
                Some(peer) => peer,
                None => {
                    return RaftRpc::TransferLeaderResp {
                        term,
                        accepted: false,
                        leader_id: current_leader,
                        target_id,
                        reason: "no transfer target peer configured".to_string(),
                    };
                }
            },
        };

        let req = RaftRpc::<E>::TimeoutNow {
            term,
            leader_id: self.cfg.node_id.clone(),
        };
        let timeout = Duration::from_millis(self.cfg.election_timeout_ms.max(1_000));
        match send_rpc(peer.addr, &req, timeout) {
            Ok(RaftRpc::TimeoutNowResp {
                term: peer_term,
                started,
                leader_id,
                reason,
            }) => {
                if peer_term > term {
                    self.step_down(peer_term, leader_id.clone());
                }
                if started {
                    let new_leader = leader_id.or_else(|| Some(peer.node_id.clone()));
                    self.step_down_runtime(new_leader.clone());
                    let current_term = self
                        .inner
                        .lock()
                        .expect("raft log mutex poisoned")
                        .current_term;
                    RaftRpc::TransferLeaderResp {
                        term: current_term,
                        accepted: true,
                        leader_id: new_leader,
                        target_id: Some(peer.node_id),
                        reason: "target became leader".to_string(),
                    }
                } else {
                    RaftRpc::TransferLeaderResp {
                        term: peer_term,
                        accepted: false,
                        leader_id,
                        target_id: Some(peer.node_id),
                        reason,
                    }
                }
            }
            Ok(other) => RaftRpc::TransferLeaderResp {
                term,
                accepted: false,
                leader_id: current_leader,
                target_id: Some(peer.node_id),
                reason: match other {
                    RaftRpc::VoteResp { .. } => "unexpected transfer response: VoteResp",
                    RaftRpc::AppendResp { .. } => "unexpected transfer response: AppendResp",
                    RaftRpc::StatusResp { .. } => "unexpected transfer response: StatusResp",
                    RaftRpc::TransferLeaderResp { .. } => {
                        "unexpected transfer response: TransferLeaderResp"
                    }
                    RaftRpc::RequestVote { .. } => "unexpected transfer response: RequestVote",
                    RaftRpc::AppendEntries { .. } => "unexpected transfer response: AppendEntries",
                    RaftRpc::StatusReq => "unexpected transfer response: StatusReq",
                    RaftRpc::TransferLeaderReq { .. } => {
                        "unexpected transfer response: TransferLeaderReq"
                    }
                    RaftRpc::TimeoutNow { .. } => "unexpected transfer response: TimeoutNow",
                    RaftRpc::TimeoutNowResp { .. } => {
                        "unexpected transfer response: TimeoutNowResp"
                    }
                }
                .to_string(),
            },
            Err(err) => RaftRpc::TransferLeaderResp {
                term,
                accepted: false,
                leader_id: current_leader,
                target_id: Some(peer.node_id),
                reason: format!("transfer RPC failed: {err}"),
            },
        }
    }

    fn handle_timeout_now(&self, term: u64, leader_id: String) -> RaftRpc<E> {
        let current_term = {
            let mut inner = self.inner.lock().expect("raft log mutex poisoned");
            if term < inner.current_term {
                return RaftRpc::TimeoutNowResp {
                    term: inner.current_term,
                    started: false,
                    leader_id: None,
                    reason: "stale transfer term".to_string(),
                };
            }
            if term > inner.current_term {
                if let Err(err) = inner.set_term_vote(term, None) {
                    tracing::warn!(err = %err, "raft timeout-now term persist failed");
                }
            }
            inner.current_term
        };

        {
            let rt = self.runtime.lock().expect("raft runtime mutex poisoned");
            if rt.role == Role::Leader {
                return RaftRpc::TimeoutNowResp {
                    term: current_term,
                    started: true,
                    leader_id: Some(self.cfg.node_id.clone()),
                    reason: "node is already leader".to_string(),
                };
            }
        }

        self.step_down_runtime_preserving_timeout(Some(leader_id));
        self.start_election();

        let final_term = {
            let inner = self.inner.lock().expect("raft log mutex poisoned");
            inner.current_term
        };
        let rt = self.runtime.lock().expect("raft runtime mutex poisoned");
        let started = rt.role == Role::Leader;
        RaftRpc::TimeoutNowResp {
            term: final_term,
            started,
            leader_id: rt.leader_id.clone(),
            reason: if started {
                "elected leader".to_string()
            } else {
                "election did not reach quorum".to_string()
            },
        }
    }

    fn election_loop(&self) {
        loop {
            thread::sleep(Duration::from_millis(self.cfg.heartbeat_ms.max(25)));
            let role = self
                .runtime
                .lock()
                .expect("raft runtime mutex poisoned")
                .role;
            if role == Role::Leader {
                self.send_heartbeats();
                continue;
            }

            let timed_out = {
                let rt = self.runtime.lock().expect("raft runtime mutex poisoned");
                rt.last_leader_seen.elapsed()
                    >= Duration::from_millis(self.cfg.election_timeout_ms + self.node_jitter_ms())
            };
            if timed_out {
                self.start_election();
            }
        }
    }

    fn start_election(&self) {
        let (term, last_log_index, last_log_term) = {
            let mut inner = self.inner.lock().expect("raft log mutex poisoned");
            let term = inner.current_term.saturating_add(1);
            if let Err(err) = inner.set_term_vote(term, Some(self.cfg.node_id.clone())) {
                tracing::warn!(err = %err, "raft candidate state persist failed");
            }
            (inner.current_term, inner.last_index(), inner.last_term())
        };

        {
            let mut rt = self.runtime.lock().expect("raft runtime mutex poisoned");
            rt.role = Role::Candidate;
            rt.leader_id = None;
            rt.last_leader_seen = Instant::now();
            rt.last_quorum_at = None;
        }

        let mut votes = 1usize;
        for peer in &self.cfg.peers {
            let req = RaftRpc::<E>::RequestVote {
                term,
                candidate_id: self.cfg.node_id.clone(),
                last_log_index,
                last_log_term,
            };
            match send_rpc(
                peer.addr,
                &req,
                Duration::from_millis(self.cfg.election_timeout_ms),
            ) {
                Ok(RaftRpc::VoteResp {
                    term: peer_term,
                    vote_granted,
                }) => {
                    if peer_term > term {
                        self.step_down(peer_term, None);
                        return;
                    }
                    if vote_granted {
                        votes += 1;
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }

        if votes >= self.cfg.majority() {
            {
                let mut rt = self.runtime.lock().expect("raft runtime mutex poisoned");
                rt.role = Role::Leader;
                rt.leader_id = Some(self.cfg.node_id.clone());
                rt.last_quorum_at = Some(Instant::now());
            }
            tracing::info!(
                node_id = %self.cfg.node_id,
                term,
                votes,
                "raft leader elected"
            );
            self.send_heartbeats();
        }
    }

    fn send_heartbeats(&self) {
        let (term, prev_log_index, prev_log_term, leader_commit) = {
            let inner = self.inner.lock().expect("raft log mutex poisoned");
            (
                inner.current_term,
                inner.last_index(),
                inner.last_term(),
                inner.commit_index,
            )
        };
        let mut acks = 1usize;
        for peer in &self.cfg.peers {
            let req = RaftRpc::<E>::AppendEntries {
                term,
                leader_id: self.cfg.node_id.clone(),
                prev_log_index,
                prev_log_term,
                entries: Vec::new(),
                leader_commit,
            };
            let started = Instant::now();
            let resp = send_rpc(
                peer.addr,
                &req,
                Duration::from_millis(self.cfg.heartbeat_ms.max(100)),
            );
            let latency_ok = matches!(&resp, Ok(RaftRpc::AppendResp { success: true, .. }));
            self.record_replication_latency(started, latency_ok);
            match resp {
                Ok(RaftRpc::AppendResp {
                    term: peer_term,
                    success,
                    ..
                }) => {
                    if peer_term > term {
                        self.step_down(peer_term, None);
                        return;
                    }
                    if success || self.replicate_full_log_to(peer, term) {
                        acks += 1;
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }
        if acks >= self.cfg.majority() {
            self.runtime
                .lock()
                .expect("raft runtime mutex poisoned")
                .last_quorum_at = Some(Instant::now());
        }
    }

    fn replicate_entry(
        &self,
        term: u64,
        prev_log_index: u64,
        prev_log_term: u64,
        env: RaftEnvelope<E>,
    ) -> bool {
        let leader_commit = {
            self.inner
                .lock()
                .expect("raft log mutex poisoned")
                .commit_index
        };
        let mut acks = 1usize;
        for peer in &self.cfg.peers {
            let req = RaftRpc::<E>::AppendEntries {
                term,
                leader_id: self.cfg.node_id.clone(),
                prev_log_index,
                prev_log_term,
                entries: vec![env.clone()],
                leader_commit,
            };
            let started = Instant::now();
            let resp = send_rpc(peer.addr, &req, Duration::from_secs(2));
            let latency_ok = matches!(&resp, Ok(RaftRpc::AppendResp { success: true, .. }));
            self.record_replication_latency(started, latency_ok);
            match resp {
                Ok(RaftRpc::AppendResp {
                    term: peer_term,
                    success,
                    ..
                }) => {
                    if peer_term > term {
                        self.step_down(peer_term, None);
                        return false;
                    }
                    if success || self.replicate_full_log_to(peer, term) {
                        acks += 1;
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }
        let ok = acks >= self.cfg.majority();
        if ok {
            self.runtime
                .lock()
                .expect("raft runtime mutex poisoned")
                .last_quorum_at = Some(Instant::now());
        }
        ok
    }

    fn replicate_full_log_to(&self, peer: &ConsensusPeer, term: u64) -> bool {
        let (entries, leader_commit) = {
            let inner = self.inner.lock().expect("raft log mutex poisoned");
            (inner.all_entries(), inner.commit_index)
        };
        if entries.is_empty() {
            return true;
        }
        let req = RaftRpc::<E>::AppendEntries {
            term,
            leader_id: self.cfg.node_id.clone(),
            prev_log_index: 0,
            prev_log_term: 0,
            leader_commit,
            entries,
        };
        let started = Instant::now();
        let resp = send_rpc(peer.addr, &req, RAFT_BULK_RPC_TIMEOUT);
        let latency_ok = matches!(&resp, Ok(RaftRpc::AppendResp { success: true, .. }));
        self.record_replication_latency(started, latency_ok);
        match resp {
            Ok(RaftRpc::AppendResp {
                term: peer_term,
                success,
                ..
            }) => {
                if peer_term > term {
                    self.step_down(peer_term, None);
                    return false;
                }
                success
            }
            _ => false,
        }
    }

    fn step_down(&self, term: u64, leader_id: Option<String>) {
        {
            let mut inner = self.inner.lock().expect("raft log mutex poisoned");
            if term > inner.current_term {
                if let Err(err) = inner.set_term_vote(term, None) {
                    tracing::warn!(err = %err, "raft step-down persist failed");
                }
            }
        }
        self.step_down_runtime(leader_id);
    }

    fn step_down_runtime(&self, leader_id: Option<String>) {
        let mut rt = self.runtime.lock().expect("raft runtime mutex poisoned");
        rt.role = Role::Follower;
        rt.leader_id = leader_id;
        rt.last_leader_seen = Instant::now();
        rt.last_quorum_at = None;
    }

    fn step_down_runtime_preserving_timeout(&self, leader_id: Option<String>) {
        let mut rt = self.runtime.lock().expect("raft runtime mutex poisoned");
        rt.role = Role::Follower;
        rt.leader_id = leader_id;
        rt.last_quorum_at = None;
    }

    fn node_jitter_ms(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.cfg.node_id.hash(&mut h);
        h.finish() % (self.cfg.election_timeout_ms / 2).max(1)
    }
}

fn send_rpc<E>(addr: SocketAddr, req: &RaftRpc<E>, timeout: Duration) -> anyhow::Result<RaftRpc<E>>
where
    E: serde::Serialize + DeserializeOwned + Clone,
{
    let stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut wr = stream.try_clone()?;
    serde_json::to_writer(&mut wr, req)?;
    wr.write_all(b"\n")?;
    wr.flush()?;

    let mut rd = BufReader::new(stream);
    let mut line = String::new();
    rd.read_line(&mut line)?;
    if line.trim().is_empty() {
        return Err(anyhow::anyhow!("empty raft rpc response"));
    }
    Ok(serde_json::from_str(line.trim())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replicated_append_rejects_gaps_and_overwrites_conflicts() {
        let dir = std::env::temp_dir().join(format!(
            "slopmud_raftlog_test_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut inner = RaftLogInner::<String>::new(dir);
        inner
            .append_replicated(RaftEnvelope {
                index: 1,
                term: 1,
                ms: 0,
                entry: "a".to_string(),
            })
            .unwrap();
        assert!(
            inner
                .append_replicated(RaftEnvelope {
                    index: 3,
                    term: 1,
                    ms: 0,
                    entry: "gap".to_string(),
                })
                .is_err()
        );
        inner
            .append_replicated(RaftEnvelope {
                index: 1,
                term: 2,
                ms: 0,
                entry: "b".to_string(),
            })
            .unwrap();
        assert_eq!(inner.entries.len(), 1);
        assert_eq!(inner.entries[0].term, 2);
        assert_eq!(inner.entries[0].entry, "b");
    }

    #[test]
    fn failed_leader_append_rolls_back_uncommitted_entry() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_raftlog_failed_append_test_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let mut inner = RaftLogInner::<String>::new(path.clone());
        inner.set_term_vote(1, Some("n0".to_string())).unwrap();
        let consensus = Consensus {
            cfg: ConsensusConfig {
                node_id: "n0".to_string(),
                bind: None,
                peers: vec![ConsensusPeer {
                    node_id: "n1".to_string(),
                    addr: "127.0.0.1:9".parse().unwrap(),
                }],
                election_timeout_ms: 5_000,
                heartbeat_ms: 500,
            },
            inner: Arc::new(Mutex::new(inner)),
            runtime: Mutex::new(RuntimeState {
                role: Role::Leader,
                leader_id: Some("n0".to_string()),
                last_leader_seen: Instant::now(),
                last_quorum_at: Some(Instant::now()),
                replication_latency: ReplicationLatencyStats::default(),
            }),
        };

        assert!(consensus.append(0, "uncommitted".to_string()).is_err());
        let inner = consensus.inner.lock().unwrap();
        assert_eq!(inner.last_index(), 0);
        assert_eq!(inner.next_log_index, 1);
        assert_eq!(inner.commit_index, 0);
        drop(inner);

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn full_log_replication_truncates_divergent_suffix() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_raftlog_full_truncate_test_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let mut inner = RaftLogInner::<String>::new(path.clone());
        inner
            .append_replicated(RaftEnvelope {
                index: 1,
                term: 1,
                ms: 0,
                entry: "shared".to_string(),
            })
            .unwrap();
        inner
            .append_replicated(RaftEnvelope {
                index: 2,
                term: 9,
                ms: 0,
                entry: "divergent".to_string(),
            })
            .unwrap();
        inner.set_commit_index(1).unwrap();

        let consensus = Consensus {
            cfg: ConsensusConfig {
                node_id: "n1".to_string(),
                bind: None,
                peers: Vec::new(),
                election_timeout_ms: 5_000,
                heartbeat_ms: 500,
            },
            inner: Arc::new(Mutex::new(inner)),
            runtime: Mutex::new(RuntimeState {
                role: Role::Follower,
                leader_id: Some("n0".to_string()),
                last_leader_seen: Instant::now(),
                last_quorum_at: None,
                replication_latency: ReplicationLatencyStats::default(),
            }),
        };

        let resp = consensus.handle_append_entries(
            2,
            "n0".to_string(),
            0,
            0,
            vec![RaftEnvelope {
                index: 1,
                term: 1,
                ms: 0,
                entry: "shared".to_string(),
            }],
            1,
        );
        match resp {
            RaftRpc::AppendResp { success, .. } => assert!(success),
            _ => panic!("unexpected raft rpc response"),
        }
        let inner = consensus.inner.lock().unwrap();
        assert_eq!(inner.last_index(), 1);
        assert_eq!(inner.next_log_index, 2);
        assert_eq!(inner.commit_index, 1);
        drop(inner);

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn poll_replay_waits_for_commit_index() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_raftlog_commit_test_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut inner = RaftLogInner::<String>::new(path);
        inner
            .append_replicated(RaftEnvelope {
                index: 1,
                term: 1,
                ms: 0,
                entry: "a".to_string(),
            })
            .unwrap();
        inner
            .append_replicated(RaftEnvelope {
                index: 2,
                term: 1,
                ms: 1,
                entry: "b".to_string(),
            })
            .unwrap();

        let mut log = RaftLog {
            inner: Arc::new(Mutex::new(inner)),
            consensus: None,
        };
        assert!(log.poll_replay().unwrap().is_empty());

        {
            let mut inner = log.inner.lock().unwrap();
            inner.set_commit_index(1).unwrap();
        }
        let replay = log.poll_replay().unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].entry, "a");

        {
            let mut inner = log.inner.lock().unwrap();
            inner.set_commit_index(2).unwrap();
        }
        let replay = log.poll_replay().unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].entry, "b");
    }

    #[test]
    fn consensus_open_commits_existing_log_without_state_file() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_raftlog_migrate_test_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let env = RaftEnvelope {
            index: 1,
            term: 0,
            ms: 0,
            entry: "legacy".to_string(),
        };
        std::fs::write(&path, format!("{}\n", serde_json::to_string(&env).unwrap())).unwrap();

        let cfg = ConsensusConfig {
            node_id: "n0".to_string(),
            bind: Some("127.0.0.1:0".parse().unwrap()),
            peers: vec![ConsensusPeer {
                node_id: "n1".to_string(),
                addr: "127.0.0.1:9".parse().unwrap(),
            }],
            election_timeout_ms: 60_000,
            heartbeat_ms: 60_000,
        };
        let (_log, replay) = RaftLog::<String>::open_with_consensus(path.clone(), cfg).unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].entry, "legacy");

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn denied_stale_vote_does_not_reset_election_timeout() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_raftlog_vote_timeout_test_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let mut inner = RaftLogInner::<String>::new(path.clone());
        inner
            .append_replicated(RaftEnvelope {
                index: 1,
                term: 3,
                ms: 0,
                entry: "newer".to_string(),
            })
            .unwrap();
        inner.set_term_vote(4, None).unwrap();

        let last_leader_seen = Instant::now() - Duration::from_secs(60);
        let consensus = Consensus {
            cfg: ConsensusConfig {
                node_id: "n0".to_string(),
                bind: None,
                peers: Vec::new(),
                election_timeout_ms: 5_000,
                heartbeat_ms: 500,
            },
            inner: Arc::new(Mutex::new(inner)),
            runtime: Mutex::new(RuntimeState {
                role: Role::Candidate,
                leader_id: Some("old".to_string()),
                last_leader_seen,
                last_quorum_at: Some(last_leader_seen),
                replication_latency: ReplicationLatencyStats::default(),
            }),
        };

        let resp = consensus.handle_request_vote(5, "stale".to_string(), 1, 1);
        match resp {
            RaftRpc::VoteResp { term, vote_granted } => {
                assert_eq!(term, 5);
                assert!(!vote_granted);
            }
            _ => panic!("unexpected raft rpc response"),
        }

        let inner = consensus.inner.lock().unwrap();
        assert_eq!(inner.current_term, 5);
        assert_eq!(inner.voted_for, None);
        drop(inner);

        let rt = consensus.runtime.lock().unwrap();
        assert_eq!(rt.role, Role::Follower);
        assert_eq!(rt.leader_id, None);
        assert_eq!(rt.last_leader_seen, last_leader_seen);
        assert_eq!(rt.last_quorum_at, None);

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn replication_latency_status_reports_distribution() {
        let mut stats = ReplicationLatencyStats::default();
        stats.record(Duration::from_millis(0), true);
        stats.record(Duration::from_millis(7), true);
        stats.record(Duration::from_millis(120), false);

        let lines = stats.status_lines().join("\n");
        assert!(lines.contains("replication_latency_ms: count=3 ok=2 err=1"));
        assert!(lines.contains("p50="));
        assert!(lines.contains("replication_latency_buckets:"));
        assert!(lines.contains("<=10ms:1"));
        assert!(lines.contains("<=250ms:1"));
    }

    #[test]
    fn raft_control_status_reports_runtime_state() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_raftlog_status_test_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let mut inner = RaftLogInner::<String>::new(path.clone());
        inner
            .append_replicated(RaftEnvelope {
                index: 1,
                term: 7,
                ms: 0,
                entry: "status".to_string(),
            })
            .unwrap();
        inner.set_commit_index(1).unwrap();
        inner.set_term_vote(7, Some("n0".to_string())).unwrap();

        let consensus = Consensus {
            cfg: ConsensusConfig {
                node_id: "n0".to_string(),
                bind: None,
                peers: vec![ConsensusPeer {
                    node_id: "n1".to_string(),
                    addr: "127.0.0.1:9".parse().unwrap(),
                }],
                election_timeout_ms: 5_000,
                heartbeat_ms: 500,
            },
            inner: Arc::new(Mutex::new(inner)),
            runtime: Mutex::new(RuntimeState {
                role: Role::Leader,
                leader_id: Some("n0".to_string()),
                last_leader_seen: Instant::now(),
                last_quorum_at: Some(Instant::now()),
                replication_latency: ReplicationLatencyStats::default(),
            }),
        };

        match consensus.handle_status() {
            RaftRpc::StatusResp {
                term,
                node_id,
                role,
                leader_id,
                commit_index,
                last_index,
                last_log_term,
                quorum_recent,
            } => {
                assert_eq!(term, 7);
                assert_eq!(node_id, "n0");
                assert_eq!(role, "Leader");
                assert_eq!(leader_id.as_deref(), Some("n0"));
                assert_eq!(commit_index, 1);
                assert_eq!(last_index, 1);
                assert_eq!(last_log_term, 7);
                assert!(quorum_recent);
            }
            _ => panic!("unexpected raft rpc response"),
        }

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn timeout_now_starts_immediate_election() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_raftlog_timeout_now_test_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let mut inner = RaftLogInner::<String>::new(path.clone());
        inner.set_term_vote(4, None).unwrap();

        let consensus = Consensus {
            cfg: ConsensusConfig {
                node_id: "n1".to_string(),
                bind: None,
                peers: Vec::new(),
                election_timeout_ms: 5_000,
                heartbeat_ms: 500,
            },
            inner: Arc::new(Mutex::new(inner)),
            runtime: Mutex::new(RuntimeState {
                role: Role::Follower,
                leader_id: Some("n0".to_string()),
                last_leader_seen: Instant::now(),
                last_quorum_at: None,
                replication_latency: ReplicationLatencyStats::default(),
            }),
        };

        match consensus.handle_timeout_now(4, "n0".to_string()) {
            RaftRpc::TimeoutNowResp {
                term,
                started,
                leader_id,
                ..
            } => {
                assert_eq!(term, 5);
                assert!(started);
                assert_eq!(leader_id.as_deref(), Some("n1"));
            }
            _ => panic!("unexpected raft rpc response"),
        }

        let rt = consensus.runtime.lock().unwrap();
        assert_eq!(rt.role, Role::Leader);
        assert_eq!(rt.leader_id.as_deref(), Some("n1"));
        drop(rt);

        let inner = consensus.inner.lock().unwrap();
        assert_eq!(inner.current_term, 5);
        assert_eq!(inner.voted_for.as_deref(), Some("n1"));
        drop(inner);

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn transfer_leader_rejects_non_leaders() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_raftlog_transfer_follower_test_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let mut inner = RaftLogInner::<String>::new(path.clone());
        inner.set_term_vote(4, None).unwrap();
        let consensus = Consensus {
            cfg: ConsensusConfig {
                node_id: "n1".to_string(),
                bind: None,
                peers: vec![ConsensusPeer {
                    node_id: "n0".to_string(),
                    addr: "127.0.0.1:9".parse().unwrap(),
                }],
                election_timeout_ms: 5_000,
                heartbeat_ms: 500,
            },
            inner: Arc::new(Mutex::new(inner)),
            runtime: Mutex::new(RuntimeState {
                role: Role::Follower,
                leader_id: Some("n0".to_string()),
                last_leader_seen: Instant::now(),
                last_quorum_at: None,
                replication_latency: ReplicationLatencyStats::default(),
            }),
        };

        match consensus.handle_transfer_leader(Some("n0".to_string())) {
            RaftRpc::TransferLeaderResp {
                accepted,
                leader_id,
                reason,
                ..
            } => {
                assert!(!accepted);
                assert_eq!(leader_id.as_deref(), Some("n0"));
                assert_eq!(reason, "node is not leader");
            }
            _ => panic!("unexpected raft rpc response"),
        }

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }
}
