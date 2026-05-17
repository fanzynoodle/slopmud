use crate::{groups, rooms};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum WorldLogEntry {
    // Compatibility: existing raft JSONL files have bare GroupLogEntry values in `entry`.
    Group(groups::GroupLogEntry),
    World(WorldEvent),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t")]
pub enum WorldEvent {
    CharacterSnapshot {
        character: CharacterSnapshot,
        reason: String,
    },
    MobSpawned {
        character_id: u64,
        room_id: String,
        name: String,
        hp: i32,
        max_hp: i32,
        boss: Option<BossProjection>,
    },
    CharacterMoved {
        character_id: u64,
        from: Option<String>,
        to: String,
        reason: String,
    },
    CharacterRemoved {
        character_id: u64,
        room_id: Option<String>,
        reason: String,
    },
    CombatSet {
        character_id: u64,
        autoattack: bool,
        target: Option<u64>,
        next_ready_ms: u64,
        reason: String,
    },
    HpSet {
        character_id: u64,
        hp: i32,
        reason: String,
    },
    StunSet {
        character_id: u64,
        stunned_until_ms: u64,
        reason: String,
    },
    BossStateSet {
        boss_id: u64,
        casting_until_ms: u64,
        seq: u64,
        present: bool,
        reason: String,
    },
    AmbientStateSet {
        bartender_id: Option<u64>,
        bartender_emote_idx: u64,
        reason: String,
    },
    PartyCreated {
        party_id: u64,
        leader: u64,
    },
    PartyMemberSet {
        party_id: u64,
        member: u64,
        present: bool,
    },
    PartyLeaderSet {
        party_id: u64,
        leader: u64,
    },
    PartyDisbanded {
        party_id: u64,
    },
    PartyInviteSet {
        invitee: u64,
        party_id: u64,
        inviter: u64,
        expires_ms: u64,
        present: bool,
    },
    RngStateSet {
        state: u64,
        reason: String,
    },
    ClockSet {
        now_ms: u64,
        reason: String,
    },
    ScheduledEventSet {
        event: ScheduledEventSnapshot,
        present: bool,
        reason: String,
    },
    RoomSet {
        room_id: String,
        room: Option<rooms::RoomDef>,
        present: bool,
        reason: String,
    },
    ClientCommandSeen {
        session: String,
        command_id: u64,
        principal: String,
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct BossProjection {
    pub casting_until_ms: u64,
    pub seq: u64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CharacterSnapshot {
    pub character_id: u64,
    pub name: String,
    pub principal: String,
    pub auth_caps: Vec<String>,
    pub is_bot: bool,
    pub bot_ever: bool,
    pub bot_ever_since_ms: Option<u64>,
    pub bot_mode_changed_ms: u64,
    pub friends: Vec<String>,
    pub room_id: String,
    pub autoassist: bool,
    pub follow_leader: bool,
    pub drink_level: u32,
    pub gold: u32,
    pub inv: std::collections::BTreeMap<String, u32>,
    pub quest: std::collections::BTreeMap<String, String>,
    pub class: Option<String>,
    pub level: u32,
    pub xp: u32,
    pub skill_points: u32,
    pub skills: std::collections::BTreeMap<String, u32>,
    pub skill_cd_ms: std::collections::BTreeMap<String, u64>,
    pub race: Option<String>,
    pub sex: String,
    pub pronouns: String,
    pub stats: AbilityScoresSnapshot,
    pub hp: i32,
    pub max_hp: i32,
    pub mana: i32,
    pub max_mana: i32,
    pub stamina: i32,
    pub max_stamina: i32,
    pub last_mana_regen_ms: u64,
    pub last_stamina_regen_ms: u64,
    pub pvp_enabled: bool,
    pub stunned_until_ms: u64,
    pub equip: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct AbilityScoresSnapshot {
    pub str_: i32,
    pub dex: i32,
    pub con: i32,
    pub int_: i32,
    pub wis: i32,
    pub cha: i32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScheduledEventSnapshot {
    pub due_ms: u64,
    pub seq: u64,
    pub kind: String,
    #[serde(default)]
    pub room_id: Option<String>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub character_id: Option<u64>,
    #[serde(default)]
    pub boss_id: Option<u64>,
    #[serde(default)]
    pub party_id: Option<u64>,
    #[serde(default)]
    pub cast_seq: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_group_entry_shape() {
        let raw = r#"{"t":"GroupCreate","group_id":1,"kind":"Admin","name":"admins"}"#;
        let parsed: WorldLogEntry = serde_json::from_str(raw).unwrap();
        match parsed {
            WorldLogEntry::Group(groups::GroupLogEntry::GroupCreate { group_id, name, .. }) => {
                assert_eq!(group_id, 1);
                assert_eq!(name, "admins");
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }

    #[test]
    fn parses_world_event_entry_shape() {
        let raw = r#"{"t":"CharacterMoved","character_id":42,"from":"a","to":"b","reason":"test"}"#;
        let parsed: WorldLogEntry = serde_json::from_str(raw).unwrap();
        match parsed {
            WorldLogEntry::World(WorldEvent::CharacterMoved {
                character_id,
                from,
                to,
                reason,
            }) => {
                assert_eq!(character_id, 42);
                assert_eq!(from.as_deref(), Some("a"));
                assert_eq!(to, "b");
                assert_eq!(reason, "test");
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }

    #[test]
    fn parses_scheduled_event_entry_shape() {
        let raw = r#"{"t":"ScheduledEventSet","event":{"due_ms":10,"seq":2,"kind":"CombatAct","character_id":7},"present":true,"reason":"test"}"#;
        let parsed: WorldLogEntry = serde_json::from_str(raw).unwrap();
        match parsed {
            WorldLogEntry::World(WorldEvent::ScheduledEventSet {
                event,
                present,
                reason,
            }) => {
                assert!(present);
                assert_eq!(reason, "test");
                assert_eq!(event.due_ms, 10);
                assert_eq!(event.seq, 2);
                assert_eq!(event.kind, "CombatAct");
                assert_eq!(event.character_id, Some(7));
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }

    #[test]
    fn parses_client_command_seen_entry_shape() {
        let raw = r#"{"t":"ClientCommandSeen","session":"42","command_id":9,"principal":"acct:alice","reason":"test"}"#;
        let parsed: WorldLogEntry = serde_json::from_str(raw).unwrap();
        match parsed {
            WorldLogEntry::World(WorldEvent::ClientCommandSeen {
                session,
                command_id,
                principal,
                reason,
            }) => {
                assert_eq!(session, "42");
                assert_eq!(command_id, 9);
                assert_eq!(principal, "acct:alice");
                assert_eq!(reason, "test");
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }
}
