use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::protocol::Message;

fn usage_and_exit() -> ! {
    eprintln!(
        "combat_refs\n\n\
USAGE:\n\
  combat_refs --ws WS_URL --scenario PATH\n\
  combat_refs --ws WS_URL --suite PATH\n\n\
ENV:\n\
  WS_URL  optional default ws://127.0.0.1:4100/v1/json\n"
    );
    std::process::exit(2);
}

#[derive(Clone, Debug)]
struct Config {
    ws_url: String,
    scenario: Option<PathBuf>,
    suite: Option<PathBuf>,
}

fn parse_args() -> Config {
    let mut ws_url =
        std::env::var("WS_URL").unwrap_or_else(|_| "ws://127.0.0.1:4100/v1/json".to_string());
    let mut scenario: Option<PathBuf> = None;
    let mut suite: Option<PathBuf> = None;

    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--ws" => ws_url = it.next().unwrap_or_else(|| usage_and_exit()),
            "--scenario" => {
                scenario = Some(
                    it.next()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| usage_and_exit()),
                )
            }
            "--suite" => {
                suite = Some(
                    it.next()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| usage_and_exit()),
                )
            }
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    if scenario.is_some() == suite.is_some() {
        usage_and_exit();
    }

    Config {
        ws_url,
        scenario,
        suite,
    }
}

#[derive(Debug, Deserialize)]
struct SuiteFile {
    scenarios: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    id: String,
    #[serde(default)]
    #[allow(dead_code)]
    expect: Option<String>, // pass | xfail
    #[serde(default)]
    #[allow(dead_code)]
    xfail_reason: Option<String>,
    timeout_ms: u64,
    actors: Vec<Actor>,
    steps: Vec<Step>,
    stop_on: Vec<StopOn>,
}

