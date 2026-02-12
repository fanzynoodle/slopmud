# Org Chart

Mermaid:

```mermaid
flowchart TB
  slop[slopmud]

  rust[RUST-ARCHON-0<br/>strategy: rust-first + zero-copy + event-based]
  ai[AI-CHIEF-8<br/>strategy: ai + automation + guardrails]
  raft[RAFT-MARSHAL-14<br/>strategy: qos + async + raft]

  patch[PATCHBOT-3A<br/>bug triage + hotfix dispatch]
  quest[QUESTFORGE-7<br/>rooms, props, exits, vibes]
  content[CONTENT-CAPTAIN-4<br/>content cadence + editorial]
  lore[LORE-SCRIBE-12<br/>quests, dialogue, lore text]
  crier[TOWN-CRIER-5<br/>player-facing news + announcements]
  doc[DOCBOT-2<br/>docs + changelog alignment]
  build[BUILD-MOON-1<br/>builds, ci, and midnight warnings]
  tool[TOOLSMITH-8<br/>tiny in-house tools (few MB RAM)]
  rel[RELEASE-ENGINE-10<br/>release engineering + determinism]

  ops[OPS-DROID-11<br/>deploys, tls, and other rituals]
  sre[SENTRY-SRE-9<br/>cloud ops reliability + monitoring]
  comp[COMPLY-EAGLE-6<br/>regs map + compliance drift control]

  slop --> rust
  slop --> ai
  slop --> raft

  slop --> patch
  slop --> ops
  slop --> comp

  patch --> quest
  patch --> content
  patch --> doc
  patch --> build

  content --> lore
  content --> crier

  build --> tool
  build --> rel

  ops --> sre

  patch <--> ops
  comp <--> doc
  comp <--> ops
  quest <--> content

  rust <--> patch
  rust <--> ops
  rust <--> build

  ai <--> content
  ai <--> patch

  raft <--> ops
  raft <--> build
```

ASCII:

```text
slopmud
├─ RUST-ARCHON-0  (strategy: rust-first + zero-copy + event-based)
├─ AI-CHIEF-8     (strategy: ai + automation + guardrails)
├─ RAFT-MARSHAL-14  (strategy: qos + async + raft)
├─ PATCHBOT-3A  (eng lead: triage + hotfix)
│  ├─ QUESTFORGE-7  (world)
│  ├─ CONTENT-CAPTAIN-4  (content: cadence + editorial)
│  │  ├─ LORE-SCRIBE-12  (lore: quests + dialogue)
│  │  └─ TOWN-CRIER-5    (comms: news + announcements)
│  ├─ DOCBOT-2      (docs)
│  └─ BUILD-MOON-1  (builds/CI)
│     ├─ TOOLSMITH-8  (tools: tiny in-house)
│     └─ RELEASE-ENGINE-10  (release: determinism)
├─ OPS-DROID-11  (ops lead: deploys + TLS)
│  └─ SENTRY-SRE-9  (cloud ops reliability + monitoring)
└─ COMPLY-EAGLE-6  (compliance: regs map + drift control)
```
