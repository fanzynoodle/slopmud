use std::collections::HashMap;

use anyhow::Context;
use flatbuffers::root_unchecked;
use serde::Deserialize;

use crate::rooms_fb;

mod embedded_areas {
    include!(concat!(env!("OUT_DIR"), "/world_areas.rs"));
}

#[derive(Clone, Debug)]
pub struct ExitDef {
    pub dir: String,
    pub to: String,
    pub sealed: bool,
    pub gate: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RoomDef {
    pub name: String,
    pub description: String,
    pub area_name: String,
    pub exits: Vec<ExitDef>,
}

#[derive(Clone, Debug)]
pub struct AreaSummary {
    pub zone_id: String,
    pub zone_name: String,
    pub start_room: Option<String>,
    pub room_count: usize,
}

#[derive(Clone)]
pub struct Rooms {
    rooms: HashMap<String, RoomDef>,
    dyn_rooms: HashMap<String, RoomDef>,
    start_room: String,
    areas: Vec<AreaSummary>,
}

impl Rooms {
    pub fn load() -> anyhow::Result<Self> {
        // For now the world is compiled in as a FlatBuffers blob.
        // Later we'll replace this with loading from files generated from docs.
        let buf = rooms_fb::build_world_buffer();
        let world = unsafe { root_unchecked::<rooms_fb::World>(&buf) };

        let mut rooms = HashMap::new();
        let mut start_room = None;

        let areas = world.areas().context("flatbuffer world missing areas")?;
        for area in areas.iter() {
            let area_id = area.id().unwrap_or("unknown").to_string();
            let area_name = area.name().unwrap_or(&area_id).to_string();

            if let Some(rooms_vec) = area.rooms() {
                for room in rooms_vec.iter() {
                    let id = room.id().unwrap_or("unknown").to_string();
                    let name = room.name().unwrap_or(&id).to_string();
                    let description = room.description().unwrap_or("").to_string();

                    let mut exits = Vec::new();
                    if let Some(exits_vec) = room.exits() {
                        for exit in exits_vec.iter() {
                            let dir = exit.dir().unwrap_or("").to_string();
                            let to = exit.to().unwrap_or("").to_string();
                            if !dir.is_empty() && !to.is_empty() {
                                exits.push(ExitDef {
                                    dir,
                                    to,
                                    sealed: false,
                                    gate: None,
                                });
                            }
                        }
                    }

                    if start_room.is_none() {
                        start_room = Some(id.clone());
                    }

                    rooms.insert(
                        id.clone(),
                        RoomDef {
                            name,
                            description,
                            area_name: area_name.clone(),
                            exits,
                        },
                    );
                }
            }
        }

        let mut start_room = start_room.context("no rooms loaded")?;

        // Overlay engine-facing area files (YAML room graphs).
        //
        // These are embedded at compile time from `world/areas/*.yaml`, so deploy stays binary-only.
        // The FlatBuffers blob remains as a fallback/default world while we migrate zones to YAML.
        let mut preferred_start_room: Option<String> = None;
        let mut areas: Vec<AreaSummary> = Vec::new();
        for (fname, s) in embedded_areas::WORLD_AREAS_YAML {
            let a = serde_yaml::from_str::<AreaFile>(s)
                .with_context(|| format!("parse embedded area yaml: {fname}"))?;
            let room_count = a.rooms.len();
            let area_name = a.zone_name.clone().unwrap_or_else(|| a.zone_id.clone());
            areas.push(AreaSummary {
                zone_id: a.zone_id.clone(),
                zone_name: area_name.clone(),
                start_room: a.start_room.clone(),
                room_count,
            });
            for r in a.rooms {
                let mut exits = Vec::new();
                if let Some(xs) = r.exits {
                    for e in xs {
                        let dir = e.dir.trim().to_string();
                        let to = e.to.trim().to_string();
                        if !dir.is_empty() && !to.is_empty() {
                            let gate = e
                                .gate
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(|s| s.to_string());
                            exits.push(ExitDef {
                                dir,
                                to,
                                sealed: e.state.as_deref() == Some("sealed"),
                                gate,
                            });
                        }
                    }
                }
                rooms.insert(
                    r.id,
                    RoomDef {
                        name: r.name,
                        description: r.desc.unwrap_or_default().trim().to_string(),
                        area_name: area_name.clone(),
                        exits,
                    },
                );
            }

            // Pick a deterministic start room (prefer the newbie school if present).
            if preferred_start_room.is_none() && a.zone_id == "newbie_school" {
                if let Some(sr) = a.start_room.as_deref() {
                    preferred_start_room = Some(sr.to_string());
                }
            }
        }

        // Prefer an explicit YAML start room, otherwise fall back to the historic default.
        if let Some(sr) = preferred_start_room {
            if rooms.contains_key(&sr) {
                start_room = sr;
            }
        } else if rooms.contains_key("newbie_school.orientation") {
            start_room = "newbie_school.orientation".to_string();
        }

        areas.sort_by(|a, b| a.zone_id.cmp(&b.zone_id));
        Ok(Self {
            rooms,
            dyn_rooms: HashMap::new(),
            start_room,
            areas,
        })
    }

