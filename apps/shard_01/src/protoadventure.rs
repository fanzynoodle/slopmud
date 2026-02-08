use std::collections::{HashMap, HashSet};

use crate::rooms::{ExitDef, RoomDef};

#[derive(Debug, Clone)]
pub struct RoomPlan {
    pub id: String,
    pub name: String,
    pub description: String,
    pub exits: Vec<ExitDef>,
}

#[derive(Debug, Clone)]
pub struct BuildPlan {
    pub adventure_id: String,
    pub rooms: Vec<RoomPlan>,
    pub start_room: String,
}

pub fn parse_protoadventure_markdown(adventure_id: &str, md: &str) -> BuildPlan {
    // Minimal parser for the current protoadventure house style:
    // - Rooms are introduced with "### R_FOO (...optional label...)"
    // - Bullet lines include "- beat:" and "- exits:"
    //
    // We do not fully parse Markdown; we just look for room headings and bullets.
    let mut in_exits_block = false;
    let mut cur: Option<RoomPlan> = None;
    let mut out: Vec<RoomPlan> = Vec::new();

    for raw in md.lines() {
        let line = raw.trim_end();
        if let Some(h) = line.strip_prefix("### ") {
            if let Some((id, label)) = parse_room_heading(h) {
                if let Some(r) = cur.take() {
                    out.push(r);
                }
                cur = Some(RoomPlan {
                    id: id.clone(),
                    name: label.unwrap_or_else(|| id.clone()),
                    description: String::new(),
                    exits: Vec::new(),
                });
                in_exits_block = false;
            }
            continue;
        }

        let Some(r) = cur.as_mut() else { continue };

        let t = line.trim();
        if let Some(rest) = t.strip_prefix("- exits:") {
            r.exits.extend(parse_exits(rest));
            in_exits_block = true;
            continue;
        }
        if in_exits_block {
            // Support:
            // - exits:
            //   - north -> R_FOO
            //   - east -> `R_BAR` (label)
            if let Some(rest) = t.strip_prefix("- ") {
                if rest.contains("->") {
                    r.exits.extend(parse_exits(rest));
                    continue;
                }
            }
            if t.starts_with('-') {
                in_exits_block = false;
            }
        }
        if let Some(rest) = t.strip_prefix("- beat:") {
            push_desc_line(&mut r.description, "beat", rest);
            continue;
        }
        if let Some(rest) = t.strip_prefix("- teach:") {
            push_desc_line(&mut r.description, "teach", rest);
            continue;
        }
        if let Some(rest) = t.strip_prefix("- note:") {
            push_desc_line(&mut r.description, "note", rest);
            continue;
        }
        if let Some(rest) = t.strip_prefix("- quest:") {
            push_desc_line(&mut r.description, "quest", rest);
            continue;
        }
        if let Some(rest) = t.strip_prefix("- feedback:") {
            push_desc_line(&mut r.description, "feedback", rest);
            continue;
        }
        if let Some(rest) = t.strip_prefix("- telegraph:") {
            push_desc_line(&mut r.description, "telegraph", rest);
            continue;
        }
        if let Some(rest) = t.strip_prefix("- failure:") {
            push_desc_line(&mut r.description, "failure", rest);
            continue;
        }
    }

    if let Some(r) = cur.take() {
        out.push(r);
    }

    // Only keep exits that point to known rooms.
    let known = out.iter().map(|r| r.id.clone()).collect::<HashSet<_>>();
    for r in &mut out {
        r.exits.retain(|e| known.contains(&e.to));
    }

    // Ensure the room graph has stable ordering.
    let mut seen = HashSet::<String>::new();
    let mut dedup = Vec::new();
    for r in out {
        if seen.insert(r.id.clone()) {
            dedup.push(r);
        }
    }

    let start_room = dedup
        .first()
        .map(|r| r.id.clone())
        .unwrap_or_else(|| "R_START".to_string());

    BuildPlan {
        adventure_id: adventure_id.to_string(),
        rooms: dedup,
        start_room,
    }
}

