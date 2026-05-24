#![allow(dead_code)]

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use mudproto::session::SessionId;
use mudproto::shard::{RESP_ERR, RESP_OUTPUT, RESP_OUTPUT_BLOB, ShardReq};
use reqwest::StatusCode;
use slopio::frame::{FrameReader, FrameWriter};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tracing::{Level, info, warn};

mod groups;
mod items;
mod protoadventure;
mod raftlog;
mod rooms;
mod rooms_fb;
mod worldlog;

#[derive(Debug, Clone, serde::Deserialize)]
struct AuthBlob {
    #[serde(default)]
    acct: Option<String>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    google_sub: Option<String>,
    #[serde(default)]
    google_email: Option<String>,
    #[serde(default)]
    oidc_sub: Option<String>,
    #[serde(default)]
    oidc_email: Option<String>,
    #[serde(default)]
    caps: Option<Vec<String>>,
}

const OPENAI_API_BASE_DEFAULT: &str = "https://api.openai.com/v1";
const OPENAI_PING_MODEL_DEFAULT: &str = "gpt-4o-mini";

const HUH_HELP: &[u8] = b"huh? (try: help)\r\n";
const HUH_LOOK: &[u8] = b"huh? (try: look)\r\n";
const HUH_GO: &[u8] = b"huh? (try: go <exit>)\r\n";
const HUH_NO_EXIT: &[u8] = b"huh? (no such exit)\r\n";

const SEALED_EXIT_MSG: &[u8] =
    b"the way is sealed. you can feel the world beyond, but gaia is not ready yet.\r\n";

const ROOM_TOWN_GATE: &str = "R_TOWN_GATE_01";
const ROOM_TAVERN: &str = "R_TOWN_TAVERN_01";
const ROOM_TOWN_JOB_BOARD: &str = "R_TOWN_JOB_01";
const ROOM_SCHOOL_ORIENTATION: &str = "R_NS_ORIENT_01";
const ROOM_SCHOOL_FIRST_FIGHT: &str = "R_NS_LABS_03";
const ROOM_CLASS_BARBARIAN: &str = "class_halls.barbarian";
const ROOM_CLASS_BARD: &str = "class_halls.bard";
const ROOM_CLASS_CLERIC: &str = "class_halls.cleric";
const ROOM_CLASS_DRUID: &str = "class_halls.druid";
const ROOM_CLASS_FIGHTER: &str = "class_halls.fighter";
const ROOM_CLASS_MONK: &str = "class_halls.monk";
const ROOM_CLASS_PALADIN: &str = "class_halls.paladin";
const ROOM_CLASS_RANGER: &str = "class_halls.ranger";
const ROOM_CLASS_ROGUE: &str = "class_halls.rogue";
const ROOM_CLASS_SORCERER: &str = "class_halls.sorcerer";
const ROOM_CLASS_WARLOCK: &str = "class_halls.warlock";
const ROOM_CLASS_WIZARD: &str = "class_halls.wizard";

const ITEM_STENCHPOUCH: &str = "stenchpouch";
const ROOM_NEWBIE_HEROES: &str = "R_NS_ORIENT_05";
const ROOM_SEWERS_JUNCTION: &str = "R_SEW_JUNC_01";

const CLASS_HALL_PREFIX: &str = "class_halls.";

const CLASS_HALL_NPCS: &[(&str, &str)] = &[
    (ROOM_CLASS_BARBARIAN, "Krag Stonefury"),
    (ROOM_CLASS_BARBARIAN, "Warchief Una"),
    (ROOM_CLASS_BARBARIAN, "Rok Loud"),
    (ROOM_CLASS_BARBARIAN, "Mira Flint"),
    (ROOM_CLASS_BARD, "Caro Strings"),
    (ROOM_CLASS_BARD, "Maestra Jun"),
    (ROOM_CLASS_BARD, "Piper Vale"),
    (ROOM_CLASS_BARD, "Tess Chronicler"),
    (ROOM_CLASS_CLERIC, "Sister Vell"),
    (ROOM_CLASS_CLERIC, "Canon Hara"),
    (ROOM_CLASS_CLERIC, "Brother Piers"),
    (ROOM_CLASS_DRUID, "Iri Moss"),
    (ROOM_CLASS_DRUID, "Grovecaller Olan"),
    (ROOM_CLASS_DRUID, "Bracken"),
    (ROOM_CLASS_DRUID, "Fern Watcher"),
    (ROOM_CLASS_FIGHTER, "Kera Forgefront"),
    (ROOM_CLASS_FIGHTER, "Captain Rhune"),
    (ROOM_CLASS_FIGHTER, "Sable Recruiter"),
    (ROOM_CLASS_FIGHTER, "Holt Veteran"),
    (ROOM_CLASS_MONK, "Toma Quiethands"),
    (ROOM_CLASS_MONK, "Master Sen"),
    (ROOM_CLASS_MONK, "Ili Swift"),
    (ROOM_CLASS_MONK, "Pema Still"),
    (ROOM_CLASS_PALADIN, "Rhea Sunsteel"),
    (ROOM_CLASS_PALADIN, "Justicar Hal"),
    (ROOM_CLASS_PALADIN, "Lumen Vowkeeper"),
    (ROOM_CLASS_PALADIN, "Alden Oathbound"),
    (ROOM_CLASS_RANGER, "Pine Flint"),
    (ROOM_CLASS_RANGER, "Tracker Mae"),
    (ROOM_CLASS_RANGER, "Jory Scout"),
    (ROOM_CLASS_RANGER, "Kestrel Pathfinder"),
    (ROOM_CLASS_ROGUE, "Lilt Fence"),
    (ROOM_CLASS_ROGUE, "Mistcut"),
    (ROOM_CLASS_ROGUE, "Nix"),
    (ROOM_CLASS_ROGUE, "Echo Glass"),
    (ROOM_CLASS_SORCERER, "Nira Sparkglass"),
    (ROOM_CLASS_SORCERER, "Wildcaster Joss"),
    (ROOM_CLASS_SORCERER, "Fenn Unstable"),
    (ROOM_CLASS_SORCERER, "Risa Flux"),
    (ROOM_CLASS_WARLOCK, "Vesh Cinder"),
    (ROOM_CLASS_WARLOCK, "Pactmaster Lira"),
    (ROOM_CLASS_WARLOCK, "Hask Bound"),
    (ROOM_CLASS_WARLOCK, "Nyla Whisper"),
    (ROOM_CLASS_WIZARD, "Mira Quill"),
    (ROOM_CLASS_WIZARD, "Archmage Sel"),
    (ROOM_CLASS_WIZARD, "Sela Archivist"),
    (ROOM_CLASS_WIZARD, "Orin Scribe"),
];

fn is_class_hall_room(room_id: &str) -> bool {
    room_id.starts_with(CLASS_HALL_PREFIX)
}

fn is_trainer_room(room_id: &str) -> bool {
    room_id == ROOM_SCHOOL_ORIENTATION || is_class_hall_room(room_id)
}

async fn openai_ping_models(
    client: &reqwest::Client,
    base: &str,
    api_key: &str,
) -> anyhow::Result<usize> {
    let url = format!("{}/models", base.trim_end_matches('/'));
    let resp = client.get(url).bearer_auth(api_key).send().await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if status != StatusCode::OK {
        anyhow::bail!("models http={}", status.as_u16());
    }

    let n = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("data")?.as_array().map(|a| a.len()))
        .unwrap_or(0);
    Ok(n)
}

async fn openai_ping_chat(
    client: &reqwest::Client,
    base: &str,
    api_key: &str,
    model: &str,
    prompt: &str,
) -> anyhow::Result<String> {
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    let req = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "max_tokens": 20,
        "temperature": 0.0,
    });

    let resp = client
        .post(url)
        .bearer_auth(api_key)
        .json(&req)
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if status != StatusCode::OK {
        anyhow::bail!("chat http={}", status.as_u16());
    }

    let content = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| {
            v.get("choices")?
                .as_array()?
                .first()?
                .get("message")?
                .get("content")?
                .as_str()
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "(no content)".to_string());

    Ok(content.trim().to_string())
}

struct DrinkItem {
    num: u32,
    name: &'static str,
    aliases: &'static [&'static str],
    min_level: u32,
    cost_gold: u32,
}

fn drink_menu() -> &'static [DrinkItem] {
    &[
        DrinkItem {
            num: 1,
            name: "fruity drink",
            aliases: &["fruity", "fruit", "juice", "1"],
            min_level: 0,
            cost_gold: 1,
        },
        DrinkItem {
            num: 2,
            name: "butter beer",
            aliases: &["butter", "beer", "butterbeer", "2"],
            min_level: 1,
            cost_gold: 2,
        },
        DrinkItem {
            num: 3,
            name: "whiskey",
            aliases: &["whiskey", "whisky", "3"],
            min_level: 2,
            cost_gold: 3,
        },
        DrinkItem {
            num: 4,
            name: "absinthe",
            aliases: &["absinthe", "4"],
            min_level: 3,
            cost_gold: 4,
        },
        DrinkItem {
            num: 5,
            name: "dragonfire rum",
            aliases: &["dragonfire", "rum", "5"],
            min_level: 4,
            cost_gold: 6,
        },
        DrinkItem {
            num: 6,
            name: "void martini",
            aliases: &["void", "martini", "6"],
            min_level: 5,
            cost_gold: 9,
        },
        DrinkItem {
            num: 7,
            name: "reality tonic",
            aliases: &["reality", "tonic", "7"],
            min_level: 6,
            cost_gold: 12,
        },
    ]
}

fn render_tavern_sign() -> String {
    let mut s = String::new();
    s.push_str("a chalkboard sign reads:\r\n");
    s.push_str("  order <num> | order <name>\r\n");
    s.push_str("  (some drinks are too stiff until your drinking level is higher)\r\n");
    s.push_str("\r\n");
    for d in drink_menu() {
        s.push_str(&format!("  {}. {} ({}g)\r\n", d.num, d.name, d.cost_gold));
    }
    s
}

fn render_job_board_for(p: &Character) -> String {
    let mut s = String::new();
    s.push_str("job board:\r\n");
    s.push_str("\r\n");

    let contracts_done = p
        .quest
        .get("q.q2_job_board.contracts_done")
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0)
        .clamp(0, 3);
    let a_done = eval_gate_expr(p, "q.q2_job_board.contract_a");
    let b_done = eval_gate_expr(p, "q.q2_job_board.contract_b");
    let c_done = eval_gate_expr(p, "q.q2_job_board.contract_c");

    let faction = p
        .quest
        .get("q.q2_job_board.faction")
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .unwrap_or("unset");

    s.push_str(&format!("contracts done: {contracts_done}/3\r\n"));
    s.push_str("\r\n");
    s.push_str("contracts (starter set):\r\n");
    s.push_str(&format!(
        "  A) pest sweep (meadowline) [{}]\r\n",
        if a_done { "DONE" } else { "PENDING" }
    ));
    s.push_str("     route: go north (edge gates), then east (meadowline gate)\r\n");
    s.push_str(&format!(
        "  B) drone tag (scrap orchard) [{}]\r\n",
        if b_done { "DONE" } else { "PENDING" }
    ));
    s.push_str("     route: go north (edge gates), then north (scrap orchard gate)\r\n");
    s.push_str(&format!(
        "  C) clinic + gate check (town) [{}]\r\n",
        if c_done { "DONE" } else { "PENDING" }
    ));
    s.push_str("     route: go south (quiet lane)\r\n");
    s.push_str("\r\n");

    if contracts_done >= 3 && faction == "unset" {
        s.push_str("choose a contact (unlocks access):\r\n");
        s.push_str("  faction civic\r\n");
        s.push_str("  faction industrial\r\n");
        s.push_str("  faction green\r\n");
        s.push_str("\r\n");
    } else if faction != "unset" {
        s.push_str(&format!("contact: {faction}\r\n"));
        s.push_str("\r\n");
    }

    s.push_str("notes:\r\n");
    let sewers = if eval_gate_expr(p, "gate.sewers.entry") {
        "open"
    } else {
        "sealed"
    };
    s.push_str(&format!("  - sewers access: {sewers}\r\n"));
    s.push_str("  - if you get lost, go back to the edge gates.\r\n");
    s
}

fn q2_room_enter(p: &mut Character, room_id: &str) -> Option<String> {
    fn truthy(p: &Character, key: &str) -> bool {
        // Keep this consistent with movement gates.
        eval_gate_expr(p, key)
    }

    fn contracts_done(p: &Character) -> i64 {
        p.quest
            .get("q.q2_job_board.contracts_done")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0)
            .clamp(0, 3)
    }

    fn set_contracts_done(p: &mut Character, v: i64) {
        p.quest.insert(
            "q.q2_job_board.contracts_done".to_string(),
            v.clamp(0, 3).to_string(),
        );
    }

    fn set_state_for_done(p: &mut Character, done: i64) {
        let cur = p
            .quest
            .get("q.q2_job_board.state")
            .map(|v| v.trim())
            .unwrap_or("");
        // Don't override later states (choice/repeatables/complete).
        if !cur.is_empty() && !cur.starts_with("contract") && cur != "unstarted" {
            return;
        }
        let st = match done.clamp(0, 3) {
            0 => "unstarted",
            1 => "contract_1",
            2 => "contract_2",
            _ => "contract_3",
        };
        p.quest
            .insert("q.q2_job_board.state".to_string(), st.to_string());
    }

    fn complete_contract(p: &mut Character, contract_key: &str) -> Option<i64> {
        if truthy(p, contract_key) {
            return None;
        }
        p.quest.insert(contract_key.to_string(), "1".to_string());
        let done = (contracts_done(p) + 1).clamp(0, 3);
        set_contracts_done(p, done);
        set_state_for_done(p, done);
        Some(done)
    }

    match room_id {
        "R_MEADOW_PEST_02" => {
            let done = complete_contract(p, "q.q2_job_board.contract_a")?;
            let mut msg = format!("contract A complete. (contracts done: {done}/3)\r\n");
            if done >= 3 && !truthy(p, "q.q2_job_board.repeatables_unlocked") {
                msg.push_str("return to the job board. (try: look board)\r\n");
            }
            Some(msg)
        }
        "R_ORCHARD_NEST_01" => {
            let done = complete_contract(p, "q.q2_job_board.contract_b")?;
            let mut msg = format!("contract B complete. (contracts done: {done}/3)\r\n");
            if done >= 3 && !truthy(p, "q.q2_job_board.repeatables_unlocked") {
                msg.push_str("return to the job board. (try: look board)\r\n");
            }
            Some(msg)
        }
        "R_TOWN_CLINIC_01" => {
            if truthy(p, "q.q2_job_board.contract_c_clinic") {
                return None;
            }
            p.quest.insert(
                "q.q2_job_board.contract_c_clinic".to_string(),
                "1".to_string(),
            );
            Some("clinic: a medic stamps a slip with brisk disinterest.\r\n".to_string())
        }
        "R_TOWN_EDGE_01" => {
            if !truthy(p, "q.q2_job_board.contract_c_clinic") {
                return None;
            }
            let done = complete_contract(p, "q.q2_job_board.contract_c")?;
            let mut msg = format!("contract C complete. (contracts done: {done}/3)\r\n");
            if done >= 3 && !truthy(p, "q.q2_job_board.repeatables_unlocked") {
                msg.push_str("return to the job board. (try: look board)\r\n");
            }
            Some(msg)
        }
        _ => None,
    }
}

fn find_drink(token: &str) -> Option<&'static DrinkItem> {
    let t = token.trim().to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    drink_menu().iter().find(|d| {
        d.name.eq_ignore_ascii_case(&t) || d.aliases.iter().any(|a| a.eq_ignore_ascii_case(&t))
    })
}

fn parse_qty_and_item(s: &str) -> Option<(u32, String)> {
    let parts = s.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() {
        return None;
    }
    if parts.len() == 1 {
        return Some((1, parts[0].to_string()));
    }

    // leading qty
    if let Ok(q) = parts[0].parse::<u32>() {
        let item = parts[1..].join(" ");
        return Some((q.max(1), item));
    }
    // trailing qty
    if let Ok(q) = parts[parts.len() - 1].parse::<u32>() {
        let item = parts[..parts.len() - 1].join(" ");
        return Some((q.max(1), item));
    }
    // trailing xN
    let last = parts[parts.len() - 1];
    if let Some(rest) = last.strip_prefix('x') {
        if let Ok(q) = rest.parse::<u32>() {
            let item = parts[..parts.len() - 1].join(" ");
            return Some((q.max(1), item));
        }
    }

    Some((1, parts.join(" ")))
}

fn parse_tell_args<'a>(line: &'a str, command: &str) -> Option<(&'a str, &'a str)> {
    let rest = command_arg(line, command)?;
    let mut parts = rest.splitn(2, |c: char| c.is_whitespace());
    let who = parts.next()?.trim();
    let msg = parts.next().unwrap_or("").trim();
    if who.is_empty() || msg.is_empty() {
        None
    } else {
        Some((who, msg))
    }
}

fn command_arg<'a>(line: &'a str, command: &str) -> Option<&'a str> {
    let Some(rest) = line.strip_prefix(command) else {
        return None;
    };
    if !rest.starts_with(' ') {
        return None;
    }
    let arg = rest.trim();
    if arg.is_empty() { None } else { Some(arg) }
}

fn shout_payload(line: &str, speaker: &str) -> Option<String> {
    command_arg(line, "shout").map(|msg| format!("{speaker} shouts: {msg}"))
}

fn room_emote_payload(line: &str, speaker: &str, command: &str) -> Option<String> {
    command_arg(line, command).map(|msg| format!("* {speaker} {msg}"))
}

fn room_emote_noarg(speaker: &str, verb: &str) -> String {
    format!("* {speaker} {verb}")
}

fn is_tavern_sellable(token: &str) -> bool {
    let t = token.trim().to_ascii_lowercase();
    t == ITEM_STENCHPOUCH || t == "stench pouch" || t == "pouch"
}

#[derive(Debug)]
enum ItemKeyMatch {
    None,
    One(String),
    Ambiguous(Vec<String>),
}

fn item_name_matches_token(item_name: &str, token: &str) -> bool {
    if let Some(def) = items::find_item_def(item_name) {
        return def.matches_token(token);
    }
    let t = token.trim().to_ascii_lowercase();
    if t.is_empty() {
        return false;
    }
    let name_lc = item_name.trim().to_ascii_lowercase();
    name_lc == t || name_lc.starts_with(&t)
}

fn find_item_key_in_inventory(inv: &HashMap<String, u32>, token: &str) -> ItemKeyMatch {
    let t = token.trim();
    if t.is_empty() {
        return ItemKeyMatch::None;
    }

    let mut matches = inv
        .iter()
        .filter(|(_, n)| **n > 0)
        .filter_map(|(k, _)| item_name_matches_token(k, t).then(|| k.clone()))
        .collect::<Vec<_>>();

    if matches.is_empty() {
        return ItemKeyMatch::None;
    }
    if matches.len() == 1 {
        return ItemKeyMatch::One(matches.remove(0));
    }

    // If there's an exact match among the set, prefer it.
    let exact = matches
        .iter()
        .filter(|k| k.eq_ignore_ascii_case(t))
        .cloned()
        .collect::<Vec<_>>();
    if exact.len() == 1 {
        return ItemKeyMatch::One(exact[0].clone());
    }

    matches.sort();
    matches.dedup();
    ItemKeyMatch::Ambiguous(matches)
}

#[derive(Debug)]
enum SlotMatch {
    None,
    One(items::EquipSlot),
    Ambiguous(Vec<items::EquipSlot>),
}

fn find_equipped_slot_by_token(equip: &Equipment, token: &str) -> SlotMatch {
    let t = token.trim();
    if t.is_empty() {
        return SlotMatch::None;
    }

    let mut matches = Vec::new();
    for &slot in items::EquipSlot::all() {
        let Some(name) = equip.get(slot) else {
            continue;
        };
        if item_name_matches_token(name, t) {
            matches.push(slot);
        }
    }

    match matches.len() {
        0 => SlotMatch::None,
        1 => SlotMatch::One(matches[0]),
        _ => SlotMatch::Ambiguous(matches),
    }
}

fn render_inventory(c: &Character) -> String {
    let mut s = String::new();
    s.push_str("inventory:\r\n");
    s.push_str(&format!(" - gold: {}g\r\n", c.gold));

    let mut items = c
        .inv
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(k, n)| (k.as_str(), *n))
        .collect::<Vec<_>>();
    items.sort_by(|a, b| a.0.cmp(b.0));

    if items.is_empty() {
        s.push_str(" - (empty)\r\n");
        return s;
    }
    for (k, n) in items {
        s.push_str(&format!(" - {} x{}\r\n", k, n));
    }
    s
}

fn equipped_armor_value(c: &Character) -> i32 {
    let mut av = 0i32;
    for &slot in items::EquipSlot::all() {
        let Some(name) = c.equip.get(slot) else {
            continue;
        };
        let Some(def) = items::find_item_def(name) else {
            continue;
        };
        if let items::ItemKind::Armor(a) = def.kind {
            av = av.saturating_add(a.armor_value);
        }
    }
    av
}

fn equipped_weapon_damage_range(c: &Character) -> Option<(i32, i32)> {
    let name = c.equip.get(items::EquipSlot::Wield)?;
    let def = items::find_item_def(name)?;
    match def.kind {
        items::ItemKind::Weapon(w) => Some((w.dmg_min, w.dmg_max)),
        _ => None,
    }
}

fn compute_ac(c: &Character) -> i32 {
    // Simple, readable defense score for now:
    // - 10 baseline
    // - DEX modifier (5e-style)
    // - total equipped armor value (piecewise)
    10 + c.stats.mod_for(Ability::Dex) + equipped_armor_value(c)
}

fn render_equipment(c: &Character) -> String {
    let mut s = String::new();
    s.push_str("equipment:\r\n");
    for &slot in items::EquipSlot::all() {
        let label = slot.as_str();
        let Some(name) = c.equip.get(slot) else {
            s.push_str(&format!(" - {label}: (empty)\r\n"));
            continue;
        };
        if let Some(def) = items::find_item_def(name) {
            match def.kind {
                items::ItemKind::Weapon(w) => {
                    let size = def.size.map(|z| z.as_str()).unwrap_or("-");
                    s.push_str(&format!(
                        " - {label}: {} [weapon dmg={}..{} size={}]\r\n",
                        def.name, w.dmg_min, w.dmg_max, size
                    ));
                }
                items::ItemKind::Armor(a) => {
                    let size = def.size.map(|z| z.as_str()).unwrap_or("-");
                    s.push_str(&format!(
                        " - {label}: {} [armor class={} av={} size={}]\r\n",
                        def.name,
                        a.class.as_str(),
                        a.armor_value,
                        size
                    ));
                }
                items::ItemKind::Accessory(a) => {
                    s.push_str(&format!(
                        " - {label}: {} [accessory slot={}]\r\n",
                        def.name,
                        a.slot.as_str()
                    ));
                }
                items::ItemKind::Consumable(_) | items::ItemKind::Misc => {
                    s.push_str(&format!(" - {label}: {}\r\n", def.name));
                }
            }
        } else {
            s.push_str(&format!(" - {label}: {name}\r\n"));
        }
    }
    let av = equipped_armor_value(c);
    let ac = compute_ac(c);
    s.push_str(&format!(
        "totals:\r\n - armor value: {av}\r\n - ac: {ac}\r\n"
    ));
    s
}

fn render_item_details(def: &items::ItemDef) -> String {
    let mut s = String::new();
    s.push_str(def.name);
    s.push_str("\r\n");

    let size = def.size.map(|z| z.as_str()).unwrap_or("-");
    s.push_str(&format!("size: {size}\r\n"));

    match def.kind {
        items::ItemKind::Weapon(w) => {
            s.push_str("type: weapon\r\n");
            s.push_str(&format!("dmg: {}..{}\r\n", w.dmg_min, w.dmg_max));
        }
        items::ItemKind::Armor(a) => {
            s.push_str("type: armor\r\n");
            s.push_str(&format!("slot: {}\r\n", a.slot.as_str()));
            s.push_str(&format!("armor class: {}\r\n", a.class.as_str()));
            s.push_str(&format!("armor value: {}\r\n", a.armor_value));
        }
        items::ItemKind::Accessory(a) => {
            s.push_str("type: accessory\r\n");
            s.push_str(&format!("slot: {}\r\n", a.slot.as_str()));
        }
        items::ItemKind::Consumable(c) => {
            s.push_str("type: consumable\r\n");
            s.push_str(&format!("heal: {}\r\n", c.heal.max(0)));
        }
        items::ItemKind::Misc => {
            s.push_str("type: misc\r\n");
        }
    }

    if !def.description.trim().is_empty() {
        s.push_str("\r\n");
        // The descriptions are stored with '\n'. Normalize to CRLF on output.
        s.push_str(&def.description.replace('\n', "\r\n"));
        if !s.ends_with("\r\n") {
            s.push_str("\r\n");
        }
    }

    s
}

fn render_party_status(world: &World, cid: CharacterId) -> String {
    let mut s = String::new();
    let Some(c) = world.chars.get(&cid) else {
        s.push_str("party: (missing character)\r\n");
        return s;
    };

    let Some(pid) = world.party_of.get(&cid).copied() else {
        s.push_str("party: none\r\n");
        s.push_str("try: party create\r\n");
        return s;
    };

    let Some(p) = world.parties.get(&pid) else {
        s.push_str("party: (stale)\r\n");
        return s;
    };

    s.push_str(&format!("party {}:\r\n", p.id));
    for mid in p.members.iter().copied().collect::<Vec<_>>() {
        let Some(m) = world.chars.get(&mid) else {
            continue;
        };
        let lead = if p.leader == mid { " (leader)" } else { "" };
        let here = if m.room_id == c.room_id {
            ""
        } else {
            " (away)"
        };
        let aa = if m.autoassist { "" } else { " (noassist)" };
        let fol = if m.follow_leader { " (follow)" } else { "" };
        s.push_str(&format!(" - {}{lead}{here}{aa}{fol}\r\n", m.name));
    }
    s
}

fn fmt_session_id(sid: SessionId) -> String {
    format!("{:032x}", sid.0)
}

fn sessions_attached_to_character(world: &World, cid: CharacterId) -> Vec<SessionId> {
    let mut out = Vec::new();
    for (sid, ss) in &world.sessions {
        if ss.controlled.iter().any(|x| *x == cid) {
            out.push(*sid);
        }
    }
    out.sort_by_key(|sid| sid.0);
    out
}

fn render_sessions_cmd(world: &World, session: SessionId) -> String {
    let mut s = String::new();
    let Some(active_cid) = world.active_char_id(session) else {
        s.push_str("sessions: not attached\r\n");
        return s;
    };
    let Some(active) = world.chars.get(&active_cid) else {
        s.push_str("sessions: (missing active character)\r\n");
        return s;
    };

    s.push_str("sessions:\r\n");
    s.push_str(&format!(
        " - your session: {} (short={})\r\n",
        fmt_session_id(session),
        session.short()
    ));
    s.push_str(&format!(
        " - active character: {} ({})\r\n",
        active.name, active.id
    ));

    let attached = sessions_attached_to_character(world, active.id);
    s.push_str(" - sessions attached to this character:\r\n");
    for sid in attached {
        let you = if sid == session { " you" } else { "" };
        let creator = if active.created_by == Some(sid) {
            " creator"
        } else {
            ""
        };
        s.push_str(&format!(
            "   - {} (short={}{}{})\r\n",
            fmt_session_id(sid),
            sid.short(),
            you,
            creator
        ));
    }

    s.push_str(" - characters controlled by your session:\r\n");
    let Some(ss) = world.sessions.get(&session) else {
        s.push_str("   - (none)\r\n");
        return s;
    };
    for cid in &ss.controlled {
        let (nm, room) = match world.chars.get(cid) {
            Some(c) => (c.name.as_str(), c.room_id.as_str()),
            None => ("(missing)", "-"),
        };
        let a = if *cid == ss.active { " active" } else { "" };
        s.push_str(&format!("   - {cid} {nm} [{room}]{a}\r\n"));
    }
    s.push_str("usage:\r\n");
    s.push_str(" - sessions\r\n");
    s.push_str(" - sessions drop <character_id>\r\n");
    s
}

fn tavern_object(room_id: &str, target: &str) -> Option<String> {
    if room_id != ROOM_TAVERN {
        return None;
    }
    let t = target.trim().to_ascii_lowercase();
    match t.as_str() {
        "sign" | "menu" | "chalkboard" | "board" => Some(render_tavern_sign()),
        "stew" | "pot" | "perpetual stew" => Some(
            "a big pot of perpetual stew.\r\nit smells like yesterday and tomorrow.\r\n"
                .to_string(),
        ),
        "bar" | "counter" => Some(
            "the bar is scarred wood polished smooth.\r\nit has seen spills it will never forgive.\r\n"
                .to_string(),
        ),
        _ => None,
    }
}

fn job_board_object(p: &Character, target: &str) -> Option<String> {
    if p.room_id != ROOM_TOWN_JOB_BOARD {
        return None;
    }
    let t = target.trim().to_ascii_lowercase();
    match t.as_str() {
        "board" | "job" | "jobs" | "contract" | "contracts" | "paper" | "papers" | "notices" => {
            Some(render_job_board_for(p))
        }
        _ => None,
    }
}

fn sewers_object(p: &Character, target: &str) -> Option<String> {
    if p.room_id != ROOM_SEWERS_JUNCTION {
        return None;
    }

    let t = target.trim().to_ascii_lowercase();
    match t.as_str() {
        "chalk" | "marks" | "board" | "sign" | "signs" => Some(render_sewers_junction_chalk(p)),
        _ => None,
    }
}

fn render_sewers_junction_chalk(p: &Character) -> String {
    let mut s = String::new();
    let valves_opened = p
        .quest
        .get("q.q3_sewer_valves.valves_opened")
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0)
        .clamp(0, 3);
    let boss = if valves_opened >= 3 {
        "unsealed"
    } else {
        "sealed"
    };
    let quarry = if eval_gate_expr(p, "gate.sewers.shortcut_to_quarry") {
        "unsealed"
    } else {
        "sealed"
    };

    s.push_str("chalk marks on the wall:\r\n");
    s.push_str("\r\n");
    s.push_str(&format!("  valves opened: {valves_opened}/3\r\n"));
    s.push_str(&format!("  boss slope: {boss}\r\n"));
    s.push_str(&format!("  quarry bypass: {quarry}\r\n"));
    s.push_str("\r\n");
    s.push_str("routes:\r\n");
    s.push_str("  go valves | go wing | go entry\r\n");
    s
}

fn heroes_object(room_id: &str, target: &str) -> Option<String> {
    if room_id != ROOM_NEWBIE_HEROES {
        return None;
    }

    let t = target.trim().to_ascii_lowercase();
    match t.as_str() {
        "hall" | "heroes" | "hero" | "statue" | "statues" | "sentinel" | "sentinels"
        | "hatchet" | "javelin" | "aradune" => Some(render_hall_of_heroes()),
        _ => None,
    }
}

fn render_hall_of_heroes() -> String {
    let mut s = String::new();
    s.push_str("hall of heroes:\r\n");
    s.push_str("\r\n");
    s.push_str("two statues watch the door:\r\n");
    s.push_str("  hatchet: a chipped hatchet. takes your bugs personally.\r\n");
    s.push_str("  javelin: a javelin pointed outward. welcomes you anyway.\r\n");
    s.push_str("\r\n");
    s.push_str("names etched in the stone:\r\n");
    s.push_str("  mud1: roy trubshaw, richard bartle\r\n");
    s.push_str("  dikumud: katja nyboe, tom madsen, michael seifert, hans henrik staerfeldt\r\n");
    s.push_str("  lpmud: lars pensjo\r\n");
    s.push_str("  tiny: jim aspnes\r\n");
    s.push_str("  circle: jeremy elson\r\n");
    s.push_str("  everquest: Aradune\r\n");
    s
}

fn academy_object(room_id: &str, target: &str) -> Option<String> {
    if room_id != ROOM_SCHOOL_ORIENTATION {
        return None;
    }
    let t = target.trim().to_ascii_lowercase();
    match t.as_str() {
        "trainer" | "instructor" | "coach" => Some(
            "a tired instructor with kind eyes.\r\ntry: class <name>, train <skill>, help\r\n"
                .to_string(),
        ),
        "placard" | "sign" | "board" => Some(
            "a training placard reads:\r\n - go east to start orientation (badge + drills)\r\n - follow the drills to dorms and the combat labs\r\n - after the sim yard, head north toward the town gate\r\n"
                .to_string(),
        ),
        _ => None,
    }
}

const COC_LINE_ITEMS: [&str; 8] = [
    "1. nothing illegal",
    "2. hard R for violence, hard PG for sex/nudity",
    "3. no soliciting",
    "4. anything you submit - consider it publicly licensed and publicly published",
    "5. don't spam",
    "6. prioritize great experiences for humans",
    "7. don't lie about being a bot",
    "8. zero privacy: we will share logs with various folks and train our models on them",
];

fn usage_and_exit() -> ! {
    eprintln!(concat!(
        "shard_01\n\n\
USAGE:\n  shard_01 [--bind HOST:PORT]\n\n\
ENV:\n  SHARD_BIND                  default 127.0.0.1:5000\n  WORLD_SEED                  default 1 (deterministic; replace with raft time/seed later)\n  WORLD_TICK_MS               default 1000\n  WORLD_TIME_SCALE_PPM        default 1000000 (1000000 = real time, 500000 = half speed)\n  BARTENDER_EMOTE_MS          default 30000\n  MOB_WANDER_MS               default 15000\n  SHARD_RAFT_LOG              default var/shard_01_raft.jsonl\n  SHARD_RAFT_NODE_ID          default NODE_ID or SHARD_BIND\n  SHARD_RAFT_BIND             optional raft RPC bind address\n  SHARD_RAFT_PEERS            optional comma-separated node@host:port peers\n  SHARD_RAFT_ELECTION_MS      optional; default 450+jitter\n  SHARD_RAFT_HEARTBEAT_MS     optional; default 120\n  SHARD_RAFT_APPLICATION_MAX_FORMAT optional; default current binary max\n  SHARD_BOOTSTRAP_ADMINS      comma-separated acct names added to admin group (genesis only)\n  SHARD_BOOTSTRAP_ADMIN_SSO   comma-separated principals added to admin group (genesis only)\n                             ex: google_email:rob@caskey.org,google_sub:123,acct:rob\n",
        "  SLOPMUD_WAL_BACKUP_ENABLED optional; enables raw WAL extent backups via child walbackupd\n  SLOPMUD_WAL_BACKUP_DIR     local WAL backup root; default SHARD_RAFT_LOG.walbackup when enabled\n  SLOPMUD_WAL_BACKUP_INTERVAL_S default 60\n  SLOPMUD_WAL_BACKUP_MAX_SEGMENT_BYTES default 536870912\n  SLOPMUD_WAL_BACKUP_S3_BUCKET optional S3 bucket for WAL backup extents/manifests\n  SLOPMUD_WAL_BACKUP_S3_PREFIX default slopmud/wal-backups\n  SLOPMUD_WALBACKUPD_BIN optional; default sibling slopmud_walbackupd\n",
        "  SLOPMUD_WAL_RESTORE_ENABLED optional; restores SHARD_RAFT_LOG before startup if missing/empty\n  SLOPMUD_WAL_RESTORE_DIR    local WAL backup root for restore\n  SLOPMUD_WAL_RESTORE_S3_URI or SLOPMUD_WAL_RESTORE_S3_BUCKET/SLOPMUD_WAL_RESTORE_S3_PREFIX\n  SLOPMUD_WAL_RESTORE_CACHE_DIR optional cache for S3 restore extents\n  SLOPMUD_WAL_RESTORE_OVERWRITE optional; replace existing log when true\n"
    ));
    std::process::exit(2);
}

#[derive(Clone, Debug)]
struct Config {
    bind: SocketAddr,
    world_seed: u64,
    tick_ms: u64,
    time_scale_ppm: u64,
    bartender_emote_ms: u64,
    mob_wander_ms: u64,
    raft_log_path: PathBuf,
    raft_consensus: raftlog::ConsensusConfig,
    wal_restore: Option<slopmud_walbackup::WalRestoreConfig>,
    wal_backup: Option<slopmud_walbackup::WalBackupConfig>,
    bootstrap_admins: Vec<String>,
    bootstrap_admin_sso: Vec<String>,
}

fn parse_args() -> Config {
    let mut bind: SocketAddr = std::env::var("SHARD_BIND")
        .unwrap_or_else(|_| "127.0.0.1:5000".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let world_seed: u64 = std::env::var("WORLD_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let tick_ms: u64 = std::env::var("WORLD_TICK_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000)
        .max(10);
    let time_scale_ppm: u64 = std::env::var("WORLD_TIME_SCALE_PPM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000);
    let bartender_emote_ms: u64 = std::env::var("BARTENDER_EMOTE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30000)
        .max(tick_ms);
    let mob_wander_ms: u64 = std::env::var("MOB_WANDER_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15000)
        .max(tick_ms);

    let raft_log_path: PathBuf = std::env::var("SHARD_RAFT_LOG")
        .unwrap_or_else(|_| "var/shard_01_raft.jsonl".to_string())
        .into();
    let raft_node_id = std::env::var("SHARD_RAFT_NODE_ID")
        .ok()
        .or_else(|| std::env::var("NODE_ID").ok())
        .unwrap_or_else(|| format!("shard-{bind}"));
    let raft_bind = std::env::var("SHARD_RAFT_BIND")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.parse().unwrap_or_else(|_| usage_and_exit()));
    let raft_peers = parse_raft_peers_env();
    let election_default = 450 + stable_node_jitter_ms(&raft_node_id, 200);
    let raft_election_timeout_ms = std::env::var("SHARD_RAFT_ELECTION_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(election_default)
        .max(150);
    let raft_heartbeat_ms = std::env::var("SHARD_RAFT_HEARTBEAT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120)
        .max(25);
    let raft_application_max_format = std::env::var("SHARD_RAFT_APPLICATION_MAX_FORMAT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(worldlog::WORLD_LOG_MAX_SUPPORTED_FORMAT)
        .clamp(1, worldlog::WORLD_LOG_MAX_SUPPORTED_FORMAT);
    let raft_consensus = raftlog::ConsensusConfig {
        node_id: raft_node_id,
        bind: raft_bind,
        peers: raft_peers,
        election_timeout_ms: raft_election_timeout_ms,
        heartbeat_ms: raft_heartbeat_ms,
        application_max_format: raft_application_max_format,
    };
    let wal_backup = slopmud_walbackup::WalBackupConfig::from_env(
        raft_log_path.clone(),
        raft_consensus.node_id.clone(),
    )
    .unwrap_or_else(|e| {
        eprintln!("bad WAL backup config: {e}");
        std::process::exit(2);
    });
    let wal_restore = slopmud_walbackup::WalRestoreConfig::from_env(raft_log_path.clone())
        .unwrap_or_else(|e| {
            eprintln!("bad WAL restore config: {e}");
            std::process::exit(2);
        });
    let bootstrap_admins: Vec<String> = std::env::var("SHARD_BOOTSTRAP_ADMINS")
        .ok()
        .map(|v| {
            v.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let bootstrap_admin_sso: Vec<String> = std::env::var("SHARD_BOOTSTRAP_ADMIN_SSO")
        .ok()
        .map(|v| {
            v.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bind" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                bind = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    Config {
        bind,
        world_seed,
        tick_ms,
        time_scale_ppm,
        bartender_emote_ms,
        mob_wander_ms,
        raft_log_path,
        raft_consensus,
        wal_restore,
        wal_backup,
        bootstrap_admins,
        bootstrap_admin_sso,
    }
}

fn parse_raft_peers_env() -> Vec<raftlog::ConsensusPeer> {
    let Some(raw) = std::env::var("SHARD_RAFT_PEERS")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (node_id, addr_s) = part
            .split_once('@')
            .map(|(id, addr)| (id.trim().to_string(), addr.trim()))
            .unwrap_or_else(|| (part.to_string(), part));
        if !addr_s.contains(':') {
            usage_and_exit();
        }
        out.push(raftlog::ConsensusPeer {
            node_id,
            addr: addr_s.to_string(),
        });
    }
    out
}

fn stable_node_jitter_ms(node_id: &str, modulo: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    node_id.hash(&mut h);
    h.finish() % modulo.max(1)
}

fn principal_from_attach(name: &str, auth: Option<&[u8]>) -> String {
    // `auth` is asserted by the broker. This shard does not validate it; it is used only for
    // authorization decisions (capabilities/groups).
    if let Some(raw) = auth {
        if let Ok(s) = std::str::from_utf8(raw) {
            if let Ok(a) = serde_json::from_str::<AuthBlob>(s) {
                if let Some(sub) = a.google_sub.as_deref() {
                    let sub = sub.trim();
                    if !sub.is_empty() {
                        return format!("google_sub:{sub}");
                    }
                }
                if let Some(sub) = a.oidc_sub.as_deref() {
                    let sub = sub.trim();
                    if !sub.is_empty() {
                        return format!("oidc_sub:{sub}");
                    }
                }
                if let Some(email) = a.google_email.as_deref() {
                    let email = email.trim().to_ascii_lowercase();
                    if !email.is_empty() {
                        return format!("google_email:{email}");
                    }
                }
                if let Some(email) = a.oidc_email.as_deref() {
                    let email = email.trim().to_ascii_lowercase();
                    if !email.is_empty() {
                        return format!("oidc_email:{email}");
                    }
                }
                if let Some(acct) = a.acct.as_deref() {
                    let acct = acct.trim().to_ascii_lowercase();
                    if !acct.is_empty() {
                        return format!("acct:{acct}");
                    }
                }
                if let Some(method) = a.method.as_deref() {
                    let method = method.trim().to_ascii_lowercase();
                    if !method.is_empty() {
                        return format!("method:{method}");
                    }
                }
            }
        }
    }
    format!("acct:{}", name.trim().to_ascii_lowercase())
}

fn caps_from_attach(auth: Option<&[u8]>) -> HashSet<groups::Capability> {
    // `auth` is asserted by the broker; treat these caps as additional effective capabilities for
    // the session's principal. Unknown caps are ignored.
    let mut out = HashSet::new();
    let Some(raw) = auth else {
        return out;
    };
    let Ok(s) = std::str::from_utf8(raw) else {
        return out;
    };
    let Ok(a) = serde_json::from_str::<AuthBlob>(s) else {
        return out;
    };
    let Some(caps) = a.caps else {
        return out;
    };
    for c in caps {
        if let Some(cap) = groups::Capability::parse(&c) {
            out.insert(cap);
        }
    }
    out
}

fn normalize_principal_token(tok: &str) -> String {
    let t = tok.trim();
    if t.is_empty() {
        return "acct:".to_string();
    }
    if t.contains(':') {
        return t.to_ascii_lowercase();
    }
    format!("acct:{}", t.to_ascii_lowercase())
}

type CharacterId = u64;
type PartyId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ClientCommandKey {
    session: SessionId,
    command_id: u64,
}

#[derive(Debug, Clone)]
struct VersionedDict<K, V> {
    known: HashMap<K, V>,
    unknown: BTreeMap<String, V>,
}

trait VersionedDictKey: Copy + Eq + std::hash::Hash {
    fn parse_persisted_key(token: &str) -> Option<Self>;
    fn persisted_key(self) -> &'static str;
}

impl VersionedDictKey for items::EquipSlot {
    fn parse_persisted_key(token: &str) -> Option<Self> {
        items::EquipSlot::parse(token)
    }

    fn persisted_key(self) -> &'static str {
        self.as_str()
    }
}

impl<K, V> VersionedDict<K, V>
where
    K: VersionedDictKey,
    V: Clone,
{
    fn new() -> Self {
        Self {
            known: HashMap::new(),
            unknown: BTreeMap::new(),
        }
    }

    fn from_snapshot(snapshot: &BTreeMap<String, V>) -> Self {
        let mut out = Self::new();
        for (key, value) in snapshot {
            if let Some(known) = K::parse_persisted_key(key) {
                out.known.insert(known, value.clone());
            } else if !key.trim().is_empty() {
                out.unknown.insert(key.clone(), value.clone());
            }
        }
        out
    }

    fn get_known(&self, key: K) -> Option<&V> {
        self.known.get(&key)
    }

    fn set_known(&mut self, key: K, value: V) {
        self.known.insert(key, value);
    }

    fn clear_known(&mut self, key: K) -> Option<V> {
        self.known.remove(&key)
    }

    fn known_entries(&self) -> impl Iterator<Item = (K, &V)> {
        self.known.iter().map(|(key, value)| (*key, value))
    }

    fn unknown_entries(&self) -> impl Iterator<Item = (&str, &V)> {
        self.unknown
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    fn snapshot_map(&self) -> BTreeMap<String, V> {
        let mut out = self.unknown.clone();
        for (key, value) in &self.known {
            out.insert(key.persisted_key().to_string(), value.clone());
        }
        out
    }
}

#[derive(Debug, Clone)]
struct Equipment {
    entries: VersionedDict<items::EquipSlot, String>,
}

impl Equipment {
    fn new() -> Self {
        Self {
            entries: VersionedDict::new(),
        }
    }

    fn from_snapshot(snapshot: &BTreeMap<String, String>) -> Self {
        Self {
            entries: VersionedDict::from_snapshot(snapshot),
        }
    }

    fn get(&self, slot: items::EquipSlot) -> Option<&String> {
        self.entries.get_known(slot)
    }

    fn set(&mut self, slot: items::EquipSlot, item: String) {
        self.entries.set_known(slot, item);
    }

    fn clear(&mut self, slot: items::EquipSlot) -> Option<String> {
        self.entries.clear_known(slot)
    }

    fn known_entries(&self) -> impl Iterator<Item = (items::EquipSlot, &String)> {
        self.entries.known_entries()
    }

    fn unknown_entries(&self) -> impl Iterator<Item = (&str, &String)> {
        self.entries.unknown_entries()
    }

    fn snapshot_map(&self) -> BTreeMap<String, String> {
        self.entries.snapshot_map()
    }
}

#[derive(Debug, Clone)]
struct Character {
    id: CharacterId,
    controller: Option<SessionId>,
    created_by: Option<SessionId>, // session that originally spawned this character (detach permissions)
    name: String,
    // Stable principal asserted by the broker (e.g. acct:alice, google_sub:..., google_email:...).
    // Used for permissions (groups/capabilities). Not displayed to players.
    principal: String,
    // Additional capabilities asserted by the broker (typically from SSO/OIDC userinfo).
    auth_caps: HashSet<groups::Capability>,
    is_bot: bool,
    bot_ever: bool, // sticky flag (silent): has this character ever been in bot mode?
    bot_ever_since_ms: Option<u64>, // world ms when bot_ever first became true
    bot_mode_changed_ms: u64, // world ms when is_bot last changed
    friends: HashSet<String>, // friend character names (case-insensitive comparisons)
    room_id: String,
    autoassist: bool,
    follow_leader: bool,
    drink_level: u32,
    gold: u32,
    inv: HashMap<String, u32>,
    quest: HashMap<String, String>, // quest/gate keys (dev-only; not persisted yet)
    class: Option<Class>,
    level: u32,
    xp: u32,
    skill_points: u32,
    skills: HashMap<String, u32>,      // skill name -> rank
    skill_cd_ms: HashMap<String, u64>, // skill name -> next ready time (world ms)
    race: Option<Race>,
    sex: Sex,
    pronouns: PronounKey,
    stats: AbilityScores,
    hp: i32,
    max_hp: i32,
    mana: i32,
    max_mana: i32,
    stamina: i32,
    max_stamina: i32,
    last_mana_regen_ms: u64,
    last_stamina_regen_ms: u64,
    // PvP opt-in (off by default). Only effective in designated PvP rooms.
    pvp_enabled: bool,
    // Simple hard CC. While stunned, the character cannot autoattack or use skills.
    stunned_until_ms: u64,
    combat: CombatState,
    equip: Equipment,
}

#[derive(Debug, Clone)]
struct Party {
    id: PartyId,
    leader: CharacterId,
    members: HashSet<CharacterId>,
}

#[derive(Debug, Clone)]
struct PartyInvite {
    party_id: PartyId,
    inviter: CharacterId,
    // Soft expiry (best-effort).
    expires_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingConfirm {
    BotOn { cid: CharacterId },
}

#[derive(Debug, Clone)]
struct SessionState {
    controlled: Vec<CharacterId>,
    active: CharacterId,
    pending_confirm: Option<PendingConfirm>,
}

#[derive(Debug, Clone)]
enum EventKind {
    RoomMsg { room_id: String, msg: String },
    EnsureTavernMob,
    BartenderEmote,
    EnsureFirstFightWorm,
    EnsureClassHallMobs,
    CombatAct { attacker_id: CharacterId },
    BossTelegraph { boss_id: CharacterId },
    BossResolve { boss_id: CharacterId, seq: u64 },
    MobWander { mob_id: CharacterId },
    Tick,
}

#[derive(Debug, Clone)]
struct ScheduledEvent {
    due_ms: u64,
    seq: u64,
    kind: EventKind,
}

impl ScheduledEvent {
    fn to_projection(&self) -> worldlog::ScheduledEventSnapshot {
        let mut out = worldlog::ScheduledEventSnapshot {
            due_ms: self.due_ms,
            seq: self.seq,
            kind: String::new(),
            room_id: None,
            msg: None,
            character_id: None,
            boss_id: None,
            party_id: None,
            cast_seq: None,
        };

        match &self.kind {
            EventKind::RoomMsg { room_id, msg } => {
                out.kind = "RoomMsg".to_string();
                out.room_id = Some(room_id.clone());
                out.msg = Some(msg.clone());
            }
            EventKind::EnsureTavernMob => out.kind = "EnsureTavernMob".to_string(),
            EventKind::BartenderEmote => out.kind = "BartenderEmote".to_string(),
            EventKind::EnsureFirstFightWorm => out.kind = "EnsureFirstFightWorm".to_string(),
            EventKind::EnsureClassHallMobs => out.kind = "EnsureClassHallMobs".to_string(),
            EventKind::CombatAct { attacker_id } => {
                out.kind = "CombatAct".to_string();
                out.character_id = Some(*attacker_id);
            }
            EventKind::BossTelegraph { boss_id } => {
                out.kind = "BossTelegraph".to_string();
                out.boss_id = Some(*boss_id);
            }
            EventKind::BossResolve { boss_id, seq } => {
                out.kind = "BossResolve".to_string();
                out.boss_id = Some(*boss_id);
                out.cast_seq = Some(*seq);
            }
            EventKind::MobWander { mob_id } => {
                out.kind = "MobWander".to_string();
                out.character_id = Some(*mob_id);
            }
            EventKind::Tick => out.kind = "Tick".to_string(),
        }
        out
    }

    fn from_projection(p: &worldlog::ScheduledEventSnapshot) -> Option<Self> {
        let kind = match p.kind.as_str() {
            "RoomMsg" => EventKind::RoomMsg {
                room_id: p.room_id.clone()?,
                msg: p.msg.clone().unwrap_or_default(),
            },
            "EnsureTavernMob" => EventKind::EnsureTavernMob,
            "BartenderEmote" => EventKind::BartenderEmote,
            "EnsureFirstFightWorm" => EventKind::EnsureFirstFightWorm,
            "EnsureClassHallMobs" => EventKind::EnsureClassHallMobs,
            "CombatAct" => EventKind::CombatAct {
                attacker_id: p.character_id?,
            },
            "BossTelegraph" => EventKind::BossTelegraph {
                boss_id: p.boss_id?,
            },
            "BossResolve" => EventKind::BossResolve {
                boss_id: p.boss_id?,
                seq: p.cast_seq?,
            },
            "MobWander" => EventKind::MobWander {
                mob_id: p.character_id?,
            },
            "Tick" => EventKind::Tick,
            _ => return None,
        };
        Some(Self {
            due_ms: p.due_ms,
            seq: p.seq,
            kind,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct BossState {
    casting_until_ms: u64,
    seq: u64,
}

impl PartialEq for ScheduledEvent {
    fn eq(&self, other: &Self) -> bool {
        self.due_ms == other.due_ms && self.seq == other.seq
    }
}
impl Eq for ScheduledEvent {}
impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ScheduledEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.due_ms.cmp(&other.due_ms) {
            Ordering::Equal => self.seq.cmp(&other.seq),
            o => o,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Race {
    Dragonborn,
    Dwarf,
    Elf,
    Gnome,
    Goliath,
    Halfling,
    Human,
    Orc,
    Tiefling,
}

impl Race {
    fn all() -> &'static [Race] {
        &[
            Race::Dragonborn,
            Race::Dwarf,
            Race::Elf,
            Race::Gnome,
            Race::Goliath,
            Race::Halfling,
            Race::Human,
            Race::Orc,
            Race::Tiefling,
        ]
    }

    fn as_str(self) -> &'static str {
        match self {
            Race::Dragonborn => "dragonborn",
            Race::Dwarf => "dwarf",
            Race::Elf => "elf",
            Race::Gnome => "gnome",
            Race::Goliath => "goliath",
            Race::Halfling => "halfling",
            Race::Human => "human",
            Race::Orc => "orc",
            Race::Tiefling => "tiefling",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "dragonborn" => Some(Race::Dragonborn),
            "dwarf" => Some(Race::Dwarf),
            "elf" => Some(Race::Elf),
            "gnome" => Some(Race::Gnome),
            "goliath" => Some(Race::Goliath),
            "halfling" => Some(Race::Halfling),
            "human" => Some(Race::Human),
            "orc" => Some(Race::Orc),
            "tiefling" => Some(Race::Tiefling),
            _ => None,
        }
    }

    fn size(self) -> items::Size {
        match self {
            Race::Gnome | Race::Halfling => items::Size::Small,
            _ => items::Size::Medium,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sex {
    Male,
    Female,
    None,
    Other,
}

impl Sex {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "male" => Some(Sex::Male),
            "female" => Some(Sex::Female),
            "none" => Some(Sex::None),
            "other" => Some(Sex::Other),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Sex::Male => "male",
            Sex::Female => "female",
            Sex::None => "none",
            Sex::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PronounKey {
    He,
    She,
    They,
}

impl PronounKey {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "he" | "him" => Some(PronounKey::He),
            "she" | "her" => Some(PronounKey::She),
            "they" | "them" => Some(PronounKey::They),
            _ => None,
        }
    }

    fn default_for_sex(sex: Sex) -> Self {
        match sex {
            Sex::Male => PronounKey::He,
            Sex::Female => PronounKey::She,
            Sex::None | Sex::Other => PronounKey::They,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            PronounKey::He => "he",
            PronounKey::She => "she",
            PronounKey::They => "they",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Barbarian,
    Bard,
    Cleric,
    Druid,
    Fighter,
    Monk,
    Paladin,
    Ranger,
    Rogue,
    Sorcerer,
    Warlock,
    Wizard,
}

impl Class {
    fn all() -> &'static [Class] {
        &[
            Class::Barbarian,
            Class::Bard,
            Class::Cleric,
            Class::Druid,
            Class::Fighter,
            Class::Monk,
            Class::Paladin,
            Class::Ranger,
            Class::Rogue,
            Class::Sorcerer,
            Class::Warlock,
            Class::Wizard,
        ]
    }

    fn as_str(self) -> &'static str {
        match self {
            Class::Barbarian => "barbarian",
            Class::Bard => "bard",
            Class::Cleric => "cleric",
            Class::Druid => "druid",
            Class::Fighter => "fighter",
            Class::Monk => "monk",
            Class::Paladin => "paladin",
            Class::Ranger => "ranger",
            Class::Rogue => "rogue",
            Class::Sorcerer => "sorcerer",
            Class::Warlock => "warlock",
            Class::Wizard => "wizard",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "barbarian" => Some(Class::Barbarian),
            "bard" => Some(Class::Bard),
            "cleric" => Some(Class::Cleric),
            "druid" => Some(Class::Druid),
            "fighter" => Some(Class::Fighter),
            "monk" => Some(Class::Monk),
            "paladin" => Some(Class::Paladin),
            "ranger" => Some(Class::Ranger),
            "rogue" => Some(Class::Rogue),
            "sorcerer" => Some(Class::Sorcerer),
            "warlock" => Some(Class::Warlock),
            "wizard" => Some(Class::Wizard),
            _ => None,
        }
    }

    fn hit_die(self) -> i32 {
        match self {
            Class::Barbarian => 12,
            Class::Fighter | Class::Paladin | Class::Ranger => 10,
            Class::Bard
            | Class::Cleric
            | Class::Druid
            | Class::Monk
            | Class::Rogue
            | Class::Warlock => 8,
            Class::Sorcerer | Class::Wizard => 6,
        }
    }

    fn attack_ability(self) -> Ability {
        match self {
            Class::Monk | Class::Rogue | Class::Ranger => Ability::Dex,
            _ => Ability::Str,
        }
    }

    fn weapon_die(self) -> i32 {
        match self {
            Class::Barbarian | Class::Fighter | Class::Paladin | Class::Ranger => 10,
            Class::Monk | Class::Rogue => 8,
            _ => 6,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum SkillTarget {
    CombatTarget,
    SelfOnly,
}

#[derive(Clone, Copy, Debug)]
enum SkillEffect {
    Damage {
        base: i32,
        per_rank: i32,
        ability: Ability,
    },
    Heal {
        base: i32,
        per_rank: i32,
        ability: Ability,
    },
}

struct SkillDef {
    id: &'static str,
    display: &'static str,
    aliases: &'static [&'static str],
    classes: &'static [Class],
    cooldown_ms: u64,
    cost_mana: i32,
    cost_stamina: i32,
    target: SkillTarget,
    effect: SkillEffect,
    tags: &'static [&'static str],
    description: &'static str,
    flavor: &'static str,
}

static CLS_FIGHTER: [Class; 1] = [Class::Fighter];
static CLS_WIZARD: [Class; 1] = [Class::Wizard];
static CLS_ROGUE: [Class; 1] = [Class::Rogue];
static CLS_CLERIC: [Class; 1] = [Class::Cleric];
static CLS_RANGER: [Class; 1] = [Class::Ranger];
static CLS_PALADIN: [Class; 1] = [Class::Paladin];
static CLS_BARBARIAN: [Class; 1] = [Class::Barbarian];
static CLS_BARD: [Class; 1] = [Class::Bard];
static CLS_DRUID: [Class; 1] = [Class::Druid];
static CLS_WARLOCK: [Class; 1] = [Class::Warlock];
static CLS_SORCERER: [Class; 1] = [Class::Sorcerer];
static CLS_MONK: [Class; 1] = [Class::Monk];

static CLS_WIS_HEALERS: [Class; 3] = [Class::Cleric, Class::Druid, Class::Paladin];

static ALL_SKILLS: [SkillDef; 24] = [
    // Fighter
    SkillDef {
        id: "power_strike",
        display: "Power Strike",
        aliases: &["power", "strike", "powerstrike"],
        classes: &CLS_FIGHTER,
        cooldown_ms: 3000,
        cost_mana: 0,
        cost_stamina: 2,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 5,
            per_rank: 1,
            ability: Ability::Str,
        },
        tags: &["martial", "weapon"],
        description: "A heavy weapon blow that rewards committing to the fight.",
        flavor: "You put your whole weight behind the swing.",
    },
    SkillDef {
        id: "shield_bash",
        display: "Shield Bash",
        aliases: &["bash", "shield"],
        classes: &CLS_FIGHTER,
        cooldown_ms: 4500,
        cost_mana: 0,
        cost_stamina: 3,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 4,
            per_rank: 1,
            ability: Ability::Str,
        },
        tags: &["martial", "control"],
        description: "A blunt slam meant to rattle your target.",
        flavor: "You drive your shield forward like a battering ram.",
    },
    // Wizard
    SkillDef {
        id: "magic_missile",
        display: "Magic Missile",
        aliases: &["missile", "mm", "magic"],
        classes: &CLS_WIZARD,
        cooldown_ms: 2500,
        cost_mana: 3,
        cost_stamina: 0,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 4,
            per_rank: 1,
            ability: Ability::Int,
        },
        tags: &["arcane", "bolt"],
        description: "A reliable arcane dart.",
        flavor: "A bright dart snaps from your fingertips.",
    },
    SkillDef {
        id: "frost_bolt",
        display: "Frost Bolt",
        aliases: &["frost", "ice", "bolt"],
        classes: &CLS_WIZARD,
        cooldown_ms: 3200,
        cost_mana: 4,
        cost_stamina: 0,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 5,
            per_rank: 1,
            ability: Ability::Int,
        },
        tags: &["arcane", "cold"],
        description: "A shard of cold that bites into muscle and breath.",
        flavor: "Cold gathers, then lashes out in a tight line.",
    },
    // Rogue
    SkillDef {
        id: "backstab",
        display: "Backstab",
        aliases: &["stab", "back"],
        classes: &CLS_ROGUE,
        cooldown_ms: 4000,
        cost_mana: 0,
        cost_stamina: 3,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 6,
            per_rank: 1,
            ability: Ability::Dex,
        },
        tags: &["martial", "precision"],
        description: "A precise strike aimed at something vital.",
        flavor: "You slide in low and strike where it hurts.",
    },
    SkillDef {
        id: "dirty_trick",
        display: "Dirty Trick",
        aliases: &["dirt", "trick", "pocket"],
        classes: &CLS_ROGUE,
        cooldown_ms: 3500,
        cost_mana: 0,
        cost_stamina: 2,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 4,
            per_rank: 1,
            ability: Ability::Dex,
        },
        tags: &["martial", "control"],
        description: "Pocket sand, elbows, and opportunism.",
        flavor: "You fight like nobody is watching.",
    },
    // Cleric / Druid / Paladin healing baseline
    SkillDef {
        id: "heal",
        display: "Heal",
        aliases: &["healing", "mend", "restore"],
        classes: &CLS_WIS_HEALERS,
        cooldown_ms: 5000,
        cost_mana: 5,
        cost_stamina: 0,
        target: SkillTarget::SelfOnly,
        effect: SkillEffect::Heal {
            base: 5,
            per_rank: 2,
            ability: Ability::Wis,
        },
        tags: &["holy", "nature"],
        description: "A simple restoration for keeping yourself standing.",
        flavor: "Warmth settles into your bones.",
    },
    // Cleric
    SkillDef {
        id: "smite",
        display: "Smite",
        aliases: &["smite", "holy"],
        classes: &CLS_CLERIC,
        cooldown_ms: 3500,
        cost_mana: 3,
        cost_stamina: 0,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 5,
            per_rank: 1,
            ability: Ability::Wis,
        },
        tags: &["holy", "radiant"],
        description: "A burst of sanctified force.",
        flavor: "A harsh light flares and judges your enemy.",
    },
    SkillDef {
        id: "holy_bolt",
        display: "Holy Bolt",
        aliases: &["bolt", "holybolt"],
        classes: &CLS_CLERIC,
        cooldown_ms: 2800,
        cost_mana: 2,
        cost_stamina: 0,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 4,
            per_rank: 1,
            ability: Ability::Wis,
        },
        tags: &["holy", "bolt"],
        description: "A quick prayer that hits like a thrown stone.",
        flavor: "Your words harden into a brief, bright impact.",
    },
    // Ranger
    SkillDef {
        id: "aimed_shot",
        display: "Aimed Shot",
        aliases: &["aim", "shot"],
        classes: &CLS_RANGER,
        cooldown_ms: 3200,
        cost_mana: 0,
        cost_stamina: 2,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 5,
            per_rank: 1,
            ability: Ability::Dex,
        },
        tags: &["martial", "precision"],
        description: "Take a breath. Pick a spot. Release.",
        flavor: "You line up a clean hit.",
    },
    SkillDef {
        id: "thorn_whip",
        display: "Thorn Whip",
        aliases: &["thorn", "whip"],
        classes: &CLS_RANGER,
        cooldown_ms: 4200,
        cost_mana: 2,
        cost_stamina: 0,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 4,
            per_rank: 1,
            ability: Ability::Wis,
        },
        tags: &["nature"],
        description: "A lash of living vine and spite.",
        flavor: "A thorny line snaps out and bites.",
    },
    // Paladin
    SkillDef {
        id: "divine_smite",
        display: "Divine Smite",
        aliases: &["dsmite", "divine", "smite"],
        classes: &CLS_PALADIN,
        cooldown_ms: 4500,
        cost_mana: 2,
        cost_stamina: 2,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 6,
            per_rank: 1,
            ability: Ability::Str,
        },
        tags: &["holy", "weapon"],
        description: "Spend faith and force together, all at once.",
        flavor: "Your strike lands with a clean, ringing certainty.",
    },
    // Barbarian
    SkillDef {
        id: "rage_strike",
        display: "Rage Strike",
        aliases: &["rage", "rstrike"],
        classes: &CLS_BARBARIAN,
        cooldown_ms: 2800,
        cost_mana: 0,
        cost_stamina: 3,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 6,
            per_rank: 1,
            ability: Ability::Str,
        },
        tags: &["martial"],
        description: "A brutal hit powered by pure refusal.",
        flavor: "You snarl and swing through the pain.",
    },
    // Bard
    SkillDef {
        id: "cutting_words",
        display: "Cutting Words",
        aliases: &["cut", "words", "cw"],
        classes: &CLS_BARD,
        cooldown_ms: 3000,
        cost_mana: 3,
        cost_stamina: 0,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 4,
            per_rank: 1,
            ability: Ability::Cha,
        },
        tags: &["arcane", "psychic"],
        description: "A line so sharp it draws blood anyway.",
        flavor: "Your voice finds the crack in them.",
    },
    // Druid
    SkillDef {
        id: "sun_spark",
        display: "Sun Spark",
        aliases: &["sun", "spark"],
        classes: &CLS_DRUID,
        cooldown_ms: 3000,
        cost_mana: 3,
        cost_stamina: 0,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 5,
            per_rank: 1,
            ability: Ability::Wis,
        },
        tags: &["nature", "radiant"],
        description: "A brief flash of daylight that doesn't ask permission.",
        flavor: "Light answers you, sharp and hot.",
    },
    // Warlock
    SkillDef {
        id: "eldritch_blast",
        display: "Eldritch Blast",
        aliases: &["blast", "eb", "eldritch"],
        classes: &CLS_WARLOCK,
        cooldown_ms: 2600,
        cost_mana: 3,
        cost_stamina: 0,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 5,
            per_rank: 1,
            ability: Ability::Cha,
        },
        tags: &["occult", "force"],
        description: "The patron's answer to your anger.",
        flavor: "Something old speaks through your hand.",
    },
    // Sorcerer
    SkillDef {
        id: "fire_bolt",
        display: "Fire Bolt",
        aliases: &["fire", "bolt", "fb"],
        classes: &CLS_SORCERER,
        cooldown_ms: 2800,
        cost_mana: 3,
        cost_stamina: 0,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 5,
            per_rank: 1,
            ability: Ability::Cha,
        },
        tags: &["arcane", "fire"],
        description: "A small piece of the sun, held badly.",
        flavor: "Heat gathers and snaps forward.",
    },
    // Monk
    SkillDef {
        id: "flurry",
        display: "Flurry",
        aliases: &["flurry", "combo"],
        classes: &CLS_MONK,
        cooldown_ms: 3200,
        cost_mana: 0,
        cost_stamina: 3,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 5,
            per_rank: 1,
            ability: Ability::Dex,
        },
        tags: &["martial"],
        description: "A quick sequence of strikes before they can breathe.",
        flavor: "Your hands move before your thoughts finish forming.",
    },
    // Extra "generic" skills to flesh out compendium a bit.
    SkillDef {
        id: "second_wind",
        display: "Second Wind",
        aliases: &["wind", "second"],
        classes: &CLS_FIGHTER,
        cooldown_ms: 8000,
        cost_mana: 0,
        cost_stamina: 0,
        target: SkillTarget::SelfOnly,
        effect: SkillEffect::Heal {
            base: 4,
            per_rank: 2,
            ability: Ability::Con,
        },
        tags: &["martial", "heal"],
        description: "A practiced breath that buys you another moment.",
        flavor: "You steady yourself and push on.",
    },
    SkillDef {
        id: "shadow_step",
        display: "Shadow Step",
        aliases: &["shadow", "step"],
        classes: &CLS_ROGUE,
        cooldown_ms: 9000,
        cost_mana: 0,
        cost_stamina: 2,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 5,
            per_rank: 1,
            ability: Ability::Dex,
        },
        tags: &["martial", "mobility"],
        description: "You move like the room blinked.",
        flavor: "You appear where they weren't watching.",
    },
    SkillDef {
        id: "arcane_pulse",
        display: "Arcane Pulse",
        aliases: &["pulse", "ap"],
        classes: &CLS_WIZARD,
        cooldown_ms: 7000,
        cost_mana: 6,
        cost_stamina: 0,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 7,
            per_rank: 1,
            ability: Ability::Int,
        },
        tags: &["arcane"],
        description: "A thick wave of pressure that shoves the world back.",
        flavor: "The air snaps like a sheet.",
    },
    SkillDef {
        id: "prayer_of_steel",
        display: "Prayer of Steel",
        aliases: &["steel", "prayer"],
        classes: &CLS_PALADIN,
        cooldown_ms: 7000,
        cost_mana: 4,
        cost_stamina: 0,
        target: SkillTarget::SelfOnly,
        effect: SkillEffect::Heal {
            base: 4,
            per_rank: 2,
            ability: Ability::Wis,
        },
        tags: &["holy", "heal"],
        description: "A short vow that closes wounds by sheer insistence.",
        flavor: "You speak a vow and feel it bind.",
    },
    SkillDef {
        id: "nature_touch",
        display: "Nature's Touch",
        aliases: &["touch", "nature"],
        classes: &CLS_DRUID,
        cooldown_ms: 6500,
        cost_mana: 5,
        cost_stamina: 0,
        target: SkillTarget::SelfOnly,
        effect: SkillEffect::Heal {
            base: 5,
            per_rank: 2,
            ability: Ability::Wis,
        },
        tags: &["nature", "heal"],
        description: "A green breath that pulls you back from the edge.",
        flavor: "You smell rain and leaf-mold. You live.",
    },
    SkillDef {
        id: "hex",
        display: "Hex",
        aliases: &["hex", "curse"],
        classes: &CLS_WARLOCK,
        cooldown_ms: 6000,
        cost_mana: 4,
        cost_stamina: 0,
        target: SkillTarget::CombatTarget,
        effect: SkillEffect::Damage {
            base: 6,
            per_rank: 1,
            ability: Ability::Cha,
        },
        tags: &["occult"],
        description: "A small curse with a long memory.",
        flavor: "You whisper something the world shouldn't hear.",
    },
];

fn skill_has_class(def: &SkillDef, class: Class) -> bool {
    def.classes.iter().any(|c| *c == class)
}

fn find_skill_any(token: &str) -> Option<&'static SkillDef> {
    let t = token.trim().to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    ALL_SKILLS.iter().find(|s| {
        s.id.eq_ignore_ascii_case(&t)
            || s.display.eq_ignore_ascii_case(&t)
            || s.aliases.iter().any(|a| a.eq_ignore_ascii_case(&t))
    })
}

fn find_skill_for_class(class: Class, token: &str) -> Option<&'static SkillDef> {
    let t = token.trim().to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    ALL_SKILLS.iter().find(|s| {
        if !skill_has_class(s, class) {
            return false;
        }
        s.id.eq_ignore_ascii_case(&t)
            || s.display.eq_ignore_ascii_case(&t)
            || s.aliases.iter().any(|a| a.eq_ignore_ascii_case(&t))
    })
}

fn skills_for_class(class: Class) -> Vec<&'static SkillDef> {
    let mut v = ALL_SKILLS
        .iter()
        .filter(|s| skill_has_class(s, class))
        .collect::<Vec<_>>();
    v.sort_by_key(|s| s.id);
    v
}

fn render_skill_compendium() -> String {
    let mut defs = ALL_SKILLS.iter().collect::<Vec<_>>();
    defs.sort_by_key(|d| d.id);

    let mut s = String::new();
    s.push_str("skills compendium:\r\n");
    for d in defs {
        let mut classes = d.classes.iter().map(|c| c.as_str()).collect::<Vec<_>>();
        classes.sort_unstable();
        let class_str = classes.join(", ");
        let cost = match (d.cost_mana, d.cost_stamina) {
            (0, 0) => "-".to_string(),
            (m, 0) => format!("{m} mana"),
            (0, st) => format!("{st} stamina"),
            (m, st) => format!("{m} mana, {st} stamina"),
        };
        s.push_str(&format!(
            " - {} ({}) [{class_str}] cd={}ms cost={}\r\n",
            d.id, d.display, d.cooldown_ms, cost
        ));
        if !d.description.trim().is_empty() {
            s.push_str(&format!("   {}\r\n", d.description));
        }
        if !d.aliases.is_empty() {
            s.push_str(&format!("   aliases: {}\r\n", d.aliases.join(", ")));
        }
    }
    s
}

fn render_skill_detail(d: &SkillDef, rank: u32) -> String {
    let mut s = String::new();
    s.push_str("skill:\r\n");
    s.push_str(&format!(" - id: {}\r\n", d.id));
    s.push_str(&format!(" - name: {}\r\n", d.display));
    s.push_str(&format!(
        " - classes: {}\r\n",
        d.classes
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    if !d.aliases.is_empty() {
        s.push_str(&format!(" - aliases: {}\r\n", d.aliases.join(", ")));
    }
    s.push_str(&format!(" - cooldown_ms: {}\r\n", d.cooldown_ms));
    if d.cost_mana > 0 {
        s.push_str(&format!(" - cost_mana: {}\r\n", d.cost_mana));
    }
    if d.cost_stamina > 0 {
        s.push_str(&format!(" - cost_stamina: {}\r\n", d.cost_stamina));
    }
    let stun_ms = skill_stun_ms(d);
    if stun_ms > 0 {
        s.push_str(&format!(" - stun_ms: {}\r\n", stun_ms));
    }
    s.push_str(&format!(
        " - target: {}\r\n",
        match d.target {
            SkillTarget::CombatTarget => "combat_target",
            SkillTarget::SelfOnly => "self",
        }
    ));
    s.push_str(&format!(
        " - trained_rank: {}\r\n",
        if rank == 0 {
            "(untrained)".to_string()
        } else {
            rank.to_string()
        }
    ));
    if !d.tags.is_empty() {
        s.push_str(&format!(" - tags: {}\r\n", d.tags.join(", ")));
    }
    if !d.description.trim().is_empty() {
        s.push_str(&format!(" - description: {}\r\n", d.description));
    }
    if !d.flavor.trim().is_empty() {
        s.push_str(&format!(" - flavor: {}\r\n", d.flavor));
    }
    s
}

fn skill_stun_ms(d: &SkillDef) -> u64 {
    // Keep this as data-driven as we can without expanding the SkillDef surface.
    match d.id {
        "shield_bash" => 1500,
        "dirty_trick" => 1200,
        _ => 0,
    }
}

fn compute_skill_amount(d: &SkillDef, rank: u32, stats: &AbilityScores) -> i32 {
    let r = rank.max(1) as i32;
    match d.effect {
        SkillEffect::Damage {
            base,
            per_rank,
            ability,
        } => {
            let m = stats.mod_for(ability);
            base + per_rank * (r - 1) + m.max(0)
        }
        SkillEffect::Heal {
            base,
            per_rank,
            ability,
        } => {
            let m = stats.mod_for(ability);
            base + per_rank * (r - 1) + m.max(0)
        }
    }
}

fn xp_needed_for_next(level: u32) -> u32 {
    // Keep it fast for now.
    10 * level.max(1)
}

fn race_from_token(tok: Option<&str>) -> Option<Race> {
    tok.and_then(Race::parse)
}

fn class_from_token(tok: Option<&str>) -> Option<Class> {
    tok.and_then(Class::parse)
}

fn sex_from_token(tok: &str) -> Sex {
    Sex::parse(tok).unwrap_or(Sex::None)
}

fn pronouns_from_token(tok: &str, sex: Sex) -> PronounKey {
    PronounKey::parse(tok).unwrap_or_else(|| PronounKey::default_for_sex(sex))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ability {
    Str,
    Dex,
    Con,
    Int,
    Wis,
    Cha,
}

#[derive(Debug, Clone, Copy)]
struct AbilityScores {
    str_: i32,
    dex: i32,
    con: i32,
    int_: i32,
    wis: i32,
    cha: i32,
}

impl AbilityScores {
    fn baseline() -> Self {
        Self {
            str_: 10,
            dex: 10,
            con: 10,
            int_: 10,
            wis: 10,
            cha: 10,
        }
    }

    fn get(&self, a: Ability) -> i32 {
        match a {
            Ability::Str => self.str_,
            Ability::Dex => self.dex,
            Ability::Con => self.con,
            Ability::Int => self.int_,
            Ability::Wis => self.wis,
            Ability::Cha => self.cha,
        }
    }

    fn set(&mut self, a: Ability, v: i32) {
        match a {
            Ability::Str => self.str_ = v,
            Ability::Dex => self.dex = v,
            Ability::Con => self.con = v,
            Ability::Int => self.int_ = v,
            Ability::Wis => self.wis = v,
            Ability::Cha => self.cha = v,
        }
    }

    fn mod_for(&self, a: Ability) -> i32 {
        // 5e style modifier = floor((score - 10) / 2)
        (self.get(a) - 10).div_euclid(2)
    }
}

#[derive(Debug, Clone)]
struct CombatState {
    autoattack: bool,
    target: Option<CharacterId>,
    next_ready_ms: u64,
    seq: u64,
}

impl CombatState {
    fn new(now_ms: u64) -> Self {
        Self {
            autoattack: false,
            target: None,
            next_ready_ms: now_ms,
            seq: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct Rng64 {
    state: u64,
}

impl Rng64 {
    fn from_seed(seed: u64) -> Self {
        let mut s = seed;
        if s == 0 {
            s = 0x9e3779b97f4a7c15;
        }
        Self { state: s }
    }

    fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn roll_range(&mut self, lo: i32, hi_inclusive: i32) -> i32 {
        debug_assert!(lo <= hi_inclusive);
        let span = (hi_inclusive - lo + 1) as u64;
        let v = (self.next_u64() % span) as i32;
        lo + v
    }
}

#[derive(Clone)]
struct World {
    rooms: rooms::Rooms,
    sessions: HashMap<SessionId, SessionState>,
    chars: HashMap<CharacterId, Character>,
    occupants: HashMap<String, HashSet<CharacterId>>,
    parties: HashMap<PartyId, Party>,
    party_invites: HashMap<CharacterId, PartyInvite>, // invitee cid -> invite
    next_party_id: PartyId,
    party_of: HashMap<CharacterId, PartyId>, // member cid -> party id
    rng: Rng64,
    next_char_id: CharacterId,
    now_ms: u64,
    clock_last_instant: std::time::Instant,
    started_instant: std::time::Instant,
    started_unix: u64,
    event_seq: u64,
    events: BinaryHeap<Reverse<ScheduledEvent>>,
    bartender_id: Option<CharacterId>,
    bartender_emote_idx: u64,
    bartender_emote_ms: u64,
    mob_wander_ms: u64,
    time_scale_ppm: u64,
    bosses: HashMap<CharacterId, BossState>,
    raft: raftlog::RaftLog<worldlog::WorldLogEntry>,
    raft_watch: HashSet<CharacterId>,
    seen_commands: HashSet<ClientCommandKey>,
    groups: groups::GroupStore,
    cluster_features: worldlog::ClusterFeatureState,
    cluster_metadata: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorldAppendMode {
    Normal,
    ClusterFeatureActivation,
}

impl World {
    fn new(
        rooms: rooms::Rooms,
        seed: u64,
        bartender_emote_ms: u64,
        mob_wander_ms: u64,
        time_scale_ppm: u64,
        raft_log_path: PathBuf,
        raft_consensus: raftlog::ConsensusConfig,
        bootstrap_admins: Vec<String>,
        bootstrap_admin_sso: Vec<String>,
    ) -> anyhow::Result<Self> {
        let started_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let (raft, replay) =
            raftlog::RaftLog::open_with_consensus(raft_log_path.clone(), raft_consensus)
                .with_context(|| format!("open raft log {}", raft_log_path.display()))?;

        let mut w = Self {
            rooms,
            sessions: HashMap::new(),
            chars: HashMap::new(),
            occupants: HashMap::new(),
            parties: HashMap::new(),
            party_invites: HashMap::new(),
            next_party_id: 1,
            party_of: HashMap::new(),
            rng: Rng64::from_seed(seed),
            next_char_id: 1,
            now_ms: 0,
            clock_last_instant: std::time::Instant::now(),
            started_instant: std::time::Instant::now(),
            started_unix,
            event_seq: 1,
            events: BinaryHeap::new(),
            bartender_id: None,
            bartender_emote_idx: 0,
            bartender_emote_ms,
            mob_wander_ms,
            time_scale_ppm,
            bosses: HashMap::new(),
            raft,
            raft_watch: HashSet::new(),
            seen_commands: HashSet::new(),
            groups: groups::GroupStore::default(),
            cluster_features: worldlog::ClusterFeatureState::default(),
            cluster_metadata: BTreeMap::new(),
        };

        for env in replay {
            w.apply_log_entry(&env.entry);
        }
        w.rebuild_occupants();
        w.ensure_genesis_groups(&bootstrap_admins, &bootstrap_admin_sso)?;
        Ok(w)
    }

    fn render_uptime(&self) -> String {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let up = self.started_instant.elapsed().as_secs();
        let mut s = String::new();
        s.push_str("uptime:\r\n");
        s.push_str(&format!(" - shard_wall_unix: {now_unix}\r\n"));
        s.push_str(&format!(" - shard_started_unix: {}\r\n", self.started_unix));
        s.push_str(&format!(" - shard_uptime_s: {up}\r\n"));
        s.push_str(&format!(" - world_time_ms: {}\r\n", self.now_ms));
        s
    }

    fn cluster_feature_status_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!(
                " - world_log_format_active: {}",
                self.cluster_features.active_world_log_format
            ),
            format!(
                " - world_log_format_supported: {}",
                worldlog::WORLD_LOG_MAX_SUPPORTED_FORMAT
            ),
            format!(
                " - snapshot_format_active: {}",
                self.cluster_features.snapshot_format
            ),
            format!(
                " - min_reader_world_log_format: {}",
                self.cluster_features.min_reader_world_log_format
            ),
            format!(" - cluster_metadata_count: {}", self.cluster_metadata.len()),
        ];
        let voters = self.world_log_format_voters(Duration::from_millis(500));
        lines.push(" - world_log_format_voters:".to_string());
        for voter in voters {
            lines.push(format!(
                "   - voter {}: reachable={} max_format={} term={} role={} leader={} commit={} last={} last_term={} quorum_recent={} latency_ms={}{}",
                voter.node_id,
                voter.reachable,
                voter.max_format,
                voter
                    .term
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                voter.role.as_deref().unwrap_or("-"),
                voter.leader_id.as_deref().unwrap_or("-"),
                voter
                    .commit_index
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                voter
                    .last_index
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                voter
                    .last_log_term
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                voter
                    .quorum_recent
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                voter
                    .latency_ms
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                voter
                    .error
                    .as_ref()
                    .map(|err| format!(" error={err}"))
                    .unwrap_or_default(),
            ));
        }
        lines
    }

    fn world_log_format_voters(
        &self,
        timeout: std::time::Duration,
    ) -> Vec<raftlog::ApplicationFormatVoter> {
        self.raft.application_format_voters(
            "single",
            worldlog::WORLD_LOG_MAX_SUPPORTED_FORMAT,
            timeout,
        )
    }

    fn ensure_persistence_format_write_allowed(
        &self,
        artifact: worldlog::PersistenceArtifactKind,
        requested_format: u32,
    ) -> anyhow::Result<()> {
        match worldlog::evaluate_persistence_format_write(
            &self.cluster_features,
            artifact,
            requested_format,
        ) {
            worldlog::PersistenceFormatDecision::Allowed => Ok(()),
            worldlog::PersistenceFormatDecision::Rejected {
                reason,
                active_snapshot_format,
                min_reader_world_log_format,
                ..
            } => anyhow::bail!(
                "persistence format {requested_format} rejected for {artifact:?}: {reason}; active_snapshot_format={active_snapshot_format} min_reader_world_log_format={min_reader_world_log_format}"
            ),
        }
    }

    fn ensure_equip_slot_write_allowed(&self, slot: items::EquipSlot) -> anyhow::Result<()> {
        let min_format = slot.min_world_log_format();
        let active = self.cluster_features.active_world_log_format;
        anyhow::ensure!(
            active >= min_format,
            "equipment slot {} requires active world log format {min_format}; active format is {active}",
            slot.as_str()
        );
        Ok(())
    }

    fn ensure_genesis_groups(
        &mut self,
        bootstrap_admins: &[String],
        bootstrap_admin_sso: &[String],
    ) -> anyhow::Result<()> {
        if self.raft.consensus_enabled() && !self.raft.accepts_client_writes() {
            warn!(
                "raft consensus enabled; deferring genesis group creation until a leader handles writes"
            );
            return Ok(());
        }

        const GROUP_ID_ADMINS: u64 = 1;
        const GROUP_ID_CLASS_BASE: u64 = 1000;

        // Ensure "admins" exists.
        if !self.groups.groups.contains_key(&GROUP_ID_ADMINS) {
            let e = groups::GroupLogEntry::GroupCreate {
                group_id: GROUP_ID_ADMINS,
                kind: groups::GroupKind::Admin,
                name: "admins".to_string(),
            };
            let _ = self.raft_append_group(e)?;
        }

        // Ensure a class group exists for every class (implied membership by class).
        for (i, class) in Class::all().iter().enumerate() {
            let id = GROUP_ID_CLASS_BASE + i as u64;
            if self.groups.groups.contains_key(&id) {
                continue;
            }
            let class_name = class.as_str().to_string();
            let e = groups::GroupLogEntry::GroupCreate {
                group_id: id,
                kind: groups::GroupKind::Class {
                    class: class_name.clone(),
                },
                name: format!("class:{class_name}"),
            };
            let _ = self.raft_append_group(e)?;
        }

        // Bootstrap admins into the admin group (genesis convenience).
        for who in bootstrap_admins {
            let who = who.trim();
            if who.is_empty() {
                continue;
            }
            // Historical env var: list of account/character names. Store as acct:<name>.
            let principal = if who.contains(':') {
                who.to_string()
            } else {
                format!("acct:{who}")
            };
            if self
                .groups
                .role_for_principal_in_group(GROUP_ID_ADMINS, &principal, "")
                .is_some()
            {
                continue;
            }
            let e = groups::GroupLogEntry::GroupMemberSet {
                group_id: GROUP_ID_ADMINS,
                member: principal,
                role: Some(groups::GroupRole::Member),
            };
            let _ = self.raft_append_group(e)?;
        }

        // Bootstrap admins from SSO principals (recommended).
        for p in bootstrap_admin_sso {
            let p = p.trim();
            if p.is_empty() {
                continue;
            }
            let member = normalize_principal_token(p);
            if self
                .groups
                .role_for_principal_in_group(GROUP_ID_ADMINS, &member, "")
                .is_some()
            {
                continue;
            }
            let e = groups::GroupLogEntry::GroupMemberSet {
                group_id: GROUP_ID_ADMINS,
                member,
                role: Some(groups::GroupRole::Member),
            };
            let _ = self.raft_append_group(e)?;
        }

        Ok(())
    }

    fn raft_append_group(
        &mut self,
        entry: groups::GroupLogEntry,
    ) -> anyhow::Result<raftlog::RaftEnvelope<worldlog::WorldLogEntry>> {
        let entry = worldlog::WorldLogEntry::Group(entry);
        let env = self.raft.append(self.now_ms(), entry.clone())?;
        self.apply_log_entry(&entry);
        Ok(env)
    }

    fn ensure_world_event_append_allowed(
        &self,
        event: &worldlog::WorldEvent,
        mode: WorldAppendMode,
    ) -> anyhow::Result<()> {
        match worldlog::world_event_write_gate(event) {
            worldlog::WorldEventWriteGate::RollingSafe => {
                anyhow::ensure!(
                    mode == WorldAppendMode::Normal,
                    "cluster feature activation append expects ClusterFeatureSet"
                );
                Ok(())
            }
            worldlog::WorldEventWriteGate::RequiresActiveWorldLogFormat {
                feature,
                min_format,
            } => {
                anyhow::ensure!(
                    mode == WorldAppendMode::Normal,
                    "cluster feature activation append expects ClusterFeatureSet"
                );
                let active = self.cluster_features.active_world_log_format;
                anyhow::ensure!(
                    active >= min_format,
                    "world log feature {feature} requires active format {min_format}; active format is {active}"
                );
                Ok(())
            }
            worldlog::WorldEventWriteGate::ClusterFeatureActivation { target_format } => {
                anyhow::ensure!(
                    mode == WorldAppendMode::ClusterFeatureActivation,
                    "cluster feature activation must use raft feature worldlog"
                );
                let active = self.cluster_features.active_world_log_format;
                anyhow::ensure!(
                    target_format > active,
                    "world log format {target_format} is not newer than active format {active}"
                );
                anyhow::ensure!(
                    target_format <= worldlog::WORLD_LOG_MAX_SUPPORTED_FORMAT,
                    "world log format {target_format} exceeds this binary's supported format {}",
                    worldlog::WORLD_LOG_MAX_SUPPORTED_FORMAT
                );
                Ok(())
            }
        }
    }

    fn raft_append_world_with_mode(
        &mut self,
        event: worldlog::WorldEvent,
        mode: WorldAppendMode,
    ) -> anyhow::Result<raftlog::RaftEnvelope<worldlog::WorldLogEntry>> {
        self.ensure_world_event_append_allowed(&event, mode)?;
        let entry = worldlog::WorldLogEntry::World(event);
        let env = self.raft.append(self.now_ms(), entry.clone())?;
        self.apply_log_entry(&entry);
        Ok(env)
    }

    fn raft_append_world(
        &mut self,
        event: worldlog::WorldEvent,
    ) -> anyhow::Result<raftlog::RaftEnvelope<worldlog::WorldLogEntry>> {
        self.raft_append_world_with_mode(event, WorldAppendMode::Normal)
    }

    fn raft_append_cluster_feature_activation(
        &mut self,
        event: worldlog::WorldEvent,
    ) -> anyhow::Result<raftlog::RaftEnvelope<worldlog::WorldLogEntry>> {
        self.raft_append_world_with_mode(event, WorldAppendMode::ClusterFeatureActivation)
    }

    fn project_world(&mut self, event: worldlog::WorldEvent) {
        if let Err(err) = self.ensure_world_event_append_allowed(&event, WorldAppendMode::Normal) {
            warn!(err = %err, "world raft append blocked by feature gate");
            return;
        }
        if let Err(err) = self.raft_append_world(event.clone()) {
            warn!(err = %err, "world raft append failed; applying projected event locally");
            self.apply_world_event(&event);
        }
    }

    fn remember_client_command(
        &mut self,
        session: SessionId,
        command_id: Option<u64>,
        principal: &str,
    ) -> bool {
        let Some(command_id) = command_id else {
            return true;
        };
        let key = ClientCommandKey {
            session,
            command_id,
        };
        if self.seen_commands.contains(&key) {
            return false;
        }
        self.project_world(worldlog::WorldEvent::ClientCommandSeen {
            session: session.0.to_string(),
            command_id,
            principal: principal.to_string(),
            reason: "broker-input".to_string(),
        });
        true
    }

    fn replay_raft_tail(&mut self) {
        let entries = match self.raft.poll_replay() {
            Ok(entries) => entries,
            Err(err) => {
                warn!(err = %err, "world raft tail replay failed");
                return;
            }
        };
        if entries.is_empty() {
            return;
        }
        for env in entries {
            self.apply_log_entry(&env.entry);
        }
        self.rebuild_occupants();
    }

    fn apply_log_entry(&mut self, entry: &worldlog::WorldLogEntry) {
        match entry {
            worldlog::WorldLogEntry::Group(entry) => self.groups.apply(entry),
            worldlog::WorldLogEntry::World(event) => self.apply_world_event(event),
        }
    }

    fn apply_world_event(&mut self, event: &worldlog::WorldEvent) {
        match event {
            worldlog::WorldEvent::CharacterSnapshot { character, .. } => {
                self.apply_character_snapshot(character);
            }
            worldlog::WorldEvent::MobSpawned {
                character_id,
                room_id,
                name,
                hp,
                max_hp,
                boss,
            } => {
                let cid = *character_id;
                if let Some(old_room) = self.chars.get(&cid).map(|c| c.room_id.clone()) {
                    self.remove_occupant_from(cid, &old_room);
                }
                let c = self.mob_character_from_projection(
                    cid,
                    room_id.clone(),
                    name.clone(),
                    *hp,
                    *max_hp,
                );
                self.chars.insert(cid, c);
                self.next_char_id = self.next_char_id.max(cid.saturating_add(1));
                if let Some(boss) = boss {
                    self.bosses.insert(
                        cid,
                        BossState {
                            casting_until_ms: boss.casting_until_ms,
                            seq: boss.seq,
                        },
                    );
                }
                if name == "bartender" && room_id == ROOM_TAVERN {
                    self.bartender_id = Some(cid);
                }
                self.reindex_char_room(cid);
            }
            worldlog::WorldEvent::CharacterMoved {
                character_id, to, ..
            } => {
                self.move_char_local(*character_id, to.clone());
            }
            worldlog::WorldEvent::CharacterRemoved { character_id, .. } => {
                self.remove_char_local(*character_id);
            }
            worldlog::WorldEvent::CombatSet {
                character_id,
                autoattack,
                target,
                next_ready_ms,
                ..
            } => {
                if let Some(c) = self.chars.get_mut(character_id) {
                    c.combat.autoattack = *autoattack;
                    c.combat.target = *target;
                    c.combat.next_ready_ms = *next_ready_ms;
                    c.combat.seq = c.combat.seq.saturating_add(1);
                }
            }
            worldlog::WorldEvent::HpSet {
                character_id, hp, ..
            } => {
                if let Some(c) = self.chars.get_mut(character_id) {
                    c.hp = *hp;
                }
            }
            worldlog::WorldEvent::StunSet {
                character_id,
                stunned_until_ms,
                ..
            } => {
                if let Some(c) = self.chars.get_mut(character_id) {
                    c.stunned_until_ms = *stunned_until_ms;
                }
            }
            worldlog::WorldEvent::BossStateSet {
                boss_id,
                casting_until_ms,
                seq,
                present,
                ..
            } => {
                if *present {
                    self.bosses.insert(
                        *boss_id,
                        BossState {
                            casting_until_ms: *casting_until_ms,
                            seq: *seq,
                        },
                    );
                } else {
                    self.bosses.remove(boss_id);
                }
            }
            worldlog::WorldEvent::AmbientStateSet {
                bartender_id,
                bartender_emote_idx,
                ..
            } => {
                self.bartender_id = *bartender_id;
                self.bartender_emote_idx = *bartender_emote_idx;
            }
            worldlog::WorldEvent::PartyCreated { party_id, leader } => {
                let mut members = HashSet::new();
                members.insert(*leader);
                self.parties.insert(
                    *party_id,
                    Party {
                        id: *party_id,
                        leader: *leader,
                        members,
                    },
                );
                self.party_of.insert(*leader, *party_id);
                self.next_party_id = self.next_party_id.max(party_id.saturating_add(1));
            }
            worldlog::WorldEvent::PartyMemberSet {
                party_id,
                member,
                present,
            } => {
                if *present {
                    if let Some(old) = self.party_of.insert(*member, *party_id) {
                        if old != *party_id {
                            if let Some(p) = self.parties.get_mut(&old) {
                                p.members.remove(member);
                            }
                        }
                    }
                    if let Some(p) = self.parties.get_mut(party_id) {
                        p.members.insert(*member);
                    }
                } else {
                    self.party_of.remove(member);
                    if let Some(p) = self.parties.get_mut(party_id) {
                        p.members.remove(member);
                    }
                }
            }
            worldlog::WorldEvent::PartyLeaderSet { party_id, leader } => {
                if let Some(p) = self.parties.get_mut(party_id) {
                    p.leader = *leader;
                    p.members.insert(*leader);
                    self.party_of.insert(*leader, *party_id);
                }
            }
            worldlog::WorldEvent::PartyDisbanded { party_id } => {
                if let Some(p) = self.parties.remove(party_id) {
                    for mid in p.members {
                        self.party_of.remove(&mid);
                    }
                }
                self.party_invites
                    .retain(|_, inv| inv.party_id != *party_id);
            }
            worldlog::WorldEvent::PartyInviteSet {
                invitee,
                party_id,
                inviter,
                expires_ms,
                present,
            } => {
                if *present {
                    self.party_invites.insert(
                        *invitee,
                        PartyInvite {
                            party_id: *party_id,
                            inviter: *inviter,
                            expires_ms: *expires_ms,
                        },
                    );
                } else {
                    self.party_invites.remove(invitee);
                }
            }
            worldlog::WorldEvent::RngStateSet { state, .. } => {
                self.rng.state = *state;
            }
            worldlog::WorldEvent::ClockSet { now_ms, .. } => {
                self.now_ms = *now_ms;
            }
            worldlog::WorldEvent::ScheduledEventSet { event, present, .. } => {
                if *present {
                    if let Some(ev) = ScheduledEvent::from_projection(event) {
                        self.schedule_event_local(ev);
                    }
                } else {
                    self.remove_scheduled_event_local(event.seq);
                }
            }
            worldlog::WorldEvent::RoomSet {
                room_id,
                room,
                present,
                ..
            } => {
                if *present {
                    if let Some(room) = room.clone() {
                        self.rooms.insert_room(room_id.clone(), room);
                    }
                } else {
                    self.rooms.remove_dyn_room(room_id);
                }
            }
            worldlog::WorldEvent::ClientCommandSeen {
                session,
                command_id,
                ..
            } => {
                if let Ok(session) = session.parse::<u128>() {
                    self.seen_commands.insert(ClientCommandKey {
                        session: SessionId(session),
                        command_id: *command_id,
                    });
                }
            }
            worldlog::WorldEvent::ClusterFeatureSet {
                active_world_log_format,
                snapshot_format,
                min_reader_world_log_format,
                ..
            } => {
                self.cluster_features = worldlog::ClusterFeatureState {
                    active_world_log_format: *active_world_log_format,
                    snapshot_format: *snapshot_format,
                    min_reader_world_log_format: *min_reader_world_log_format,
                };
            }
            worldlog::WorldEvent::ClusterMetadataSet { key, value, .. } => {
                if let Some(value) = value {
                    self.cluster_metadata.insert(key.clone(), value.clone());
                } else {
                    self.cluster_metadata.remove(key);
                }
            }
        }
    }

    fn snapshot_for_character(&self, c: &Character) -> worldlog::CharacterSnapshot {
        worldlog::CharacterSnapshot {
            character_id: c.id,
            name: c.name.clone(),
            principal: c.principal.clone(),
            auth_caps: {
                let mut caps = c
                    .auth_caps
                    .iter()
                    .map(|cap| cap.as_str().to_string())
                    .collect::<Vec<_>>();
                caps.sort_unstable();
                caps
            },
            is_bot: c.is_bot,
            bot_ever: c.bot_ever,
            bot_ever_since_ms: c.bot_ever_since_ms,
            bot_mode_changed_ms: c.bot_mode_changed_ms,
            friends: {
                let mut friends = c.friends.iter().cloned().collect::<Vec<_>>();
                friends.sort_unstable();
                friends
            },
            room_id: c.room_id.clone(),
            autoassist: c.autoassist,
            follow_leader: c.follow_leader,
            drink_level: c.drink_level,
            gold: c.gold,
            inv: c.inv.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            quest: c
                .quest
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            class: c.class.map(|v| v.as_str().to_string()),
            level: c.level,
            xp: c.xp,
            skill_points: c.skill_points,
            skills: c.skills.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            skill_cd_ms: c.skill_cd_ms.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            race: c.race.map(|v| v.as_str().to_string()),
            sex: c.sex.as_str().to_string(),
            pronouns: c.pronouns.as_str().to_string(),
            stats: worldlog::AbilityScoresSnapshot {
                str_: c.stats.str_,
                dex: c.stats.dex,
                con: c.stats.con,
                int_: c.stats.int_,
                wis: c.stats.wis,
                cha: c.stats.cha,
            },
            hp: c.hp,
            max_hp: c.max_hp,
            mana: c.mana,
            max_mana: c.max_mana,
            stamina: c.stamina,
            max_stamina: c.max_stamina,
            last_mana_regen_ms: c.last_mana_regen_ms,
            last_stamina_regen_ms: c.last_stamina_regen_ms,
            pvp_enabled: c.pvp_enabled,
            stunned_until_ms: c.stunned_until_ms,
            equip: c.equip.snapshot_map(),
        }
    }

    fn apply_character_snapshot(&mut self, s: &worldlog::CharacterSnapshot) {
        let existing = self.chars.get(&s.character_id).cloned();
        if let Some(old) = existing.as_ref() {
            self.remove_occupant_from(s.character_id, &old.room_id);
        }

        let sex = sex_from_token(&s.sex);
        let pronouns = pronouns_from_token(&s.pronouns, sex);
        let equip = Equipment::from_snapshot(&s.equip);

        let auth_caps = s
            .auth_caps
            .iter()
            .filter_map(|cap| groups::Capability::parse(cap))
            .collect::<HashSet<_>>();

        let c = Character {
            id: s.character_id,
            controller: existing.as_ref().and_then(|c| c.controller),
            created_by: existing.as_ref().and_then(|c| c.created_by),
            name: s.name.clone(),
            principal: s.principal.clone(),
            auth_caps,
            is_bot: s.is_bot,
            bot_ever: s.bot_ever,
            bot_ever_since_ms: s.bot_ever_since_ms,
            bot_mode_changed_ms: s.bot_mode_changed_ms,
            friends: s.friends.iter().cloned().collect(),
            room_id: s.room_id.clone(),
            autoassist: s.autoassist,
            follow_leader: s.follow_leader,
            drink_level: s.drink_level,
            gold: s.gold,
            inv: s.inv.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            quest: s
                .quest
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            class: class_from_token(s.class.as_deref()),
            level: s.level,
            xp: s.xp,
            skill_points: s.skill_points,
            skills: s.skills.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            skill_cd_ms: s.skill_cd_ms.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            race: race_from_token(s.race.as_deref()),
            sex,
            pronouns,
            stats: AbilityScores {
                str_: s.stats.str_,
                dex: s.stats.dex,
                con: s.stats.con,
                int_: s.stats.int_,
                wis: s.stats.wis,
                cha: s.stats.cha,
            },
            hp: s.hp,
            max_hp: s.max_hp,
            mana: s.mana,
            max_mana: s.max_mana,
            stamina: s.stamina,
            max_stamina: s.max_stamina,
            last_mana_regen_ms: s.last_mana_regen_ms,
            last_stamina_regen_ms: s.last_stamina_regen_ms,
            pvp_enabled: s.pvp_enabled,
            stunned_until_ms: s.stunned_until_ms,
            combat: existing
                .as_ref()
                .map(|c| c.combat.clone())
                .unwrap_or_else(|| CombatState::new(self.now_ms)),
            equip,
        };
        self.next_char_id = self.next_char_id.max(s.character_id.saturating_add(1));
        self.chars.insert(s.character_id, c);
        self.reindex_char_room(s.character_id);
    }

    fn snapshot_character_projected(&mut self, cid: CharacterId, reason: &'static str) {
        let Some(c) = self.chars.get(&cid) else {
            return;
        };
        let character = self.snapshot_for_character(c);
        self.project_world(worldlog::WorldEvent::CharacterSnapshot {
            character,
            reason: reason.to_string(),
        });
    }

    fn effective_caps_for(&self, c: &Character) -> HashSet<groups::Capability> {
        let class = c.class.map(|class| class.as_str()).unwrap_or("");
        let mut out = self
            .groups
            .effective_caps_for_principal(&c.principal, class);
        out.extend(c.auth_caps.iter().copied());
        out
    }

    fn has_cap(&self, c: &Character, cap: groups::Capability) -> bool {
        let caps = self.effective_caps_for(c);
        caps.contains(&groups::Capability::AdminAll) || caps.contains(&cap)
    }

    fn is_admin(&self, c: &Character) -> bool {
        let caps = self.effective_caps_for(c);
        caps.contains(&groups::Capability::AdminAll)
    }

    fn has_group_cap(&self, c: &Character, group_id: u64, cap: groups::Capability) -> bool {
        if self.is_admin(c) {
            return true;
        }
        let class = c.class.map(|class| class.as_str()).unwrap_or("");
        let caps = self
            .groups
            .caps_for_principal_in_group(group_id, &c.name, class);
        caps.contains(&cap)
    }

    fn resolve_group_id(&self, token: &str) -> Option<u64> {
        let t = token.trim();
        if t.is_empty() {
            return None;
        }
        if let Ok(id) = t.parse::<u64>() {
            if self.groups.groups.contains_key(&id) {
                return Some(id);
            }
        }
        self.groups
            .group_ids_by_name
            .get(&t.to_ascii_lowercase())
            .copied()
    }

    fn render_group(&self, group_id: u64) -> String {
        let Some(g) = self.groups.groups.get(&group_id) else {
            return format!("group: {group_id} (missing)\r\n");
        };
        let mut s = String::new();
        s.push_str(&format!("group: {} ({})\r\n", g.name, g.kind.as_str()));
        s.push_str(&format!(" - id: {}\r\n", g.id));

        let mut members = g
            .members
            .iter()
            .map(|(k, v)| format!(" - {k}: {}\r\n", v.as_str()))
            .collect::<Vec<_>>();
        members.sort_unstable();
        s.push_str("members:\r\n");
        if members.is_empty() {
            s.push_str(" - (none)\r\n");
        } else {
            for m in members {
                s.push_str(&m);
            }
        }

        let mut pol = g
            .policies
            .iter()
            .map(|(k, v)| format!(" - {k}={v}\r\n"))
            .collect::<Vec<_>>();
        pol.sort_unstable();
        s.push_str("policies:\r\n");
        if pol.is_empty() {
            s.push_str(" - (none)\r\n");
        } else {
            for x in pol {
                s.push_str(&x);
            }
        }

        s.push_str("role_caps:\r\n");
        for r in groups::GroupRole::ALL {
            let mut caps = g
                .role_caps
                .get(r)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.as_str().to_string())
                .collect::<Vec<_>>();
            caps.sort_unstable();
            s.push_str(&format!(" - {}: {}\r\n", r.as_str(), caps.join(" ")));
        }
        s
    }

    fn regen_resources(&mut self) {
        const MANA_REGEN_MS: u64 = 1500;
        const STAMINA_REGEN_MS: u64 = 1000;

        let now = self.now_ms;
        let mut changed = Vec::new();
        for c in self.chars.values_mut() {
            if c.controller.is_none() {
                continue;
            }

            let before = (
                c.mana,
                c.stamina,
                c.last_mana_regen_ms,
                c.last_stamina_regen_ms,
            );
            if c.max_mana > 0 && c.mana < c.max_mana {
                let dt = now.saturating_sub(c.last_mana_regen_ms);
                let n = dt / MANA_REGEN_MS;
                if n > 0 {
                    c.mana = (c.mana + n as i32).min(c.max_mana);
                    c.last_mana_regen_ms = now;
                }
            } else {
                c.last_mana_regen_ms = now;
            }

            if c.max_stamina > 0 && c.stamina < c.max_stamina {
                let dt = now.saturating_sub(c.last_stamina_regen_ms);
                let n = dt / STAMINA_REGEN_MS;
                if n > 0 {
                    c.stamina = (c.stamina + n as i32).min(c.max_stamina);
                    c.last_stamina_regen_ms = now;
                }
            } else {
                c.last_stamina_regen_ms = now;
            }
            let after = (
                c.mana,
                c.stamina,
                c.last_mana_regen_ms,
                c.last_stamina_regen_ms,
            );
            if before != after {
                changed.push(c.id);
            }
        }
        for cid in changed {
            self.snapshot_character_projected(cid, "resource-regen");
        }
    }

    fn occupants_of(&self, room_id: &str) -> impl Iterator<Item = &CharacterId> {
        self.occupants
            .get(room_id)
            .map(|s| s.iter())
            .into_iter()
            .flatten()
    }

    fn active_char_id(&self, session: SessionId) -> Option<CharacterId> {
        self.sessions.get(&session).map(|s| s.active)
    }

    fn active_char(&self, session: SessionId) -> Option<&Character> {
        let cid = self.active_char_id(session)?;
        self.chars.get(&cid)
    }

    fn active_char_mut(&mut self, session: SessionId) -> Option<&mut Character> {
        let cid = self.active_char_id(session)?;
        self.chars.get_mut(&cid)
    }

    fn detach_session(&mut self, session: SessionId) -> Vec<Character> {
        let Some(ss) = self.sessions.remove(&session) else {
            return Vec::new();
        };

        let mut detached = Vec::new();
        for cid in ss.controlled {
            self.raft_watch.remove(&cid);
            if let Some(c) = self.chars.get_mut(&cid) {
                c.controller = None;
                detached.push(c.clone());
            }
        }
        detached
    }

    fn attach_existing_character(
        &mut self,
        session: SessionId,
        name: &str,
        principal: &str,
        auth_caps: HashSet<groups::Capability>,
        is_bot: bool,
        race: Race,
        class: Class,
        _sex: Sex,
        _pronouns: PronounKey,
    ) -> Option<CharacterId> {
        let name_lc = name.trim().to_ascii_lowercase();
        let principal_lc = principal.trim().to_ascii_lowercase();
        let cid = self
            .chars
            .iter()
            .filter(|(_, c)| c.principal.eq_ignore_ascii_case(&principal_lc))
            .map(|(cid, _)| *cid)
            .next()
            .or_else(|| {
                self.chars
                    .iter()
                    .filter(|(_, c)| c.name.eq_ignore_ascii_case(&name_lc))
                    .map(|(cid, _)| *cid)
                    .next()
            })?;

        let now_ms = self.now_ms;
        let mut changed = false;
        if let Some(c) = self.chars.get_mut(&cid) {
            c.controller = Some(session);
            if c.created_by.is_none() {
                c.created_by = Some(session);
            }
            if c.auth_caps != auth_caps {
                c.auth_caps = auth_caps;
                changed = true;
            }
            if c.is_bot != is_bot {
                c.is_bot = is_bot;
                c.bot_mode_changed_ms = now_ms;
                if is_bot && !c.bot_ever {
                    c.bot_ever = true;
                    c.bot_ever_since_ms = Some(now_ms);
                }
                changed = true;
            }
            if c.race.is_none() {
                c.race = Some(race);
                changed = true;
            }
            if c.class.is_none() {
                c.class = Some(class);
                c.stats = assign_core_stats_for_class(class);
                c.max_hp = compute_max_hp(class, &c.stats).max(1);
                c.hp = c.max_hp;
                c.max_mana = compute_max_mana(class, &c.stats, c.level).max(0);
                c.mana = c.max_mana;
                c.max_stamina = compute_max_stamina(class, &c.stats, c.level).max(0);
                c.stamina = c.max_stamina;
                c.last_mana_regen_ms = now_ms;
                c.last_stamina_regen_ms = now_ms;
                changed = true;
            }
        }

        let ss = self.sessions.entry(session).or_insert(SessionState {
            controlled: Vec::new(),
            active: cid,
            pending_confirm: None,
        });
        if !ss.controlled.contains(&cid) {
            ss.controlled.push(cid);
        }
        ss.active = cid;
        ss.pending_confirm = None;

        if changed {
            self.snapshot_character_projected(cid, "attach-refresh");
        }

        Some(cid)
    }

    fn now_ms(&self) -> u64 {
        self.now_ms
    }

    fn scaled_wall_dt_ms(&self, dt_ms: u64) -> u64 {
        let scaled = (dt_ms as u128).saturating_mul(self.time_scale_ppm as u128) / 1_000_000u128;
        scaled.min(u64::MAX as u128) as u64
    }

    fn advance_clock_from_wall(&mut self) {
        let now = std::time::Instant::now();
        let dt_ms = now
            .checked_duration_since(self.clock_last_instant)
            .unwrap_or_default()
            .as_millis();
        self.clock_last_instant = now;
        let dt_ms = u64::try_from(dt_ms).unwrap_or(u64::MAX);
        let dt_ms = self.scaled_wall_dt_ms(dt_ms);
        if dt_ms > 0 {
            self.project_world(worldlog::WorldEvent::ClockSet {
                now_ms: self.now_ms.saturating_add(dt_ms),
                reason: "wall-clock".to_string(),
            });
        }
    }

    fn apply_tick(&mut self, dt_ms: u64) {
        // Important: world time advances only via explicit ticks (raft-driven in the future).
        self.project_world(worldlog::WorldEvent::ClockSet {
            now_ms: self.now_ms.saturating_add(dt_ms),
            reason: "tick".to_string(),
        });
    }

    fn schedule_event_local(&mut self, ev: ScheduledEvent) {
        self.remove_scheduled_event_local(ev.seq);
        self.event_seq = self.event_seq.max(ev.seq.saturating_add(1));
        self.events.push(Reverse(ev));
    }

    fn remove_scheduled_event_local(&mut self, seq: u64) -> Option<ScheduledEvent> {
        let mut kept = BinaryHeap::new();
        let mut removed = None;
        while let Some(Reverse(ev)) = self.events.pop() {
            if ev.seq == seq && removed.is_none() {
                removed = Some(ev);
            } else {
                kept.push(Reverse(ev));
            }
        }
        self.events = kept;
        removed
    }

    fn schedule_at_ms(&mut self, due_ms: u64, kind: EventKind) {
        let ev = ScheduledEvent {
            due_ms,
            seq: self.event_seq,
            kind,
        };
        self.event_seq = self.event_seq.saturating_add(1);
        self.project_world(worldlog::WorldEvent::ScheduledEventSet {
            event: ev.to_projection(),
            present: true,
            reason: "schedule".to_string(),
        });
    }

    fn schedule_in_ms(&mut self, delay_ms: u64, kind: EventKind) {
        self.schedule_at_ms(self.now_ms.saturating_add(delay_ms), kind);
    }

    fn pop_due_event(&mut self) -> Option<ScheduledEvent> {
        let Reverse(ev) = self.events.peek()?.clone();
        if ev.due_ms > self.now_ms {
            return None;
        }
        self.project_world(worldlog::WorldEvent::ScheduledEventSet {
            event: ev.to_projection(),
            present: false,
            reason: "pop-due".to_string(),
        });
        Some(ev)
    }

    fn mob_character_from_projection(
        &self,
        cid: CharacterId,
        room_id: String,
        name: String,
        hp: i32,
        max_hp: i32,
    ) -> Character {
        Character {
            id: cid,
            controller: None,
            created_by: None,
            name,
            principal: "mob:".to_string(),
            auth_caps: HashSet::new(),
            is_bot: false,
            bot_ever: false,
            bot_ever_since_ms: None,
            bot_mode_changed_ms: self.now_ms,
            friends: HashSet::new(),
            room_id: room_id.clone(),
            autoassist: false,
            follow_leader: false,
            drink_level: 0,
            gold: 0,
            inv: HashMap::new(),
            quest: HashMap::new(),
            class: None,
            level: 1,
            xp: 0,
            skill_points: 0,
            skills: HashMap::new(),
            skill_cd_ms: HashMap::new(),
            race: None,
            sex: Sex::None,
            pronouns: PronounKey::They,
            stats: AbilityScores::baseline(),
            hp,
            max_hp,
            mana: 0,
            max_mana: 0,
            stamina: 0,
            max_stamina: 0,
            last_mana_regen_ms: self.now_ms,
            last_stamina_regen_ms: self.now_ms,
            pvp_enabled: false,
            stunned_until_ms: 0,
            combat: CombatState::new(self.now_ms),
            equip: Equipment::new(),
        }
    }

    fn rebuild_occupants(&mut self) {
        self.occupants.clear();
        let pairs = self
            .chars
            .iter()
            .map(|(cid, c)| (*cid, c.room_id.clone()))
            .collect::<Vec<_>>();
        for (cid, room_id) in pairs {
            self.occupants.entry(room_id).or_default().insert(cid);
        }
    }

    fn remove_occupant_from(&mut self, cid: CharacterId, room_id: &str) {
        if let Some(s) = self.occupants.get_mut(room_id) {
            s.remove(&cid);
            if s.is_empty() {
                self.occupants.remove(room_id);
            }
        }
    }

    fn reindex_char_room(&mut self, cid: CharacterId) {
        let Some(room_id) = self.chars.get(&cid).map(|c| c.room_id.clone()) else {
            return;
        };
        self.remove_occupant_from(cid, &room_id);
        self.occupants.entry(room_id).or_default().insert(cid);
    }

    fn move_char_local(&mut self, cid: CharacterId, to: String) -> Option<String> {
        let from = self.chars.get(&cid)?.room_id.clone();
        if from == to {
            return Some(from);
        }
        self.remove_occupant_from(cid, &from);
        self.occupants.entry(to.clone()).or_default().insert(cid);
        if let Some(c) = self.chars.get_mut(&cid) {
            c.room_id = to;
        }
        if self.bartender_id == Some(cid)
            && self
                .chars
                .get(&cid)
                .is_some_and(|c| c.room_id != ROOM_TAVERN)
        {
            self.bartender_id = None;
        }
        Some(from)
    }

    fn move_char_projected(&mut self, cid: CharacterId, to: String, reason: &'static str) {
        let from = self.chars.get(&cid).map(|c| c.room_id.clone());
        self.project_world(worldlog::WorldEvent::CharacterMoved {
            character_id: cid,
            from,
            to,
            reason: reason.to_string(),
        });
    }

    fn set_combat_projected(
        &mut self,
        cid: CharacterId,
        autoattack: bool,
        target: Option<CharacterId>,
        next_ready_ms: u64,
        reason: &'static str,
    ) {
        self.project_world(worldlog::WorldEvent::CombatSet {
            character_id: cid,
            autoattack,
            target,
            next_ready_ms,
            reason: reason.to_string(),
        });
    }

    fn set_hp_projected(&mut self, cid: CharacterId, hp: i32, reason: &'static str) {
        self.project_world(worldlog::WorldEvent::HpSet {
            character_id: cid,
            hp,
            reason: reason.to_string(),
        });
    }

    fn set_stun_projected(
        &mut self,
        cid: CharacterId,
        stunned_until_ms: u64,
        reason: &'static str,
    ) {
        self.project_world(worldlog::WorldEvent::StunSet {
            character_id: cid,
            stunned_until_ms,
            reason: reason.to_string(),
        });
    }

    fn set_boss_projected(
        &mut self,
        boss_id: CharacterId,
        casting_until_ms: u64,
        seq: u64,
        reason: &'static str,
    ) {
        self.project_world(worldlog::WorldEvent::BossStateSet {
            boss_id,
            casting_until_ms,
            seq,
            present: true,
            reason: reason.to_string(),
        });
    }

    fn clear_boss_projected(&mut self, boss_id: CharacterId, reason: &'static str) {
        let Some(bs) = self.bosses.get(&boss_id).copied() else {
            return;
        };
        self.project_world(worldlog::WorldEvent::BossStateSet {
            boss_id,
            casting_until_ms: bs.casting_until_ms,
            seq: bs.seq,
            present: false,
            reason: reason.to_string(),
        });
    }

    fn set_ambient_projected(
        &mut self,
        bartender_id: Option<CharacterId>,
        bartender_emote_idx: u64,
        reason: &'static str,
    ) {
        self.project_world(worldlog::WorldEvent::AmbientStateSet {
            bartender_id,
            bartender_emote_idx,
            reason: reason.to_string(),
        });
    }

    fn set_room_projected(&mut self, room_id: String, room: rooms::RoomDef, reason: &'static str) {
        self.project_world(worldlog::WorldEvent::RoomSet {
            room_id,
            room: Some(room),
            present: true,
            reason: reason.to_string(),
        });
    }

    fn remove_room_projected(&mut self, room_id: String, reason: &'static str) {
        self.project_world(worldlog::WorldEvent::RoomSet {
            room_id,
            room: None,
            present: false,
            reason: reason.to_string(),
        });
    }

    fn rng_next_u64_projected(&mut self, reason: &'static str) -> u64 {
        let value = self.rng.next_u64();
        self.project_world(worldlog::WorldEvent::RngStateSet {
            state: self.rng.state,
            reason: reason.to_string(),
        });
        value
    }

    fn rng_roll_range_projected(
        &mut self,
        lo: i32,
        hi_inclusive: i32,
        reason: &'static str,
    ) -> i32 {
        debug_assert!(lo <= hi_inclusive);
        let span = (hi_inclusive - lo + 1) as u64;
        let v = (self.rng_next_u64_projected(reason) % span) as i32;
        lo + v
    }

    fn spawn_mob_with_projection(
        &mut self,
        room_id: String,
        name: String,
        hp: i32,
        max_hp: i32,
        boss: Option<worldlog::BossProjection>,
    ) -> CharacterId {
        let cid = self.next_char_id;
        self.project_world(worldlog::WorldEvent::MobSpawned {
            character_id: cid,
            room_id,
            name,
            hp,
            max_hp,
            boss,
        });
        cid
    }

    fn spawn_mob(&mut self, room_id: String, name: String) -> CharacterId {
        self.spawn_mob_with_projection(room_id, name, 1, 1, None)
    }

    fn spawn_stenchworm(&mut self, room_id: String) -> CharacterId {
        self.spawn_mob_with_projection(room_id, "stenchworm".to_string(), 9, 9, None)
    }

    fn inv_add(&mut self, cid: CharacterId, item: &str, n: u32) {
        let Some(c) = self.chars.get_mut(&cid) else {
            return;
        };
        let e = c.inv.entry(item.to_string()).or_insert(0);
        *e = (*e).saturating_add(n);
        self.snapshot_character_projected(cid, "inventory-add");
    }

    fn inv_take_one(&mut self, cid: CharacterId, item: &str) -> bool {
        let Some(c) = self.chars.get_mut(&cid) else {
            return false;
        };
        let k = item.to_string();
        let Some(v) = c.inv.get_mut(&k) else {
            return false;
        };
        if *v == 0 {
            c.inv.remove(&k);
            return false;
        }
        *v -= 1;
        if *v == 0 {
            c.inv.remove(&k);
        }
        self.snapshot_character_projected(cid, "inventory-take-one");
        true
    }

    fn inv_take_n(&mut self, cid: CharacterId, item: &str, n: u32) -> u32 {
        let Some(c) = self.chars.get_mut(&cid) else {
            return 0;
        };
        let k = item.to_string();
        let Some(v) = c.inv.get_mut(&k) else {
            return 0;
        };
        let take = (*v).min(n);
        *v -= take;
        if *v == 0 {
            c.inv.remove(&k);
        }
        if take > 0 {
            self.snapshot_character_projected(cid, "inventory-take-n");
        }
        take
    }

    fn find_mob_in_room(&self, room_id: &str, token: &str) -> Option<CharacterId> {
        let t = token.trim().to_ascii_lowercase();
        if t.is_empty() {
            return None;
        }
        let occ = self.occupants.get(room_id)?;
        for cid in occ {
            let Some(c) = self.chars.get(cid) else {
                continue;
            };
            if c.controller.is_some() {
                continue;
            }
            let name_lc = c.name.to_ascii_lowercase();
            if name_lc == t || name_lc.starts_with(&t) {
                return Some(*cid);
            }
        }
        None
    }

    fn find_player_in_room(&self, room_id: &str, token: &str) -> Option<CharacterId> {
        let t = token.trim().to_ascii_lowercase();
        if t.is_empty() {
            return None;
        }
        let occ = self.occupants.get(room_id)?;
        for cid in occ {
            let Some(c) = self.chars.get(cid) else {
                continue;
            };
            if c.controller.is_none() {
                continue;
            }
            let name_lc = c.name.to_ascii_lowercase();
            if name_lc == t || name_lc.starts_with(&t) {
                return Some(*cid);
            }
        }
        None
    }

    fn is_pvp_room(&self, room_id: &str) -> bool {
        // Keep PvP constrained to explicit \"arena\" rooms for now.
        room_id.starts_with("arena.")
    }

    fn can_pvp_ids(&self, attacker_id: CharacterId, target_id: CharacterId) -> bool {
        if attacker_id == target_id {
            return false;
        }
        let Some(att) = self.chars.get(&attacker_id) else {
            return false;
        };
        let Some(tgt) = self.chars.get(&target_id) else {
            return false;
        };
        if att.controller.is_none() || tgt.controller.is_none() {
            return false;
        }
        if att.room_id != tgt.room_id {
            return false;
        }
        if !self.is_pvp_room(&att.room_id) {
            return false;
        }
        if !att.pvp_enabled || !tgt.pvp_enabled {
            return false;
        }
        is_built(att) && is_built(tgt) && att.hp > 0 && tgt.hp > 0
    }

    fn start_combat(&mut self, attacker_id: CharacterId, target_id: CharacterId) {
        if !self.chars.contains_key(&attacker_id) {
            return;
        }
        self.set_combat_projected(
            attacker_id,
            true,
            Some(target_id),
            self.now_ms,
            "start-combat",
        );
        self.schedule_at_ms(self.now_ms, EventKind::CombatAct { attacker_id });

        // If you start combat with a mob, it retaliates.
        let target_is_mob = self
            .chars
            .get(&target_id)
            .is_some_and(|t| t.controller.is_none());
        let target_is_player = self
            .chars
            .get(&target_id)
            .is_some_and(|t| t.controller.is_some());
        let allow_player_retaliate = target_is_player && self.can_pvp_ids(attacker_id, target_id);

        if target_is_mob || allow_player_retaliate {
            self.set_combat_projected(target_id, true, Some(attacker_id), self.now_ms, "retaliate");
            self.schedule_at_ms(
                self.now_ms,
                EventKind::CombatAct {
                    attacker_id: target_id,
                },
            );
        }

        // Party auto-assist (best-effort).
        self.party_assist_attackers(attacker_id, target_id);
    }

    fn spawn_named_mob(&mut self, room_id: String, token: &str) -> Option<CharacterId> {
        let t = token.trim().to_ascii_lowercase();
        if t.is_empty() {
            return None;
        }
        match t.as_str() {
            "stenchworm" => Some(self.spawn_stenchworm(room_id)),
            "dummy" | "training_dummy" => {
                Some(self.spawn_mob_with_projection(room_id, "dummy".to_string(), 120, 120, None))
            }
            "rat" => Some(self.spawn_mob_with_projection(room_id, "rat".to_string(), 5, 5, None)),
            "spitter" => {
                Some(self.spawn_mob_with_projection(room_id, "spitter".to_string(), 7, 7, None))
            }
            "grease_king" => {
                let cid = self.spawn_mob_with_projection(
                    room_id.clone(),
                    "grease_king".to_string(),
                    60,
                    60,
                    Some(worldlog::BossProjection {
                        casting_until_ms: 0,
                        seq: 1,
                    }),
                );
                // Start boss mechanics quickly so reference scenarios can sync on the telegraph.
                self.schedule_in_ms(800, EventKind::BossTelegraph { boss_id: cid });
                Some(cid)
            }
            _ => Some(self.spawn_mob_with_projection(room_id, t.to_string(), 10, 10, None)),
        }
    }

    async fn apply_stun(
        &mut self,
        fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
        attacker_id: CharacterId,
        target_id: CharacterId,
        stun_ms: u64,
    ) -> anyhow::Result<()> {
        if stun_ms == 0 {
            return Ok(());
        }
        let now = self.now_ms();
        let Some(att) = self.chars.get(&attacker_id).cloned() else {
            return Ok(());
        };
        let Some(tgt) = self.chars.get(&target_id).cloned() else {
            return Ok(());
        };
        if att.room_id != tgt.room_id {
            return Ok(());
        }

        // Extend stun (do not shorten).
        let until = now.saturating_add(stun_ms);
        let stunned_until_ms = tgt.stunned_until_ms.max(until);
        self.set_stun_projected(target_id, stunned_until_ms, "skill-stun");
        let _ = self
            .broadcast_room(
                fw,
                &att.room_id,
                &format!("* {} stuns {}.", att.name, tgt.name),
            )
            .await;

        // Interrupt boss casts if applicable.
        if let Some(bs) = self.bosses.get(&target_id).copied() {
            if bs.casting_until_ms > now {
                self.set_boss_projected(target_id, 0, bs.seq.saturating_add(1), "boss-interrupt");
                let _ = self
                    .broadcast_room(fw, &att.room_id, &format!("* {} is interrupted!", tgt.name))
                    .await;
            }
        }

        Ok(())
    }

    fn remove_char_local(&mut self, cid: CharacterId) -> Option<Character> {
        self.party_invites.remove(&cid);
        self.party_leave_local(cid);
        let c = self.chars.remove(&cid)?;
        self.remove_occupant_from(cid, &c.room_id);
        self.bosses.remove(&cid);
        if self.bartender_id == Some(cid) {
            self.bartender_id = None;
        }
        Some(c)
    }

    fn remove_char_projected(
        &mut self,
        cid: CharacterId,
        reason: &'static str,
    ) -> Option<Character> {
        let snapshot = self.chars.get(&cid).cloned();
        let room_id = snapshot.as_ref().map(|c| c.room_id.clone());
        self.project_world(worldlog::WorldEvent::CharacterRemoved {
            character_id: cid,
            room_id,
            reason: reason.to_string(),
        });
        snapshot
    }

    fn spawn_character(
        &mut self,
        controller: SessionId,
        name: String,
        principal: String,
        auth_caps: HashSet<groups::Capability>,
        is_bot: bool,
        race: Race,
        class: Class,
        sex: Sex,
        pronouns: PronounKey,
    ) -> CharacterId {
        let cid = self.next_char_id;
        self.next_char_id = self.next_char_id.saturating_add(1);

        let room_id = self.rooms.start_room().to_string();
        let stats = assign_core_stats_for_class(class);
        let max_hp = compute_max_hp(class, &stats).max(1);
        let hp = max_hp;
        let max_mana = compute_max_mana(class, &stats, 1).max(0);
        let mana = max_mana;
        let max_stamina = compute_max_stamina(class, &stats, 1).max(0);
        let stamina = max_stamina;

        let now_ms = self.now_ms;
        let c = Character {
            id: cid,
            controller: Some(controller),
            created_by: Some(controller),
            name,
            principal,
            auth_caps,
            is_bot,
            bot_ever: is_bot,
            bot_ever_since_ms: is_bot.then_some(now_ms),
            bot_mode_changed_ms: now_ms,
            friends: HashSet::new(),
            room_id: room_id.clone(),
            autoassist: true,
            follow_leader: false,
            drink_level: 0,
            gold: 0,
            inv: HashMap::new(),
            quest: HashMap::new(),
            class: Some(class),
            level: 1,
            xp: 0,
            skill_points: 0,
            skills: HashMap::new(),
            skill_cd_ms: HashMap::new(),
            race: Some(race),
            sex,
            pronouns,
            stats,
            hp,
            max_hp,
            mana,
            max_mana,
            stamina,
            max_stamina,
            last_mana_regen_ms: self.now_ms,
            last_stamina_regen_ms: self.now_ms,
            pvp_enabled: false,
            stunned_until_ms: 0,
            combat: CombatState::new(self.now_ms),
            equip: Equipment::new(),
        };

        self.chars.insert(cid, c);
        self.reindex_char_room(cid);

        let ss = self.sessions.entry(controller).or_insert(SessionState {
            controlled: Vec::new(),
            active: cid,
            pending_confirm: None,
        });
        ss.controlled.push(cid);
        ss.active = cid;

        // Starter kit (size-based).
        let kit = starter_kit_for_size(race.size());
        for (item, n) in kit {
            self.inv_add(cid, item, *n);
        }
        let extras =
            starter_kit_extras_for_world_log_format(self.cluster_features.active_world_log_format);
        for (item, n) in extras {
            self.inv_add(cid, item, *n);
        }

        cid
    }

    async fn broadcast_room(
        &self,
        fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
        room_id: &str,
        msg: &str,
    ) -> std::io::Result<()> {
        let mut b = Vec::with_capacity(msg.len() + 2);
        b.extend_from_slice(msg.as_bytes());
        b.extend_from_slice(b"\r\n");

        // A session may control multiple characters in a room; don't duplicate output.
        let mut seen = HashSet::<SessionId>::new();
        for cid in self.occupants_of(room_id) {
            let Some(c) = self.chars.get(cid) else {
                continue;
            };
            let Some(controller) = c.controller else {
                continue;
            };
            if !seen.insert(controller) {
                continue;
            }
            write_resp_async(fw, RESP_OUTPUT, controller, &b).await?;
        }
        Ok(())
    }

    async fn broadcast_room_blob(
        &self,
        fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
        room_id: &str,
        prefix: &[u8],
        path: &[u8],
        len: u64,
        suffix: &[u8],
    ) -> std::io::Result<()> {
        let mut seen = HashSet::<SessionId>::new();
        for cid in self.occupants_of(room_id) {
            let Some(c) = self.chars.get(cid) else {
                continue;
            };
            let Some(controller) = c.controller else {
                continue;
            };
            if !seen.insert(controller) {
                continue;
            }
            write_blob_resp_async(fw, controller, prefix, path, len, suffix).await?;
        }
        Ok(())
    }

    async fn broadcast_all_sessions(
        &self,
        fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
        msg: &str,
    ) -> std::io::Result<()> {
        let mut b = Vec::with_capacity(msg.len() + 2);
        b.extend_from_slice(msg.as_bytes());
        b.extend_from_slice(b"\r\n");

        for sid in self.sessions.keys() {
            write_resp_async(fw, RESP_OUTPUT, *sid, &b).await?;
        }
        Ok(())
    }

    async fn broadcast_raft(
        &self,
        fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
        line: &str,
    ) -> std::io::Result<()> {
        let mut b = Vec::with_capacity(line.len() + 2);
        b.extend_from_slice(line.as_bytes());
        if !line.ends_with("\r\n") {
            b.extend_from_slice(b"\r\n");
        }

        let mut seen = HashSet::<SessionId>::new();
        for cid in self.raft_watch.iter().copied() {
            for s in sessions_attached_to_character(self, cid) {
                if !seen.insert(s) {
                    continue;
                }
                write_resp_async(fw, RESP_OUTPUT, s, &b).await?;
            }
        }
        Ok(())
    }

    fn render_room_for(&self, room_id: &str, viewer: SessionId) -> String {
        let mut s = self.rooms.render_room(room_id);

        let mut others = Vec::new();
        for cid in self.occupants_of(room_id) {
            let Some(c) = self.chars.get(cid) else {
                continue;
            };
            if c.controller == Some(viewer) {
                continue;
            }
            let tag = if c.controller.is_none() { " (mob)" } else { "" };
            others.push(format!("{}{}", c.name, tag));
        }
        if others.is_empty() {
            s.push_str("here: nobody\r\n");
        } else {
            others.sort();
            s.push_str(&format!("here: {}\r\n", others.join(", ")));
        }
        s
    }

    fn online_character_names(&self) -> Vec<String> {
        let mut names = self
            .chars
            .values()
            .filter(|c| c.controller.is_some())
            .map(|c| c.name.clone())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn online_bot_character_names(&self) -> Vec<String> {
        let mut names = self
            .chars
            .values()
            .filter(|c| c.controller.is_some() && c.is_bot)
            .map(|c| c.name.clone())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn online_human_character_names(&self) -> Vec<String> {
        let mut names = self
            .chars
            .values()
            .filter(|c| c.controller.is_some() && !c.is_bot)
            .map(|c| c.name.clone())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    async fn award_xp(
        &mut self,
        fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
        cid: CharacterId,
        xp: u32,
    ) {
        // Split XP across party members in the same room (total XP stays constant).
        let Some(pid) = self.party_of.get(&cid).copied() else {
            self.award_xp_one(fw, cid, xp, false).await;
            return;
        };

        let Some(att) = self.chars.get(&cid).cloned() else {
            return;
        };

        let elig = self
            .parties
            .get(&pid)
            .map(|p| p.members.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .filter(|mid| {
                self.chars
                    .get(mid)
                    .is_some_and(|m| m.controller.is_some() && m.room_id == att.room_id && m.hp > 0)
            })
            .collect::<Vec<_>>();

        if elig.len() <= 1 {
            self.award_xp_one(fw, cid, xp, false).await;
            return;
        }

        let n = elig.len() as u32;
        let base = xp / n;
        let rem = xp % n;
        for (i, mid) in elig.iter().enumerate() {
            let add = base + if i == 0 { rem } else { 0 };
            self.award_xp_one(fw, *mid, add, true).await;
        }
    }

    async fn award_xp_one(
        &mut self,
        fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
        cid: CharacterId,
        xp: u32,
        party: bool,
    ) {
        let Some(c) = self.chars.get_mut(&cid) else {
            return;
        };
        if let Some(sid) = c.controller {
            let msg = if party {
                format!("party xp: +{}.\r\n", xp)
            } else {
                format!("you gain {} xp.\r\n", xp)
            };
            let _ = write_resp_async(fw, RESP_OUTPUT, sid, msg.as_bytes()).await;
        }
        c.xp = c.xp.saturating_add(xp);

        while c.xp >= xp_needed_for_next(c.level) {
            let need = xp_needed_for_next(c.level);
            c.xp -= need;
            c.level = c.level.saturating_add(1);
            c.skill_points = c.skill_points.saturating_add(1);
            c.max_hp = (c.max_hp + 2).max(1);
            c.hp = c.max_hp;
            if let Some(class) = c.class {
                c.max_mana = compute_max_mana(class, &c.stats, c.level).max(0);
                c.mana = c.mana.min(c.max_mana).max(0);
                c.max_stamina = compute_max_stamina(class, &c.stats, c.level).max(0);
                c.stamina = c.stamina.min(c.max_stamina).max(0);
            }

            if let Some(sid) = c.controller {
                let msg = format!(
                    "level up! you are now level {}. (+1 skill point)\r\n",
                    c.level
                );
                let _ = write_resp_async(fw, RESP_OUTPUT, sid, msg.as_bytes()).await;
            }
        }
        self.snapshot_character_projected(cid, "xp-award");
    }

    async fn party_send(
        &self,
        fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
        pid: PartyId,
        msg: &str,
    ) -> std::io::Result<()> {
        let Some(p) = self.parties.get(&pid) else {
            return Ok(());
        };
        let mut b = Vec::with_capacity(msg.len() + 2);
        b.extend_from_slice(msg.as_bytes());
        b.extend_from_slice(b"\r\n");

        let mut seen = HashSet::<SessionId>::new();
        for mid in &p.members {
            let Some(c) = self.chars.get(mid) else {
                continue;
            };
            let Some(sid) = c.controller else {
                continue;
            };
            if !seen.insert(sid) {
                continue;
            }
            write_resp_async(fw, RESP_OUTPUT, sid, &b).await?;
        }
        Ok(())
    }

    fn party_members_vec(&self, pid: PartyId) -> Vec<CharacterId> {
        self.parties
            .get(&pid)
            .map(|p| p.members.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default()
    }

    fn party_create(&mut self, leader: CharacterId) -> PartyId {
        let pid = self.next_party_id;
        self.project_world(worldlog::WorldEvent::PartyCreated {
            party_id: pid,
            leader,
        });
        pid
    }

    fn party_leave_local(&mut self, cid: CharacterId) {
        let Some(pid) = self.party_of.remove(&cid) else {
            return;
        };

        let Some(p) = self.parties.get_mut(&pid) else {
            return;
        };
        p.members.remove(&cid);

        if p.members.is_empty() {
            self.parties.remove(&pid);
            self.party_invites.retain(|_, inv| inv.party_id != pid);
            return;
        }

        if p.leader == cid {
            if let Some(&new_leader) = p.members.iter().next() {
                p.leader = new_leader;
            }
        }
    }

    fn party_leave(&mut self, cid: CharacterId) {
        let Some(pid) = self.party_of.get(&cid).copied() else {
            return;
        };
        self.project_world(worldlog::WorldEvent::PartyMemberSet {
            party_id: pid,
            member: cid,
            present: false,
        });
        let Some(p) = self.parties.get(&pid).cloned() else {
            return;
        };

        if p.members.is_empty() {
            self.project_world(worldlog::WorldEvent::PartyDisbanded { party_id: pid });
            return;
        }

        // If leader left, promote an arbitrary remaining member.
        if p.leader == cid {
            if let Some(&new_leader) = p.members.iter().next() {
                self.project_world(worldlog::WorldEvent::PartyLeaderSet {
                    party_id: pid,
                    leader: new_leader,
                });
            }
        }
    }

    fn party_disband(&mut self, pid: PartyId) {
        self.project_world(worldlog::WorldEvent::PartyDisbanded { party_id: pid });
    }

    fn party_invite_projected(
        &mut self,
        invitee: CharacterId,
        party_id: PartyId,
        inviter: CharacterId,
        expires_ms: u64,
    ) {
        self.project_world(worldlog::WorldEvent::PartyInviteSet {
            invitee,
            party_id,
            inviter,
            expires_ms,
            present: true,
        });
    }

    fn party_invite_remove_projected(&mut self, invitee: CharacterId) {
        let Some(inv) = self.party_invites.get(&invitee).cloned() else {
            return;
        };
        self.project_world(worldlog::WorldEvent::PartyInviteSet {
            invitee,
            party_id: inv.party_id,
            inviter: inv.inviter,
            expires_ms: inv.expires_ms,
            present: false,
        });
    }

    fn find_player_by_prefix(&self, token: &str) -> Option<CharacterId> {
        let t = token.trim().to_ascii_lowercase();
        if t.is_empty() {
            return None;
        }

        // Prefer an exact match to avoid ambiguity when multiple names share a prefix.
        let mut exact: Option<CharacterId> = None;
        for c in self.chars.values() {
            if c.controller.is_none() {
                continue;
            }
            let n = c.name.to_ascii_lowercase();
            if n == t {
                if exact.is_some() && exact != Some(c.id) {
                    return None; // ambiguous exact match
                }
                exact = Some(c.id);
            }
        }
        if exact.is_some() {
            return exact;
        }

        let mut found: Option<CharacterId> = None;
        for c in self.chars.values() {
            if c.controller.is_none() {
                continue;
            }
            let n = c.name.to_ascii_lowercase();
            if n.starts_with(&t) {
                if found.is_some() && found != Some(c.id) {
                    return None; // ambiguous prefix
                }
                found = Some(c.id);
            }
        }
        found
    }

    fn party_assist_attackers(&mut self, attacker_id: CharacterId, target_id: CharacterId) {
        let Some(att) = self.chars.get(&attacker_id).cloned() else {
            return;
        };
        let Some(pid) = self.party_of.get(&attacker_id).copied() else {
            return;
        };

        let Some(p) = self.parties.get(&pid).cloned() else {
            return;
        };
        for mid in p.members {
            if mid == attacker_id {
                continue;
            }
            let Some(m) = self.chars.get(&mid).cloned() else {
                continue;
            };
            if m.controller.is_none() {
                continue;
            }
            if !m.autoassist {
                continue;
            }
            if m.room_id != att.room_id {
                continue;
            }
            if m.combat.autoattack {
                continue;
            }
            self.start_combat(mid, target_id);
        }
    }

    fn load_protoadventure(&mut self, adventure_id: &str) -> anyhow::Result<String> {
        let adventure_id = adventure_id.trim();
        if adventure_id.is_empty() {
            anyhow::bail!("empty adventure_id");
        }
        if !adventure_id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            anyhow::bail!("invalid adventure_id (allowed: a-z0-9-_)");
        }

        let path = format!("protoadventures/{adventure_id}.md");
        let md = std::fs::read_to_string(&path).with_context(|| format!("read {path}"))?;
        let plan = protoadventure::parse_protoadventure_markdown(adventure_id, &md);
        let instance_prefix = format!("proto.{adventure_id}");

        let rooms = protoadventure::instantiate_rooms(&instance_prefix, &plan);
        if rooms.is_empty() {
            anyhow::bail!("no rooms parsed (missing or malformed '## Room Flow')");
        }

        let start_room = format!("{instance_prefix}.{}", plan.start_room);

        let evac_to = if self.rooms.has_room(ROOM_TOWN_GATE) {
            ROOM_TOWN_GATE.to_string()
        } else {
            self.rooms.start_room().to_string()
        };

        // Swap the instance content atomically-ish:
        // 1) parse/build (already done)
        // 2) evacuate occupants out of the old instance
        // 3) clear old rooms
        // 4) insert new rooms
        self.evacuate_rooms_with_prefix(&instance_prefix, &evac_to);
        for room_id in self.rooms.dyn_room_ids_with_prefix(&instance_prefix) {
            self.remove_room_projected(room_id, "proto-clear");
        }
        for (room_id, def) in rooms {
            self.set_room_projected(room_id, def, "proto-load");
        }

        Ok(start_room)
    }

    fn evacuate_rooms_with_prefix(&mut self, prefix: &str, to_room: &str) -> usize {
        let p = format!("{prefix}.");
        let room_ids = self
            .occupants
            .keys()
            .filter(|rid| rid.starts_with(&p))
            .cloned()
            .collect::<Vec<_>>();

        let to_room = to_room.to_string();
        let mut moved = 0usize;
        for rid in room_ids {
            let cids = self
                .occupants
                .get(&rid)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>();
            for cid in cids {
                if !self.chars.contains_key(&cid) {
                    continue;
                }
                self.move_char_projected(cid, to_room.clone(), "proto-evacuate");
                moved += 1;
            }
        }
        moved
    }
}

fn graveyard_room_id() -> &'static str {
    "R_TOWN_GRAVEYARD_01"
}

fn is_built(p: &Character) -> bool {
    p.race.is_some() && p.class.is_some()
}

fn starter_kit_for_size(size: items::Size) -> &'static [(&'static str, u32)] {
    // Keep this tiny and deterministic. It's a tutorial kit, not "real" loot.
    static SMALL: [(&str, u32); 5] = [
        ("practice sword (small)", 1),
        ("wooden buckler (small)", 1),
        ("training tunic (small)", 1),
        ("training boots (small)", 1),
        ("field bandage", 3),
    ];
    static MEDIUM: [(&str, u32); 5] = [
        ("practice sword (medium)", 1),
        ("wooden buckler (medium)", 1),
        ("training tunic (medium)", 1),
        ("training boots (medium)", 1),
        ("field bandage", 3),
    ];
    static LARGE: [(&str, u32); 5] = [
        ("practice sword (medium)", 1),
        ("wooden buckler (medium)", 1),
        ("training tunic (medium)", 1),
        ("training boots (medium)", 1),
        ("field bandage", 3),
    ];

    match size {
        items::Size::Small => &SMALL,
        items::Size::Medium => &MEDIUM,
        items::Size::Large => &LARGE,
    }
}

fn starter_kit_extras_for_world_log_format(
    active_world_log_format: u32,
) -> &'static [(&'static str, u32)] {
    static NONE: [(&str, u32); 0] = [];
    static FORMAT2: [(&str, u32); 1] = [("training charm", 1)];
    if active_world_log_format >= items::EquipSlot::Neck.min_world_log_format() {
        &FORMAT2
    } else {
        &NONE
    }
}

fn render_build_prompt(p: &Character) -> String {
    let mut s = String::new();
    if p.race.is_none() || p.class.is_none() {
        s.push_str("character setup:\r\n");
        if p.race.is_none() {
            s.push_str(" - choose race: `race <name>` (try: `race list`)\r\n");
        }
        if p.class.is_none() {
            s.push_str(" - choose class: `class <name>` (try: `class list`)\r\n");
        }
        s.push_str(" - see stats: `stats`\r\n");
    }
    s
}

fn assign_core_stats_for_class(class: Class) -> AbilityScores {
    // Standard array: 15,14,13,12,10,8 allocated by class priorities.
    let mut arr = vec![15, 14, 13, 12, 10, 8];
    let priority: &[Ability] = match class {
        Class::Barbarian => &[
            Ability::Str,
            Ability::Con,
            Ability::Dex,
            Ability::Wis,
            Ability::Cha,
            Ability::Int,
        ],
        Class::Bard => &[
            Ability::Cha,
            Ability::Dex,
            Ability::Con,
            Ability::Int,
            Ability::Wis,
            Ability::Str,
        ],
        Class::Cleric => &[
            Ability::Wis,
            Ability::Con,
            Ability::Str,
            Ability::Cha,
            Ability::Dex,
            Ability::Int,
        ],
        Class::Druid => &[
            Ability::Wis,
            Ability::Con,
            Ability::Dex,
            Ability::Int,
            Ability::Cha,
            Ability::Str,
        ],
        Class::Fighter => &[
            Ability::Str,
            Ability::Con,
            Ability::Dex,
            Ability::Wis,
            Ability::Cha,
            Ability::Int,
        ],
        Class::Monk => &[
            Ability::Dex,
            Ability::Wis,
            Ability::Con,
            Ability::Str,
            Ability::Int,
            Ability::Cha,
        ],
        Class::Paladin => &[
            Ability::Str,
            Ability::Cha,
            Ability::Con,
            Ability::Wis,
            Ability::Dex,
            Ability::Int,
        ],
        Class::Ranger => &[
            Ability::Dex,
            Ability::Wis,
            Ability::Con,
            Ability::Str,
            Ability::Int,
            Ability::Cha,
        ],
        Class::Rogue => &[
            Ability::Dex,
            Ability::Int,
            Ability::Con,
            Ability::Wis,
            Ability::Cha,
            Ability::Str,
        ],
        Class::Sorcerer => &[
            Ability::Cha,
            Ability::Con,
            Ability::Dex,
            Ability::Wis,
            Ability::Int,
            Ability::Str,
        ],
        Class::Warlock => &[
            Ability::Cha,
            Ability::Con,
            Ability::Dex,
            Ability::Wis,
            Ability::Int,
            Ability::Str,
        ],
        Class::Wizard => &[
            Ability::Int,
            Ability::Con,
            Ability::Dex,
            Ability::Wis,
            Ability::Cha,
            Ability::Str,
        ],
    };

    let mut scores = AbilityScores::baseline();
    for &a in priority {
        let v = arr.remove(0);
        scores.set(a, v);
    }
    scores
}

fn compute_max_hp(class: Class, scores: &AbilityScores) -> i32 {
    let hd = class.hit_die();
    let con = scores.mod_for(Ability::Con);
    (hd + con).max(1)
}

fn compute_max_mana(class: Class, scores: &AbilityScores, level: u32) -> i32 {
    // Keep it simple: casters have meaningful mana, martials have near-zero.
    let lvl = level.max(1) as i32;
    match class {
        Class::Wizard => (6 + 3 * lvl + 2 * scores.mod_for(Ability::Int)).max(0),
        Class::Sorcerer | Class::Warlock | Class::Bard => {
            (6 + 3 * lvl + 2 * scores.mod_for(Ability::Cha)).max(0)
        }
        Class::Cleric | Class::Druid => (6 + 3 * lvl + 2 * scores.mod_for(Ability::Wis)).max(0),
        Class::Paladin | Class::Ranger => (2 + 2 * lvl + scores.mod_for(Ability::Wis)).max(0),
        _ => 0,
    }
}

fn compute_max_stamina(class: Class, scores: &AbilityScores, level: u32) -> i32 {
    let lvl = level.max(1) as i32;
    let con = scores.mod_for(Ability::Con);
    match class {
        Class::Fighter | Class::Barbarian | Class::Monk | Class::Rogue => {
            (8 + 2 * lvl + con).max(0)
        }
        Class::Paladin | Class::Ranger => (6 + 2 * lvl + con).max(0),
        _ => (3 + lvl + con).max(0),
    }
}

fn render_stats(p: &Character) -> String {
    let mut s = String::new();
    s.push_str("stats:\r\n");
    s.push_str(&format!(" - level: {}\r\n", p.level));
    s.push_str(&format!(
        " - xp: {}/{} (to next)\r\n",
        p.xp,
        xp_needed_for_next(p.level)
    ));
    s.push_str(&format!(" - skill points: {}\r\n", p.skill_points));
    s.push_str(&format!(" - gold: {}g\r\n", p.gold));
    s.push_str(&format!(
        " - race: {}\r\n",
        p.race.map(|r| r.as_str()).unwrap_or("-")
    ));
    s.push_str(&format!(
        " - size: {}\r\n",
        p.race.map(|r| r.size().as_str()).unwrap_or("-")
    ));
    s.push_str(&format!(
        " - class: {}\r\n",
        p.class.map(|c| c.as_str()).unwrap_or("-")
    ));
    s.push_str(&format!(" - sex: {}\r\n", p.sex.as_str()));
    s.push_str(&format!(" - pronouns: {}\r\n", p.pronouns.as_str()));
    s.push_str(&format!(" - hp: {}/{}\r\n", p.hp, p.max_hp));
    s.push_str(&format!(" - mana: {}/{}\r\n", p.mana, p.max_mana));
    s.push_str(&format!(" - stamina: {}/{}\r\n", p.stamina, p.max_stamina));
    s.push_str(&format!(
        " - pvp: {}\r\n",
        if p.pvp_enabled { "on" } else { "off" }
    ));
    if p.stunned_until_ms > 0 {
        s.push_str(&format!(" - stunned_until_ms: {}\r\n", p.stunned_until_ms));
    }
    let av = equipped_armor_value(p);
    let ac = compute_ac(p);
    s.push_str(&format!(" - armor value: {av}\r\n"));
    s.push_str(&format!(" - ac: {ac}\r\n"));
    if let Some((a, b)) = equipped_weapon_damage_range(p) {
        s.push_str(&format!(" - weapon dmg: {a}..{b}\r\n"));
    } else {
        s.push_str(" - weapon dmg: (unarmed)\r\n");
    }
    s.push_str(&format!(
        " - STR {} ({:+})\r\n",
        p.stats.str_,
        p.stats.mod_for(Ability::Str)
    ));
    s.push_str(&format!(
        " - DEX {} ({:+})\r\n",
        p.stats.dex,
        p.stats.mod_for(Ability::Dex)
    ));
    s.push_str(&format!(
        " - CON {} ({:+})\r\n",
        p.stats.con,
        p.stats.mod_for(Ability::Con)
    ));
    s.push_str(&format!(
        " - INT {} ({:+})\r\n",
        p.stats.int_,
        p.stats.mod_for(Ability::Int)
    ));
    s.push_str(&format!(
        " - WIS {} ({:+})\r\n",
        p.stats.wis,
        p.stats.mod_for(Ability::Wis)
    ));
    s.push_str(&format!(
        " - CHA {} ({:+})\r\n",
        p.stats.cha,
        p.stats.mod_for(Ability::Cha)
    ));
    s
}

fn health_read(c: &Character) -> &'static str {
    if c.max_hp <= 0 {
        return "unknown";
    }
    let pct = (c.hp.max(0) as i64 * 100) / c.max_hp as i64;
    match pct {
        90.. => "fresh",
        60..=89 => "wounded",
        30..=59 => "hurt",
        1..=29 => "near death",
        _ => "down",
    }
}

fn threat_read(viewer: &Character, target: &Character) -> &'static str {
    let delta = target.level as i32 - viewer.level as i32;
    match delta {
        i32::MIN..=-4 => "trivial",
        -3..=-2 => "easy",
        -1 => "manageable",
        0 => "even match",
        1..=2 => "dangerous",
        3..=4 => "very dangerous",
        _ => "deadly",
    }
}

fn render_consider(viewer: &Character, target: &Character) -> String {
    let mut s = String::new();
    let kind = if target.controller.is_none() {
        "mob"
    } else if target.is_bot {
        "bot"
    } else {
        "player"
    };
    s.push_str(&format!("consider {}:\r\n", target.name));
    s.push_str(&format!(" - kind: {kind}\r\n"));
    s.push_str(&format!(" - level: {}\r\n", target.level));
    if let Some(class) = target.class {
        s.push_str(&format!(" - class: {}\r\n", class.as_str()));
    }
    s.push_str(&format!(
        " - health: {} ({}/{})\r\n",
        health_read(target),
        target.hp,
        target.max_hp
    ));
    s.push_str(&format!(" - ac: {}\r\n", compute_ac(target)));
    if let Some((a, b)) = equipped_weapon_damage_range(target) {
        s.push_str(&format!(" - weapon dmg: {a}..{b}\r\n"));
    } else {
        s.push_str(" - weapon dmg: (unarmed)\r\n");
    }
    if target.controller.is_some() {
        s.push_str(&format!(
            " - pvp: {}\r\n",
            if target.pvp_enabled { "on" } else { "off" }
        ));
    }
    s.push_str(&format!(" - read: {}\r\n", threat_read(viewer, target)));
    s
}

async fn write_resp_async(
    fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
    t: u8,
    session: SessionId,
    body: &[u8],
) -> std::io::Result<()> {
    let mut hdr = [0u8; 1 + SessionId::LEN];
    hdr[0] = t;
    hdr[1..].copy_from_slice(&session.to_be_bytes());
    fw.write_frame_parts(&[&hdr, body]).await
}

async fn write_blob_resp_async(
    fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
    session: SessionId,
    prefix: &[u8],
    path: &[u8],
    len: u64,
    suffix: &[u8],
) -> std::io::Result<()> {
    let prefix_len: u16 = prefix.len().try_into().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "output blob prefix too long",
        )
    })?;
    let path_len: u16 = path.len().try_into().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "output blob path too long")
    })?;
    let suffix_len: u16 = suffix.len().try_into().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "output blob suffix too long",
        )
    })?;

    let mut hdr = [0u8; 1 + SessionId::LEN];
    hdr[0] = RESP_OUTPUT_BLOB;
    hdr[1..].copy_from_slice(&session.to_be_bytes());
    let prefix_len = prefix_len.to_be_bytes();
    let path_len = path_len.to_be_bytes();
    let len = len.to_be_bytes();
    let suffix_len = suffix_len.to_be_bytes();
    fw.write_frame_parts(&[
        &hdr,
        &prefix_len,
        prefix,
        &path_len,
        path,
        &len,
        &suffix_len,
        suffix,
    ])
    .await
}

type SharedWorld = Arc<Mutex<World>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,shard_01=info".into()),
        )
        .with_target(false)
        .with_max_level(Level::INFO)
        .init();

    let cfg = parse_args();
    let wal_daemon = if cfg.wal_restore.is_some() || cfg.wal_backup.is_some() {
        Some(
            slopmud_walbackup::daemon::WalBackupDaemonHandle::spawn_for_wal_from_env(
                &cfg.raft_log_path,
                &cfg.raft_consensus.node_id,
            )
            .await?,
        )
    } else {
        None
    };
    if let Some(wal_restore) = cfg.wal_restore.clone() {
        let wal_daemon = wal_daemon.as_ref().ok_or_else(|| {
            anyhow::anyhow!("WAL restore requested but walbackupd is not running")
        })?;
        match wal_daemon.restore_wal().await? {
            slopmud_walbackup::WalRestoreOutcome::SkippedExisting { path, bytes } => {
                info!(path = %path.display(), bytes, "wal restore skipped; target already exists");
            }
            slopmud_walbackup::WalRestoreOutcome::NoManifest => {
                if wal_restore.missing_manifest_ok {
                    info!("wal restore skipped; no backup manifest was found");
                } else {
                    anyhow::bail!("WAL restore enabled but no backup manifest was found");
                }
            }
            slopmud_walbackup::WalRestoreOutcome::Restored {
                path,
                bytes,
                source_len,
                manifest_relpath,
            } => {
                let state_path =
                    PathBuf::from(format!("{}.state.json", cfg.raft_log_path.display()));
                match std::fs::remove_file(&state_path) {
                    Ok(()) => {
                        info!(path = %state_path.display(), "removed stale raft state after WAL restore");
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
                info!(
                    path = %path.display(),
                    bytes,
                    source_len,
                    manifest = %manifest_relpath,
                    "wal restore completed before shard startup"
                );
            }
        }
    }

    let listener = TcpListener::bind(cfg.bind).await?;
    info!(bind = %cfg.bind, "shard_01 listening");

    if let Some(wal_backup) = cfg.wal_backup.clone() {
        info!(
            dir = %wal_backup.local_dir.display(),
            node = %wal_backup.node_id,
            interval_s = wal_backup.interval.as_secs(),
            s3_bucket = wal_backup.s3_bucket.as_deref().unwrap_or("-"),
            "wal backup enabled"
        );
        let wal_daemon = wal_daemon
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WAL backup requested but walbackupd is not running"))?;
        wal_daemon.start_wal_backup().await?;
    }
    let _wal_daemon = wal_daemon;

    let rooms = rooms::Rooms::load()?;
    let world = Arc::new(Mutex::new(World::new(
        rooms,
        cfg.world_seed,
        cfg.bartender_emote_ms,
        cfg.mob_wander_ms,
        cfg.time_scale_ppm,
        cfg.raft_log_path.clone(),
        cfg.raft_consensus.clone(),
        cfg.bootstrap_admins.clone(),
        cfg.bootstrap_admin_sso.clone(),
    )?));

    {
        let mut world = world.lock().await;
        world.schedule_at_ms(0, EventKind::EnsureTavernMob);
        world.schedule_at_ms(0, EventKind::EnsureFirstFightWorm);
        world.schedule_at_ms(0, EventKind::EnsureClassHallMobs);
    }

    loop {
        let (stream, peer) = listener.accept().await?;
        info!(peer = %peer, "broker connected");

        if let Err(e) = handle_broker(stream, world.clone(), cfg.clone()).await {
            warn!(peer = %peer, err = %e, "broker connection ended with error");
        }
    }
}

async fn handle_broker(
    stream: TcpStream,
    shared_world: SharedWorld,
    cfg: Config,
) -> anyhow::Result<()> {
    let (rd, wr) = stream.into_split();
    let mut fr = FrameReader::new(rd);
    let mut fw = FrameWriter::new(wr);

    let mut world = shared_world.lock().await;
    world.replay_raft_tail();
    world.ensure_genesis_groups(&cfg.bootstrap_admins, &cfg.bootstrap_admin_sso)?;
    if !world.raft.accepts_client_writes() {
        return Ok(());
    }
    process_due_events(&mut world, &mut fw).await?;

    loop {
        if !world.raft.accepts_client_writes() {
            return Ok(());
        }
        world.advance_clock_from_wall();
        world.regen_resources();
        process_due_events(&mut world, &mut fw).await?;

        let sleep_ms = match world.events.peek() {
            Some(Reverse(ev)) => ev.due_ms.saturating_sub(world.now_ms()),
            None => u64::MAX,
        };

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(sleep_ms.min(250))) => {
                // Wake up to process due events and notice Raft leadership changes.
            }
            res = fr.read_frame() => {
                let frame = match res? {
                    Some(f) => f,
                    None => break,
                };
                world.replay_raft_tail();
                if !world.raft.accepts_client_writes() {
                    return Ok(());
                }
                let req = mudproto::shard::parse_req(frame)?;
        match req {
            ShardReq::Attach {
                session,
                is_bot,
                quiet,
                auth,
                race,
                class,
                sex,
                pronouns,
                name,
            } => {
                let name = String::from_utf8_lossy(&name).trim().to_string();
                if name.is_empty() {
                    let _ = write_resp_async(&mut fw, RESP_ERR, session, b"bad name\r\n").await;
                    continue;
                }
                let principal = principal_from_attach(&name, auth.as_deref());
                let auth_caps = caps_from_attach(auth.as_deref());

                // If the broker ever re-attaches the same session, drop prior characters first.
                let removed = world.detach_session(session);
                for c in removed {
                    let leave_msg = format!("* {} left", c.name);
                    let _ = world.broadcast_room(&mut fw, &c.room_id, &leave_msg).await;
                }

                let race_tok = race
                    .as_ref()
                    .map(|b| String::from_utf8_lossy(b).trim().to_ascii_lowercase());
                let class_tok = class
                    .as_ref()
                    .map(|b| String::from_utf8_lossy(b).trim().to_ascii_lowercase());
                let sex_tok = sex
                    .as_ref()
                    .map(|b| String::from_utf8_lossy(b).trim().to_ascii_lowercase());
                let pro_tok = pronouns
                    .as_ref()
                    .map(|b| String::from_utf8_lossy(b).trim().to_ascii_lowercase());

                let race = race_tok
                    .as_deref()
                    .and_then(Race::parse)
                    .unwrap_or(Race::Human);
                let class = class_tok
                    .as_deref()
                    .and_then(Class::parse)
                    .unwrap_or(Class::Fighter);
                let sex = sex_tok
                    .as_deref()
                    .and_then(Sex::parse)
                    .unwrap_or(Sex::None);
                let pronouns = pro_tok
                    .as_deref()
                    .and_then(PronounKey::parse)
                    .unwrap_or_else(|| PronounKey::default_for_sex(sex));

                let cid = world
                    .attach_existing_character(
                        session,
                        &name,
                        &principal,
                        auth_caps.clone(),
                        is_bot,
                        race,
                        class,
                        sex,
                        pronouns,
                    )
                    .unwrap_or_else(|| {
                        world.spawn_character(
                            session,
                            name.clone(),
                            principal,
                            auth_caps,
                            is_bot,
                            race,
                            class,
                            sex,
                            pronouns,
                        )
                    });
                let c = world
                    .chars
                    .get(&cid)
                    .expect("spawn_character inserts char");
                let room_id = c.room_id.clone();

                if !quiet {
                    let join_msg = format!("* {name} joined");
                    world.broadcast_room(&mut fw, &room_id, &join_msg).await?;

                    let mut hi = format!(
                        "hi {name}\r\n(type: help, rules, look, stats, kill <mob>, go <exit>, exit)\r\n"
                    );
                    let shown_race = c.race.unwrap_or(race);
                    let shown_class = c.class.unwrap_or(class);
                    hi.push_str(&format!(
                        "race: {} | class: {} | sex: {} | pronouns: {}\r\n",
                        shown_race.as_str(),
                        shown_class.as_str(),
                        c.sex.as_str(),
                        c.pronouns.as_str(),
                    ));
                    hi.push_str(&world.render_room_for(&room_id, session));
                    hi.push_str(&render_build_prompt(c));
                    write_resp_async(&mut fw, RESP_OUTPUT, session, hi.as_bytes()).await?;
                }
            }
            ShardReq::Detach { session } => {
                let removed = world.detach_session(session);
                for c in removed {
                    let leave_msg = format!("* {} left", c.name);
                    let _ = world.broadcast_room(&mut fw, &c.room_id, &leave_msg).await;
                }
            }
            ShardReq::InputBlob {
                session,
                command,
                path,
                len,
            } => {
                let Some(p) = world.active_char(session).cloned() else {
                    let _ = write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                    continue;
                };
                if command.as_ref() == b"say" {
                    let prefix = format!("{}: ", p.name);
                    world
                        .broadcast_room_blob(
                            &mut fw,
                            &p.room_id,
                            prefix.as_bytes(),
                            path.as_ref(),
                            len,
                            b"\r\n",
                        )
                        .await?;
                } else {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (unknown blob command)\r\n",
                    )
                    .await?;
                }
            }
            ShardReq::Input {
                session,
                command_id,
                line,
            } => {
                let line_s = String::from_utf8_lossy(&line);
                let line = line_s.trim();
                if line.is_empty() {
                    continue;
                }

                let lc = line.to_ascii_lowercase();

                let Some(p) = world.active_char(session).cloned() else {
                    let _ = write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                    continue;
                };
                if !world.remember_client_command(session, command_id, &p.principal) {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"ok: duplicate command ignored\r\n",
                    )
                    .await?;
                    continue;
                }

                // Session-level interactive confirmations (single-step prompts).
                if let Some(pending) = world
                    .sessions
                    .get_mut(&session)
                    .and_then(|ss| ss.pending_confirm.take())
                {
                    match pending {
                        PendingConfirm::BotOn { cid } => match lc.as_str() {
                            "yes" | "y" => {
                                let now_ms = world.now_ms();
                                let mut ok = false;
                                if let Some(c) = world.chars.get_mut(&cid) {
                                    if c.controller == Some(session) {
                                        if !c.is_bot {
                                            c.is_bot = true;
                                            c.bot_mode_changed_ms = now_ms;
                                        }
                                        if !c.bot_ever {
                                            c.bot_ever = true;
                                            c.bot_ever_since_ms = Some(now_ms);
                                        }
                                        ok = true;
                                    }
                                }
                                if ok {
                                    world.snapshot_character_projected(cid, "bot-confirm-on");
                                }
                                let msg = if ok {
                                    b"bot: on\r\n" as &[u8]
                                } else {
                                    b"bot: request expired\r\n"
                                };
                                write_resp_async(&mut fw, RESP_OUTPUT, session, msg).await?;
                                continue;
                            }
                            "no" | "n" | "cancel" => {
                                write_resp_async(
                                    &mut fw,
                                    RESP_OUTPUT,
                                    session,
                                    b"bot: cancelled\r\n",
                                )
                                .await?;
                                continue;
                            }
                            _ => {
                                if let Some(ss) = world.sessions.get_mut(&session) {
                                    ss.pending_confirm = Some(pending);
                                }
                                write_resp_async(
                                    &mut fw,
                                    RESP_OUTPUT,
                                    session,
                                    b"bot: type: yes | no\r\n",
                                )
                                .await?;
                                continue;
                            }
                        },
                    }
                }

                if lc == "help" {
                    write_resp_async(&mut fw, RESP_OUTPUT, session, help_text().as_bytes()).await?;
                    continue;
                }
                if lc == "areas" {
                    let s = world.rooms.render_areas();
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "rules" || lc == "coc" || lc == "code_of_conduct" {
                    write_resp_async(&mut fw, RESP_OUTPUT, session, rules_text().as_bytes())
                        .await?;
                    continue;
                }
                if is_buildinfo_command(&lc) {
                    let s = render_buildinfo();
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "aiping" || lc.starts_with("aiping ") || lc == "ai ping" || lc.starts_with("ai ping ")
                {
                    if !world.is_admin(&p) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"nope: admin.all\r\n")
                            .await?;
                        continue;
                    }

                    let suffix = if lc.starts_with("aiping") {
                        line.get("aiping".len()..).unwrap_or("")
                    } else {
                        line.get("ai ping".len()..).unwrap_or("")
                    };
                    let prompt = suffix.trim();
                    let prompt = if prompt.is_empty() {
                        "hey are you there? reply with exactly: pong"
                    } else {
                        prompt
                    };

                    let api_key = match std::env::var("OPENAI_API_KEY") {
                        Ok(v) if !v.trim().is_empty() => v,
                        _ => {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"aiping: missing OPENAI_API_KEY\r\n",
                            )
                            .await?;
                            continue;
                        }
                    };
                    let base = std::env::var("OPENAI_API_BASE")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| OPENAI_API_BASE_DEFAULT.to_string());
                    let model = std::env::var("OPENAI_PING_MODEL")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| OPENAI_PING_MODEL_DEFAULT.to_string());

                    let client = reqwest::Client::builder()
                        .timeout(Duration::from_secs(5))
                        .build()?;

                    let t0 = std::time::Instant::now();
                    let models = openai_ping_models(&client, &base, &api_key).await;
                    let chat = openai_ping_chat(&client, &base, &api_key, &model, prompt).await;
                    let ms = t0.elapsed().as_millis();

                    let mut s = String::new();
                    s.push_str("aiping:\r\n");
                    s.push_str(&format!(" - base: {}\r\n", base.trim_end_matches('/')));
                    s.push_str(&format!(" - model: {model}\r\n"));
                    match models {
                        Ok(n) => s.push_str(&format!(" - models: ok ({n})\r\n")),
                        Err(e) => s.push_str(&format!(" - models: err ({e})\r\n")),
                    }
                    match chat {
                        Ok(txt) => {
                            let one = txt.replace('\r', "").replace('\n', "\\n");
                            let out = if one.chars().count() > 200 {
                                one.chars().take(200).collect::<String>() + " [truncated]"
                            } else {
                                one
                            };
                            s.push_str(&format!(" - chat: ok ({out})\r\n"));
                        }
                        Err(e) => s.push_str(&format!(" - chat: err ({e})\r\n")),
                    }
                    s.push_str(&format!(" - ms: {ms}\r\n"));

                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "uptime" {
                    let s = world.render_uptime();
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "caps" || lc == "capabilities" || lc == "caps me" {
                    let mut caps = world
                        .effective_caps_for(&p)
                        .into_iter()
                        .map(|c| c.as_str().to_string())
                        .collect::<Vec<_>>();
                    caps.sort_unstable();
                    let mut s = String::new();
                    s.push_str("caps:\r\n");
                    if caps.is_empty() {
                        s.push_str(" - (none)\r\n");
                    } else {
                        for c in caps {
                            s.push_str(&format!(" - {c}\r\n"));
                        }
                    }
                    s.push_str("try: caps list\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "caps list" {
                    let mut s = String::new();
                    s.push_str("capabilities:\r\n");
                    for c in groups::Capability::ALL {
                        s.push_str(&format!(" - {}\r\n", c.as_str()));
                    }
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "groups" {
                    let me_lc = p.principal.to_ascii_lowercase();
                    let my_class = p.class.map(|c| c.as_str()).unwrap_or("");
                    let mut rows = Vec::new();
                    for g in world.groups.groups.values() {
                        let mut role = g.members.get(&me_lc).copied();
                        if role.is_none() {
                            role = g.implied_role_for_class(my_class);
                        }
                        if let Some(r) = role {
                            rows.push(format!(
                                " - {}: {} ({})\r\n",
                                g.id,
                                g.name,
                                r.as_str()
                            ));
                        }
                    }
                    rows.sort_unstable();
                    let mut s = String::new();
                    s.push_str("groups:\r\n");
                    if rows.is_empty() {
                        s.push_str(" - (none)\r\n");
                    } else {
                        for r in rows {
                            s.push_str(&r);
                        }
                    }
                    s.push_str("try: group list\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "group" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: group list | group show <id|name> | group create <kind> <name> | group add <group> <player> [role] | group remove <group> <player> | group role <group> <player> <role> | group policy set <group> <key> <value> | group policy del <group> <key> | group rolecaps set <group> <role> <cap...>)\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc == "group list" {
                    let mut rows = world
                        .groups
                        .groups
                        .values()
                        .map(|g| format!(" - {}: {} ({})\r\n", g.id, g.name, g.kind.as_str()))
                        .collect::<Vec<_>>();
                    rows.sort_unstable();
                    let mut s = String::new();
                    s.push_str("groups:\r\n");
                    for r in rows {
                        s.push_str(&r);
                    }
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("group show ") {
                    let token = rest.trim();
                    let Some(gid) = world.resolve_group_id(token) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group show <id|name>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let s = world.render_group(gid);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = line.strip_prefix("group create ") {
                    if !world.has_cap(&p, groups::Capability::GroupCreate) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"nope: group.create\r\n")
                            .await?;
                        continue;
                    }
                    let mut it = rest.trim().split_whitespace();
                    let Some(kind_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group create <kind> <name>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(name_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group create <kind> <name>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(kind) = groups::GroupKind::parse(kind_tok) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (kind: admin | guild | custom | class:<class>)\r\n",
                        )
                        .await?;
                        continue;
                    };

                    let name = name_tok.trim().to_string();
                    if name.is_empty() {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (bad name)\r\n")
                            .await?;
                        continue;
                    }
                    if world
                        .groups
                        .group_ids_by_name
                        .contains_key(&name.to_ascii_lowercase())
                    {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"group: name already exists\r\n",
                        )
                        .await?;
                        continue;
                    }

                    // Avoid reserved IDs.
                    let max_id = world
                        .groups
                        .groups
                        .keys()
                        .copied()
                        .max()
                        .unwrap_or(1999);
                    let group_id = (max_id + 1).max(2000);

                    let env = world.raft_append_group(groups::GroupLogEntry::GroupCreate {
                        group_id,
                        kind: kind.clone(),
                        name: name.clone(),
                    })?;
                    let _ = world
                        .broadcast_raft(
                            &mut fw,
                            &format!("raft[{}] {}", env.index, serde_json::to_string(&env)?),
                        )
                        .await;

                    // For non-class groups, default the creator to owner.
                    if !matches!(kind, groups::GroupKind::Class { .. }) {
                        let env2 =
                            world.raft_append_group(groups::GroupLogEntry::GroupMemberSet {
                                group_id,
                                member: p.name.clone(),
                                role: Some(groups::GroupRole::Owner),
                            })?;
                        let _ = world
                            .broadcast_raft(
                                &mut fw,
                                &format!(
                                    "raft[{}] {}",
                                    env2.index,
                                    serde_json::to_string(&env2)?
                                ),
                            )
                            .await;
                    }

                    let msg = format!("group: created {group_id} ({name})\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = line.strip_prefix("group add ") {
                    let mut it = rest.trim().split_whitespace();
                    let Some(group_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group add <group> <player> [role])\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(member_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group add <group> <player> [role])\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(gid) = world.resolve_group_id(group_tok) else {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (bad group)\r\n")
                            .await?;
                        continue;
                    };
                    if !world.has_group_cap(&p, gid, groups::Capability::GroupMemberAdd) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"nope: group.member.add\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let role = it
                        .next()
                        .and_then(groups::GroupRole::parse)
                        .unwrap_or(groups::GroupRole::Member);
                    let member = normalize_principal_token(member_tok);
                    let env = world.raft_append_group(groups::GroupLogEntry::GroupMemberSet {
                        group_id: gid,
                        member,
                        role: Some(role),
                    })?;
                    let _ = world
                        .broadcast_raft(
                            &mut fw,
                            &format!("raft[{}] {}", env.index, serde_json::to_string(&env)?),
                        )
                        .await;
                    let msg = format!("group: {gid} add {member_tok} ({})\r\n", role.as_str());
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = line.strip_prefix("group remove ") {
                    let mut it = rest.trim().split_whitespace();
                    let Some(group_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group remove <group> <player>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(member_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group remove <group> <player>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(gid) = world.resolve_group_id(group_tok) else {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (bad group)\r\n")
                            .await?;
                        continue;
                    };
                    if !world.has_group_cap(&p, gid, groups::Capability::GroupMemberRemove) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"nope: group.member.remove\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let member = normalize_principal_token(member_tok);
                    let env = world.raft_append_group(groups::GroupLogEntry::GroupMemberSet {
                        group_id: gid,
                        member,
                        role: None,
                    })?;
                    let _ = world
                        .broadcast_raft(
                            &mut fw,
                            &format!("raft[{}] {}", env.index, serde_json::to_string(&env)?),
                        )
                        .await;
                    let msg = format!("group: {gid} remove {member_tok}\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = line.strip_prefix("group role ") {
                    let mut it = rest.trim().split_whitespace();
                    let Some(group_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group role <group> <player> <role>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(member_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group role <group> <player> <role>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(role_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group role <group> <player> <role>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(gid) = world.resolve_group_id(group_tok) else {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (bad group)\r\n")
                            .await?;
                        continue;
                    };
                    if !world.has_group_cap(&p, gid, groups::Capability::GroupRoleSet) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"nope: group.role.set\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(role) = groups::GroupRole::parse(role_tok) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (role: owner | officer | member | guest)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let member = normalize_principal_token(member_tok);
                    let env = world.raft_append_group(groups::GroupLogEntry::GroupMemberSet {
                        group_id: gid,
                        member,
                        role: Some(role),
                    })?;
                    let _ = world
                        .broadcast_raft(
                            &mut fw,
                            &format!("raft[{}] {}", env.index, serde_json::to_string(&env)?),
                        )
                        .await;
                    let msg = format!("group: {gid} role {member_tok} {}\r\n", role.as_str());
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = line.strip_prefix("group policy set ") {
                    let mut it = rest.trim().split_whitespace();
                    let Some(group_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group policy set <group> <key> <value>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(key_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group policy set <group> <key> <value>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(value_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group policy set <group> <key> <value>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(gid) = world.resolve_group_id(group_tok) else {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (bad group)\r\n")
                            .await?;
                        continue;
                    };
                    if !world.has_group_cap(&p, gid, groups::Capability::GroupPolicySet) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"nope: group.policy.set\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let env = world.raft_append_group(groups::GroupLogEntry::GroupPolicySet {
                        group_id: gid,
                        key: key_tok.to_string(),
                        value: Some(value_tok.to_string()),
                    })?;
                    let _ = world
                        .broadcast_raft(
                            &mut fw,
                            &format!("raft[{}] {}", env.index, serde_json::to_string(&env)?),
                        )
                        .await;
                    let msg = format!("group: {gid} policy set {key_tok}={value_tok}\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = line.strip_prefix("group policy del ") {
                    let mut it = rest.trim().split_whitespace();
                    let Some(group_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group policy del <group> <key>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(key_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group policy del <group> <key>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(gid) = world.resolve_group_id(group_tok) else {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (bad group)\r\n")
                            .await?;
                        continue;
                    };
                    if !world.has_group_cap(&p, gid, groups::Capability::GroupPolicySet) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"nope: group.policy.set\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let env = world.raft_append_group(groups::GroupLogEntry::GroupPolicySet {
                        group_id: gid,
                        key: key_tok.to_string(),
                        value: None,
                    })?;
                    let _ = world
                        .broadcast_raft(
                            &mut fw,
                            &format!("raft[{}] {}", env.index, serde_json::to_string(&env)?),
                        )
                        .await;
                    let msg = format!("group: {gid} policy del {key_tok}\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = line.strip_prefix("group rolecaps set ") {
                    let mut it = rest.trim().split_whitespace();
                    let Some(group_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group rolecaps set <group> <role> <cap...>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(role_tok) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: group rolecaps set <group> <role> <cap...>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(gid) = world.resolve_group_id(group_tok) else {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (bad group)\r\n")
                            .await?;
                        continue;
                    };
                    if !world.has_group_cap(&p, gid, groups::Capability::GroupRoleCapsSet) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"nope: group.rolecaps.set\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(role) = groups::GroupRole::parse(role_tok) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (role: owner | officer | member | guest)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let mut caps = Vec::new();
                    for tok in it {
                        let Some(c) = groups::Capability::parse(tok) else {
                            let msg = format!("huh? (bad capability: {tok})\r\n");
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                            caps.clear();
                            break;
                        };
                        caps.push(c);
                    }
                    if caps.is_empty() {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (try: caps list)\r\n")
                            .await?;
                        continue;
                    }
                    let env = world.raft_append_group(groups::GroupLogEntry::GroupRoleCapsSet {
                        group_id: gid,
                        role,
                        caps: caps.clone(),
                    })?;
                    let _ = world
                        .broadcast_raft(
                            &mut fw,
                            &format!("raft[{}] {}", env.index, serde_json::to_string(&env)?),
                        )
                        .await;
                    let msg = format!(
                        "group: {gid} rolecaps set {} {}\r\n",
                        role.as_str(),
                        caps.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(" ")
                    );
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if lc == "raft" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: raft status | raft feature worldlog <n> [check] | raft metadata set <key> <value> | raft tail [n] | raft watch on|off)\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc == "raft status" {
                    if !world.is_admin(&p) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"nope: admin.all\r\n")
                            .await?;
                        continue;
                    }
                    let mut s = String::new();
                    s.push_str("raft:\r\n");
                    for line in world.raft.status_lines() {
                        s.push_str(&line);
                        s.push_str("\r\n");
                    }
                    for line in world.cluster_feature_status_lines() {
                        s.push_str(&line);
                        s.push_str("\r\n");
                    }
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                let words = lc.split_whitespace().collect::<Vec<_>>();
                if words.first() == Some(&"raft")
                    && words.get(1) == Some(&"feature")
                    && words.get(2) == Some(&"worldlog")
                {
                    if !world.is_admin(&p) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"nope: admin.all\r\n")
                            .await?;
                        continue;
                    }
                    if !(words.len() == 4 || (words.len() == 5 && words.get(4) == Some(&"check")))
                    {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: raft feature worldlog <n> [check])\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let dry_run = words.get(4) == Some(&"check");
                    let Some(target) = words[3].parse::<u32>().ok().filter(|n| *n > 0) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: raft feature worldlog <n> [check])\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let active = world.cluster_features.active_world_log_format;
                    if target < active {
                        let msg =
                            format!("raft feature: downgrade blocked ({active} -> {target})\r\n");
                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        continue;
                    }
                    if target == active {
                        let prefix = if dry_run {
                            "raft feature check"
                        } else {
                            "raft feature"
                        };
                        let msg = format!("{prefix}: worldlog format {target} already active\r\n");
                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        continue;
                    }
                    if target > worldlog::WORLD_LOG_MAX_SUPPORTED_FORMAT {
                        let msg = format!(
                            "raft feature: this binary supports worldlog format <= {}\r\n",
                            worldlog::WORLD_LOG_MAX_SUPPORTED_FORMAT
                        );
                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        continue;
                    }

                    let voter_infos = world.world_log_format_voters(Duration::from_millis(500));
                    let voters = voter_infos
                        .iter()
                        .map(|v| worldlog::ClusterFormatVoter {
                            node_id: v.node_id.as_str(),
                            max_world_log_format: v.max_format,
                        })
                        .collect::<Vec<_>>();
                    match worldlog::evaluate_world_log_format_activation(target, &voters) {
                        worldlog::ClusterFormatActivation::Allowed => {
                            if dry_run {
                                let msg = format!(
                                    "raft feature check: worldlog format {target} would activate\r\n"
                                );
                                write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes())
                                    .await?;
                                continue;
                            }
                            let env = match world.raft_append_cluster_feature_activation(
                                worldlog::WorldEvent::ClusterFeatureSet {
                                    active_world_log_format: target,
                                    snapshot_format: target,
                                    min_reader_world_log_format: target,
                                    reason: "admin-activate-worldlog".to_string(),
                                },
                            ) {
                                Ok(env) => env,
                                Err(err) => {
                                    let msg = format!("raft feature: not activated: {err}\r\n");
                                    write_resp_async(
                                        &mut fw,
                                        RESP_OUTPUT,
                                        session,
                                        msg.as_bytes(),
                                    )
                                    .await?;
                                    continue;
                                }
                            };
                            let _ = world
                                .broadcast_raft(
                                    &mut fw,
                                    &format!(
                                        "raft[{}] {}",
                                        env.index,
                                        serde_json::to_string(&env)?
                                    ),
                                )
                                .await;
                            let msg =
                                format!("raft feature: worldlog format {target} active\r\n");
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        }
                        worldlog::ClusterFormatActivation::Rejected {
                            unsupported_voters,
                            ..
                        } => {
                            let unsupported = unsupported_voters
                                .iter()
                                .map(|id| {
                                    voter_infos
                                        .iter()
                                        .find(|v| v.node_id == *id)
                                        .map(|v| match &v.error {
                                            Some(err) if !v.reachable => {
                                                format!("{}(unreachable:{err})", v.node_id)
                                            }
                                            _ => format!("{}(max={})", v.node_id, v.max_format),
                                        })
                                        .unwrap_or_else(|| (*id).to_string())
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            let msg = format!(
                                "raft feature{}: worldlog format {target} blocked; unsupported voters: {unsupported}\r\n",
                                if dry_run { " check" } else { "" },
                            );
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        }
                    }
                    continue;
                }
                if words.first() == Some(&"raft") && words.get(1) == Some(&"metadata") {
                    if !world.is_admin(&p) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"nope: admin.all\r\n")
                            .await?;
                        continue;
                    }
                    let Some(action) = words.get(2).copied() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: raft metadata set <key> <value> | raft metadata del <key>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(key) = words.get(3).copied() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: raft metadata set <key> <value> | raft metadata del <key>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let key_ok = !key.is_empty()
                        && key.len() <= 64
                        && key
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
                    if !key_ok {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"raft metadata: bad key\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let value = match action {
                        "set" if words.len() >= 5 => Some(words[4..].join(" ")),
                        "del" if words.len() == 4 => None,
                        _ => {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (try: raft metadata set <key> <value> | raft metadata del <key>)\r\n",
                            )
                            .await?;
                            continue;
                        }
                    };
                    if value.as_ref().is_some_and(|v| v.len() > 256) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"raft metadata: value too long\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let event = worldlog::WorldEvent::ClusterMetadataSet {
                        key: key.to_string(),
                        value: value.clone(),
                        reason: format!("admin-metadata-{action}"),
                    };
                    let env = match world.raft_append_world(event) {
                        Ok(env) => env,
                        Err(err) => {
                            let msg = format!("raft metadata: not {action}: {err}\r\n");
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes())
                                .await?;
                            continue;
                        }
                    };
                    let _ = world
                        .broadcast_raft(
                            &mut fw,
                            &format!("raft[{}] {}", env.index, serde_json::to_string(&env)?),
                        )
                        .await;
                    let msg = match value {
                        Some(value) => format!("raft metadata: set {key}={value}\r\n"),
                        None => format!("raft metadata: deleted {key}\r\n"),
                    };
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("raft tail") {
                    if !world.has_cap(&p, groups::Capability::RaftTail) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"nope: raft.tail\r\n")
                            .await?;
                        continue;
                    }
                    let n = rest
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .unwrap_or(20)
                        .clamp(1, 200);
                    let mut s = String::new();
                    s.push_str(&format!(
                        "raft:\r\n - path: {}\r\n - next_index: {}\r\n",
                        world.raft.path().display(),
                        world.raft.next_index()
                    ));
                    for line in world.raft.recent_lines(n) {
                        s.push_str(&line);
                        s.push_str("\r\n");
                    }
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("raft watch ") {
                    if !world.has_cap(&p, groups::Capability::RaftWatch) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"nope: raft.watch\r\n")
                            .await?;
                        continue;
                    }
                    match rest.trim() {
                        "on" => {
                            world.raft_watch.insert(p.id);
                            write_resp_async(&mut fw, RESP_OUTPUT, session, b"raft: watch on\r\n")
                                .await?;
                        }
                        "off" => {
                            world.raft_watch.remove(&p.id);
                            write_resp_async(&mut fw, RESP_OUTPUT, session, b"raft: watch off\r\n")
                                .await?;
                        }
                        _ => {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (try: raft watch on|off)\r\n",
                            )
                            .await?;
                        }
                    }
                    continue;
                }
                if lc == "where" || lc == "room" {
                    let s = format!("room: {}\r\n", p.room_id);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "warp" {
                    if !world.has_cap(&p, groups::Capability::WorldWarp) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"nope: world.warp\r\n")
                            .await?;
                        continue;
                    }
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: warp <room_id>)\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc.starts_with("warp ") {
                    if !world.has_cap(&p, groups::Capability::WorldWarp) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"nope: world.warp\r\n")
                            .await?;
                        continue;
                    }
                    let to = line[5..].trim();
                    if to.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: warp <room_id>)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    teleport_to(&mut world, &mut fw, session, to, "warps").await?;
                    continue;
                }

                if lc == "spawn" {
                    if !world.has_cap(&p, groups::Capability::WorldWarp) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"nope: world.spawn\r\n")
                            .await?;
                        continue;
                    }
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: spawn rat 3 | spawn grease_king 1)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("spawn ") {
                    if !world.has_cap(&p, groups::Capability::WorldWarp) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"nope: world.spawn\r\n")
                            .await?;
                        continue;
                    }
                    let mut it = rest.split_whitespace();
                    let Some(kind) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: spawn rat 3 | spawn grease_king 1)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let n: u32 = it
                        .next()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(1)
                        .clamp(1, 20);

                    let room_id = p.room_id.clone();
                    let mut spawned = 0u32;
                    for _ in 0..n {
                        if world.spawn_named_mob(room_id.clone(), kind).is_some() {
                            spawned += 1;
                        }
                    }
                    let msg = format!("spawned: {} x {}\r\n", spawned, kind);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    process_due_events(&mut world, &mut fw).await?;
                    continue;
                }
                if lc == "stats" || lc == "score" {
                    let s = world
                        .chars
                        .get(&p.id)
                        .map(render_stats)
                        .unwrap_or_else(|| "huh?\r\n".to_string());
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "consider" || lc == "con" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: consider <mob|player>)\r\n",
                    )
                    .await?;
                    continue;
                }
                let consider_target = if lc.starts_with("consider ") {
                    Some(line["consider ".len()..].trim())
                } else if lc.starts_with("con ") {
                    Some(line["con ".len()..].trim())
                } else {
                    None
                };
                if let Some(target) = consider_target {
                    if target.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: consider <mob|player>)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(target_id) = world
                        .find_mob_in_room(&p.room_id, target)
                        .or_else(|| world.find_player_in_room(&p.room_id, target))
                    else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you don't see that here)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let s = world
                        .chars
                        .get(&target_id)
                        .map(|target| render_consider(&p, target))
                        .unwrap_or_else(|| "huh?\r\n".to_string());
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "look" || lc == "l" {
                    let s = world.render_room_for(&p.room_id, session);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "exits" || lc == "exit list" {
                    let s = world.rooms.render_exits(&p.room_id);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc.starts_with("look ") {
                    let target = line[5..].trim();
                    if let Some(desc) = tavern_object(&p.room_id, target) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, desc.as_bytes()).await?;
                    } else if let Some(desc) = job_board_object(&p, target) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, desc.as_bytes()).await?;
                    } else if let Some(desc) = sewers_object(&p, target) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, desc.as_bytes()).await?;
                    } else if let Some(desc) = heroes_object(&p.room_id, target) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, desc.as_bytes()).await?;
                    } else if let Some(desc) = academy_object(&p.room_id, target) {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, desc.as_bytes()).await?;
                    } else {
                        // If it's not a room object, fall back to inventory/equipment.
                        let mut found: Option<String> = None;
                        let mut ambiguous: Option<String> = None;
                        if let Some(cid) = world.active_char_id(session) {
                            if let Some(pc) = world.chars.get(&cid) {
                                match find_item_key_in_inventory(&pc.inv, target) {
                                    ItemKeyMatch::One(k) => {
                                        if let Some(def) = items::find_item_def(&k) {
                                            found = Some(render_item_details(def));
                                        } else if let Some(drink) = find_drink(&k) {
                                            found = Some(format!(
                                                "{}\r\ntype: drink\r\nquaff: increases drinking level by 1\r\n",
                                                drink.name
                                            ));
                                        } else {
                                            found = Some(format!("{k}\r\n"));
                                        }
                                    }
                                    ItemKeyMatch::Ambiguous(xs) => {
                                        ambiguous = Some(format!(
                                            "huh? (ambiguous; try one of: {})\r\n",
                                            xs.join(", ")
                                        ));
                                    }
                                    ItemKeyMatch::None => {}
                                }

                                if found.is_none() && ambiguous.is_none() {
                                    if let Some(slot) = items::EquipSlot::parse(target) {
                                        if let Some(name) = pc.equip.get(slot) {
                                            if let Some(def) = items::find_item_def(name) {
                                                found = Some(render_item_details(def));
                                            } else {
                                                found = Some(format!("{name}\r\n"));
                                            }
                                        }
                                    }
                                }

                                if found.is_none() && ambiguous.is_none() {
                                    match find_equipped_slot_by_token(&pc.equip, target) {
                                        SlotMatch::One(slot) => {
                                            if let Some(name) = pc.equip.get(slot) {
                                                if let Some(def) = items::find_item_def(name) {
                                                    found = Some(render_item_details(def));
                                                } else {
                                                    found = Some(format!("{name}\r\n"));
                                                }
                                            }
                                        }
                                        SlotMatch::Ambiguous(_) => {
                                            ambiguous = Some(
                                                "huh? (ambiguous; try looking by slot: look body)\r\n"
                                                    .to_string(),
                                            );
                                        }
                                        SlotMatch::None => {}
                                    }
                                }
                            }
                        }

                        if let Some(msg) = found {
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        } else if let Some(msg) = ambiguous {
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        } else {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (nothing like that to look at here)\r\n",
                            )
                            .await?;
                        }
                    }
                        continue;
                    }

                    if lc == "faction" {
                        let cur = p
                            .quest
                            .get("q.q2_job_board.faction")
                            .map(|v| v.trim())
                            .filter(|v| !v.is_empty())
                            .unwrap_or("unset");
                        let msg = format!(
                            "faction: {cur}\r\ntry: faction civic|industrial|green (at the job board)\r\n"
                        );
                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        continue;
                    }
                    if let Some(rest) = lc.strip_prefix("faction ") {
                        if p.room_id != ROOM_TOWN_JOB_BOARD {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (try this at the job board; go to the square and: look board)\r\n",
                            )
                            .await?;
                            continue;
                        }
                        let token = rest.trim();
                        if token.is_empty() {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (try: faction civic|industrial|green)\r\n",
                            )
                            .await?;
                            continue;
                        }
                        let contracts_done = p
                            .quest
                            .get("q.q2_job_board.contracts_done")
                            .and_then(|v| v.trim().parse::<i64>().ok())
                            .unwrap_or(0)
                            .clamp(0, 3);
                        if contracts_done < 3 {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"board clerk: finish the 3 starter contracts first.\r\n(try: look board)\r\n",
                            )
                            .await?;
                            continue;
                        }
                        let already = p
                            .quest
                            .get("q.q2_job_board.faction")
                            .map(|v| v.trim())
                            .is_some_and(|v| !v.is_empty() && v != "unset");
                        if already {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"board clerk: contact already chosen.\r\n",
                            )
                            .await?;
                            continue;
                        }

                        let faction = match token {
                            "civic" | "industrial" | "green" => token,
                            _ => {
                                write_resp_async(
                                    &mut fw,
                                    RESP_OUTPUT,
                                    session,
                                    b"huh? (try: faction civic|industrial|green)\r\n",
                                )
                                .await?;
                                continue;
                            }
                        };

                        if let Some(c) = world.active_char_mut(session) {
                            c.quest
                                .insert("q.q2_job_board.faction".to_string(), faction.to_string());
                            c.quest.insert(
                                "q.q2_job_board.repeatables_unlocked".to_string(),
                                "1".to_string(),
                            );
                            c.quest
                                .insert("gate.job_board.repeatables".to_string(), "1".to_string());
                            if faction == "civic" || faction == "industrial" {
                                c.quest.insert("gate.sewers.entry".to_string(), "1".to_string());
                            }
                            if faction == "industrial" || faction == "green" {
                                c.quest.insert("gate.quarry.entry".to_string(), "1".to_string());
                            }
                            c.quest.insert(
                                "q.q2_job_board.state".to_string(),
                                "repeatables".to_string(),
                            );
                        }
                        world.snapshot_character_projected(p.id, "job-board-faction");

                        let msg = format!(
                            "board clerk: stamped. contact set to {faction}.\r\n(sewers access may now be unsealed.)\r\n"
                        );
                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        let room_msg = format!("* {} signs the job board ledger.", p.name);
                        let _ = world.broadcast_room(&mut fw, &p.room_id, &room_msg).await;
                        continue;
                    }

                    if lc == "race" {
                        let cur = world
                            .active_char(session)
                            .and_then(|c| c.race)
                        .map(|r| r.as_str())
                        .unwrap_or("(none)");
                    let msg = format!("race: {cur}\r\ntry: race list | race human\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if lc == "race list" {
                    let mut s = String::new();
                    s.push_str("races:\r\n");
                    for r in Race::all() {
                        s.push_str(" - ");
                        s.push_str(r.as_str());
                        s.push_str("\r\n");
                    }
                    s.push_str("choose with: race <name>\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("race ") {
                    if p.room_id != ROOM_SCHOOL_ORIENTATION {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (find a trainer; try this in the academy)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let token = rest.trim();
                    let Some(race) = Race::parse(token) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: race list)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(cid) = world.active_char_id(session) else {
                        let _ =
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                        continue;
                    };
                    let already = world.chars.get(&cid).and_then(|c| c.race).is_some();
                    if already {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (race already chosen)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if let Some(pc) = world.chars.get_mut(&cid) {
                        pc.race = Some(race);
                    }
                    // Give a tiny starter kit sized to the character.
                    let kit = starter_kit_for_size(race.size());
                    for (item, n) in kit {
                        world.inv_add(cid, item, *n);
                    }
                    let extras = starter_kit_extras_for_world_log_format(
                        world.cluster_features.active_world_log_format,
                    );
                    for (item, n) in extras {
                        world.inv_add(cid, item, *n);
                    }
                    world.snapshot_character_projected(cid, "race-choice");
                    let msg = format!(
                        "trainer: {} it is. starter kit issued.\r\n(try: i, equip, wear tunic, wield sword)\r\n",
                        race.as_str()
                    );
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }

                if lc == "class" {
                    let cur = world
                        .active_char(session)
                        .and_then(|c| c.class)
                        .map(|c| c.as_str())
                        .unwrap_or("(none)");
                    let msg = format!("class: {cur}\r\ntry: class list | class fighter\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if lc == "class list" {
                    let mut s = String::new();
                    s.push_str("classes:\r\n");
                    for c in Class::all() {
                        s.push_str(" - ");
                        s.push_str(c.as_str());
                        s.push_str("\r\n");
                    }
                    s.push_str("choose with: class <name>\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("class ") {
                    if !is_trainer_room(&p.room_id) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (find a trainer; try this in the academy or a class hall)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let token = rest.trim();
                    let Some(class) = Class::parse(token) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: class list)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(cid) = world.active_char_id(session) else {
                        let _ =
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                        continue;
                    };
                    let already = world.chars.get(&cid).and_then(|c| c.class).is_some();
                    if already {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (class already chosen)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let now = world.now_ms();
                    if let Some(pc) = world.chars.get_mut(&cid) {
                        pc.class = Some(class);
                        pc.stats = assign_core_stats_for_class(class);
                        pc.max_hp = compute_max_hp(class, &pc.stats).max(1);
                        pc.hp = pc.max_hp;
                        pc.max_mana = compute_max_mana(class, &pc.stats, pc.level).max(0);
                        pc.mana = pc.max_mana;
                        pc.max_stamina = compute_max_stamina(class, &pc.stats, pc.level).max(0);
                        pc.stamina = pc.max_stamina;
                        pc.last_mana_regen_ms = now;
                        pc.last_stamina_regen_ms = now;
                    }
                    world.snapshot_character_projected(cid, "class-choice");
                    let msg = format!("trainer: welcome, {}.\r\n", class.as_str());
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }

                if lc == "train" || lc == "train list" {
                    if !is_trainer_room(&p.room_id) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (find a trainer; try this in the academy or a class hall)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(pc) = world.active_char(session) else {
                        let _ =
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                        continue;
                    };
                    let Some(class) = pc.class else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"trainer: pick a class first. try: class fighter\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let mut s = String::new();
                    s.push_str(&format!(
                        "trainer: skills for {} (skill points: {})\r\n",
                        class.as_str(),
                        pc.skill_points
                    ));
                    let defs = skills_for_class(class);
                    if defs.is_empty() {
                        s.push_str(" - (none yet)\r\n");
                    } else {
                        for d in defs {
                            let r = pc.skills.get(d.id).copied().unwrap_or(0);
                            s.push_str(&format!(" - {} ({}) (rank {})\r\n", d.id, d.display, r));
                        }
                        s.push_str("train with: train <skill>\r\n");
                    }
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("train ") {
                    if !is_trainer_room(&p.room_id) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (find a trainer; try this in the academy or a class hall)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let token = rest.trim();
                    if token.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: train list)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(cid) = world.active_char_id(session) else {
                        let _ =
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                        continue;
                    };
                    let class = world.chars.get(&cid).and_then(|c| c.class);
                    let Some(class) = class else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"trainer: pick a class first. try: class fighter\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(def) = find_skill_for_class(class, token) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"trainer: i can't teach that. try: train list\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let (sp, rank) = match world.chars.get(&cid) {
                        Some(pc) => (
                            pc.skill_points,
                            pc.skills.get(def.id).copied().unwrap_or(0),
                        ),
                        None => (0, 0),
                    };
                    let max_rank = 5u32;
                    if rank >= max_rank {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"trainer: you've mastered that already.\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if sp == 0 {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"trainer: you need a skill point. go fight.\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if let Some(pc) = world.chars.get_mut(&cid) {
                        pc.skill_points -= 1;
                        pc.skills.insert(def.id.to_string(), rank.saturating_add(1).max(1));
                    }
                    world.snapshot_character_projected(cid, "train-skill");
                    let msg = format!(
                        "trainer: trained {} (rank {}).\r\n",
                        def.id,
                        rank.saturating_add(1).max(1)
                    );
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }

                if lc == "skills" || lc == "skills list" {
                    let Some(pc) = world.active_char(session) else {
                        let _ =
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                        continue;
                    };
                    let mut s = String::new();
                    s.push_str(&format!(
                        "skills (points: {} | mana: {}/{} | stamina: {}/{}):\r\n",
                        pc.skill_points, pc.mana, pc.max_mana, pc.stamina, pc.max_stamina
                    ));
                    if pc.skills.is_empty() {
                        s.push_str(" - (none)\r\n");
                        s.push_str("try: train list\r\n");
                    } else {
                        let mut items = pc.skills.iter().collect::<Vec<_>>();
                        items.sort_by_key(|(k, _)| *k);
                        for (k, v) in items {
                            let ready = pc
                                .skill_cd_ms
                                .get(k.as_str())
                                .copied()
                                .unwrap_or(0)
                                <= world.now_ms();
                            let r = if ready { "ready" } else { "cooldown" };
                            let display = find_skill_any(k.as_str()).map(|d| d.display).unwrap_or("-");
                            s.push_str(&format!(" - {} ({}) (rank {}) [{r}]\r\n", k, display, v));
                        }
                        s.push_str("details: skills <skill>\r\n");
                    }
                    s.push_str("compendium: skills compendium\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "skills compendium" {
                    let s = render_skill_compendium();
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("skills ") {
                    let token = rest.trim();
                    if token.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: skills compendium | skills power_strike)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(def) = find_skill_any(token) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (no such skill in the compendium)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let rank = world
                        .active_char(session)
                        .and_then(|c| c.skills.get(def.id))
                        .copied()
                        .unwrap_or(0);
                    let s = render_skill_detail(def, rank);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }

                if lc == "use" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: use power_strike | use bandage | quaff fruity)\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc == "cast" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: cast magic_missile | cast heal)\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc == "quaff" || lc == "drink" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: quaff fruity)\r\n",
                    )
                    .await?;
                    continue;
                }

                let use_cmd = if let Some(rest) = lc.strip_prefix("use ") {
                    Some(("use", rest))
                } else if let Some(rest) = lc.strip_prefix("cast ") {
                    Some(("use", rest))
                } else if let Some(rest) = lc.strip_prefix("invoke ") {
                    Some(("use", rest))
                } else if let Some(rest) = lc.strip_prefix("quaff ") {
                    Some(("quaff", rest))
                } else if let Some(rest) = lc.strip_prefix("drink ") {
                    Some(("quaff", rest))
                } else {
                    None
                };
                if let Some((verb, rest)) = use_cmd {
                    let token = rest.trim();
                    if token.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: use power_strike | use bandage | quaff fruity)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let Some(attacker_id) = world.active_char_id(session) else {
                        let _ =
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                        continue;
                    };
                    if world
                        .chars
                        .get(&attacker_id)
                        .is_some_and(|c| world.now_ms() < c.stunned_until_ms)
                    {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you are stunned)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let (class, target) = match world.chars.get(&attacker_id) {
                        Some(pc) => (pc.class, pc.combat.target),
                        None => (None, None),
                    };

                    // Skill path (combat skills + magic).
                    if verb == "use" {
                        if let Some(class) = class {
                            if let Some(def) = find_skill_for_class(class, token) {
                                let (rank, mana, stamina, ready_at, stats) = match world.chars.get(&attacker_id) {
                                    Some(pc) => (
                                        pc.skills.get(def.id).copied().unwrap_or(0),
                                        pc.mana,
                                        pc.stamina,
                                        pc.skill_cd_ms.get(def.id).copied().unwrap_or(0),
                                        pc.stats,
                                    ),
                                    None => (0, 0, 0, 0, AbilityScores::baseline()),
                                };
                                if rank == 0 {
                                    write_resp_async(
                                        &mut fw,
                                        RESP_OUTPUT,
                                        session,
                                        b"huh? (you haven't trained that. try: train list)\r\n",
                                    )
                                    .await?;
                                    continue;
                                }
                                if world.now_ms() < ready_at {
                                    write_resp_async(
                                        &mut fw,
                                        RESP_OUTPUT,
                                        session,
                                        b"huh? (skill on cooldown)\r\n",
                                    )
                                    .await?;
                                    continue;
                                }
                                if def.cost_mana > 0 && mana < def.cost_mana {
                                    write_resp_async(
                                        &mut fw,
                                        RESP_OUTPUT,
                                        session,
                                        b"huh? (not enough mana)\r\n",
                                    )
                                    .await?;
                                    continue;
                                }
                                if def.cost_stamina > 0 && stamina < def.cost_stamina {
                                    write_resp_async(
                                        &mut fw,
                                        RESP_OUTPUT,
                                        session,
                                        b"huh? (not enough stamina)\r\n",
                                    )
                                    .await?;
                                    continue;
                                }

                                let now = world.now_ms();
                                if let Some(pc) = world.chars.get_mut(&attacker_id) {
                                    pc.skill_cd_ms
                                        .insert(def.id.to_string(), now + def.cooldown_ms);
                                    pc.mana = (pc.mana - def.cost_mana).max(0);
                                    pc.stamina = (pc.stamina - def.cost_stamina).max(0);
                                }
                                world.snapshot_character_projected(attacker_id, "skill-cost");

                                let amount = compute_skill_amount(def, rank, &stats).max(0);
                                match def.target {
                                    SkillTarget::CombatTarget => {
                                        let Some(target_id) = target else {
                                            write_resp_async(
                                                &mut fw,
                                                RESP_OUTPUT,
                                                session,
                                                b"huh? (no target; try: kill stenchworm)\r\n",
                                            )
                                            .await?;
                                            continue;
                                        };
                                        if world
                                            .chars
                                            .get(&target_id)
                                            .is_some_and(|c| world.now_ms() < c.stunned_until_ms)
                                        {
                                            // You can still hit a stunned target, but calling it out makes
                                            // reference scenarios easier to read.
                                        }
                                        let target_is_player = world
                                            .chars
                                            .get(&target_id)
                                            .is_some_and(|c| c.controller.is_some());
                                        if target_is_player && !world.can_pvp_ids(attacker_id, target_id) {
                                            write_resp_async(
                                                &mut fw,
                                                RESP_OUTPUT,
                                                session,
                                                b"huh? (pvp not allowed)\r\n",
                                            )
                                            .await?;
                                            continue;
                                        }
                                        let msg = format!(
                                            "* {} uses {} on {} for {}.",
                                            p.name,
                                            def.id,
                                            world
                                                .chars
                                                .get(&target_id)
                                                .map(|c| c.name.as_str())
                                                .unwrap_or("something"),
                                            amount
                                        );
                                        let killed = if target_is_player {
                                            apply_damage_to_player(
                                                &mut world,
                                                &mut fw,
                                                attacker_id,
                                                target_id,
                                                amount,
                                                msg,
                                            )
                                            .await?
                                        } else {
                                            apply_damage_to_mob(
                                                &mut world,
                                                &mut fw,
                                                attacker_id,
                                                target_id,
                                                amount,
                                                msg,
                                            )
                                            .await?
                                        };
                                        if killed {
                                            let now = world.now_ms();
                                            world.set_combat_projected(
                                                attacker_id,
                                                false,
                                                None,
                                                now,
                                                "skill-target-killed",
                                            );
                                        } else {
                                            let stun_ms = skill_stun_ms(def);
                                            if stun_ms > 0 {
                                                world
                                                    .apply_stun(&mut fw, attacker_id, target_id, stun_ms)
                                                    .await?;
                                            }
                                        }
                                    }
                                    SkillTarget::SelfOnly => {
                                        let (new_hp, healed) = if let Some(pc) = world.chars.get(&attacker_id) {
                                            let new_hp = (pc.hp + amount).min(pc.max_hp);
                                            (new_hp, new_hp - pc.hp)
                                        } else {
                                            (0, 0)
                                        };
                                        if healed > 0 {
                                            world.set_hp_projected(attacker_id, new_hp, "skill-heal");
                                        }
                                        let msg = format!("you use {}. (+{} hp)\r\n", def.id, healed);
                                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                                        let room_msg = format!("* {} uses {}.", p.name, def.id);
                                        let _ = world.broadcast_room(&mut fw, &p.room_id, &room_msg).await;
                                    }
                                }
                                process_due_events(&mut world, &mut fw).await?;
                                continue;
                            }
                        }
                    }

                    // Drink path (inventory item produced by `order`).
                    if let Some(drink) = find_drink(token) {
                        if !world.inv_take_one(attacker_id, drink.name) {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (you don't have that)\r\n",
                            )
                            .await?;
                            continue;
                        }
                        if let Some(pc) = world.chars.get_mut(&attacker_id) {
                            pc.drink_level = pc.drink_level.saturating_add(1);
                        }
                        world.snapshot_character_projected(attacker_id, "drink-quaff");
                        let msg = format!("you quaff a {}.\r\n", drink.name);
                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        let room_msg = format!("* {} quaffs a {}.", p.name, drink.name);
                        let _ = world.broadcast_room(&mut fw, &p.room_id, &room_msg).await;
                        continue;
                    }

                    // Generic consumables (bandages, etc).
                    let inv_match = world
                        .chars
                        .get(&attacker_id)
                        .map(|c| find_item_key_in_inventory(&c.inv, token))
                        .unwrap_or(ItemKeyMatch::None);
                    let item_key = match inv_match {
                        ItemKeyMatch::One(k) => k,
                        ItemKeyMatch::Ambiguous(xs) => {
                            let msg =
                                format!("huh? (ambiguous; try one of: {})\r\n", xs.join(", "));
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                            continue;
                        }
                        ItemKeyMatch::None => {
                            if verb == "use" && class.is_none() {
                                write_resp_async(
                                    &mut fw,
                                    RESP_OUTPUT,
                                    session,
                                    b"huh? (unknown; pick a class: class list)\r\n",
                                )
                                .await?;
                            } else {
                                write_resp_async(
                                    &mut fw,
                                    RESP_OUTPUT,
                                    session,
                                    b"huh? (unknown skill/item)\r\n",
                                )
                                .await?;
                            }
                            continue;
                        }
                    };

                    let Some(def) = items::find_item_def(&item_key) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you can't use that)\r\n",
                        )
                        .await?;
                        continue;
                    };

                    match def.kind {
                        items::ItemKind::Consumable(cdef) => {
                            let heal = cdef.heal.max(0);
                            if heal == 0 {
                                write_resp_async(
                                    &mut fw,
                                    RESP_OUTPUT,
                                    session,
                                    b"huh? (that does nothing)\r\n",
                                )
                                .await?;
                                continue;
                            }
                            let (hp, max_hp) = match world.chars.get(&attacker_id) {
                                Some(pc) => (pc.hp, pc.max_hp),
                                None => (0, 0),
                            };
                            if hp >= max_hp {
                                write_resp_async(
                                    &mut fw,
                                    RESP_OUTPUT,
                                    session,
                                    b"you're already at full hp.\r\n",
                                )
                                .await?;
                                continue;
                            }

                            if !world.inv_take_one(attacker_id, &item_key) {
                                write_resp_async(
                                    &mut fw,
                                    RESP_OUTPUT,
                                    session,
                                    b"huh? (you don't have that)\r\n",
                                )
                                .await?;
                                continue;
                            }

                            let (new_hp, healed) = match world.chars.get(&attacker_id) {
                                Some(pc) => {
                                    let new_hp = (pc.hp + heal).min(pc.max_hp);
                                    (new_hp, new_hp - pc.hp)
                                }
                                None => (0, 0),
                            };
                            if healed > 0 {
                                world.set_hp_projected(attacker_id, new_hp, "consumable-heal");
                            }
                            let msg = format!("you use {}. (+{} hp)\r\n", def.name, healed);
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                            let room_msg = format!("* {} uses {}.", p.name, def.name);
                            let _ = world.broadcast_room(&mut fw, &p.room_id, &room_msg).await;
                        }
                        items::ItemKind::Weapon(_)
                        | items::ItemKind::Armor(_)
                        | items::ItemKind::Accessory(_) => {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (try: equip <item> | wear <item> | wield <item>)\r\n",
                            )
                            .await?;
                        }
                        items::ItemKind::Misc => {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (you can't use that)\r\n",
                            )
                            .await?;
                        }
                    }
                    continue;
                }

                if lc == "menu" {
                    if p.room_id == ROOM_TAVERN {
                        let s = render_tavern_sign();
                        write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    } else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (no menu here)\r\n",
                        )
                        .await?;
                    }
                    continue;
                }

                if lc == "i" || lc == "inv" || lc == "inventory" {
                    let s = render_inventory(&p);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }

                if lc == "eq" || lc == "equip" || lc == "equipment" {
                    let s = world
                        .chars
                        .get(&p.id)
                        .map(render_equipment)
                        .unwrap_or_else(|| "huh?\r\n".to_string());
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }

                if lc == "wear" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: wear tunic)\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc == "wield" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: wield sword)\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc == "equip" {
                    // Handled above (equipment list).
                    continue;
                }

                let equip_cmd = if let Some(rest) = lc.strip_prefix("equip ") {
                    Some(("equip", rest))
                } else if let Some(rest) = lc.strip_prefix("wear ") {
                    Some(("wear", rest))
                } else if let Some(rest) = lc.strip_prefix("wield ") {
                    Some(("wield", rest))
                } else {
                    None
                };
                if let Some((verb, rest)) = equip_cmd {
                    let token = rest.trim();
                    if token.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: equip sword)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(cid) = world.active_char_id(session) else {
                        let _ =
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                        continue;
                    };
                    let inv_match = world
                        .chars
                        .get(&cid)
                        .map(|c| find_item_key_in_inventory(&c.inv, token))
                        .unwrap_or(ItemKeyMatch::None);
                    let item_key = match inv_match {
                        ItemKeyMatch::One(k) => k,
                        ItemKeyMatch::Ambiguous(xs) => {
                            let msg = format!("huh? (ambiguous; try one of: {})\r\n", xs.join(", "));
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                            continue;
                        }
                        ItemKeyMatch::None => {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (you don't have that)\r\n",
                            )
                            .await?;
                            continue;
                        }
                    };

                    let Some(def) = items::find_item_def(&item_key) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you can't equip that)\r\n",
                        )
                        .await?;
                        continue;
                    };

                    match (verb, def.kind) {
                        ("wear", items::ItemKind::Armor(_))
                        | ("wear", items::ItemKind::Accessory(_)) => {}
                        ("wear", _) => {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (you can't wear that)\r\n",
                            )
                            .await?;
                            continue;
                        }
                        ("wield", items::ItemKind::Weapon(_)) => {}
                        ("wield", _) => {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (you can't wield that)\r\n",
                            )
                            .await?;
                            continue;
                        }
                        _ => {}
                    }

                    let Some(slot) = def.equip_slot() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you can't equip that)\r\n",
                        )
                        .await?;
                        continue;
                    };

                    if let Err(err) = world.ensure_equip_slot_write_allowed(slot) {
                        let msg = format!("huh? ({err})\r\n");
                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        continue;
                    }

                    // Enforce sizing if the item is sized.
                    if let Some(item_size) = def.size {
                        let Some(char_size) = world
                            .chars
                            .get(&cid)
                            .and_then(|c| c.race)
                            .map(|r| r.size())
                        else {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (pick a race first: race list)\r\n",
                            )
                            .await?;
                            continue;
                        };
                        if item_size != char_size {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (that doesn't fit you)\r\n",
                            )
                            .await?;
                            continue;
                        }
                    }

                    let occupied = world
                        .chars
                        .get(&cid)
                        .and_then(|c| c.equip.get(slot))
                        .cloned();
                    if let Some(cur) = occupied {
                        let msg = format!(
                            "huh? (you're already wearing something there: {})\r\n",
                            cur
                        );
                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        continue;
                    }

                    if !world.inv_take_one(cid, &item_key) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you don't have that)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if let Some(pc) = world.chars.get_mut(&cid) {
                        pc.equip.set(slot, item_key.clone());
                    }
                    world.snapshot_character_projected(cid, "equip-item");

                    let msg = format!("you equip {}.\r\n", item_key);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    let room_msg = format!("* {} equips {}.", p.name, item_key);
                    let _ = world
                        .broadcast_room(&mut fw, &p.room_id, &room_msg)
                        .await;
                    continue;
                }

                if lc == "remove" || lc == "unequip" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: remove body)\r\n",
                    )
                    .await?;
                    continue;
                }

                let remove_cmd = if let Some(rest) = lc.strip_prefix("remove ") {
                    Some(rest)
                } else if let Some(rest) = lc.strip_prefix("unequip ") {
                    Some(rest)
                } else if let Some(rest) = lc.strip_prefix("unwear ") {
                    Some(rest)
                } else if let Some(rest) = lc.strip_prefix("unwield ") {
                    Some(rest)
                } else {
                    None
                };
                if let Some(rest) = remove_cmd {
                    let token = rest.trim();
                    if token.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: remove body)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(cid) = world.active_char_id(session) else {
                        let _ =
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                        continue;
                    };

                    if token.eq_ignore_ascii_case("all") {
                        let mut removed = Vec::new();
                        if let Some(pc) = world.chars.get_mut(&cid) {
                            for &slot in items::EquipSlot::all() {
                                if let Some(item) = pc.equip.clear(slot) {
                                    removed.push(item);
                                }
                            }
                        }
                        if removed.is_empty() {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"you aren't wearing anything.\r\n",
                            )
                            .await?;
                            continue;
                        }
                        for item in removed.iter() {
                            world.inv_add(cid, item, 1);
                        }
                        world.snapshot_character_projected(cid, "remove-all-equipment");
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"you remove everything.\r\n",
                        )
                        .await?;
                        let room_msg = format!("* {} removes some gear.", p.name);
                        let _ = world
                            .broadcast_room(&mut fw, &p.room_id, &room_msg)
                            .await;
                        continue;
                    }

                    let slot = if let Some(slot) = items::EquipSlot::parse(token) {
                        Some(slot)
                    } else {
                        match world
                            .chars
                            .get(&cid)
                            .map(|c| find_equipped_slot_by_token(&c.equip, token))
                            .unwrap_or(SlotMatch::None)
                        {
                            SlotMatch::One(s) => Some(s),
                            SlotMatch::Ambiguous(_) => {
                                write_resp_async(
                                    &mut fw,
                                    RESP_OUTPUT,
                                    session,
                                    b"huh? (ambiguous; try removing by slot: remove body)\r\n",
                                )
                                .await?;
                                continue;
                            }
                            SlotMatch::None => None,
                        }
                    };

                    let Some(slot) = slot else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you're not wearing that)\r\n",
                        )
                        .await?;
                        continue;
                    };

                    let removed = world
                        .chars
                        .get_mut(&cid)
                        .and_then(|pc| pc.equip.clear(slot));
                    let Some(item) = removed else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (nothing there)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    world.inv_add(cid, &item, 1);
                    world.snapshot_character_projected(cid, "remove-equipment");
                    let msg = format!("you remove {}.\r\n", item);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    let room_msg = format!("* {} removes {}.", p.name, item);
                    let _ = world
                        .broadcast_room(&mut fw, &p.room_id, &room_msg)
                        .await;
                    continue;
                }

                if lc == "kill" {
                    let attacker_id = p.id;
                    let on = world
                        .chars
                        .get(&attacker_id)
                        .is_some_and(|c| c.combat.autoattack);
                    if on {
                        let now = world.now_ms();
                        world.set_combat_projected(
                            attacker_id,
                            false,
                            None,
                            now,
                            "kill-toggle-off",
                        );
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"autoattack off\r\n").await?;
                    } else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: kill stenchworm)\r\n",
                        )
                        .await?;
                    }
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("kill ") {
                    let target = rest.trim();
                    let Some(attacker_id) = world.active_char_id(session) else {
                        let _ =
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                        continue;
                    };
                    if world
                        .chars
                        .get(&attacker_id)
                        .is_some_and(|c| world.now_ms() < c.stunned_until_ms)
                    {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you are stunned)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if target.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: kill stenchworm)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let target_id = if let Some(id) = world.find_mob_in_room(&p.room_id, target) {
                        id
                    } else if let Some(id) = world.find_player_in_room(&p.room_id, target) {
                        id
                    } else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (no such thing to kill here)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if !is_built(&p) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"finish setup first (race/class)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let target_is_player = world
                        .chars
                        .get(&target_id)
                        .is_some_and(|c| c.controller.is_some());
                    if target_is_player {
                        if attacker_id == target_id {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (can't target yourself)\r\n",
                            )
                            .await?;
                            continue;
                        }
                        if !world.is_pvp_room(&p.room_id) {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (not a pvp room)\r\n",
                            )
                            .await?;
                            continue;
                        }
                        let att_pvp = world
                            .chars
                            .get(&attacker_id)
                            .is_some_and(|c| c.pvp_enabled);
                        if !att_pvp {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (pvp is off; try: pvp on)\r\n",
                            )
                            .await?;
                            continue;
                        }
                        let tgt_pvp = world
                            .chars
                            .get(&target_id)
                            .is_some_and(|c| c.pvp_enabled);
                        if !tgt_pvp {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (target has pvp off)\r\n",
                            )
                            .await?;
                            continue;
                        }
                        if !world.can_pvp_ids(attacker_id, target_id) {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (pvp not allowed)\r\n",
                            )
                            .await?;
                            continue;
                        }
                    }
                    let toggled_off = world
                        .chars
                        .get(&attacker_id)
                        .is_some_and(|a| a.combat.autoattack && a.combat.target == Some(target_id));
                    if toggled_off {
                        let now = world.now_ms();
                        world.set_combat_projected(
                            attacker_id,
                            false,
                            None,
                            now,
                            "kill-toggle-target-off",
                        );
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"autoattack off\r\n").await?;
                        process_due_events(&mut world, &mut fw).await?;
                        continue;
                    }
                    write_resp_async(&mut fw, RESP_OUTPUT, session, b"you attack.\r\n").await?;
                    world.start_combat(attacker_id, target_id);
                    process_due_events(&mut world, &mut fw).await?;
                    continue;
                }

                if lc == "sell" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: sell stenchpouch 2)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("sell ") {
                    if p.room_id != ROOM_TAVERN {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (no buyer here)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if world.bartender_id.is_none() {
                        let now = world.now_ms();
                        world.schedule_at_ms(now, EventKind::EnsureTavernMob);
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"bartender isn't here (yet)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let rest = rest.trim();
                    if rest.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: sell stenchpouch 2)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let mut qty: u32 = 1;
                    let mut item = rest.to_string();
                    if rest.starts_with("all ") {
                        qty = u32::MAX;
                        item = rest[4..].trim().to_string();
                    } else if let Some((q, it)) = parse_qty_and_item(rest) {
                        qty = q;
                        item = it;
                    }

                    if !is_tavern_sellable(&item) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"bartender: i don't want that.\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let Some(attacker_id) = world.active_char_id(session) else {
                        let _ =
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                        continue;
                    };

                    let taken = world.inv_take_n(attacker_id, ITEM_STENCHPOUCH, qty);
                    if taken == 0 {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"bartender: you don't have any stenchpouches.\r\n",
                        )
                        .await?;
                        continue;
                    }

                    // 1g per stenchpouch
                    if let Some(pc) = world.chars.get_mut(&attacker_id) {
                        pc.gold = pc.gold.saturating_add(taken);
                    }
                    world.snapshot_character_projected(attacker_id, "sell-item");
                    let msg = format!("bartender: fine. {}g.\r\n", taken);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }

                if lc == "order" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: order 1)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("order ") {
                    let rest = rest.trim();
                    if p.room_id != ROOM_TAVERN {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (no bartender here)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if rest.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: order 1)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let Some((qty, item)) = parse_qty_and_item(rest) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: order 2 fruity)\r\n",
                        )
                        .await?;
                        continue;
                    };

                    let Some(drink) = find_drink(&item) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: look sign)\r\n",
                        )
                        .await?;
                        continue;
                    };

                    if world.bartender_id.is_none() {
                        let now = world.now_ms();
                        world.schedule_at_ms(now, EventKind::EnsureTavernMob);
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"bartender isn't here (yet)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let Some(attacker_id) = world.active_char_id(session) else {
                        let _ =
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await;
                        continue;
                    };

                    let mut served = 0u32;
                    for _ in 0..qty {
                        let (drink_level, gold) = {
                            let Some(pc) = world.chars.get(&attacker_id) else {
                                break;
                            };
                            (pc.drink_level, pc.gold)
                        };

                        if drink_level < drink.min_level {
                            static DENY: [&str; 5] = [
                                "that's a little stiff for you.",
                                "woah. not yet. you'll fold like paper.",
                                "nope. you want to *taste* your drink, right?",
                                "you'll come back to that one. trust me.",
                                "that one bites. pick something softer.",
                            ];
                            let i = ((drink_level + drink.num) as usize) % DENY.len();
                            let msg = format!(
                                "bartender: {} try a {}.\r\n",
                                DENY[i], drink_menu()[0].name
                            );
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                            break;
                        }

                        if gold < drink.cost_gold {
                            let msg = format!(
                                "bartender: that'll be {}g.\r\n",
                                drink.cost_gold
                            );
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                            break;
                        }

                        if let Some(pc) = world.chars.get_mut(&attacker_id) {
                            pc.gold -= drink.cost_gold;
                            let e = pc.inv.entry(drink.name.to_string()).or_insert(0);
                            *e = (*e).saturating_add(1);
                        }
                        world.snapshot_character_projected(attacker_id, "tavern-order");
                        served += 1;
                    }

                    if served > 0 {
                        let msg = if served == 1 {
                            format!("* bartender slides {} a {}.", p.name, drink.name)
                        } else {
                            format!("* bartender slides {} {} {}.", p.name, served, drink.name)
                        };
                        world.broadcast_room(&mut fw, &p.room_id, &msg).await?;
                    }
                    continue;
                }

                if let Some(msg) = line.strip_prefix("say ") {
                    let msg = msg.trim();
                    if !msg.is_empty() {
                        let say = format!("{}: {msg}", p.name);
                        world.broadcast_room(&mut fw, &p.room_id, &say).await?;
                    }
                    continue;
                }

                if lc == "shout" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: shout <msg>)\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc == "yell" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: yell <msg>)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some(shout) = shout_payload(line, &p.name) {
                    world.broadcast_all_sessions(&mut fw, &shout).await?;
                    continue;
                }
                if let Some(msg) = command_arg(line, "yell") {
                    let shout = format!("{} shouts: {}", p.name, msg);
                    world.broadcast_all_sessions(&mut fw, &shout).await?;
                    continue;
                }

                if lc == "emote" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: emote <action>)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some(emote) = room_emote_payload(line, &p.name, "emote") {
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "em" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: emote <action>)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some(emote) = room_emote_payload(line, &p.name, "em") {
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }

                if lc == "me" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: me <action>)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some(emote) = room_emote_payload(line, &p.name, "me") {
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "pose" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: pose <action>)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some(emote) = room_emote_payload(line, &p.name, "pose") {
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "dance" {
                    let emote = room_emote_noarg(&p.name, "dances");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "twirl" {
                    let emote = room_emote_noarg(&p.name, "twirls");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "smile" {
                    let emote = room_emote_noarg(&p.name, "smiles");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "nod" {
                    let emote = room_emote_noarg(&p.name, "nods");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "bow" {
                    let emote = room_emote_noarg(&p.name, "bows");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "laugh" {
                    let emote = room_emote_noarg(&p.name, "laughs");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "sigh" {
                    let emote = room_emote_noarg(&p.name, "sighs");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "shrug" {
                    let emote = room_emote_noarg(&p.name, "shrugs");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "wink" {
                    let emote = room_emote_noarg(&p.name, "winks");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "salute" {
                    let emote = room_emote_noarg(&p.name, "salutes");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "wave" {
                    let emote = room_emote_noarg(&p.name, "waves");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "clap" {
                    let emote = room_emote_noarg(&p.name, "claps");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }
                if lc == "cheer" {
                    let emote = room_emote_noarg(&p.name, "cheers");
                    world.broadcast_room(&mut fw, &p.room_id, &emote).await?;
                    continue;
                }

                if lc == "tell" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: tell <player> <msg>)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some((who, msg)) = parse_tell_args(line, "tell") {
                    let Some(tgt_id) = world.find_player_by_prefix(who) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"tell: no such player (or ambiguous)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if tgt_id == p.id {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"tell: (to yourself)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(tgt_sid) = world.chars.get(&tgt_id).and_then(|c| c.controller) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"tell: player is not online\r\n",
                        )
                        .await?;
                        continue;
                    };

                    let tgt_name = world
                        .chars
                        .get(&tgt_id)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| who.to_string());

                    let out_to = format!("{} tells you: {}\r\n", p.name, msg);
                    let out_from = format!("you tell {}: {}\r\n", tgt_name, msg);
                    let _ = write_resp_async(&mut fw, RESP_OUTPUT, tgt_sid, out_to.as_bytes()).await;
                    write_resp_async(&mut fw, RESP_OUTPUT, session, out_from.as_bytes()).await?;
                    continue;
                }
                if lc == "whisper" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: whisper <player> <msg>)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some((who, msg)) = parse_tell_args(line, "whisper") {
                    let Some(tgt_id) = world.find_player_by_prefix(who) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"tell: no such player (or ambiguous)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if tgt_id == p.id {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"tell: (to yourself)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(tgt_sid) = world.chars.get(&tgt_id).and_then(|c| c.controller) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"tell: player is not online\r\n",
                        )
                        .await?;
                        continue;
                    };

                    let tgt_name = world
                        .chars
                        .get(&tgt_id)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| who.to_string());

                    let out_to = format!("{} whispers: {}\r\n", p.name, msg);
                    let out_from = format!("you whisper {}: {}\r\n", tgt_name, msg);
                    let _ = write_resp_async(&mut fw, RESP_OUTPUT, tgt_sid, out_to.as_bytes()).await;
                    write_resp_async(&mut fw, RESP_OUTPUT, session, out_from.as_bytes()).await?;
                    continue;
                }

                if lc == "friends" || lc == "friend" || lc == "friends list" || lc == "friend list"
                {
                    let mut names = world
                        .chars
                        .get(&p.id)
                        .map(|c| c.friends.iter().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    names.sort_by(|a, b| {
                        a.to_ascii_lowercase()
                            .cmp(&b.to_ascii_lowercase())
                            .then_with(|| a.cmp(b))
                    });
                    let mut s = String::new();
                    s.push_str("friends:\r\n");
                    if names.is_empty() {
                        s.push_str(" - nobody\r\n");
                    } else {
                        for n in names {
                            s.push_str(" - ");
                            s.push_str(&n);
                            s.push_str("\r\n");
                        }
                    }
                    s.push_str("usage: friends add <player> | friends del <player>\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "friends add" || lc == "friend add" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"usage: friends add <player>\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc.starts_with("friends add ") || lc.starts_with("friend add ") {
                    let rest = if lc.starts_with("friends add ") {
                        line[12..].trim()
                    } else {
                        line[11..].trim()
                    };
                    if rest.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"usage: friends add <player>\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let Some(tgt_id) = world.find_player_by_prefix(rest) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"friends: no such player (or ambiguous)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if tgt_id == p.id {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"friends: can't add yourself\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let tgt_name = world
                        .chars
                        .get(&tgt_id)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| rest.to_string());

                    let already = world
                        .chars
                        .get(&p.id)
                        .is_some_and(|c| c.friends.iter().any(|f| f.eq_ignore_ascii_case(&tgt_name)));
                    if already {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"friends: already added\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if let Some(c) = world.chars.get_mut(&p.id) {
                        c.friends.insert(tgt_name.clone());
                    }
                    world.snapshot_character_projected(p.id, "friends-add");
                    let msg = format!("friends: added {}\r\n", tgt_name);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }

                if lc == "friends del"
                    || lc == "friends remove"
                    || lc == "friends rm"
                    || lc == "friend del"
                    || lc == "friend remove"
                    || lc == "friend rm"
                {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"usage: friends del <player>\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc.starts_with("friends del ")
                    || lc.starts_with("friends remove ")
                    || lc.starts_with("friends rm ")
                    || lc.starts_with("friend del ")
                    || lc.starts_with("friend remove ")
                    || lc.starts_with("friend rm ")
                {
                    let rest = if lc.starts_with("friends del ") {
                        line[12..].trim()
                    } else if lc.starts_with("friends rm ") {
                        line[11..].trim()
                    } else if lc.starts_with("friends remove ") {
                        line[15..].trim()
                    } else if lc.starts_with("friend del ") {
                        line[11..].trim()
                    } else if lc.starts_with("friend rm ") {
                        line[10..].trim()
                    } else {
                        line[14..].trim() // friend remove
                    };

                    if rest.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"usage: friends del <player>\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let Some(c) = world.chars.get(&p.id) else {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh?\r\n").await?;
                        continue;
                    };

                    // Prefer exact match, then prefix match, case-insensitive.
                    let mut matches = c
                        .friends
                        .iter()
                        .filter(|f| f.eq_ignore_ascii_case(rest))
                        .cloned()
                        .collect::<Vec<_>>();
                    if matches.is_empty() {
                        let rest_lc = rest.trim().to_ascii_lowercase();
                        matches = c
                            .friends
                            .iter()
                            .filter(|f| f.to_ascii_lowercase().starts_with(&rest_lc))
                            .cloned()
                            .collect::<Vec<_>>();
                    }

                    if matches.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"friends: not found\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if matches.len() > 1 {
                        matches.sort_by(|a, b| {
                            a.to_ascii_lowercase()
                                .cmp(&b.to_ascii_lowercase())
                                .then_with(|| a.cmp(b))
                        });
                        let msg = format!(
                            "friends: ambiguous; try one of: {}\r\n",
                            matches.join(", ")
                        );
                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        continue;
                    }

                    let target = matches.pop().unwrap();
                    if let Some(c) = world.chars.get_mut(&p.id) {
                        c.friends.retain(|f| !f.eq_ignore_ascii_case(&target));
                    }
                    world.snapshot_character_projected(p.id, "friends-remove");
                    let msg = format!("friends: removed {}\r\n", target);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }

                if lc == "bot" {
                    let on = world.chars.get(&p.id).is_some_and(|c| c.is_bot);
                    let msg = if on {
                        b"bot: on\r\nusage: bot on|off\r\n" as &[u8]
                    } else {
                        b"bot: off\r\nusage: bot on|off\r\n"
                    };
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg).await?;
                    continue;
                }
                if lc == "bot off" {
                    let now_ms = world.now_ms();
                    if let Some(c) = world.chars.get_mut(&p.id) {
                        if c.is_bot {
                            c.is_bot = false;
                            c.bot_mode_changed_ms = now_ms;
                            world.snapshot_character_projected(p.id, "bot-off");
                            write_resp_async(&mut fw, RESP_OUTPUT, session, b"bot: off\r\n").await?;
                        } else {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"bot: already off\r\n",
                            )
                            .await?;
                        }
                    }
                    continue;
                }
                if lc == "bot on" {
                    let now_ms = world.now_ms();
                    let Some(c) = world.chars.get(&p.id) else {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh?\r\n").await?;
                        continue;
                    };
                    if c.is_bot {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"bot: already on\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if !c.bot_ever {
                        if let Some(ss) = world.sessions.get_mut(&session) {
                            ss.pending_confirm = Some(PendingConfirm::BotOn { cid: p.id });
                        }
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"bot: are you sure?\r\ntype: yes | no\r\n",
                        )
                        .await?;
                        continue;
                    }

                    if let Some(c) = world.chars.get_mut(&p.id) {
                        c.is_bot = true;
                        c.bot_mode_changed_ms = now_ms;
                        // bot_ever is already true; keep the original timestamp.
                    }
                    world.snapshot_character_projected(p.id, "bot-on");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, b"bot: on\r\n").await?;
                    continue;
                }

                if lc == "assist" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: assist on|off)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("assist ") {
                    let v = rest.trim();
                    let on = match v {
                        "on" => Some(true),
                        "off" => Some(false),
                        _ => None,
                    };
                    let Some(on) = on else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: assist on|off)\r\n",
                        )
                        .await?;
                        continue;
                    };

                    if let Some(pc) = world.active_char_mut(session) {
                        pc.autoassist = on;
                    }
                    world.snapshot_character_projected(p.id, "assist-toggle");
                    let msg = if on {
                        b"assist: on\r\n" as &[u8]
                    } else {
                        b"assist: off\r\n" as &[u8]
                    };
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg).await?;
                    continue;
                }

                if lc == "follow" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: follow on|off)\r\n",
                    )
                    .await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("follow ") {
                    let v = rest.trim();
                    let on = match v {
                        "on" => Some(true),
                        "off" => Some(false),
                        _ => None,
                    };
                    let Some(on) = on else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: follow on|off)\r\n",
                        )
                        .await?;
                        continue;
                    };

                    let in_party = world.active_char_id(session).and_then(|cid| world.party_of.get(&cid).copied()).is_some();
                    if !in_party {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you are not in a party)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    if let Some(pc) = world.active_char_mut(session) {
                        pc.follow_leader = on;
                    }
                    world.snapshot_character_projected(p.id, "follow-toggle");
                    let msg = if on {
                        b"follow: on\r\n" as &[u8]
                    } else {
                        b"follow: off\r\n" as &[u8]
                    };
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg).await?;
                    continue;
                }

                if lc == "pvp" {
                    let on = world
                        .chars
                        .get(&p.id)
                        .map(|c| c.pvp_enabled)
                        .unwrap_or(false);
                    let msg = if on {
                        "pvp: on (arena rooms only)\r\n"
                    } else {
                        "pvp: off\r\n"
                    };
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("pvp ") {
                    let v = rest.trim();
                    let on = match v {
                        "on" => Some(true),
                        "off" => Some(false),
                        _ => None,
                    };
                    let Some(on) = on else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: pvp on|off)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if let Some(pc) = world.active_char_mut(session) {
                        pc.pvp_enabled = on;
                    }
                    world.snapshot_character_projected(p.id, "pvp-toggle");
                    let msg = if on {
                        b"pvp: on\r\n" as &[u8]
                    } else {
                        b"pvp: off\r\n" as &[u8]
                    };
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg).await?;
                    continue;
                }

                if lc == "party" {
                    let cid = p.id;
                    let s = render_party_status(&world, cid);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "party create" || lc == "party new" {
                    let cid = p.id;
                    if world.party_of.get(&cid).is_some() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: you are already in a party\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let pid = world.party_create(cid);
                    let msg = format!("party: created (id={pid})\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }
                if lc == "party disband" {
                    let cid = p.id;
                    let Some(pid) = world.party_of.get(&cid).copied() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: you are not in a party\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(party) = world.parties.get(&pid).cloned() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: internal error\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if party.leader != cid {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: only the leader can disband\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let leader_name = world
                        .chars
                        .get(&cid)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| p.name.clone());
                    let member_sids = party
                        .members
                        .iter()
                        .filter_map(|mid| world.chars.get(mid).and_then(|c| c.controller))
                        .collect::<Vec<_>>();

                    world.party_disband(pid);

                    for sid in member_sids {
                        let msg = if sid == session {
                            "party: disbanded\r\n".to_string()
                        } else {
                            format!("party: disbanded by {leader_name}\r\n")
                        };
                        let _ = write_resp_async(&mut fw, RESP_OUTPUT, sid, msg.as_bytes()).await;
                    }
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("party kick ") {
                    let target = rest.trim();
                    if target.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: party kick <player>)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let leader_id = p.id;
                    let Some(pid) = world.party_of.get(&leader_id).copied() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: you are not in a party\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(party) = world.parties.get(&pid).cloned() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: internal error\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if party.leader != leader_id {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: only the leader can kick\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let Some(target_id) = world.find_player_by_prefix(target) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: player not found (or ambiguous)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if target_id == leader_id {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: ok\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if world.party_of.get(&target_id).copied() != Some(pid) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: that player is not in your party\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let target_sid = world
                        .chars
                        .get(&target_id)
                        .and_then(|c| c.controller);
                    world.party_leave(target_id);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, b"party: kicked\r\n").await?;
                    if let Some(ts) = target_sid {
                        let msg = format!("party: you were kicked by {}\r\n", p.name);
                        let _ = write_resp_async(&mut fw, RESP_OUTPUT, ts, msg.as_bytes()).await;
                    }
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("party lead ") {
                    let target = rest.trim();
                    if target.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: party lead <player>)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let leader_id = p.id;
                    let Some(pid) = world.party_of.get(&leader_id).copied() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: you are not in a party\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(party) = world.parties.get(&pid).cloned() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: internal error\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if party.leader != leader_id {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: only the leader can transfer lead\r\n",
                        )
                        .await?;
                        continue;
                    }

                    // Find a party member by unique prefix.
                    let t = target.to_ascii_lowercase();
                    let mut found: Option<CharacterId> = None;
                    for mid in &party.members {
                        let Some(c) = world.chars.get(mid) else {
                            continue;
                        };
                        if c.controller.is_none() {
                            continue;
                        }
                        let n = c.name.to_ascii_lowercase();
                        if n == t || n.starts_with(&t) {
                            if found.is_some() && found != Some(*mid) {
                                found = None;
                                break;
                            }
                            found = Some(*mid);
                        }
                    }
                    let Some(new_leader) = found else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: player not found (or ambiguous)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if new_leader == leader_id {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"party: ok\r\n").await?;
                        continue;
                    }
                    world.project_world(worldlog::WorldEvent::PartyLeaderSet {
                        party_id: pid,
                        leader: new_leader,
                    });
                    let name = world
                        .chars
                        .get(&new_leader)
                        .map(|c| c.name.as_str())
                        .unwrap_or("someone");
                    let msg = format!("party: leader is now {name}\r\n");
                    // Best-effort notify all party members.
                    let member_sids = party
                        .members
                        .iter()
                        .filter_map(|mid| world.chars.get(mid).and_then(|c| c.controller))
                        .collect::<Vec<_>>();
                    for sid in member_sids {
                        let _ = write_resp_async(&mut fw, RESP_OUTPUT, sid, msg.as_bytes()).await;
                    }
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("party invite ") {
                    let target = rest.trim();
                    if target.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: party invite <player>)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let inviter_id = p.id;
                    let pid = world
                        .party_of
                        .get(&inviter_id)
                        .copied()
                        .unwrap_or_else(|| world.party_create(inviter_id));

                    let Some(party) = world.parties.get(&pid).cloned() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: internal error\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if party.leader != inviter_id {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: only the leader can invite\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let Some(invitee_id) = world.find_player_by_prefix(target) else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: player not found (or ambiguous)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if invitee_id == inviter_id {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: ok\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if world.party_of.get(&invitee_id).is_some() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: that player is already in a party\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let expires_ms = world.now_ms().saturating_add(60_000);
                    world.party_invite_projected(invitee_id, pid, inviter_id, expires_ms);

                    let msg = format!(
                        "party: invited {}\r\n",
                        world
                            .chars
                            .get(&invitee_id)
                            .map(|c| c.name.as_str())
                            .unwrap_or("?")
                    );
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    if let Some(invitee) = world.chars.get(&invitee_id).cloned() {
                        if let Some(sid2) = invitee.controller {
                            let from = world
                                .chars
                                .get(&inviter_id)
                                .map(|c| c.name.as_str())
                                .unwrap_or("someone");
                            let msg2 = format!("party invite from {from}. type: party accept\r\n");
                            let _ = write_resp_async(&mut fw, RESP_OUTPUT, sid2, msg2.as_bytes()).await;
                        }
                    }
                    continue;
                }
                if lc == "party accept" {
                    let invitee_id = p.id;
                    let Some(inv) = world.party_invites.get(&invitee_id).cloned() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: no pending invites\r\n",
                        )
                        .await?;
                        continue;
                    };
                    world.party_invite_remove_projected(invitee_id);
                    if inv.expires_ms < world.now_ms() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: invite expired\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if !world.parties.contains_key(&inv.party_id) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: invite stale\r\n",
                        )
                        .await?;
                        continue;
                    }
                    world.project_world(worldlog::WorldEvent::PartyMemberSet {
                        party_id: inv.party_id,
                        member: invitee_id,
                        present: true,
                    });
                    let member_ids = world.party_members_vec(inv.party_id);
                    let msg = format!("party: joined (id={})\r\n", inv.party_id);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;

                    // Notify the whole party (including leader) so party automation can react.
                    let join_name = world
                        .chars
                        .get(&invitee_id)
                        .map(|c| c.name.as_str())
                        .unwrap_or("someone");
                    let msg2 = format!("party: {join_name} joined\r\n");
                    let member_sids = member_ids
                        .iter()
                        .filter_map(|mid| world.chars.get(mid).and_then(|c| c.controller))
                        .collect::<Vec<_>>();
                    for sid in member_sids {
                        let _ = write_resp_async(&mut fw, RESP_OUTPUT, sid, msg2.as_bytes()).await;
                    }
                    continue;
                }
                if lc == "party leave" {
                    let cid = p.id;
                    if world.party_of.get(&cid).is_none() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: you are not in a party\r\n",
                        )
                        .await?;
                        continue;
                    }
                    world.party_leave(cid);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, b"party: left\r\n").await?;
                    continue;
                }

                if let Some(rest) = line.strip_prefix("party say ") {
                    let msg = rest.trim();
                    if msg.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: party say <msg>)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(pid) = world.party_of.get(&p.id).copied() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you are not in a party)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let s = format!("[party {pid}] {}: {}", p.name, msg);
                    let _ = world.party_send(&mut fw, pid, &s).await;
                    continue;
                }

                if let Some(rest) = lc.strip_prefix("party run ") {
                    let adventure = rest.trim();
                    if adventure.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: party run q1-first-day-on-gaia)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(pid) = world.party_of.get(&p.id).copied() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you are not in a party)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(party) = world.parties.get(&pid).cloned() else {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"party: stale\r\n").await?;
                        continue;
                    };
                    if party.leader != p.id {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"party: only the leader can start a run\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let mut aid = adventure.to_string();
                    if let Some(x) = aid.strip_suffix(".md") {
                        aid = x.to_string();
                    }
                    if aid.contains('/') || aid.contains("..") || aid.trim().is_empty() {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh?\r\n").await?;
                        continue;
                    }

                    let path = format!("protoadventures/{aid}.md");
                    let md = match std::fs::read_to_string(&path) {
                        Ok(s) => s,
                        Err(_) => {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"party: adventure not found\r\n",
                            )
                            .await?;
                            continue;
                        }
                    };

                    let plan = protoadventure::parse_protoadventure_markdown(&aid, &md);
                    let instance_prefix = format!("inst.p{pid}");

                    // Reset any prior instance content for this party prefix (and evacuate stragglers).
                    let evac_to = if world.rooms.has_room(ROOM_TOWN_GATE) {
                        ROOM_TOWN_GATE.to_string()
                    } else {
                        world.rooms.start_room().to_string()
                    };
                    world.evacuate_rooms_with_prefix(&instance_prefix, &evac_to);
                    for rid in world.rooms.dyn_room_ids_with_prefix(&instance_prefix) {
                        world.remove_room_projected(rid, "party-run-clear");
                    }

                    let rooms = protoadventure::instantiate_rooms(&instance_prefix, &plan);
                    let start_room = format!("{instance_prefix}.{}", plan.start_room);
                    let _ = world
                        .party_send(&mut fw, pid, &format!("party: constructing {aid}..."))
                        .await;
                    for (rid, def) in rooms {
                        world.set_room_projected(rid, def, "party-run-room");
                    }

                    let member_ids = world.party_members_vec(pid);
                    for mid in member_ids.iter().copied() {
                        if world.chars.contains_key(&mid) {
                            world.move_char_projected(mid, start_room.clone(), "party-run-enter");
                        }
                    }

                    let _ = world
                        .party_send(&mut fw, pid, "* your party enters a new run.")
                        .await;

                    for mid in member_ids {
                        let Some(c) = world.chars.get(&mid) else {
                            continue;
                        };
                        let Some(sid) = c.controller else {
                            continue;
                        };
                        let s = world.render_room_for(&start_room, sid);
                        let _ = write_resp_async(&mut fw, RESP_OUTPUT, sid, s.as_bytes()).await;
                    }
                    continue;
                }

                if lc == "who" {
                    let names = world.online_character_names();
                    let mut s = String::new();
                    s.push_str("online (players):\r\n");
                    for n in names {
                        s.push_str(" - ");
                        s.push_str(&n);
                        s.push_str("\r\n");
                    }
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }

                if lc == "botsense" {
                    let bots = world.online_bot_character_names();
                    if bots.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"botsense: all clear\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let mut s = String::new();
                    s.push_str("botsense (bot-mode players):\r\n");
                    for n in bots {
                        s.push_str(" - ");
                        s.push_str(&n);
                        s.push_str("\r\n");
                    }
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }

                if lc == "humans" || lc == "whomans" {
                    let names = world.online_human_character_names();
                    let mut s = String::new();
                    s.push_str("online (humans):\r\n");
                    for n in names {
                        s.push_str(" - ");
                        s.push_str(&n);
                        s.push_str("\r\n");
                    }
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }

                if lc == "sessions" {
                    let s = render_sessions_cmd(&world, session);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("sessions drop ") {
                    let token = rest.trim();
                    if token.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"usage: sessions drop <character_id>\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(ss) = world.sessions.get(&session).cloned() else {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"not attached\r\n").await?;
                        continue;
                    };
                    let Ok(cid) = token.parse::<u64>() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (character_id must be a number)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if !ss.controlled.iter().any(|x| *x == cid) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you don't control that character)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if ss.controlled.len() == 1 {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (can't drop your last character)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let Some(c) = world.chars.get(&cid).cloned() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (missing character)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if c.created_by != Some(session) {
                        // Future: system sessions / forced attachments. For now, only allow dropping
                        // characters this session spawned.
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (you can only drop characters this session started)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let _ = world
                        .broadcast_room(&mut fw, &c.room_id, &format!("* {} disconnects.", c.name))
                        .await;

                    let _removed = world.remove_char_projected(cid, "session-drop");
                    if let Some(ssm) = world.sessions.get_mut(&session) {
                        ssm.controlled.retain(|x| *x != cid);
                        if ssm.active == cid {
                            if let Some(&next) = ssm.controlled.first() {
                                ssm.active = next;
                            }
                        }
                    }

                    let msg = format!("sessions: dropped {} ({})\r\n", c.name, c.id);
                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    continue;
                }

                if lc == "proto" {
                    if !world.has_cap(&p, groups::Capability::WorldProtoLoad) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"nope: world.proto.load\r\n",
                        )
                        .await?;
                        continue;
                    }
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: proto list | proto <adventure_id> | proto exit)\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc == "proto list" {
                    if !world.has_cap(&p, groups::Capability::WorldProtoLoad) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"nope: world.proto.load\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let ids = list_protoadventures().unwrap_or_default();
                    let mut s = String::new();
                    s.push_str("protoadventures:\r\n");
                    for id in ids {
                        s.push_str(" - ");
                        s.push_str(&id);
                        s.push_str("\r\n");
                    }
                    s.push_str("load with: proto <adventure_id>\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if lc == "proto exit" {
                    if !world.has_cap(&p, groups::Capability::WorldProtoLoad) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"nope: world.proto.load\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if world.rooms.has_room(ROOM_TOWN_GATE) {
                        teleport_to(&mut world, &mut fw, session, ROOM_TOWN_GATE, "returns").await?;
                    } else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (town gate not found)\r\n",
                        )
                        .await?;
                    }
                    continue;
                }
                if let Some(rest) = line.strip_prefix("proto ") {
                    if !world.has_cap(&p, groups::Capability::WorldProtoLoad) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"nope: world.proto.load\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let mut id = rest.trim().to_ascii_lowercase();
                    if id.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: proto list | proto <adventure_id>)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if let Some(stripped) = id.strip_suffix(".md") {
                        id = stripped.to_string();
                    }
                    if !id.chars().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'
                    }) {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (invalid adventure_id)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    let start = match world.load_protoadventure(&id) {
                        Ok(s) => s,
                        Err(e) => {
                            let msg = format!("proto: failed to load {id}: {e}\r\n");
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                            continue;
                        }
                    };
                    teleport_to(&mut world, &mut fw, session, &start, "enters").await?;
                    continue;
                }

                if lc == "quest" {
                    write_resp_async(
                        &mut fw,
                        RESP_OUTPUT,
                        session,
                        b"huh? (try: quest list | quest get <key> | quest set <key> <value> | quest del <key>)\r\n",
                    )
                    .await?;
                    continue;
                }
                if lc == "quest list" {
                    if p.quest.is_empty() {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"quest: (none)\r\n")
                            .await?;
                        continue;
                    }
                    let mut xs = p
                        .quest
                        .iter()
                        .map(|(k, v)| format!(" - {k}={v}\r\n"))
                        .collect::<Vec<_>>();
                    xs.sort_unstable();
                    let mut s = String::new();
                    s.push_str("quest:\r\n");
                    for x in xs {
                        s.push_str(&x);
                    }
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = line.strip_prefix("quest get ") {
                    let key = rest.trim();
                    if key.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: quest get <key>)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    if let Some(v) = p.quest.get(key) {
                        let s = format!("quest: {key}={v}\r\n");
                        write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    } else {
                        let s = format!("quest: {key}=(unset)\r\n");
                        write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    }
                    continue;
                }
                if let Some(rest) = line.strip_prefix("quest set ") {
                    let mut it = rest.trim().split_whitespace();
                    let Some(key) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: quest set <key> <value>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    let Some(value) = it.next() else {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: quest set <key> <value>)\r\n",
                        )
                        .await?;
                        continue;
                    };
                    if let Some(c) = world.active_char_mut(session) {
                        c.quest.insert(key.to_string(), value.to_string());
                    }
                    world.snapshot_character_projected(p.id, "quest-set");
                    let s = format!("quest: set {key}={value}\r\n");
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }
                if let Some(rest) = line.strip_prefix("quest del ") {
                    let key = rest.trim();
                    if key.is_empty() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (try: quest del <key>)\r\n",
                        )
                        .await?;
                        continue;
                    }
                    let removed = world
                        .active_char_mut(session)
                        .and_then(|c| c.quest.remove(key))
                        .is_some();
                    if removed {
                        world.snapshot_character_projected(p.id, "quest-del");
                    }
                    let s = if removed {
                        format!("quest: del {key}\r\n")
                    } else {
                        format!("quest: del {key} (missing)\r\n")
                    };
                    write_resp_async(&mut fw, RESP_OUTPUT, session, s.as_bytes()).await?;
                    continue;
                }

                if lc == "turn" {
                    write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (try: turn valve)\r\n")
                        .await?;
                    continue;
                }
                if let Some(rest) = lc.strip_prefix("turn ") {
                    let target = rest.trim();
                    if target.is_empty() {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (try: turn valve)\r\n")
                            .await?;
                        continue;
                    }

                    let valve_ix = match p.room_id.as_str() {
                        "R_SEW_VALVE1_02" => Some(1),
                        "R_SEW_VALVE2_02" => Some(2),
                        "R_SEW_VALVE3_02" => Some(3),
                        _ => None,
                    };

                    if valve_ix.is_none() {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                            session,
                            b"huh? (nothing to turn here)\r\n",
                        )
                        .await?;
                        continue;
                    }

                    if target != "valve" && target != "wheel" && target != "valve wheel" {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (try: turn valve)\r\n")
                            .await?;
                        continue;
                    }

                    let key_done = format!("q.q3_sewer_valves.valve_{}", valve_ix.unwrap());
                    let (opened, already) = {
                        let Some(c) = world.active_char_mut(session) else {
                            write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await?;
                            continue;
                        };

                        let already = c
                            .quest
                            .get(&key_done)
                            .map(|v| v.trim())
                            .is_some_and(|v| !v.is_empty() && v != "0" && v != "false");
                        if already {
                            let opened = c
                                .quest
                                .get("q.q3_sewer_valves.valves_opened")
                                .and_then(|v| v.trim().parse::<i64>().ok())
                                .unwrap_or(0)
                                .clamp(0, 3);
                            (opened, true)
                        } else {
                            c.quest.insert(key_done, "1".to_string());
                            let mut opened = c
                                .quest
                                .get("q.q3_sewer_valves.valves_opened")
                                .and_then(|v| v.trim().parse::<i64>().ok())
                                .unwrap_or(0)
                                .clamp(0, 3);
                            opened = (opened + 1).clamp(0, 3);
                            c.quest
                                .insert("q.q3_sewer_valves.valves_opened".to_string(), opened.to_string());
                            (opened, false)
                        }
                    };

                    if already {
                        let msg = format!("the valve is already open. (valves opened: {opened}/3)\r\n");
                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                    } else {
                        world.snapshot_character_projected(p.id, "sewer-valve");
                        let msg = format!("you turn the valve wheel. (valves opened: {opened}/3)\r\n");
                        write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        let room_msg = format!("* {} turns the valve wheel.", p.name);
                        let _ = world.broadcast_room(&mut fw, &p.room_id, &room_msg).await;
                        }
                        continue;
                    }

                    if lc == "light" {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (try: light pylon)\r\n")
                            .await?;
                        continue;
                    }
                    if let Some(rest) = lc.strip_prefix("light ") {
                        let target = rest.trim();
                        if target.is_empty() {
                            write_resp_async(&mut fw, RESP_OUTPUT, session, b"huh? (try: light pylon)\r\n")
                                .await?;
                            continue;
                        }

                        let pylon_ix = match p.room_id.as_str() {
                            "R_HILL_PYLON_01" => Some(1),
                            "R_HILL_PYLON_02" => Some(2),
                            "R_HILL_PYLON_03" => Some(3),
                            _ => None,
                        };
                        if pylon_ix.is_none() {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (nothing to light here)\r\n",
                            )
                            .await?;
                            continue;
                        }

                        if target != "pylon" && target != "ward" && target != "tower" && target != "beacon" {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (try: light pylon)\r\n",
                            )
                            .await?;
                            continue;
                        }

                        let key_done = format!("q.q6_hillfort_signal.pylon_{}", pylon_ix.unwrap());
                        let (lit, already) = {
                            let Some(c) = world.active_char_mut(session) else {
                                write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n").await?;
                                continue;
                            };
                            let already = c
                                .quest
                                .get(&key_done)
                                .map(|v| v.trim())
                                .is_some_and(|v| !v.is_empty() && v != "0" && v != "false");

                            if already {
                                let lit = c
                                    .quest
                                    .get("q.q6_hillfort_signal.pylons_lit")
                                    .and_then(|v| v.trim().parse::<i64>().ok())
                                    .unwrap_or(0)
                                    .clamp(0, 3);
                                (lit, true)
                            } else {
                                c.quest.insert(key_done, "1".to_string());
                                let mut lit = c
                                    .quest
                                    .get("q.q6_hillfort_signal.pylons_lit")
                                    .and_then(|v| v.trim().parse::<i64>().ok())
                                    .unwrap_or(0)
                                    .clamp(0, 3);
                                lit = (lit + 1).clamp(0, 3);
                                c.quest.insert(
                                    "q.q6_hillfort_signal.pylons_lit".to_string(),
                                    lit.to_string(),
                                );
                                (lit, false)
                            }
                        };

                        if already {
                            let msg = format!("the pylon is already lit. (pylons lit: {lit}/3)\r\n");
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                        } else {
                            world.snapshot_character_projected(p.id, "hillfort-pylon");
                            let mut msg = format!("you relight the ward pylon. (pylons lit: {lit}/3)\r\n");
                            if lit >= 3 {
                                msg.push_str("somewhere deeper in the fort, a gate finally agrees.\r\n");
                            }
                            write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                            let room_msg = format!("* {} relights the ward pylon.", p.name);
                            let _ = world.broadcast_room(&mut fw, &p.room_id, &room_msg).await;
                        }
                        continue;
                    }

                    if lc == "pull" {
                        write_resp_async(
                            &mut fw,
                            RESP_OUTPUT,
                        session,
                        b"huh? (try: pull lever)\r\n",
                    )
                    .await?;
                    continue;
                }
                    if let Some(rest) = lc.strip_prefix("pull ") {
                        let target = rest.trim();
                        if target.is_empty() {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (try: pull lever)\r\n",
                            )
                            .await?;
                            continue;
                        }

                        if target != "lever" && target != "bypass" && target != "switch" && target != "quarry" {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                            session,
                            b"huh? (try: pull lever)\r\n",
                        )
                            .await?;
                            continue;
                        }

                        enum PullLeverKind {
                            SewersBypass,
                            RailSwitch1,
                            RailSwitch2,
                        }
                        let kind = match p.room_id.as_str() {
                            "R_SEW_REWARD_01" => Some(PullLeverKind::SewersBypass),
                            "R_RAIL_YARD_04" => Some(PullLeverKind::RailSwitch1),
                            "R_RAIL_YARD_08" => Some(PullLeverKind::RailSwitch2),
                            _ => None,
                        };
                        let Some(kind) = kind else {
                            write_resp_async(
                                &mut fw,
                                RESP_OUTPUT,
                                session,
                                b"huh? (nothing to pull here)\r\n",
                            )
                            .await?;
                            continue;
                        };

                        match kind {
                            PullLeverKind::SewersBypass => {
                                let (valves_opened, already) = {
                                    let Some(c) = world.active_char_mut(session) else {
                                        write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n")
                                            .await?;
                                        continue;
                                    };
                                    let valves_opened = c
                                        .quest
                                        .get("q.q3_sewer_valves.valves_opened")
                                        .and_then(|v| v.trim().parse::<i64>().ok())
                                        .unwrap_or(0)
                                        .clamp(0, 3);

                                    let already = eval_gate_expr(c, "gate.sewers.shortcut_to_quarry");

                                    if !already && valves_opened >= 3 {
                                        c.quest.insert(
                                            "gate.sewers.shortcut_to_quarry".to_string(),
                                            "1".to_string(),
                                        );
                                    }

                                    (valves_opened, already)
                                };

                                if valves_opened < 3 {
                                    write_resp_async(
                                        &mut fw,
                                        RESP_OUTPUT,
                                        session,
                                        b"the lever doesn't budge. something upstream still has pressure.\r\n",
                                    )
                                    .await?;
                                    continue;
                                }
                                if already {
                                    write_resp_async(
                                        &mut fw,
                                        RESP_OUTPUT,
                                        session,
                                        b"the lever is already down. the quarry bypass is unsealed.\r\n",
                                    )
                                    .await?;
                                    continue;
                                }

                                world.snapshot_character_projected(p.id, "sewer-bypass");
                                write_resp_async(
                                    &mut fw,
                                    RESP_OUTPUT,
                                    session,
                                    b"you pull the bypass lever. somewhere deep in the tunnels, metal shifts.\r\n",
                                )
                                .await?;
                                let room_msg =
                                    format!("* {} pulls the bypass lever. something heavy shifts.", p.name);
                                let _ = world.broadcast_room(&mut fw, &p.room_id, &room_msg).await;
                                continue;
                            }
                            PullLeverKind::RailSwitch1 | PullLeverKind::RailSwitch2 => {
                                let (pulled, already, unlocked_now) = {
                                    let Some(c) = world.active_char_mut(session) else {
                                        write_resp_async(&mut fw, RESP_ERR, session, b"not attached\r\n")
                                            .await?;
                                        continue;
                                    };
                                    let (key, label) = match kind {
                                        PullLeverKind::RailSwitch1 => ("e.e17.lever_1", "1"),
                                        PullLeverKind::RailSwitch2 => ("e.e17.lever_2", "2"),
                                        _ => unreachable!(),
                                    };
                                    let already = eval_gate_expr(c, key);
                                    if !already {
                                        c.quest.insert(key.to_string(), "1".to_string());
                                    }

                                    let l1 = eval_gate_expr(c, "e.e17.lever_1") as i64;
                                    let l2 = eval_gate_expr(c, "e.e17.lever_2") as i64;
                                    let pulled = (l1 + l2).clamp(0, 2);
                                    let has_pass = eval_gate_expr(c, "gate.rail_spur.pass");
                                    let unlocked_now = !has_pass && pulled >= 2;
                                    if unlocked_now {
                                        c.quest.insert("gate.rail_spur.pass".to_string(), "1".to_string());
                                    }
                                    // Keep a short state key so `quest list` is readable in dev.
                                    c.quest.insert(
                                        "e.e17.last_lever".to_string(),
                                        label.to_string(),
                                    );
                                    (pulled, already, unlocked_now)
                                };

                                if already {
                                    let msg = format!(
                                        "the switch is already thrown. (rail levers: {pulled}/2)\r\n"
                                    );
                                    write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                                    continue;
                                }

                                world.snapshot_character_projected(p.id, "rail-switch");
                                let mut msg =
                                    format!("you throw the switch lever. (rail levers: {pulled}/2)\r\n");
                                if unlocked_now {
                                    msg.push_str(
                                        "somewhere in the terminal, a lock tag snaps loose.\r\n",
                                    );
                                }
                                write_resp_async(&mut fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
                                let room_msg = format!("* {} throws the switch lever.", p.name);
                                let _ = world.broadcast_room(&mut fw, &p.room_id, &room_msg).await;
                                continue;
                            }
                        }
                    }

                let mut moved = false;

                if lc == "go" {
                    write_resp_async(&mut fw, RESP_OUTPUT, session, HUH_GO).await?;
                    continue;
                }

                if let Some(rest) = line.strip_prefix("go ") {
                    let token = rest.trim();
                    if token.is_empty() {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, HUH_GO).await?;
                        continue;
                    }
                    moved = try_move(&mut world, &mut fw, session, token).await?;
                    if !moved {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, HUH_NO_EXIT).await?;
                        let exits = world.rooms.render_exits(&p.room_id);
                        write_resp_async(&mut fw, RESP_OUTPUT, session, exits.as_bytes()).await?;
                    }
                } else if world.rooms.find_exit(&p.room_id, line).is_some()
                    || normalize_dir(line).is_some()
                {
                    // Navigation via exit name or alias.
                    moved = try_move(&mut world, &mut fw, session, line).await?;
                    if !moved {
                        write_resp_async(&mut fw, RESP_OUTPUT, session, HUH_NO_EXIT).await?;
                        let exits = world.rooms.render_exits(&p.room_id);
                        write_resp_async(&mut fw, RESP_OUTPUT, session, exits.as_bytes()).await?;
                    }
                }

                if moved {
                    continue;
                }

                write_resp_async(&mut fw, RESP_OUTPUT, session, HUH_HELP).await?;
            }
        }
                world.advance_clock_from_wall();
                process_due_events(&mut world, &mut fw).await?;
            }
        }
    }

    // Broker disconnected: drop all in-memory session state.
    Ok(())
}

async fn process_due_events(
    world: &mut World,
    fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
) -> anyhow::Result<()> {
    while let Some(ev) = world.pop_due_event() {
        handle_event(world, fw, ev).await?;
    }
    Ok(())
}

async fn handle_event(
    world: &mut World,
    fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
    ev: ScheduledEvent,
) -> anyhow::Result<()> {
    match ev.kind {
        EventKind::Tick => {
            // Room polling hooks live here. Keep deterministic: only use world.now_ms().
            if world.bartender_id.is_none() {
                world.schedule_at_ms(world.now_ms(), EventKind::EnsureTavernMob);
            }
            world.schedule_at_ms(world.now_ms(), EventKind::EnsureFirstFightWorm);
        }
        EventKind::RoomMsg { room_id, msg } => {
            let _ = world.broadcast_room(fw, &room_id, &msg).await;
        }
        EventKind::EnsureTavernMob => {
            // Re-check periodically in case the world was reset/replayed.
            world.schedule_in_ms(60_000, EventKind::EnsureTavernMob);

            let ok = world
                .bartender_id
                .and_then(|id| world.chars.get(&id))
                .is_some_and(|c| c.controller.is_none() && c.room_id == ROOM_TAVERN);
            if ok {
                return Ok(());
            }

            let room_id = ROOM_TAVERN.to_string();
            let mob_id = world.spawn_mob(room_id.clone(), "bartender".to_string());
            world.set_ambient_projected(Some(mob_id), 0, "ensure-tavern-mob");

            let _ = world
                .broadcast_room(
                    fw,
                    &room_id,
                    "* the bartender appears behind the bar, polishing a glass.",
                )
                .await;

            world.schedule_in_ms(world.bartender_emote_ms, EventKind::BartenderEmote);
        }
        EventKind::BartenderEmote => {
            static EMOTES: [&str; 6] = [
                "* the bartender washes a stack of mugs.",
                "* the bartender feeds the perpetual stew. it bubbles politely.",
                "* the bartender scrubs a plate like it insulted their family.",
                "* the bartender stirs the stew and nods to nobody in particular.",
                "* the bartender wipes the bar until it shines.",
                "* the bartender hums a tune you almost remember.",
            ];

            // If the bartender vanished, re-ensure and stop.
            let Some(id) = world.bartender_id else {
                world.schedule_at_ms(world.now_ms(), EventKind::EnsureTavernMob);
                return Ok(());
            };
            let Some(c) = world.chars.get(&id) else {
                world.set_ambient_projected(None, world.bartender_emote_idx, "bartender-missing");
                world.schedule_at_ms(world.now_ms(), EventKind::EnsureTavernMob);
                return Ok(());
            };
            if c.room_id != ROOM_TAVERN {
                // Don't emote from elsewhere; just re-ensure.
                world.set_ambient_projected(None, world.bartender_emote_idx, "bartender-left");
                world.schedule_at_ms(world.now_ms(), EventKind::EnsureTavernMob);
                return Ok(());
            }

            let i = (world.bartender_emote_idx as usize) % EMOTES.len();
            world.set_ambient_projected(
                Some(id),
                world.bartender_emote_idx.saturating_add(1),
                "bartender-emote",
            );
            let _ = world.broadcast_room(fw, ROOM_TAVERN, EMOTES[i]).await;
            world.schedule_in_ms(world.bartender_emote_ms, EventKind::BartenderEmote);
        }
        EventKind::EnsureFirstFightWorm => {
            // Only spawn when a player is in the room, and only if there's no worm already.
            world.schedule_in_ms(5_000, EventKind::EnsureFirstFightWorm);

            let Some(occ) = world.occupants.get(ROOM_SCHOOL_FIRST_FIGHT) else {
                return Ok(());
            };
            let any_player = occ
                .iter()
                .any(|cid| world.chars.get(cid).and_then(|c| c.controller).is_some());
            if !any_player {
                return Ok(());
            }
            let worm_present = occ.iter().any(|cid| {
                world
                    .chars
                    .get(cid)
                    .is_some_and(|c| c.controller.is_none() && c.name == "stenchworm")
            });
            if worm_present {
                return Ok(());
            }

            let room_id = ROOM_SCHOOL_FIRST_FIGHT.to_string();
            world.spawn_stenchworm(room_id.clone());
            let _ = world
                .broadcast_room(
                    fw,
                    &room_id,
                    "* something wet wriggles in the drain. a stenchworm emerges.",
                )
                .await;
        }
        EventKind::EnsureClassHallMobs => {
            world.schedule_in_ms(60_000, EventKind::EnsureClassHallMobs);

            for (room_id, name) in CLASS_HALL_NPCS {
                if !world.rooms.has_room(room_id) {
                    continue;
                }
                if world.find_mob_in_room(room_id, name).is_some() {
                    continue;
                }
                world.spawn_mob(room_id.to_string(), name.to_string());
            }
        }
        EventKind::CombatAct { attacker_id } => {
            let Some(att) = world.chars.get(&attacker_id).cloned() else {
                return Ok(());
            };
            if !att.combat.autoattack {
                return Ok(());
            }
            let Some(target_id) = att.combat.target else {
                return Ok(());
            };
            if world.now_ms() < att.stunned_until_ms {
                world.schedule_at_ms(att.stunned_until_ms, EventKind::CombatAct { attacker_id });
                return Ok(());
            }
            if world.now_ms() < att.combat.next_ready_ms {
                world.schedule_at_ms(
                    att.combat.next_ready_ms,
                    EventKind::CombatAct { attacker_id },
                );
                return Ok(());
            }

            let Some(tgt) = world.chars.get(&target_id).cloned() else {
                world.set_combat_projected(
                    attacker_id,
                    false,
                    None,
                    att.combat.next_ready_ms,
                    "target-missing",
                );
                return Ok(());
            };
            if tgt.room_id != att.room_id {
                world.set_combat_projected(
                    attacker_id,
                    false,
                    None,
                    att.combat.next_ready_ms,
                    "target-left-room",
                );
                return Ok(());
            }

            let att_is_player = att.controller.is_some();
            let tgt_is_player = tgt.controller.is_some();

            // No mob-vs-mob combat for now.
            if !att_is_player && !tgt_is_player {
                world.set_combat_projected(
                    attacker_id,
                    false,
                    None,
                    att.combat.next_ready_ms,
                    "mob-vs-mob-disabled",
                );
                return Ok(());
            }
            // PvP is opt-in and only allowed in designated rooms.
            if att_is_player && tgt_is_player && !world.can_pvp_ids(attacker_id, target_id) {
                world.set_combat_projected(
                    attacker_id,
                    false,
                    None,
                    att.combat.next_ready_ms,
                    "pvp-not-allowed",
                );
                return Ok(());
            }

            let dmg = if att_is_player {
                compute_autoattack_damage(world, attacker_id)
            } else {
                compute_mob_autoattack_damage(world, &att)
            };

            let msg = format!("* {} hits {} for {}.", att.name, tgt.name, dmg);
            let killed = if tgt_is_player {
                apply_damage_to_player(world, fw, attacker_id, target_id, dmg, msg).await?
            } else {
                apply_damage_to_mob(world, fw, attacker_id, target_id, dmg, msg).await?
            };
            if killed {
                world.set_combat_projected(
                    attacker_id,
                    false,
                    None,
                    world.now_ms(),
                    "target-killed",
                );
                return Ok(());
            }

            let next = world.now_ms().saturating_add(1_000);
            world.set_combat_projected(attacker_id, true, Some(target_id), next, "autoattack-next");
            world.schedule_at_ms(next, EventKind::CombatAct { attacker_id });
        }
        EventKind::BossTelegraph { boss_id } => {
            let now = world.now_ms();
            let Some(b) = world.chars.get(&boss_id).cloned() else {
                world.clear_boss_projected(boss_id, "boss-missing");
                return Ok(());
            };
            if b.controller.is_some() || b.hp <= 0 {
                world.clear_boss_projected(boss_id, "boss-inactive");
                return Ok(());
            }
            // Decide next action without holding a mutable borrow across awaits/scheduling.
            let mut schedule_after_cast: Option<u64> = None;
            let mut resolve_at: Option<(u64, u64)> = None; // (due_ms, seq)
            let Some(bs) = world.bosses.get(&boss_id).copied() else {
                return Ok(());
            };
            // If still casting, don't start another cast; try again when cast ends.
            if bs.casting_until_ms > now {
                schedule_after_cast = Some(bs.casting_until_ms);
            } else {
                // Only one scripted boss for now.
                if b.name != "grease_king" {
                    return Ok(());
                }
                let cast_ms = 2500u64;
                let casting_until_ms = now.saturating_add(cast_ms);
                let seq = bs.seq.saturating_add(1);
                world.set_boss_projected(boss_id, casting_until_ms, seq, "boss-telegraph");
                resolve_at = Some((casting_until_ms, seq));
            }

            if let Some(due) = schedule_after_cast {
                world.schedule_at_ms(due, EventKind::BossTelegraph { boss_id });
                return Ok(());
            }

            let _ = world
                .broadcast_room(
                    fw,
                    &b.room_id,
                    "* grease_king begins casting grease_crush. interrupt it!",
                )
                .await;

            if let Some((due, seq)) = resolve_at {
                world.schedule_at_ms(due, EventKind::BossResolve { boss_id, seq });
                // Periodic cadence.
                world.schedule_in_ms(6500, EventKind::BossTelegraph { boss_id });
            }
        }
        EventKind::BossResolve { boss_id, seq } => {
            let now = world.now_ms();
            let Some(b) = world.chars.get(&boss_id).cloned() else {
                world.clear_boss_projected(boss_id, "boss-resolve-missing");
                return Ok(());
            };
            if b.controller.is_some() || b.hp <= 0 {
                world.clear_boss_projected(boss_id, "boss-resolve-inactive");
                return Ok(());
            }
            let Some(bs) = world.bosses.get(&boss_id).copied() else {
                return Ok(());
            };
            let should_resolve =
                if bs.seq != seq || bs.casting_until_ms == 0 || now < bs.casting_until_ms {
                    // Interrupted or superseded.
                    false
                } else {
                    world.set_boss_projected(boss_id, 0, bs.seq, "boss-resolve");
                    true
                };
            if !should_resolve {
                return Ok(());
            }

            let _ = world
                .broadcast_room(fw, &b.room_id, "* grease_king unleashes grease_crush!")
                .await;

            // AoE hit all players in the room.
            let victims = world
                .occupants_of(&b.room_id)
                .filter_map(|cid| world.chars.get(cid))
                .filter(|c| c.controller.is_some() && c.hp > 0)
                .map(|c| c.id)
                .collect::<Vec<_>>();

            for vid in victims {
                let vname = world
                    .chars
                    .get(&vid)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "someone".to_string());
                let dmg = 8;
                let msg = format!("* grease_king crushes {} for {}.", vname, dmg);
                let _ = apply_damage_to_player(world, fw, boss_id, vid, dmg, msg).await?;
            }
        }
        EventKind::MobWander { mob_id } => {
            let Some(m) = world.chars.get(&mob_id).cloned() else {
                return Ok(());
            };
            if m.controller.is_some() {
                return Ok(());
            }

            let exits = world
                .rooms
                .exits_raw(&m.room_id)
                .into_iter()
                .filter(|e| world.rooms.has_room(&e.to))
                .collect::<Vec<_>>();
            if exits.is_empty() {
                world.schedule_in_ms(world.mob_wander_ms, EventKind::MobWander { mob_id });
                return Ok(());
            }

            let idx = (world.rng_next_u64_projected("mob-wander") as usize) % exits.len();
            let ex = &exits[idx];
            let from = m.room_id.clone();
            let to = ex.to.clone();
            let dir = ex.dir.clone();

            world.move_char_projected(mob_id, to.clone(), "mob-wander");

            let _ = world
                .broadcast_room(fw, &from, &format!("* {} wanders {dir}.", m.name))
                .await;
            let _ = world
                .broadcast_room(fw, &to, &format!("* {} wanders in.", m.name))
                .await;

            world.schedule_in_ms(world.mob_wander_ms, EventKind::MobWander { mob_id });
        }
    }
    Ok(())
}

fn compute_autoattack_damage(world: &mut World, attacker_id: CharacterId) -> i32 {
    let Some((weapon_name, class, stats)) = world.chars.get(&attacker_id).map(|att| {
        (
            att.equip.get(items::EquipSlot::Wield).cloned(),
            att.class,
            att.stats,
        )
    }) else {
        return 2;
    };

    let mut dmg = if let Some(wname) = weapon_name.as_deref() {
        if let Some(def) = items::find_item_def(wname) {
            if let items::ItemKind::Weapon(w) = def.kind {
                world.rng_roll_range_projected(w.dmg_min, w.dmg_max, "weapon-autoattack")
            } else {
                2
            }
        } else {
            2
        }
    } else {
        2
    };

    // Add a small ability contribution. Keep simple, but let class identity matter a bit.
    if let Some(class) = class {
        let abil = class.attack_ability();
        dmg = dmg.saturating_add(stats.mod_for(abil));
    }

    dmg.max(1)
}

fn compute_mob_autoattack_damage(world: &mut World, att: &Character) -> i32 {
    // Keep simple and deterministic; use the per-world RNG.
    match att.name.as_str() {
        "dummy" => 0,
        "spitter" => world.rng_roll_range_projected(4, 6, "mob-autoattack"),
        "grease_king" => world.rng_roll_range_projected(2, 5, "mob-autoattack"),
        _ => world.rng_roll_range_projected(1, 3, "mob-autoattack"),
    }
}

async fn apply_damage_to_mob(
    world: &mut World,
    fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
    attacker_id: CharacterId,
    target_id: CharacterId,
    dmg: i32,
    msg: String,
) -> anyhow::Result<bool> {
    let Some(att) = world.chars.get(&attacker_id).cloned() else {
        return Ok(false);
    };
    let Some(tgt) = world.chars.get(&target_id).cloned() else {
        return Ok(false);
    };
    if tgt.controller.is_some() {
        return Ok(false);
    }
    if att.room_id != tgt.room_id {
        return Ok(false);
    }

    let room_id = att.room_id.clone();
    let _ = world.broadcast_room(fw, &room_id, &msg).await;

    let new_hp = {
        let Some(t) = world.chars.get(&target_id) else {
            return Ok(false);
        };
        t.hp.saturating_sub(dmg)
    };
    world.set_hp_projected(target_id, new_hp, "combat-damage");
    let dead = new_hp <= 0;
    if !dead {
        return Ok(false);
    }

    let Some(deadc) = world.remove_char_projected(target_id, "mob-killed") else {
        return Ok(true);
    };
    let _ = world
        .broadcast_room(fw, &room_id, &format!("* {} dies.", deadc.name))
        .await;

    if deadc.name == "stenchworm" {
        world.inv_add(attacker_id, ITEM_STENCHPOUCH, 1);
        if let Some(sid) = att.controller {
            let _ = write_resp_async(fw, RESP_OUTPUT, sid, b"you loot a stenchpouch.\r\n").await;
        }
        world.award_xp(fw, attacker_id, 10).await;

        // Re-spawn for the next trainee if needed.
        world.schedule_at_ms(world.now_ms(), EventKind::EnsureFirstFightWorm);
    }

    Ok(true)
}

async fn apply_damage_to_player(
    world: &mut World,
    fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
    attacker_id: CharacterId,
    target_id: CharacterId,
    dmg: i32,
    msg: String,
) -> anyhow::Result<bool> {
    let Some(att) = world.chars.get(&attacker_id).cloned() else {
        return Ok(false);
    };
    let Some(tgt) = world.chars.get(&target_id).cloned() else {
        return Ok(false);
    };
    if tgt.controller.is_none() {
        return Ok(false);
    }
    if att.room_id != tgt.room_id {
        return Ok(false);
    }

    let room_id = att.room_id.clone();
    let _ = world.broadcast_room(fw, &room_id, &msg).await;

    let new_hp = {
        let Some(t) = world.chars.get(&target_id) else {
            return Ok(false);
        };
        t.hp.saturating_sub(dmg)
    };
    world.set_hp_projected(target_id, new_hp, "combat-damage");
    let dead = new_hp <= 0;
    if !dead {
        return Ok(false);
    }

    let _ = world
        .broadcast_room(fw, &room_id, &format!("* {} dies.", tgt.name))
        .await;

    send_to_graveyard(world, fw, target_id).await?;
    Ok(true)
}

async fn send_to_graveyard(
    world: &mut World,
    fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
    cid: CharacterId,
) -> anyhow::Result<()> {
    let Some(c) = world.chars.get(&cid).cloned() else {
        return Ok(());
    };
    let to = if world.rooms.has_room(graveyard_room_id()) {
        graveyard_room_id().to_string()
    } else {
        world.rooms.start_room().to_string()
    };

    world.move_char_projected(cid, to.clone(), "graveyard");
    let reset_hp = world.chars.get(&cid).map(|cc| cc.max_hp.max(1));
    if let Some(reset_hp) = reset_hp {
        world.set_hp_projected(cid, reset_hp, "death-reset");
    }
    world.set_combat_projected(cid, false, None, world.now_ms(), "death-reset");

    let _ = world
        .broadcast_room(fw, &to, &format!("* {} arrives, shivering.", c.name))
        .await;

    if let Some(sid) = c.controller {
        let _ = write_resp_async(fw, RESP_OUTPUT, sid, b"you died.\\r\\n").await;
        let s = world.render_room_for(&to, sid);
        let _ = write_resp_async(fw, RESP_OUTPUT, sid, s.as_bytes()).await;
    }

    Ok(())
}

fn eval_gate_expr(p: &Character, expr: &str) -> bool {
    let expr = expr.trim();
    if expr.is_empty() {
        return true;
    }

    let lookup = |key: &str| -> Option<&str> { p.quest.get(key).map(String::as_str) };

    let truthy = |v: Option<&str>| -> bool {
        let Some(v) = v else {
            return false;
        };
        let v = v.trim();
        if v.is_empty() {
            return false;
        }
        if let Ok(n) = v.parse::<i64>() {
            return n != 0;
        }
        matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on")
    };

    // Operator precedence: match multi-char ops before single-char.
    let ops: [(&str, &str); 7] = [
        (">=", "ge"),
        ("<=", "le"),
        ("!=", "ne"),
        ("==", "eq"),
        (">", "gt"),
        ("<", "lt"),
        ("=", "eq"),
    ];
    for (tok, op) in ops {
        let Some(i) = expr.find(tok) else {
            continue;
        };
        let key = expr[..i].trim();
        let rhs = expr[i + tok.len()..].trim();
        if key.is_empty() || rhs.is_empty() {
            return false;
        }

        let lhs_raw = lookup(key).unwrap_or("0").trim();
        let lhs_num = lhs_raw.parse::<i64>().ok();
        let rhs_num = rhs.parse::<i64>().ok();

        match op {
            "eq" => match (lhs_num, rhs_num) {
                (Some(a), Some(b)) => return a == b,
                _ => return lhs_raw == rhs,
            },
            "ne" => match (lhs_num, rhs_num) {
                (Some(a), Some(b)) => return a != b,
                _ => return lhs_raw != rhs,
            },
            "ge" => return lhs_num.is_some_and(|a| rhs_num.is_some_and(|b| a >= b)),
            "le" => return lhs_num.is_some_and(|a| rhs_num.is_some_and(|b| a <= b)),
            "gt" => return lhs_num.is_some_and(|a| rhs_num.is_some_and(|b| a > b)),
            "lt" => return lhs_num.is_some_and(|a| rhs_num.is_some_and(|b| a < b)),
            _ => return false,
        }
    }

    // No operator: treat as a boolean gate key.
    truthy(lookup(expr))
}

async fn try_move(
    world: &mut World,
    fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
    session: SessionId,
    token: &str,
) -> anyhow::Result<bool> {
    let Some(cid) = world.active_char_id(session) else {
        return Ok(false);
    };
    let Some(p) = world.chars.get(&cid).cloned() else {
        return Ok(false);
    };
    let Some(ex) = world.rooms.find_exit(&p.room_id, token) else {
        return Ok(false);
    };
    let dir = ex.dir.as_str();
    let next = ex.to.as_str();

    if !is_built(&p) {
        if p.room_id == ROOM_SCHOOL_ORIENTATION {
            write_resp_async(
                fw,
                RESP_OUTPUT,
                session,
                b"trainer: finish setup first (race/class)\r\n",
            )
            .await?;
            return Ok(true);
        }
        if next != ROOM_SCHOOL_ORIENTATION {
            write_resp_async(
                fw,
                RESP_OUTPUT,
                session,
                b"finish setup first: return to orientation (race/class)\r\n",
            )
            .await?;
            return Ok(true);
        }
    }

    let gate_ok = ex.gate.as_deref().map_or(true, |g| eval_gate_expr(&p, g));
    if !gate_ok {
        if let Some(g) = ex.gate.as_deref() {
            let msg = format!("the way is sealed. (gate: {g})\r\n");
            write_resp_async(fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
        } else {
            write_resp_async(fw, RESP_OUTPUT, session, SEALED_EXIT_MSG).await?;
        }
        return Ok(true);
    }

    // A sealed exit can be conditionally opened by a gate expression.
    if ex.sealed && ex.gate.is_none() {
        write_resp_async(fw, RESP_OUTPUT, session, SEALED_EXIT_MSG).await?;
        return Ok(true);
    }

    if !world.rooms.has_room(next) {
        write_resp_async(fw, RESP_OUTPUT, session, SEALED_EXIT_MSG).await?;
        return Ok(true);
    }

    let from = p.room_id.clone();
    let to = next.to_string();

    world
        .broadcast_room(fw, &from, &format!("* {} goes {dir}", p.name))
        .await?;

    world.move_char_projected(cid, to.clone(), "player-move");

    world
        .broadcast_room(fw, &to, &format!("* {} arrives", p.name))
        .await?;

    let q2_msg = world
        .chars
        .get_mut(&cid)
        .and_then(|pp| q2_room_enter(pp, &to));
    if q2_msg.is_some() {
        world.snapshot_character_projected(cid, "q2-room-enter");
    }

    let s = world.render_room_for(&to, session);
    write_resp_async(fw, RESP_OUTPUT, session, s.as_bytes()).await?;
    if let Some(msg) = q2_msg {
        write_resp_async(fw, RESP_OUTPUT, session, msg.as_bytes()).await?;
    }

    // Party follow: if the mover is the party leader, bring along members in the same room
    // who have follow enabled.
    if let Some(pid) = world.party_of.get(&cid).copied() {
        let is_leader = world
            .parties
            .get(&pid)
            .is_some_and(|party| party.leader == cid);
        if is_leader {
            let mut followers = Vec::new();
            if let Some(party) = world.parties.get(&pid) {
                for mid in &party.members {
                    if *mid == cid {
                        continue;
                    }
                    let Some(m) = world.chars.get(mid) else {
                        continue;
                    };
                    if m.controller.is_none() {
                        continue;
                    }
                    if m.room_id != from {
                        continue;
                    }
                    if !m.follow_leader {
                        continue;
                    }
                    followers.push((*mid, m.controller));
                }
            }

            for (mid, msid) in followers {
                world.move_char_projected(mid, to.clone(), "party-follow");
                if let Some(msid) = msid {
                    let _ = world
                        .broadcast_room(
                            fw,
                            &from,
                            &format!(
                                "* {} follows {}.",
                                world
                                    .chars
                                    .get(&mid)
                                    .map(|c| c.name.as_str())
                                    .unwrap_or("someone"),
                                p.name
                            ),
                        )
                        .await;
                    let _ = world
                        .broadcast_room(
                            fw,
                            &to,
                            &format!(
                                "* {} arrives, following {}.",
                                world
                                    .chars
                                    .get(&mid)
                                    .map(|c| c.name.as_str())
                                    .unwrap_or("someone"),
                                p.name
                            ),
                        )
                        .await;
                    let rs = world.render_room_for(&to, msid);
                    let _ = write_resp_async(fw, RESP_OUTPUT, msid, rs.as_bytes()).await;
                }
            }
        }
    }
    Ok(true)
}

fn help_text() -> String {
    let s = "\
help\r\n\
report\r\n\
report last [n]\r\n\
report search <text>\r\n\
report submit <line_id> <reason> [note...]\r\n\
report locate <line_id>\r\n\
report reasons\r\n\
rules\r\n\
buildinfo\r\n\
version\r\n\
aiping\r\n\
uptime\r\n\
stats\r\n\
consider <mob|player>\r\n\
look\r\n\
look <thing>\r\n\
look board\r\n\
look chalk\r\n\
exits\r\n\
faction\r\n\
faction <civic|industrial|green>\r\n\
turn valve\r\n\
pull lever\r\n\
light pylon\r\n\
areas\r\n\
proto list\r\n\
proto <adventure_id>\r\n\
proto exit\r\n\
quest list\r\n\
quest get <key>\r\n\
quest set <key> <value>\r\n\
quest del <key>\r\n\
where (room)\r\n\
warp <room_id>\r\n\
party\r\n\
party create\r\n\
party disband\r\n\
party invite <player>\r\n\
party kick <player>\r\n\
party accept\r\n\
party leave\r\n\
party lead <player>\r\n\
party say <msg>\r\n\
party run <adventure_id>\r\n\
assist on|off\r\n\
follow on|off\r\n\
bot\r\n\
bot on|off\r\n\
botsense\r\n\
race list\r\n\
race <name>\r\n\
sessions\r\n\
sessions drop <character_id>\r\n\
class list\r\n\
class <name>\r\n\
train list\r\n\
train <skill>\r\n\
skills\r\n\
skills compendium\r\n\
skills <skill>\r\n\
equip\r\n\
equip <item>\r\n\
equipment (eq)\r\n\
remove <slot|item>\r\n\
wear <item>\r\n\
wield <item>\r\n\
quaff <item>\r\n\
use <skill|item>\r\n\
cast <skill>\r\n\
menu\r\n\
order <num|name>\r\n\
order <qty> <name>\r\n\
sell <item> <qty>\r\n\
i\r\n\
kill <mob|player>\r\n\
pvp on|off\r\n\
spawn <mob> [n]\r\n\
tell <player> <msg>\r\n\
friends\r\n\
friends add <player>\r\n\
friends del <player>\r\n\
who\r\n\
humans\r\n\
whomans\r\n\
go <exit>\r\n\
(or just type an exit name / alias)\r\n\
say <msg>\r\n\
emote <action>\r\n\
me <action>\r\n\
em <action>\r\n\
pose <action>\r\n\
yell <msg>\r\n\
whisper <player> <msg>\r\n\
dance\r\n\
twirl\r\n\
smile\r\n\
nod\r\n\
bow\r\n\
laugh\r\n\
sigh\r\n\
shrug\r\n\
wink\r\n\
salute\r\n\
wave\r\n\
clap\r\n\
cheer\r\n\
shout <msg>\r\n\
exit\r\n\
";
    s.to_string()
}

fn is_buildinfo_command(lc: &str) -> bool {
    lc == "buildinfo" || lc == "version"
}

fn render_buildinfo() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let profile = option_env!("SLOPMUD_PROFILE").unwrap_or("unknown");
    let sha = option_env!("SLOPMUD_GIT_SHA").unwrap_or("unknown");
    let dirty = option_env!("SLOPMUD_GIT_DIRTY").unwrap_or("0") == "1";
    let built_utc = option_env!("SLOPMUD_BUILD_UTC").unwrap_or("unknown");
    let built_unix = option_env!("SLOPMUD_BUILD_UNIX").unwrap_or("unknown");

    let mut s = String::new();
    s.push_str("buildinfo:\r\n");
    s.push_str(&format!(" - version: {version}\r\n"));
    s.push_str(&format!(
        " - git: {sha}{}\r\n",
        if dirty { " (dirty)" } else { "" }
    ));
    s.push_str(&format!(" - built_at_utc: {built_utc}\r\n"));
    s.push_str(&format!(" - built_at_unix: {built_unix}\r\n"));
    s.push_str(&format!(" - profile: {profile}\r\n"));
    s
}

fn list_protoadventures() -> anyhow::Result<Vec<String>> {
    let dir = Path::new("protoadventures");
    let mut out = Vec::new();
    for ent in std::fs::read_dir(dir).context("read protoadventures/")? {
        let ent = ent.context("read_dir entry")?;
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.eq_ignore_ascii_case("README.md") || name.eq_ignore_ascii_case("_TEMPLATE.md") {
            continue;
        }
        if name.starts_with('_') {
            continue;
        }
        let Some(stem) = name.strip_suffix(".md") else {
            continue;
        };
        out.push(stem.to_string());
    }
    out.sort_unstable();
    Ok(out)
}

async fn teleport_to(
    world: &mut World,
    fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
    session: SessionId,
    to_room: &str,
    verb: &str,
) -> anyhow::Result<()> {
    let Some(cid) = world.active_char_id(session) else {
        return Ok(());
    };
    let Some(p) = world.chars.get(&cid).cloned() else {
        return Ok(());
    };
    if !world.rooms.has_room(to_room) {
        write_resp_async(
            fw,
            RESP_OUTPUT,
            session,
            b"huh? (destination room not found)\r\n",
        )
        .await?;
        return Ok(());
    }

    let from = p.room_id.clone();
    let to = to_room.to_string();

    world
        .broadcast_room(fw, &from, &format!("* {} {verb}", p.name))
        .await?;

    world.move_char_projected(cid, to.clone(), "teleport");

    let s = world.render_room_for(&to, session);
    write_resp_async(fw, RESP_OUTPUT, session, s.as_bytes()).await?;
    Ok(())
}

fn rules_text() -> String {
    let mut s = String::new();
    s.push_str("code of conduct:\r\n");
    for li in COC_LINE_ITEMS {
        s.push_str(li);
        s.push_str("\r\n");
    }
    s
}

fn normalize_dir(line: &str) -> Option<&'static str> {
    match line.trim().to_ascii_lowercase().as_str() {
        "north" | "n" => Some("north"),
        "south" | "s" => Some("south"),
        "east" | "e" => Some("east"),
        "west" | "w" => Some("west"),
        "up" | "u" => Some("up"),
        "down" | "d" => Some("down"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn version_command_is_advertised_and_returns_build_timestamp() {
        assert!(is_buildinfo_command("version"));
        assert!(is_buildinfo_command("buildinfo"));
        assert!(!is_buildinfo_command("versions"));

        let help = help_text();
        assert!(help.contains("version\r\n"));
        assert!(help.contains("buildinfo\r\n"));

        let out = render_buildinfo();
        assert!(out.starts_with("buildinfo:\r\n"));
        assert!(out.contains(" - version: "));
        assert!(out.contains(" - git: "));
        assert!(out.contains(" - built_at_utc: "));
        assert!(out.contains(" - built_at_unix: "));
        assert!(out.contains(" - profile: "));
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum StateClass {
        RaftProjected,
        DerivedIndex,
        ProjectionHandle,
        TransportLocal,
        StaticConfig,
        RuntimeConfig,
        RuntimeClock,
        MigrationDebt,
    }

    struct StateField {
        owner: &'static str,
        field: &'static str,
        class: StateClass,
        domain: &'static str,
        debt: Option<&'static str>,
    }

    const fn sf(
        owner: &'static str,
        field: &'static str,
        class: StateClass,
        domain: &'static str,
        debt: Option<&'static str>,
    ) -> StateField {
        StateField {
            owner,
            field,
            class,
            domain,
            debt,
        }
    }

    struct MigrationDebt {
        domain: &'static str,
        reason: &'static str,
        sentinels: &'static [&'static str],
    }

    static STATE_FIELD_MANIFEST: &[StateField] = &[
        sf("World", "rooms", StateClass::StaticConfig, "rooms", None),
        sf(
            "World",
            "sessions",
            StateClass::TransportLocal,
            "transport",
            None,
        ),
        sf(
            "World",
            "chars",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "World",
            "occupants",
            StateClass::DerivedIndex,
            "runtime-character",
            None,
        ),
        sf("World", "parties", StateClass::RaftProjected, "party", None),
        sf(
            "World",
            "party_invites",
            StateClass::RaftProjected,
            "party",
            None,
        ),
        sf(
            "World",
            "next_party_id",
            StateClass::DerivedIndex,
            "party",
            None,
        ),
        sf("World", "party_of", StateClass::DerivedIndex, "party", None),
        sf("World", "rng", StateClass::RaftProjected, "random", None),
        sf(
            "World",
            "next_char_id",
            StateClass::DerivedIndex,
            "runtime-character",
            None,
        ),
        sf(
            "World",
            "now_ms",
            StateClass::RaftProjected,
            "timeline",
            None,
        ),
        sf(
            "World",
            "clock_last_instant",
            StateClass::RuntimeClock,
            "timeline",
            None,
        ),
        sf(
            "World",
            "started_instant",
            StateClass::RuntimeClock,
            "process",
            None,
        ),
        sf(
            "World",
            "started_unix",
            StateClass::RuntimeClock,
            "process",
            None,
        ),
        sf(
            "World",
            "event_seq",
            StateClass::RaftProjected,
            "timeline",
            None,
        ),
        sf(
            "World",
            "events",
            StateClass::RaftProjected,
            "timeline",
            None,
        ),
        sf(
            "World",
            "bartender_id",
            StateClass::RaftProjected,
            "ambient",
            None,
        ),
        sf(
            "World",
            "bartender_emote_idx",
            StateClass::RaftProjected,
            "ambient",
            None,
        ),
        sf(
            "World",
            "bartender_emote_ms",
            StateClass::RuntimeConfig,
            "config",
            None,
        ),
        sf(
            "World",
            "mob_wander_ms",
            StateClass::RuntimeConfig,
            "config",
            None,
        ),
        sf(
            "World",
            "time_scale_ppm",
            StateClass::RuntimeConfig,
            "config",
            None,
        ),
        sf("World", "bosses", StateClass::RaftProjected, "boss", None),
        sf(
            "World",
            "raft",
            StateClass::ProjectionHandle,
            "raft-log",
            None,
        ),
        sf(
            "World",
            "raft_watch",
            StateClass::TransportLocal,
            "transport",
            None,
        ),
        sf(
            "World",
            "seen_commands",
            StateClass::RaftProjected,
            "idempotency",
            None,
        ),
        sf("World", "groups", StateClass::RaftProjected, "group", None),
        sf(
            "World",
            "cluster_features",
            StateClass::RaftProjected,
            "cluster-feature",
            None,
        ),
        sf(
            "World",
            "cluster_metadata",
            StateClass::RaftProjected,
            "cluster-feature",
            None,
        ),
        sf(
            "SessionState",
            "controlled",
            StateClass::TransportLocal,
            "transport",
            None,
        ),
        sf(
            "SessionState",
            "active",
            StateClass::TransportLocal,
            "transport",
            None,
        ),
        sf(
            "SessionState",
            "pending_confirm",
            StateClass::TransportLocal,
            "transport",
            None,
        ),
        sf(
            "Character",
            "id",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "controller",
            StateClass::TransportLocal,
            "transport",
            None,
        ),
        sf(
            "Character",
            "created_by",
            StateClass::TransportLocal,
            "transport",
            None,
        ),
        sf(
            "Character",
            "name",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "principal",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "auth_caps",
            StateClass::RaftProjected,
            "auth",
            None,
        ),
        sf(
            "Character",
            "is_bot",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "bot_ever",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "bot_ever_since_ms",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "bot_mode_changed_ms",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "friends",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "room_id",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "autoassist",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "follow_leader",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "drink_level",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "gold",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "inv",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "quest",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "class",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "level",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf("Character", "xp", StateClass::RaftProjected, "player", None),
        sf(
            "Character",
            "skill_points",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "skills",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "skill_cd_ms",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "race",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "sex",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "pronouns",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "stats",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "hp",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "max_hp",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "mana",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "max_mana",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "stamina",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "max_stamina",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "last_mana_regen_ms",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "last_stamina_regen_ms",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "pvp_enabled",
            StateClass::RaftProjected,
            "player",
            None,
        ),
        sf(
            "Character",
            "stunned_until_ms",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "combat",
            StateClass::RaftProjected,
            "runtime-character",
            None,
        ),
        sf(
            "Character",
            "equip",
            StateClass::RaftProjected,
            "player",
            None,
        ),
    ];

    static MIGRATION_DEBT_MANIFEST: &[MigrationDebt] = &[];

    fn struct_field_names(source: &str, owner: &str) -> BTreeSet<String> {
        let needle = format!("struct {owner} {{");
        let Some(start) = source.find(&needle) else {
            panic!("missing struct {owner}");
        };
        let body_start = start + needle.len();
        let mut depth = 1i32;
        let mut end = body_start;
        for (off, ch) in source[body_start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = body_start + off;
                        break;
                    }
                }
                _ => {}
            }
        }
        assert!(end > body_start, "could not parse struct {owner}");
        source[body_start..end]
            .lines()
            .filter_map(|line| {
                let line = line.split("//").next().unwrap_or("").trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with("pub ") {
                    return None;
                }
                let (name, _) = line.split_once(':')?;
                let name = name.trim();
                if name.is_empty() {
                    return None;
                }
                Some(name.to_string())
            })
            .collect()
    }

    fn fields_for(owner: &'static str) -> Vec<&'static StateField> {
        STATE_FIELD_MANIFEST
            .iter()
            .filter(|f| f.owner == owner)
            .collect()
    }

    #[test]
    fn state_boundary_manifest_covers_stateful_struct_fields() {
        let source = include_str!("main.rs");
        for owner in ["World", "Character", "SessionState"] {
            let parsed = struct_field_names(source, owner);
            let declared = fields_for(owner)
                .into_iter()
                .map(|f| f.field.to_string())
                .collect::<BTreeSet<_>>();
            let missing = parsed.difference(&declared).cloned().collect::<Vec<_>>();
            let stale = declared.difference(&parsed).cloned().collect::<Vec<_>>();
            assert!(
                missing.is_empty() && stale.is_empty(),
                "state boundary manifest mismatch for {owner}: missing={missing:?} stale={stale:?}"
            );
        }
    }

    #[test]
    fn migration_debt_is_explicit_and_domain_tracked() {
        let debts = MIGRATION_DEBT_MANIFEST
            .iter()
            .map(|d| d.domain)
            .collect::<BTreeSet<_>>();
        for field in STATE_FIELD_MANIFEST
            .iter()
            .filter(|f| f.class == StateClass::MigrationDebt)
        {
            assert!(
                field.debt.is_some_and(|d| !d.trim().is_empty()),
                "migration debt field {}.{} needs a reason",
                field.owner,
                field.field
            );
            assert!(
                debts.contains(field.domain),
                "migration debt field {}.{} uses untracked domain {}",
                field.owner,
                field.field,
                field.domain
            );
        }

        let source = include_str!("main.rs");
        for debt in MIGRATION_DEBT_MANIFEST {
            assert!(
                !debt.reason.trim().is_empty(),
                "migration debt {} needs a reason",
                debt.domain
            );
            for sentinel in debt.sentinels {
                assert!(
                    source.contains(sentinel),
                    "migration debt {} sentinel {sentinel:?} no longer exists; update the manifest",
                    debt.domain
                );
            }
        }
    }

    #[test]
    fn only_declared_state_classes_are_allowed_to_stay_local() {
        let mut counts = BTreeMap::<&str, usize>::new();
        for f in STATE_FIELD_MANIFEST {
            if !matches!(
                f.class,
                StateClass::RaftProjected
                    | StateClass::DerivedIndex
                    | StateClass::ProjectionHandle
                    | StateClass::TransportLocal
                    | StateClass::StaticConfig
                    | StateClass::RuntimeConfig
                    | StateClass::RuntimeClock
                    | StateClass::MigrationDebt
            ) {
                panic!("unsupported state class for {}.{}", f.owner, f.field);
            }
            *counts.entry(f.owner).or_default() += 1;
        }
        assert!(counts.get("World").copied().unwrap_or_default() > 0);
        assert!(counts.get("Character").copied().unwrap_or_default() > 0);
        assert!(counts.get("SessionState").copied().unwrap_or_default() > 0);
    }

    #[test]
    fn client_command_seen_projection_survives_replay() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_seen_commands_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let rooms = rooms::Rooms::load().unwrap();
        let mut world = World::new(
            rooms,
            1,
            30_000,
            15_000,
            1_000_000,
            path.clone(),
            raftlog::ConsensusConfig::disabled("test".to_string()),
            vec![],
            vec![],
        )
        .unwrap();
        let session = SessionId(123);
        assert!(world.remember_client_command(session, Some(7), "acct:alice"));
        assert!(!world.remember_client_command(session, Some(7), "acct:alice"));
        drop(world);

        let rooms = rooms::Rooms::load().unwrap();
        let mut replayed = World::new(
            rooms,
            1,
            30_000,
            15_000,
            1_000_000,
            path,
            raftlog::ConsensusConfig::disabled("test".to_string()),
            vec![],
            vec![],
        )
        .unwrap();
        assert!(!replayed.remember_client_command(session, Some(7), "acct:alice"));
        assert!(replayed.remember_client_command(session, Some(8), "acct:alice"));
    }

    #[test]
    fn cluster_feature_projection_survives_replay() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_cluster_features_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let rooms = rooms::Rooms::load().unwrap();
        let mut world = World::new(
            rooms,
            1,
            30_000,
            15_000,
            1_000_000,
            path.clone(),
            raftlog::ConsensusConfig::disabled("test".to_string()),
            vec![],
            vec![],
        )
        .unwrap();
        assert!(
            world
                .ensure_persistence_format_write_allowed(
                    worldlog::PersistenceArtifactKind::WorldSnapshot,
                    2,
                )
                .is_err(),
            "format-2 snapshots must be blocked before activation"
        );
        world
            .raft_append_cluster_feature_activation(worldlog::WorldEvent::ClusterFeatureSet {
                active_world_log_format: 2,
                snapshot_format: 2,
                min_reader_world_log_format: 2,
                reason: "test-activate".to_string(),
            })
            .unwrap();
        world
            .raft_append_world(worldlog::WorldEvent::ClusterMetadataSet {
                key: "rollout.probe".to_string(),
                value: Some("active".to_string()),
                reason: "test-format2".to_string(),
            })
            .unwrap();
        assert_eq!(world.cluster_features.active_world_log_format, 2);
        assert_eq!(
            world
                .cluster_metadata
                .get("rollout.probe")
                .map(String::as_str),
            Some("active")
        );
        world
            .ensure_persistence_format_write_allowed(
                worldlog::PersistenceArtifactKind::WorldSnapshot,
                2,
            )
            .unwrap();
        drop(world);

        let rooms = rooms::Rooms::load().unwrap();
        let replayed = World::new(
            rooms,
            1,
            30_000,
            15_000,
            1_000_000,
            path.clone(),
            raftlog::ConsensusConfig::disabled("test".to_string()),
            vec![],
            vec![],
        )
        .unwrap();
        assert_eq!(
            replayed.cluster_features,
            worldlog::ClusterFeatureState {
                active_world_log_format: 2,
                snapshot_format: 2,
                min_reader_world_log_format: 2,
            }
        );
        assert_eq!(
            replayed
                .cluster_metadata
                .get("rollout.probe")
                .map(String::as_str),
            Some("active")
        );

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn format2_equipment_slot_requires_active_world_log_format() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_equipment_slot_gate_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let rooms = rooms::Rooms::load().unwrap();
        let mut world = World::new(
            rooms,
            1,
            30_000,
            15_000,
            1_000_000,
            path.clone(),
            raftlog::ConsensusConfig::disabled("test".to_string()),
            vec![],
            vec![],
        )
        .unwrap();
        world
            .ensure_equip_slot_write_allowed(items::EquipSlot::Wield)
            .unwrap();
        assert!(starter_kit_extras_for_world_log_format(1).is_empty());
        let err = world
            .ensure_equip_slot_write_allowed(items::EquipSlot::Neck)
            .unwrap_err()
            .to_string();
        assert!(err.contains("requires active world log format 2"));

        world
            .raft_append_cluster_feature_activation(worldlog::WorldEvent::ClusterFeatureSet {
                active_world_log_format: 2,
                snapshot_format: 2,
                min_reader_world_log_format: 2,
                reason: "test-activate".to_string(),
            })
            .unwrap();
        world
            .ensure_equip_slot_write_allowed(items::EquipSlot::Neck)
            .unwrap();
        assert_eq!(
            starter_kit_extras_for_world_log_format(2),
            &[("training charm", 1)]
        );

        let entry: worldlog::WorldLogEntry = serde_json::from_str(
            r#"{
                "t":"CharacterSnapshot",
                "character":{
                    "character_id":42,
                    "name":"Alice",
                    "room_id":"town.gate",
                    "equip":{"neck":"training charm"}
                },
                "reason":"format2-replay"
            }"#,
        )
        .unwrap();
        world.apply_log_entry(&entry);
        let character = world.chars.get(&42).unwrap();
        assert_eq!(
            character
                .equip
                .get(items::EquipSlot::Neck)
                .map(String::as_str),
            Some("training charm")
        );
        let snapshot = world.snapshot_for_character(character);
        assert_eq!(
            snapshot.equip.get("neck").map(String::as_str),
            Some("training charm")
        );

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn versioned_dict_splits_known_and_unknown_entries() {
        let raw = BTreeMap::from([
            ("wield".to_string(), "practice sword (medium)".to_string()),
            ("ring".to_string(), "copper ring".to_string()),
        ]);
        let mut dict = VersionedDict::<items::EquipSlot, String>::from_snapshot(&raw);

        assert_eq!(
            dict.get_known(items::EquipSlot::Wield).map(String::as_str),
            Some("practice sword (medium)")
        );
        assert_eq!(
            dict.known_entries()
                .map(|(key, value)| (key.as_str().to_string(), value.clone()))
                .collect::<BTreeMap<_, _>>(),
            BTreeMap::from([("wield".to_string(), "practice sword (medium)".to_string())])
        );
        assert_eq!(
            dict.unknown_entries()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect::<Vec<_>>(),
            vec![("ring".to_string(), "copper ring".to_string())]
        );

        dict.set_known(
            items::EquipSlot::Shield,
            "wooden buckler (medium)".to_string(),
        );
        dict.clear_known(items::EquipSlot::Wield);
        let snapshot = dict.snapshot_map();
        assert_eq!(
            snapshot.get("ring").map(String::as_str),
            Some("copper ring")
        );
        assert_eq!(
            snapshot.get("shield").map(String::as_str),
            Some("wooden buckler (medium)")
        );
        assert!(!snapshot.contains_key("wield"));
    }

    #[test]
    fn future_equipment_slots_are_preserved_but_not_actionable() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_future_equipment_preserve_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let rooms = rooms::Rooms::load().unwrap();
        let mut world = World::new(
            rooms,
            1,
            30_000,
            15_000,
            1_000_000,
            path.clone(),
            raftlog::ConsensusConfig::disabled("test".to_string()),
            vec![],
            vec![],
        )
        .unwrap();
        let entry: worldlog::WorldLogEntry = serde_json::from_str(
            r#"{
                "t":"CharacterSnapshot",
                "character":{
                    "character_id":77,
                    "name":"Future Alice",
                    "room_id":"town.gate",
                    "equip":{
                        "ring":"copper ring",
                        "wield":"practice sword (medium)"
                    }
                },
                "reason":"future-slot-replay"
            }"#,
        )
        .unwrap();
        world.apply_log_entry(&entry);

        let character = world.chars.get_mut(&77).unwrap();
        assert_eq!(
            character
                .equip
                .get(items::EquipSlot::Wield)
                .map(String::as_str),
            Some("practice sword (medium)")
        );
        assert!(matches!(
            find_equipped_slot_by_token(&character.equip, "ring"),
            SlotMatch::None
        ));
        character.gold = 12;
        world.snapshot_character_projected(77, "future-slot-unrelated-writeback");

        let snapshot = world.snapshot_for_character(world.chars.get(&77).unwrap());
        assert_eq!(snapshot.gold, 12);
        assert_eq!(
            snapshot.equip.get("ring").map(String::as_str),
            Some("copper ring")
        );
        assert_eq!(
            snapshot.equip.get("wield").map(String::as_str),
            Some("practice sword (medium)")
        );

        let removed = world
            .chars
            .get_mut(&77)
            .and_then(|c| c.equip.clear(items::EquipSlot::Wield));
        assert_eq!(removed.as_deref(), Some("practice sword (medium)"));
        world.snapshot_character_projected(77, "future-slot-known-clear");
        let snapshot = world.snapshot_for_character(world.chars.get(&77).unwrap());
        assert_eq!(
            snapshot.equip.get("ring").map(String::as_str),
            Some("copper ring")
        );
        assert!(!snapshot.equip.contains_key("wield"));

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn format2_metadata_event_requires_active_world_log_format() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_cluster_metadata_gate_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let rooms = rooms::Rooms::load().unwrap();
        let mut world = World::new(
            rooms,
            1,
            30_000,
            15_000,
            1_000_000,
            path.clone(),
            raftlog::ConsensusConfig::disabled("test".to_string()),
            vec![],
            vec![],
        )
        .unwrap();
        let event = worldlog::WorldEvent::ClusterMetadataSet {
            key: "rollout.probe".to_string(),
            value: Some("blocked".to_string()),
            reason: "test-format2".to_string(),
        };
        let err = world
            .raft_append_world(event.clone())
            .unwrap_err()
            .to_string();
        assert!(err.contains("requires active format 2"));
        world.project_world(event);
        assert!(!world.cluster_metadata.contains_key("rollout.probe"));
        let log = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!log.contains("ClusterMetadataSet"));

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn cluster_feature_activation_cannot_use_normal_append_path() {
        let path = std::env::temp_dir().join(format!(
            "slopmud_cluster_feature_gate_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = PathBuf::from(format!("{}.state.json", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&state_path);

        let rooms = rooms::Rooms::load().unwrap();
        let mut world = World::new(
            rooms,
            1,
            30_000,
            15_000,
            1_000_000,
            path.clone(),
            raftlog::ConsensusConfig::disabled("test".to_string()),
            vec![],
            vec![],
        )
        .unwrap();
        let err = world
            .raft_append_world(worldlog::WorldEvent::ClusterFeatureSet {
                active_world_log_format: 2,
                snapshot_format: 2,
                min_reader_world_log_format: 2,
                reason: "test-activate".to_string(),
            })
            .unwrap_err()
            .to_string();
        assert!(err.contains("cluster feature activation must use raft feature worldlog"));
        assert_eq!(
            world.cluster_features,
            worldlog::ClusterFeatureState::default()
        );
        world.project_world(worldlog::WorldEvent::ClusterFeatureSet {
            active_world_log_format: 2,
            snapshot_format: 2,
            min_reader_world_log_format: 2,
            reason: "test-project".to_string(),
        });
        assert_eq!(
            world.cluster_features,
            worldlog::ClusterFeatureState::default()
        );
        let log = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!log.contains("ClusterFeatureSet"));

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn random_sources_are_seeded_and_declared() {
        let source = include_str!("main.rs");
        let forbidden_sources = [
            format!("{}{}", "thread_", "rng"),
            format!("{}{}", "rand::", "random"),
            format!("{}{}", "SmallRng::from_", "entropy"),
        ];
        for forbidden in forbidden_sources {
            assert!(
                !source.contains(&forbidden),
                "non-seeded rng source {forbidden} is not allowed in shard state"
            );
        }
        assert!(source.contains("Rng64::from_seed"));
    }

    fn enclosing_fn_for_line<'a>(source_before_line: impl Iterator<Item = &'a str>) -> String {
        let mut current = String::new();
        for line in source_before_line {
            let trimmed = line.trim_start();
            let rest = trimmed
                .strip_prefix("fn ")
                .or_else(|| trimmed.strip_prefix("async fn "));
            if let Some(rest) = rest {
                let name = rest
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("");
                current = name.to_string();
            }
        }
        current
    }

    #[test]
    fn occupant_index_mutations_stay_inside_projection_helpers() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from([
            "apply_world_event",
            "rebuild_occupants",
            "remove_occupant_from",
            "reindex_char_room",
            "move_char_local",
        ]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_occupants = line.contains("occupants.clear(")
                || line.contains("occupants.entry(")
                || line.contains("occupants.get_mut(")
                || line.contains("occupants.remove(");
            if !mutates_occupants {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "occupants mutation at line {} is outside projection helpers: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn combat_state_mutations_stay_inside_projection_helpers() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from(["apply_world_event"]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_combat = line.contains(".combat.autoattack = ")
                || line.contains(".combat.target = ")
                || line.contains(".combat.next_ready_ms = ");
            if !mutates_combat {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "combat state mutation at line {} is outside projection apply path: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn stun_state_mutations_stay_inside_projection_helpers() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from(["apply_world_event", "apply_character_snapshot"]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_stun = line.contains(".stunned_until_ms = ");
            if !mutates_stun {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "stun state mutation at line {} is outside raft projection apply path: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn boss_state_mutations_stay_inside_projection_helpers() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from(["apply_world_event", "remove_char_local"]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_boss = line.contains(".bosses.insert(")
                || line.contains(".bosses.remove(")
                || line.contains(".bosses.get_mut(");
            if !mutates_boss {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "boss state mutation at line {} is outside raft projection apply path: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn ambient_state_mutations_stay_inside_projection_helpers() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from(["apply_world_event", "move_char_local", "remove_char_local"]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_ambient =
                line.contains(".bartender_id = ") || line.contains(".bartender_emote_idx = ");
            if !mutates_ambient {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "ambient state mutation at line {} is outside raft projection apply path: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn rng_cursor_mutations_stay_inside_projection_helpers() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from(["apply_world_event", "rng_next_u64_projected"]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_rng = line.contains(".rng.next_u64(") || line.contains(".rng.state = ");
            if !mutates_rng {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "rng cursor mutation at line {} is outside raft projection helpers: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn scheduled_event_mutations_stay_inside_projection_helpers() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from([
            "apply_world_event",
            "schedule_event_local",
            "remove_scheduled_event_local",
            "schedule_at_ms",
        ]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_schedule = line.contains(".events.push(")
                || line.contains(".events.pop(")
                || line.contains(".event_seq = ");
            if !mutates_schedule {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "scheduled event mutation at line {} is outside raft projection helpers: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn dynamic_room_mutations_stay_inside_projection_helpers() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from(["apply_world_event"]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_rooms = line.contains(".rooms.insert_room(")
                || line.contains(".rooms.remove_dyn_room(")
                || line.contains(".rooms.clear_dyn_rooms_with_prefix(");
            if !mutates_rooms {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "dynamic room mutation at line {} is outside raft projection apply path: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn party_membership_mutations_stay_inside_projection_helpers() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from([
            "apply_world_event",
            "party_leave_local",
            "remove_char_local",
        ]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_party_state = line.contains(".parties.insert(")
                || line.contains(".parties.remove(")
                || line.contains(".parties.get_mut(")
                || line.contains(".party_of.insert(")
                || line.contains(".party_of.remove(")
                || line.contains(".party_invites.insert(")
                || line.contains(".party_invites.remove(")
                || line.contains(".party_invites.retain(");
            if !mutates_party_state {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "party state mutation at line {} is outside raft projection apply path: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn world_clock_mutations_stay_inside_projection_apply_path() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from(["apply_world_event"]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_clock = line.contains(".now_ms = ");
            if !mutates_clock {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "world clock mutation at line {} is outside raft projection apply path: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn cluster_feature_mutations_stay_inside_projection_apply_path() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from(["apply_world_event"]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_cluster_features = line.contains(".cluster_features = ");
            if !mutates_cluster_features {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "cluster feature mutation at line {} is outside raft projection apply path: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn cluster_metadata_mutations_stay_inside_projection_apply_path() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from(["apply_world_event"]);
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_cluster_metadata = line.contains(".cluster_metadata.insert(")
                || line.contains(".cluster_metadata.remove(");
            if !mutates_cluster_metadata {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            assert!(
                allowed.contains(f.as_str()),
                "cluster metadata mutation at line {} is outside raft projection apply path: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn direct_player_state_mutations_are_snapshotted_or_projected() {
        let source = include_str!("main.rs");
        let lines = source.lines().collect::<Vec<_>>();
        let allowed = BTreeSet::from([
            "apply_world_event",
            "apply_character_snapshot",
            "attach_existing_character",
            "award_xp_one",
            "complete_contract",
            "mob_character_from_projection",
            "q2_room_enter",
            "regen_resources",
            "set_contracts_done",
            "set_state_for_done",
            "spawn_character",
        ]);

        for (idx, line) in lines.iter().enumerate() {
            if line.contains("line.contains(") {
                continue;
            }
            let mutates_player_state = line.contains(".quest.insert(")
                || line.contains(".quest.remove(")
                || line.contains(".skills.insert(")
                || line.contains(".skill_points = ")
                || line.contains(".skill_points -= ")
                || line.contains(".mana = ")
                || line.contains(".stamina = ")
                || line.contains(".drink_level = ")
                || line.contains(".gold = ")
                || line.contains(".gold -= ")
                || line.contains(".friends.insert(")
                || line.contains(".friends.retain(")
                || line.contains(".autoassist = ")
                || line.contains(".follow_leader = ")
                || line.contains(".pvp_enabled = ")
                || line.contains(".equip.set(")
                || line.contains(".equip.clear(")
                || line.contains(".hp = ")
                || line.contains(".max_hp = ")
                || line.contains(".max_mana = ")
                || line.contains(".max_stamina = ")
                || line.contains(".race = ")
                || line.contains(".class = ")
                || line.contains(".sex = ")
                || line.contains(".pronouns = ")
                || line.contains(".is_bot = ")
                || line.contains(".bot_ever = ")
                || line.contains(".bot_ever_since_ms = ")
                || line.contains(".bot_mode_changed_ms = ");
            if !mutates_player_state {
                continue;
            }
            let f = enclosing_fn_for_line(lines[..=idx].iter().copied());
            if allowed.contains(f.as_str()) {
                continue;
            }
            let lookahead = lines.iter().skip(idx).take(45).any(|l| {
                l.contains("snapshot_character_projected(")
                    || l.contains("set_hp_projected(")
                    || l.contains("set_combat_projected(")
            });
            assert!(
                lookahead,
                "player state mutation at line {} is not followed by a raft projection snapshot: {}",
                idx + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn command_arg_extracts_action_text() {
        assert_eq!(
            command_arg("shout hello everyone", "shout"),
            Some("hello everyone")
        );
        assert_eq!(command_arg("emote   grins", "emote"), Some("grins"));
        assert_eq!(command_arg("pose", "pose"), None);
        assert_eq!(command_arg("pose   ", "pose"), None);
        assert_eq!(command_arg("pose   bows", "pose"), Some("bows"));
    }

    #[test]
    fn shout_payload_rejects_empty_messages() {
        assert_eq!(
            shout_payload("shout hello everyone", "Alice"),
            Some("Alice shouts: hello everyone".to_string())
        );
        assert_eq!(shout_payload("shout", "Alice"), None);
        assert_eq!(shout_payload("shout   ", "Alice"), None);
    }

    #[test]
    fn parse_tell_args_parses_targets_and_messages() {
        assert_eq!(
            parse_tell_args("tell alice hi there", "tell"),
            Some(("alice", "hi there"))
        );
        assert_eq!(
            parse_tell_args("whisper bob let's go", "whisper"),
            Some(("bob", "let's go"))
        );
        assert_eq!(parse_tell_args("tell", "tell"), None);
        assert_eq!(parse_tell_args("tell   ", "tell"), None);
        assert_eq!(parse_tell_args("whisper bob", "whisper"), None);
    }

    #[test]
    fn room_emote_payload_supports_aliases() {
        assert_eq!(
            room_emote_payload("emote bows", "Alice", "emote"),
            Some("* Alice bows".to_string())
        );
        assert_eq!(
            room_emote_payload("me dances", "Alice", "me"),
            Some("* Alice dances".to_string())
        );
        assert_eq!(
            room_emote_payload("pose salutes", "Alice", "pose"),
            Some("* Alice salutes".to_string())
        );
        assert_eq!(room_emote_payload("pose", "Alice", "pose"), None);
        assert_eq!(room_emote_payload("pose   ", "Alice", "pose"), None);
        assert_eq!(
            room_emote_payload("em grins", "Alice", "em"),
            Some("* Alice grins".to_string())
        );
    }

    #[test]
    fn room_emote_noarg_generates_room_motion() {
        assert_eq!(room_emote_noarg("Alice", "dances"), "* Alice dances");
        assert_eq!(room_emote_noarg("Alice", "twirls"), "* Alice twirls");
        assert_eq!(room_emote_noarg("Alice", "smiles"), "* Alice smiles");
        assert_eq!(room_emote_noarg("Alice", "bows"), "* Alice bows");
        assert_eq!(room_emote_noarg("Alice", "laughs"), "* Alice laughs");
        assert_eq!(room_emote_noarg("Alice", "sighs"), "* Alice sighs");
        assert_eq!(room_emote_noarg("Alice", "shrugs"), "* Alice shrugs");
        assert_eq!(room_emote_noarg("Alice", "waves"), "* Alice waves");
        assert_eq!(room_emote_noarg("Alice", "claps"), "* Alice claps");
        assert_eq!(room_emote_noarg("Alice", "cheers"), "* Alice cheers");
    }
}