    pub fn start_room(&self) -> &str {
        &self.start_room
    }

    pub fn render_areas(&self) -> String {
        if self.areas.is_empty() {
            // Fallback: list unique area names derived from room defs.
            let mut xs = self
                .rooms
                .values()
                .map(|r| r.area_name.as_str())
                .collect::<Vec<_>>();
            xs.sort_unstable();
            xs.dedup();

            let mut s = String::new();
            s.push_str(&format!("areas: {}\\r\\n", xs.len()));
            for name in xs {
                s.push_str(" - ");
                s.push_str(name);
                s.push_str("\\r\\n");
            }
            return s;
        }

        let mut s = String::new();
        s.push_str(&format!("areas: {}\\r\\n", self.areas.len()));
        for a in &self.areas {
            s.push_str(" - ");
            s.push_str(&a.zone_id);
            s.push_str(" (");
            s.push_str(&a.zone_name);
            s.push_str(")");
            s.push_str(&format!(
                " rooms={} start={}\\r\\n",
                a.room_count,
                a.start_room.as_deref().unwrap_or("-")
            ));
        }
        s
    }

    pub fn has_room(&self, room_id: &str) -> bool {
        self.dyn_rooms.contains_key(room_id) || self.rooms.contains_key(room_id)
    }

    pub fn clear_dyn_rooms_with_prefix(&mut self, prefix: &str) -> usize {
        let p = if prefix.ends_with('.') {
            prefix.to_string()
        } else {
            format!("{prefix}.")
        };
        let keys = self
            .dyn_rooms
            .keys()
            .filter(|k| k.starts_with(&p))
            .cloned()
            .collect::<Vec<_>>();
        let n = keys.len();
        for k in keys {
            self.dyn_rooms.remove(&k);
        }
        n
    }

    pub fn insert_room(&mut self, room_id: String, def: RoomDef) {
        self.dyn_rooms.insert(room_id, def);
    }

    pub fn find_exit(&self, room_id: &str, token: &str) -> Option<&ExitDef> {
        let room = self
            .dyn_rooms
            .get(room_id)
            .or_else(|| self.rooms.get(room_id))?;
        let t = token.trim();
        if t.is_empty() {
            return None;
        }

        // Exact match on the exit name (current schema calls it "dir").
        if let Some(ex) = room.exits.iter().find(|e| e.dir.eq_ignore_ascii_case(t)) {
            return Some(ex);
        }

        // Direction aliases (n/s/e/w/u/d) also match directional exits.
        let Some(canon) = normalize_dir_token(t) else {
            // As a convenience alias: if the player types a single letter and it uniquely
            // prefixes an exit name in this room, treat it as that exit.
            let t_lc = t.to_ascii_lowercase();
            if t_lc.len() == 1 {
                let mut found: Option<&ExitDef> = None;
                for ex in &room.exits {
                    if ex.dir.to_ascii_lowercase().starts_with(&t_lc) {
                        if found.is_some() {
                            return None; // ambiguous
                        }
                        found = Some(ex);
                    }
                }
                if found.is_some() {
                    return found;
                }
            }
            return None;
        };
        room.exits
            .iter()
            .find(|e| e.dir.eq_ignore_ascii_case(canon))
    }