pub fn instantiate_rooms(instance_prefix: &str, plan: &BuildPlan) -> Vec<(String, RoomDef)> {
    // Convert BuildPlan into concrete RoomDefs with instance-prefixed IDs.
    let mut map = HashMap::<String, String>::new();
    for r in &plan.rooms {
        map.insert(r.id.clone(), format!("{instance_prefix}.{}", r.id));
    }

    let mut out = Vec::new();
    for r in &plan.rooms {
        let id = map.get(&r.id).expect("in map").clone();
        let mut exits = Vec::new();
        for e in &r.exits {
            if let Some(to) = map.get(&e.to) {
                exits.push(ExitDef {
                    dir: e.dir.clone(),
                    to: to.clone(),
                    sealed: false,
                    gate: None,
                });
            }
        }
        out.push((
            id,
            RoomDef {
                name: r.name.clone(),
                description: r.description.trim().to_string(),
                area_name: plan.adventure_id.clone(),
                exits,
            },
        ));
    }
    out
}

fn parse_room_heading(h: &str) -> Option<(String, Option<String>)> {
    // Examples:
    // "R_NS_ORIENT_01 (CL_NS_ORIENTATION, HUB_NS_ORIENTATION)"
    // "R_NS_ORIENT_02 (badge desk)"
    // "R_TOWN_JOBBOARD_TEASER"
    // "Entry: R_SEW_JUNC_DRONE_01 (HUB_SEWERS_JUNCTION)"
    let mut label = None;
    let id = extract_room_id(h)?;
    if let Some(l) = h
        .find('(')
        .and_then(|i| h[i + 1..].find(')').map(|j| (i, j)))
    {
        let (i, j) = l;
        let inside = h[i + 1..i + 1 + j].trim();
        if !inside.is_empty() {
            // Prefer a short human label if it doesn't look like a cluster list.
            if !inside.contains("CL_")
                && !inside.contains("HUB_")
                && !inside.contains("setpiece.")
                && inside.len() <= 48
            {
                label = Some(inside.to_string());
            }
        }
    }
    Some((id, label))
}

fn extract_room_id(h: &str) -> Option<String> {
    let bs = h.as_bytes();
    for i in 0..bs.len().saturating_sub(1) {
        if bs[i] != b'R' || bs[i + 1] != b'_' {
            continue;
        }
        let mut j = i + 2;
        while j < bs.len() {
            let c = bs[j] as char;
            if c.is_ascii_alphanumeric() || c == '_' {
                j += 1;
            } else {
                break;
            }
        }
        if j > i + 2 {
            return Some(h[i..j].to_string());
        }
    }
    None
}

fn push_desc_line(dst: &mut String, k: &str, v: &str) {
    let v = v.trim();
    if v.is_empty() {
        return;
    }
    if !dst.is_empty() {
        dst.push('\n');
    }
    dst.push_str(k);
    dst.push_str(": ");
    dst.push_str(v);
}

fn parse_exits(rest: &str) -> Vec<ExitDef> {
    // Example: "east -> `R_NS_ORIENT_02`"
    // Also allow: "north -> R_FOO"
    // Multiple exits can be comma-separated.
    let mut out = Vec::new();
    for part in rest.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let Some((dir, rhs)) = p.split_once("->") else {
            continue;
        };
        let dir = dir.trim().to_string();
        let mut to = rhs.trim().to_string();
        if let Some(i) = to.find('`') {
            if let Some(j) = to[i + 1..].find('`') {
                to = to[i + 1..i + 1 + j].to_string();
            }
        }
        // Strip "(future)" etc.
        if let Some(sp) = to.split_whitespace().next() {
            to = sp.trim().to_string();
        }
        if !dir.is_empty() && !to.is_empty() {
            out.push(ExitDef {
                dir,
                to,
                sealed: false,
                gate: None,
            });
        }
    }
    out
}
