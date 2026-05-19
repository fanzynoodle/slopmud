use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde_json::json;

const SHARD_BIN: &str = "target/debug/shard_01";
const BROKER_BIN: &str = "target/debug/slopmud";

fn usage_and_exit() -> ! {
    eprintln!(
        "e2e_shard_raft_trio\n\n\
USAGE:\n\
  e2e_shard_raft_trio [--skip-build]\n"
    );
    std::process::exit(2);
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn pick_free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn wait_for_port(addr: SocketAddr, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    let mut last = None;
    while Instant::now() < deadline {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(500)) {
            Ok(_) => return Ok(()),
            Err(err) => {
                last = Some(err);
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    anyhow::bail!("timed out waiting for {addr}: {last:?}");
}

fn wait_for_file_nonempty(path: &PathBuf, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.metadata().is_ok_and(|m| m.len() > 0) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    anyhow::bail!("timed out waiting for nonempty {}", path.display());
}

fn ensure_built(envs: &HashMap<String, String>, skip_build: bool) -> anyhow::Result<()> {
    if skip_build && PathBuf::from(SHARD_BIN).exists() && PathBuf::from(BROKER_BIN).exists() {
        return Ok(());
    }
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "-q", "-p", "shard_01", "-p", "slopmud"])
        .envs(envs);
    let status = cmd.status().context("spawn cargo build")?;
    anyhow::ensure!(status.success(), "cargo build failed with status {status}");
    Ok(())
}

fn stop_child(child: &mut Option<Child>) {
    let Some(mut child) = child.take() else {
        return;
    };
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) | Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn append_log_file(path: PathBuf) -> anyhow::Result<(File, File)> {
    let out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open log {}", path.display()))?;
    let err = out
        .try_clone()
        .with_context(|| format!("clone log {}", path.display()))?;
    Ok((out, err))
}

fn send_line(stream: &mut TcpStream, line: &str) -> anyhow::Result<()> {
    stream.write_all(line.trim().as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

struct Client {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl Client {
    fn connect(addr: SocketAddr, timeout: Duration) -> anyhow::Result<Self> {
        let deadline = Instant::now() + timeout;
        let mut last = None;
        while Instant::now() < deadline {
            match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
                Ok(stream) => {
                    stream.set_nodelay(true).ok();
                    return Ok(Self {
                        stream,
                        buf: Vec::new(),
                    });
                }
                Err(err) => {
                    last = Some(err);
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
        anyhow::bail!("connect {addr} failed: {last:?}");
    }

    fn read_some(&mut self, timeout: Duration) -> anyhow::Result<Vec<u8>> {
        self.stream.set_read_timeout(Some(timeout))?;
        let mut chunk = [0u8; 4096];
        match self.stream.read(&mut chunk) {
            Ok(0) => Ok(Vec::new()),
            Ok(n) => Ok(chunk[..n].to_vec()),
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(Vec::new())
            }
            Err(err) => Err(err.into()),
        }
    }

    fn read_until<S: AsRef<[u8]>>(
        &mut self,
        needles: &[S],
        timeout: Duration,
    ) -> anyhow::Result<Vec<u8>> {
        let needles = needles
            .iter()
            .map(|s| s.as_ref().to_vec())
            .collect::<Vec<_>>();
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let mut best: Option<(usize, usize)> = None;
            for needle in &needles {
                if let Some(pos) = find_subslice(&self.buf, needle) {
                    if best.is_none_or(|(best_pos, _)| pos < best_pos) {
                        best = Some((pos, needle.len()));
                    }
                }
            }
            if let Some((pos, len)) = best {
                let end = pos + len;
                let out = self.buf[..end].to_vec();
                self.buf.drain(..end);
                return Ok(out);
            }

            let chunk = self.read_some(Duration::from_millis(250))?;
            if chunk.is_empty() {
                std::thread::sleep(Duration::from_millis(20));
            } else {
                self.buf.extend_from_slice(&chunk);
            }
        }
        anyhow::bail!(
            "timeout waiting for {:?}; got:\n{}",
            needles
                .iter()
                .map(|n| String::from_utf8_lossy(n).into_owned())
                .collect::<Vec<_>>(),
            String::from_utf8_lossy(&self.buf)
        );
    }

    fn send_line(&mut self, line: &str) -> anyhow::Result<()> {
        send_line(&mut self.stream, line)
    }
}

fn assert_no_failover_notice(data: &[u8]) -> anyhow::Result<()> {
    let text = String::from_utf8_lossy(data).to_ascii_lowercase();
    for needle in [
        "shard disconnected",
        "shard offline",
        "input dropped",
        "reconnecting",
        "hi alice",
    ] {
        anyhow::ensure!(
            !text.contains(needle),
            "visible failover artifact {needle:?} in:\n{text}"
        );
    }
    Ok(())
}

fn connect_and_create(name: &str, is_bot: bool, broker_addr: SocketAddr) -> anyhow::Result<Client> {
    let mut client = Client::connect(broker_addr, Duration::from_secs(12))?;
    client.read_until(&[b"name:"], Duration::from_secs(8))?;
    client.send_line(name)?;

    let out = client.read_until(
        &[
            b"type: password".as_slice(),
            b"set password".as_slice(),
            b"password (never logged/echoed)".as_slice(),
        ],
        Duration::from_secs(12),
    )?;
    if out
        .windows(b"type: password".len())
        .any(|w| w == b"type: password")
    {
        client.send_line("password")?;
        client.read_until(
            &[
                b"set password".as_slice(),
                b"password (never logged/echoed)".as_slice(),
            ],
            Duration::from_secs(12),
        )?;
    }
    client.send_line(&format!("pw-{name}-1234"))?;
    client.read_until(&[b"type: human | bot"], Duration::from_secs(12))?;
    client.send_line(if is_bot { "bot" } else { "human" })?;
    client.read_until(&[b"type: agree"], Duration::from_secs(12))?;
    client.send_line("agree")?;
    client.read_until(&[b"code of conduct:"], Duration::from_secs(12))?;
    client.read_until(&[b"type: agree"], Duration::from_secs(12))?;
    client.send_line("agree")?;
    client.read_until(
        &[b"choose race:".as_slice(), b"type: race list".as_slice()],
        Duration::from_secs(12),
    )?;
    client.send_line("race human")?;
    client.read_until(
        &[b"choose class:".as_slice(), b"type: class list".as_slice()],
        Duration::from_secs(12),
    )?;
    client.send_line("class fighter")?;
    client.read_until(&[b"sex:"], Duration::from_secs(12))?;
    client.send_line("none")?;
    client.read_until(&[b">"], Duration::from_secs(15))?;
    client.send_line("look")?;
    client.read_until(&[b"Orientation Wing"], Duration::from_secs(15))?;
    Ok(client)
}

fn command_text(
    client: &mut Client,
    line: &str,
    needles: &[&str],
    timeout: Duration,
) -> anyhow::Result<String> {
    client.send_line(line)?;
    let needle_bytes = needles.iter().map(|s| s.as_bytes()).collect::<Vec<_>>();
    let mut out = client.read_until(&needle_bytes, timeout)?;
    if !client.buf.is_empty() {
        out.extend_from_slice(&client.buf);
        client.buf.clear();
    }
    out.extend(client.read_some(Duration::from_millis(200))?);
    assert_no_failover_notice(&out)?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn raft_feature_worldlog(client: &mut Client, target: u32, check: bool) -> anyhow::Result<String> {
    let suffix = if check { " check" } else { "" };
    command_text(
        client,
        &format!("raft feature worldlog {target}{suffix}"),
        &[
            &format!("worldlog format {target} blocked"),
            &format!("worldlog format {target} would activate"),
            &format!("worldlog format {target} active"),
            &format!("worldlog format {target} already active"),
            "raft feature: not activated",
        ],
        Duration::from_secs(12),
    )
}

fn current_leader_index(client: &mut Client) -> anyhow::Result<usize> {
    client.send_line("raft status")?;
    let mut out = client.read_until(
        &[
            b" - quorum_recent: true".as_slice(),
            b" - quorum_recent: false".as_slice(),
        ],
        Duration::from_secs(8),
    )?;
    out.extend(client.read_some(Duration::from_millis(100))?);
    let text = String::from_utf8_lossy(&out);
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("- node_id: n") {
            return rest
                .parse::<usize>()
                .with_context(|| format!("parse leader index from {line:?}"));
        }
    }
    anyhow::bail!("could not parse leader node from raft status:\n{text}");
}

fn raft_rpc(
    addr: SocketAddr,
    payload: serde_json::Value,
    timeout: Duration,
) -> anyhow::Result<serde_json::Value> {
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(serde_json::to_string(&payload)?.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut buf = Vec::new();
    let mut one = [0u8; 1];
    while !buf.ends_with(b"\n") {
        let n = stream.read(&mut one)?;
        if n == 0 {
            break;
        }
        buf.push(one[0]);
    }
    anyhow::ensure!(!buf.is_empty(), "empty raft rpc response from {addr}");
    Ok(serde_json::from_slice(&buf)?)
}

struct Harness {
    envs: HashMap<String, String>,
    shard_addrs: Vec<SocketAddr>,
    raft_addrs: Vec<SocketAddr>,
    broker_addr: SocketAddr,
    raft_paths: Vec<PathBuf>,
    accounts_path: PathBuf,
    log_dir: PathBuf,
    shards: Vec<Option<Child>>,
    restart_counts: Vec<u32>,
    max_formats: Vec<u32>,
    broker: Option<Child>,
}

impl Harness {
    fn new(envs: HashMap<String, String>) -> anyhow::Result<Self> {
        let run_id = now_nanos().to_string();
        let shard_addrs = (0..3)
            .map(|_| Ok(SocketAddr::from(([127, 0, 0, 1], pick_free_port()?))))
            .collect::<std::io::Result<Vec<_>>>()?;
        let raft_addrs = (0..3)
            .map(|_| Ok(SocketAddr::from(([127, 0, 0, 1], pick_free_port()?))))
            .collect::<std::io::Result<Vec<_>>>()?;
        let broker_addr = SocketAddr::from(([127, 0, 0, 1], pick_free_port()?));
        let raft_paths = (0..3)
            .map(|i| PathBuf::from(format!("/tmp/slopmud_shard_raft_trio_{run_id}_{i}.jsonl")))
            .collect::<Vec<_>>();
        let accounts_path = PathBuf::from(format!("/tmp/slopmud_accounts_trio_{run_id}.json"));
        let log_dir = PathBuf::from(format!("/tmp/slopmud_trio_{run_id}"));
        std::fs::create_dir_all(&log_dir)?;

        Ok(Self {
            envs,
            shard_addrs,
            raft_addrs,
            broker_addr,
            raft_paths,
            accounts_path,
            log_dir,
            shards: (0..3).map(|_| None).collect(),
            restart_counts: vec![0, 0, 0],
            max_formats: vec![1, 1, 1],
            broker: None,
        })
    }

    fn base_env(&self) -> HashMap<String, String> {
        let mut env = self.envs.clone();
        env.insert("RUST_BACKTRACE".to_string(), "1".to_string());
        env.insert("WORLD_TICK_MS".to_string(), "100".to_string());
        env.insert("BARTENDER_EMOTE_MS".to_string(), "1000".to_string());
        env.insert("MOB_WANDER_MS".to_string(), "10000".to_string());
        env.insert("SLOPMUD_BIND".to_string(), self.broker_addr.to_string());
        env.insert("SHARD_ADDR".to_string(), self.shard_addrs[0].to_string());
        env.insert(
            "SHARD_ADDRS".to_string(),
            self.shard_addrs
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        env.insert(
            "SLOPMUD_ACCOUNTS_PATH".to_string(),
            self.accounts_path.display().to_string(),
        );
        env.insert("SLOPMUD_EVENTLOG_ENABLED".to_string(), "0".to_string());
        env.insert("SLOPMUD_NEARLINE_ENABLED".to_string(), "0".to_string());
        env.insert("SHARD_RAFT_HEARTBEAT_MS".to_string(), "50".to_string());
        env.insert("SHARD_BOOTSTRAP_ADMINS".to_string(), "Alice".to_string());
        env
    }

    fn shard_env(&self, i: usize) -> HashMap<String, String> {
        let mut env = self.base_env();
        env.insert("SHARD_BIND".to_string(), self.shard_addrs[i].to_string());
        env.insert("NODE_ID".to_string(), format!("shard-trio-{i}"));
        env.insert("SHARD_RAFT_NODE_ID".to_string(), format!("n{i}"));
        env.insert(
            "SHARD_RAFT_BIND".to_string(),
            self.raft_addrs[i].to_string(),
        );
        env.insert(
            "SHARD_RAFT_LOG".to_string(),
            self.raft_paths[i].display().to_string(),
        );
        let election_ms = if self.restart_counts[i] == 0 {
            220 + (i as u64 * 170)
        } else {
            2000 + (i as u64 * 100)
        };
        env.insert(
            "SHARD_RAFT_ELECTION_MS".to_string(),
            election_ms.to_string(),
        );
        env.insert(
            "SHARD_RAFT_PEERS".to_string(),
            (0..3)
                .filter(|j| *j != i)
                .map(|j| format!("n{}@{}", j, self.raft_addrs[j]))
                .collect::<Vec<_>>()
                .join(","),
        );
        env.insert(
            "SHARD_RAFT_APPLICATION_MAX_FORMAT".to_string(),
            self.max_formats[i].to_string(),
        );
        env
    }

    fn start_shard(&mut self, i: usize) -> anyhow::Result<()> {
        let (out, err) = append_log_file(self.log_dir.join(format!("shard_{i}.log")))?;
        let child = Command::new(SHARD_BIN)
            .envs(self.shard_env(i))
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .spawn()
            .with_context(|| format!("spawn {SHARD_BIN}"))?;
        wait_for_port(self.shard_addrs[i], Duration::from_secs(12))?;
        wait_for_port(self.raft_addrs[i], Duration::from_secs(12))?;
        self.shards[i] = Some(child);
        Ok(())
    }

    fn restart_shard(&mut self, i: usize) -> anyhow::Result<()> {
        self.restart_counts[i] += 1;
        self.start_shard(i)
    }

    fn replace_shard(&mut self, i: usize, max_format: u32) -> anyhow::Result<()> {
        self.max_formats[i] = max_format;
        stop_child(&mut self.shards[i]);
        self.restart_shard(i)
    }

    fn stop_shard(&mut self, i: usize) {
        stop_child(&mut self.shards[i]);
    }

    fn start_broker(&mut self) -> anyhow::Result<()> {
        let (out, err) = append_log_file(self.log_dir.join("broker.log"))?;
        let child = Command::new(BROKER_BIN)
            .envs(self.base_env())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .spawn()
            .with_context(|| format!("spawn {BROKER_BIN}"))?;
        wait_for_port(self.broker_addr, Duration::from_secs(12))?;
        self.broker = Some(child);
        Ok(())
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        stop_child(&mut self.broker);
        for shard in &mut self.shards {
            stop_child(shard);
        }
    }
}

fn parse_args() -> bool {
    let mut skip_build = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--skip-build" => skip_build = true,
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }
    skip_build
}

fn run() -> anyhow::Result<()> {
    let skip_build = parse_args();
    if let Err(err) = pick_free_port() {
        if matches!(err.raw_os_error(), Some(1 | 13)) {
            eprintln!("SKIP: e2e_shard_raft_trio requires TCP sockets");
            return Ok(());
        }
        return Err(err.into());
    }

    let envs = std::env::vars().collect::<HashMap<_, _>>();
    ensure_built(&envs, skip_build)?;
    let mut h = Harness::new(envs)?;

    for i in 0..3 {
        h.start_shard(i)?;
    }
    for addr in &h.raft_addrs {
        wait_for_port(*addr, Duration::from_secs(12))?;
    }
    h.start_broker()?;

    let mut alice = connect_and_create("Alice", false, h.broker_addr)?;
    let _ = alice.read_some(Duration::from_millis(300))?;

    wait_for_file_nonempty(&h.raft_paths[0], Duration::from_secs(15))?;
    alice.send_line("quest set trio.probe alive")?;
    let out = alice.read_until(&[b"quest: set trio.probe=alive"], Duration::from_secs(8))?;
    assert_no_failover_notice(&out)?;

    let text = command_text(
        &mut alice,
        "raft metadata set rollout.probe before",
        &["requires active format 2"],
        Duration::from_secs(8),
    )?;
    anyhow::ensure!(
        text.contains("requires active format 2"),
        "format-2 metadata unexpectedly allowed before activation:\n{text}"
    );

    let text = raft_feature_worldlog(&mut alice, 2, true)?;
    anyhow::ensure!(
        text.contains("blocked") && text.contains("max=1"),
        "AAA activation check should block on legacy voters:\n{text}"
    );

    h.replace_shard(0, 2)?;
    let text = raft_feature_worldlog(&mut alice, 2, true)?;
    anyhow::ensure!(
        text.contains("blocked") && text.contains("max=1"),
        "AAB activation check should block on legacy voters:\n{text}"
    );

    h.replace_shard(1, 2)?;
    let text = raft_feature_worldlog(&mut alice, 2, true)?;
    anyhow::ensure!(
        text.contains("blocked") && text.contains("max=1"),
        "ABB activation check should block on the last legacy voter:\n{text}"
    );

    h.stop_shard(2);
    let text = raft_feature_worldlog(&mut alice, 2, true)?;
    anyhow::ensure!(
        text.contains("blocked") && text.contains("unreachable"),
        "activation should block on unreachable voter:\n{text}"
    );

    h.replace_shard(2, 2)?;
    let text = raft_feature_worldlog(&mut alice, 2, true)?;
    anyhow::ensure!(
        text.contains("would activate"),
        "BBB activation dry-run should pass:\n{text}"
    );
    let text = raft_feature_worldlog(&mut alice, 2, false)?;
    anyhow::ensure!(
        text.contains("worldlog format 2 active"),
        "BBB activation should append the feature record:\n{text}"
    );
    let text = command_text(
        &mut alice,
        "raft metadata set rollout.probe after",
        &["raft metadata: set rollout.probe=after"],
        Duration::from_secs(8),
    )?;
    anyhow::ensure!(
        text.contains("raft metadata: set rollout.probe=after"),
        "format-2 metadata not written after activation:\n{text}"
    );
    let text = command_text(
        &mut alice,
        "raft status",
        &["world_log_format_active: 2"],
        Duration::from_secs(8),
    )?;
    anyhow::ensure!(
        text.contains("cluster_metadata_count:") && text.contains("voter n"),
        "raft status did not include feature/voter detail:\n{text}"
    );

    for i in 0..3 {
        h.replace_shard(i, 2)?;
    }
    let text = command_text(
        &mut alice,
        "raft status",
        &["world_log_format_active: 2"],
        Duration::from_secs(12),
    )?;
    anyhow::ensure!(
        text.contains("world_log_format_active: 2"),
        "active format did not survive restart/replay:\n{text}"
    );

    let leader = current_leader_index(&mut alice)?;
    let lease_target = (leader + 1) % 3;
    let contenders = 24usize;
    let barrier = Arc::new(Barrier::new(contenders));
    let mut handles = Vec::new();
    for i in 0..contenders {
        let b = barrier.clone();
        let addr = h.raft_addrs[leader];
        handles.push(std::thread::spawn(
            move || -> anyhow::Result<(String, bool)> {
                let token = format!("e2e-race-{i}");
                b.wait();
                let resp = raft_rpc(
                    addr,
                    json!({
                        "t": "RestartLeaseReq",
                        "node_id": format!("n{lease_target}"),
                        "token": token,
                        "ttl_ms": 30_000,
                    }),
                    Duration::from_secs(4),
                )?;
                anyhow::ensure!(
                    resp.get("t").and_then(|v| v.as_str()) == Some("RestartLeaseResp"),
                    "unexpected restart lease response: {resp}"
                );
                Ok((
                    resp.get("token")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    resp.get("accepted").and_then(|v| v.as_bool()) == Some(true),
                ))
            },
        ));
    }
    let mut winning_tokens = Vec::new();
    for handle in handles {
        let (token, accepted) = handle
            .join()
            .map_err(|_| anyhow::anyhow!("restart lease race thread panicked"))??;
        if accepted {
            winning_tokens.push(token);
        }
    }
    anyhow::ensure!(
        winning_tokens.len() == 1,
        "expected exactly one restart lease winner, got {winning_tokens:?}"
    );
    let transfer_while_leased = raft_rpc(
        h.raft_addrs[leader],
        json!({"t":"TransferLeaderReq","target_id":format!("n{lease_target}")}),
        Duration::from_secs(4),
    )?;
    anyhow::ensure!(
        transfer_while_leased.get("t").and_then(|v| v.as_str()) == Some("TransferLeaderResp")
            && transfer_while_leased
                .get("accepted")
                .and_then(|v| v.as_bool())
                == Some(false)
            && transfer_while_leased
                .get("reason")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("restart lease active")),
        "leader transfer should be blocked while restart lease is active: {transfer_while_leased}"
    );
    let release = raft_rpc(
        h.raft_addrs[leader],
        json!({
            "t": "RestartLeaseReleaseReq",
            "node_id": format!("n{lease_target}"),
            "token": winning_tokens[0].clone(),
        }),
        Duration::from_secs(4),
    )?;
    anyhow::ensure!(
        release.get("t").and_then(|v| v.as_str()) == Some("RestartLeaseReleaseResp")
            && release.get("accepted").and_then(|v| v.as_bool()) == Some(true),
        "restart lease release failed: {release}"
    );

    let leader = current_leader_index(&mut alice)?;
    let target = (leader + 1) % 3;
    let resp = raft_rpc(
        h.raft_addrs[leader],
        json!({"t":"TransferLeaderReq","target_id":format!("n{target}")}),
        Duration::from_secs(4),
    )?;
    anyhow::ensure!(
        resp.get("t").and_then(|v| v.as_str()) == Some("TransferLeaderResp")
            && resp.get("accepted").and_then(|v| v.as_bool()) == Some(true),
        "leadership transfer failed: {resp}"
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if current_leader_index(&mut alice)? == target {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::ensure!(
        current_leader_index(&mut alice)? == target,
        "leadership did not move to n{target}"
    );

    alice.send_line("quest get trio.probe")?;
    let out = alice.read_until(&[b"quest: trio.probe=alive"], Duration::from_secs(8))?;
    assert_no_failover_notice(&out)?;

    let mut killed_leaders = Vec::new();
    for step in 1..=3 {
        let victim = current_leader_index(&mut alice)?;
        killed_leaders.push(victim);
        h.stop_shard(victim);

        alice.send_line("quest get trio.probe")?;
        let mut out = alice.read_until(&[b"quest: trio.probe=alive"], Duration::from_secs(20))?;
        out.extend(alice.read_some(Duration::from_millis(300))?);
        assert_no_failover_notice(&out)?;

        alice.send_line(&format!("quest set trio.step {step}"))?;
        let out = alice.read_until(
            &[format!("quest: set trio.step={step}").as_bytes()],
            Duration::from_secs(8),
        )?;
        assert_no_failover_notice(&out)?;

        h.restart_shard(victim)?;
    }

    killed_leaders.sort_unstable();
    killed_leaders.dedup();
    anyhow::ensure!(
        killed_leaders.len() >= 2,
        "expected leadership to move, killed leaders: {killed_leaders:?}"
    );

    alice.send_line("quest get trio.step")?;
    let out = alice.read_until(&[b"quest: trio.step=3"], Duration::from_secs(8))?;
    assert_no_failover_notice(&out)?;

    println!("ok: e2e_shard_raft_trio");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    run()
}