    pub fn render_exits(&self, room_id: &str) -> String {
        let Some(room) = self
            .dyn_rooms
            .get(room_id)
            .or_else(|| self.rooms.get(room_id))
        else {
            return "exits: (room not found)\r\n".to_string();
        };
        if room.exits.is_empty() {
            return "exits: none\r\n".to_string();
        }

        let mut xs = room
            .exits
            .iter()
            .map(|e| format_exit_label(e.dir.as_str()))
            .collect::<Vec<_>>();
        xs.sort_unstable();
        format!("exits: {}\r\n", xs.join(", "))
    }

    pub fn render_room(&self, room_id: &str) -> String {
        let Some(room) = self
            .dyn_rooms
            .get(room_id)
            .or_else(|| self.rooms.get(room_id))
        else {
            return "room not found\r\n".to_string();
        };

        let mut s = String::new();
        s.push_str(&format!(
            "== {} ({}) [{}] ==\r\n",
            room.name, room.area_name, room_id
        ));
        if !room.description.is_empty() {
            s.push_str(&room.description);
            s.push_str("\r\n");
        }

        s.push_str(&self.render_exits(room_id));
        s
    }

    pub fn exits_raw(&self, room_id: &str) -> Vec<ExitDef> {
        let Some(room) = self
            .dyn_rooms
            .get(room_id)
            .or_else(|| self.rooms.get(room_id))
        else {
            return Vec::new();
        };
        room.exits.clone()
    }
}

#[derive(Debug, Deserialize)]
struct AreaFile {
    #[allow(dead_code)]
    version: u32,
    zone_id: String,
    zone_name: Option<String>,
    #[allow(dead_code)]
    area_id: Option<String>,
    start_room: Option<String>,
    rooms: Vec<AreaRoom>,
}

#[derive(Debug, Deserialize)]
struct AreaRoom {
    id: String,
    name: String,
    desc: Option<String>,
    #[allow(dead_code)]
    cluster: Option<String>,
    #[allow(dead_code)]
    tags: Option<Vec<String>>,
    exits: Option<Vec<AreaExit>>,
}

#[derive(Debug, Deserialize)]
struct AreaExit {
    dir: String,
    to: String,
    #[allow(dead_code)]
    len: Option<u32>,
    #[allow(dead_code)]
    state: Option<String>,
    #[allow(dead_code)]
    opens_area: Option<String>,
    gate: Option<String>,
}

fn normalize_dir_token(line: &str) -> Option<&'static str> {
    match line.to_ascii_lowercase().as_str() {
        "north" | "n" => Some("north"),
        "south" | "s" => Some("south"),
        "east" | "e" => Some("east"),
        "west" | "w" => Some("west"),
        "up" | "u" => Some("up"),
        "down" | "d" => Some("down"),
        _ => None,
    }
}

fn format_exit_label(dir: &str) -> String {
    match dir.to_ascii_lowercase().as_str() {
        "north" => "north (n)".to_string(),
        "south" => "south (s)".to_string(),
        "east" => "east (e)".to_string(),
        "west" => "west (w)".to_string(),
        "up" => "up (u)".to_string(),
        "down" => "down (d)".to_string(),
        "back" => "back (b)".to_string(),
        _ => dir.to_string(),
    }
}