#[derive(Debug, Deserialize)]
struct Actor {
    name: String,
    #[serde(default)]
    is_bot: bool,
    #[serde(default)]
    race: Option<String>,
    #[serde(default)]
    class: Option<String>,
    #[serde(default)]
    sex: Option<String>,
    #[serde(default)]
    pronouns: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Step {
    actor: String,
    send: String,
    #[serde(default)]
    wait_ms: Option<u64>,
    #[serde(default)]
    wait_for_contains: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StopOn {
    AnyContains { text: String },
}

#[derive(Debug, Clone, Serialize)]
struct ScenarioReport {
    id: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    err: Option<String>,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    matched: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tail: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SuiteReport {
    ok: bool,
    reports: Vec<ScenarioReport>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[allow(dead_code)] // Protocol fields are matched by serde; scenarios don't use every field.
enum JsonOut {
    Hello { mode: String },
    Attached { session: String },
    Output { text: String },
    Err { text: String },
    Pong {},
}

#[derive(Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum JsonIn<'a> {
    Attach {
        name: &'a str,
        is_bot: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        race: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        class: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sex: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pronouns: Option<&'a str>,
    },
    Input {
        line: &'a str,
    },
}

#[allow(dead_code)] // Some scaffolding is for future ref scenarios.
struct ActorConn {
    name: String,
    sink: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    rx: tokio::sync::mpsc::Receiver<String>,
}

async fn connect_actor(ws_url: &str, a: &Actor) -> anyhow::Result<ActorConn> {
    let (ws, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .with_context(|| format!("connect {ws_url}"))?;
    let (mut sink, mut stream) = ws.split();

    let attach = serde_json::to_string(&JsonIn::Attach {
        name: &a.name,
        is_bot: a.is_bot,
        race: a.race.as_deref(),
        class: a.class.as_deref(),
        sex: a.sex.as_deref(),
        pronouns: a.pronouns.as_deref(),
    })?;
    sink.send(Message::Text(attach.into())).await?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(256);
    let name = a.name.clone();
    tokio::spawn(async move {
        while let Some(m) = stream.next().await {
            let Ok(m) = m else { break };
            let Message::Text(s) = m else { continue };
            if let Ok(msg) = serde_json::from_str::<JsonOut>(&s) {
                match msg {
                    JsonOut::Output { text } => {
                        let _ = tx.send(text).await;
                    }
                    JsonOut::Err { text } => {
                        let _ = tx.send(format!("[err] {text}")).await;
                    }
                    _ => {}
                }
            }
        }
    });

    // Best-effort: wait for "attached" or some output.
    let _ = tokio::time::timeout(Duration::from_millis(800), async {
        let mut got = 0u32;
        while got < 2 {
            if rx.is_closed() {
                break;
            }
            if let Some(_) = rx.recv().await {
                got += 1;
            } else {
                break;
            }
        }
    })
    .await;

    Ok(ActorConn { name, sink, rx })
}

async fn actor_send(conn: &mut ActorConn, line: &str) -> anyhow::Result<()> {
    let msg = serde_json::to_string(&JsonIn::Input { line })?;
    conn.sink.send(Message::Text(msg.into())).await?;
    Ok(())
}

fn stop_matchers(stop_on: &[StopOn]) -> Vec<String> {
    stop_on
        .iter()
        .map(|s| match s {
            StopOn::AnyContains { text } => text.clone(),
        })
        .collect()
}

async fn run_scenario(ws_url: &str, s: &Scenario) -> ScenarioReport {
    let start = Instant::now();
    let mut matched: Vec<String> = Vec::new();
    let mut tail: Vec<String> = Vec::new();

    let mut conns: HashMap<String, ActorConn> = HashMap::new();
    for a in &s.actors {
        match connect_actor(ws_url, a).await {
            Ok(c) => {
                conns.insert(a.name.clone(), c);
            }
            Err(e) => {
                return ScenarioReport {
                    id: s.id.clone(),
                    ok: false,
                    err: Some(format!("connect failed for {}: {e}", a.name)),
                    duration_ms: start.elapsed().as_millis() as u64,
                    matched,
                    tail,
                };
            }
        }
    }

    let matchers = stop_matchers(&s.stop_on);
    let deadline = Instant::now() + Duration::from_millis(s.timeout_ms.max(100));

    let note_output =
        |actor: &str, txt: &str, tail: &mut Vec<String>, matched: &mut Vec<String>| {
            let one = txt.replace('\r', "").replace('\n', "\\n");
            tail.push(format!("{actor}: {one}"));
            if tail.len() > 25 {
                tail.drain(0..(tail.len() - 25));
            }
            for m in &matchers {
                if txt.contains(m) {
                    matched.push(m.clone());
                }
            }
        };

    // Step executor.
    for st in &s.steps {
        let Some(c) = conns.get_mut(&st.actor) else {
            return ScenarioReport {
                id: s.id.clone(),
                ok: false,
                err: Some(format!("unknown actor in step: {}", st.actor)),
                duration_ms: start.elapsed().as_millis() as u64,
                matched,
                tail,
            };
        };
        if let Err(e) = actor_send(c, &st.send).await {
            return ScenarioReport {
                id: s.id.clone(),
                ok: false,
                err: Some(format!("send failed ({}): {e}", st.actor)),
                duration_ms: start.elapsed().as_millis() as u64,
                matched,
                tail,
            };
        }
        if let Some(ms) = st.wait_ms {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
        if let Some(want) = st.wait_for_contains.as_deref() {
            // Wait for a specific output on this actor stream.
            loop {
                if Instant::now() > deadline {
                    return ScenarioReport {
                        id: s.id.clone(),
                        ok: false,
                        err: Some("timeout".to_string()),
                        duration_ms: start.elapsed().as_millis() as u64,
                        matched,
                        tail,
                    };
                }
                if !matched.is_empty() {
                    return ScenarioReport {
                        id: s.id.clone(),
                        ok: true,
                        err: None,
                        duration_ms: start.elapsed().as_millis() as u64,
                        matched,
                        tail,
                    };
                }
                let Some(c) = conns.get_mut(&st.actor) else {
                    break;
                };
                let remain = deadline.saturating_duration_since(Instant::now());
                let step = remain.min(Duration::from_millis(300));
                let got = match tokio::time::timeout(step, c.rx.recv()).await {
                    Ok(v) => v,
                    Err(_) => None,
                };
                let Some(txt) = got else { continue };
                note_output(&st.actor, &txt, &mut tail, &mut matched);
                if txt.contains(want) {
                    break;
                }
            }
        }
        if Instant::now() > deadline {
            return ScenarioReport {
                id: s.id.clone(),
                ok: false,
                err: Some("timeout".to_string()),
                duration_ms: start.elapsed().as_millis() as u64,
                matched,
                tail,
            };
        }
    }

    loop {
        if Instant::now() > deadline {
            return ScenarioReport {
                id: s.id.clone(),
                ok: false,
                err: Some("timeout".to_string()),
                duration_ms: start.elapsed().as_millis() as u64,
                matched,
                tail,
            };
        }

        let mut progressed = false;
        for (actor, c) in conns.iter_mut() {
            match c.rx.try_recv() {
                Ok(txt) => {
                    progressed = true;
                    note_output(actor, &txt, &mut tail, &mut matched);
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {}
            }
        }

        if !matched.is_empty() {
            return ScenarioReport {
                id: s.id.clone(),
                ok: true,
                err: None,
                duration_ms: start.elapsed().as_millis() as u64,
                matched,
                tail,
            };
        }

        if !progressed {
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
}

#[allow(dead_code)]
fn should_xfail(s: &Scenario) -> bool {
    s.expect
        .as_deref()
        .unwrap_or("pass")
        .eq_ignore_ascii_case("xfail")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = parse_args();

    let mut reports = Vec::new();
    let mut suite_ok = true;

    if let Some(path) = cfg.scenario.as_ref() {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let s: Scenario = serde_json::from_str(&raw).with_context(|| "parse scenario")?;
        let rep = run_scenario(&cfg.ws_url, &s).await;
        // For now, "xfail" scenarios are expected to match their `stop_on` signature which
        // represents the expected failure mode (ex: "huh?").
        let passed = rep.ok;
        suite_ok &= passed;
        reports.push(ScenarioReport { ok: passed, ..rep });
    } else if let Some(path) = cfg.suite.as_ref() {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let suite: SuiteFile = serde_json::from_str(&raw).with_context(|| "parse suite")?;

        for sp in suite.scenarios {
            let raw = std::fs::read_to_string(&sp).with_context(|| format!("read {sp}"))?;
            let s: Scenario = serde_json::from_str(&raw).with_context(|| format!("parse {sp}"))?;
            let rep = run_scenario(&cfg.ws_url, &s).await;
            let passed = rep.ok;
            suite_ok &= passed;
            reports.push(ScenarioReport { ok: passed, ..rep });
        }
    }

    let out = SuiteReport {
        ok: suite_ok,
        reports,
    };
    println!("{}", serde_json::to_string_pretty(&out)?);
    if !suite_ok {
        std::process::exit(1);
    }
    Ok(())
}
