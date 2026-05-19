use crate::{groups, rooms};

pub const WORLD_LOG_FORMAT_VERSION: u32 = 1;
pub const WORLD_LOG_MAX_SUPPORTED_FORMAT: u32 = 2;
pub const WORLD_SNAPSHOT_MAX_SUPPORTED_FORMAT: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClusterFeatureState {
    pub active_world_log_format: u32,
    pub snapshot_format: u32,
    pub min_reader_world_log_format: u32,
}

impl Default for ClusterFeatureState {
    fn default() -> Self {
        Self {
            active_world_log_format: WORLD_LOG_FORMAT_VERSION,
            snapshot_format: WORLD_LOG_FORMAT_VERSION,
            min_reader_world_log_format: WORLD_LOG_FORMAT_VERSION,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClusterFormatVoter<'a> {
    pub node_id: &'a str,
    pub max_world_log_format: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClusterFormatActivation<'a> {
    Allowed,
    Rejected {
        target_format: u32,
        unsupported_voters: Vec<&'a str>,
    },
}

pub fn evaluate_world_log_format_activation<'a>(
    target_format: u32,
    voters: &'a [ClusterFormatVoter<'a>],
) -> ClusterFormatActivation<'a> {
    if voters.is_empty() {
        return ClusterFormatActivation::Rejected {
            target_format,
            unsupported_voters: Vec::new(),
        };
    }

    let unsupported_voters = voters
        .iter()
        .filter(|v| v.max_world_log_format < target_format)
        .map(|v| v.node_id)
        .collect::<Vec<_>>();
    if unsupported_voters.is_empty() {
        ClusterFormatActivation::Allowed
    } else {
        ClusterFormatActivation::Rejected {
            target_format,
            unsupported_voters,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistenceArtifactKind {
    WorldSnapshot,
    LsmtManifest,
    LsmtRun,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersistenceFormatDecision {
    Allowed,
    Rejected {
        artifact: PersistenceArtifactKind,
        requested_format: u32,
        active_snapshot_format: u32,
        min_reader_world_log_format: u32,
        reason: &'static str,
    },
}

pub fn evaluate_persistence_format_write(
    state: &ClusterFeatureState,
    artifact: PersistenceArtifactKind,
    requested_format: u32,
) -> PersistenceFormatDecision {
    if requested_format == 0 {
        return PersistenceFormatDecision::Rejected {
            artifact,
            requested_format,
            active_snapshot_format: state.snapshot_format,
            min_reader_world_log_format: state.min_reader_world_log_format,
            reason: "format must be positive",
        };
    }
    if requested_format > WORLD_SNAPSHOT_MAX_SUPPORTED_FORMAT {
        return PersistenceFormatDecision::Rejected {
            artifact,
            requested_format,
            active_snapshot_format: state.snapshot_format,
            min_reader_world_log_format: state.min_reader_world_log_format,
            reason: "format is newer than this binary supports",
        };
    }
    if requested_format > state.snapshot_format {
        return PersistenceFormatDecision::Rejected {
            artifact,
            requested_format,
            active_snapshot_format: state.snapshot_format,
            min_reader_world_log_format: state.min_reader_world_log_format,
            reason: "snapshot format is not active",
        };
    }
    if requested_format > state.min_reader_world_log_format {
        return PersistenceFormatDecision::Rejected {
            artifact,
            requested_format,
            active_snapshot_format: state.snapshot_format,
            min_reader_world_log_format: state.min_reader_world_log_format,
            reason: "minimum reader format is too old",
        };
    }
    PersistenceFormatDecision::Allowed
}

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
    ClusterFeatureSet {
        active_world_log_format: u32,
        snapshot_format: u32,
        min_reader_world_log_format: u32,
        reason: String,
    },
    ClusterMetadataSet {
        key: String,
        value: Option<String>,
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixedVersionCompatibility {
    // Safe while a Raft cluster contains old and new binaries: old readers either
    // understand the event already or can ignore optional fields.
    RollingSafe,
    // Requires a cluster-wide feature/version activation after every member runs
    // a binary that understands the event or semantic change.
    RequiresClusterFeature(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldEventWriteGate {
    RollingSafe,
    RequiresActiveWorldLogFormat {
        feature: &'static str,
        min_format: u32,
    },
    ClusterFeatureActivation {
        target_format: u32,
    },
}

pub fn world_log_entry_mixed_version_compatibility(
    entry: &WorldLogEntry,
) -> MixedVersionCompatibility {
    match entry {
        WorldLogEntry::Group(_) => MixedVersionCompatibility::RollingSafe,
        WorldLogEntry::World(event) => world_event_mixed_version_compatibility(event),
    }
}

pub fn world_event_mixed_version_compatibility(event: &WorldEvent) -> MixedVersionCompatibility {
    match event {
        WorldEvent::ClusterFeatureSet { .. } => {
            MixedVersionCompatibility::RequiresClusterFeature("cluster_feature_activation")
        }
        WorldEvent::ClusterMetadataSet { .. } => {
            MixedVersionCompatibility::RequiresClusterFeature("world_log_format_2")
        }
        WorldEvent::CharacterSnapshot { .. }
        | WorldEvent::MobSpawned { .. }
        | WorldEvent::CharacterMoved { .. }
        | WorldEvent::CharacterRemoved { .. }
        | WorldEvent::CombatSet { .. }
        | WorldEvent::HpSet { .. }
        | WorldEvent::StunSet { .. }
        | WorldEvent::BossStateSet { .. }
        | WorldEvent::AmbientStateSet { .. }
        | WorldEvent::PartyCreated { .. }
        | WorldEvent::PartyMemberSet { .. }
        | WorldEvent::PartyLeaderSet { .. }
        | WorldEvent::PartyDisbanded { .. }
        | WorldEvent::PartyInviteSet { .. }
        | WorldEvent::RngStateSet { .. }
        | WorldEvent::ClockSet { .. }
        | WorldEvent::ScheduledEventSet { .. }
        | WorldEvent::RoomSet { .. }
        | WorldEvent::ClientCommandSeen { .. } => MixedVersionCompatibility::RollingSafe,
    }
}

pub fn world_event_write_gate(event: &WorldEvent) -> WorldEventWriteGate {
    match event {
        WorldEvent::ClusterFeatureSet {
            active_world_log_format,
            ..
        } => WorldEventWriteGate::ClusterFeatureActivation {
            target_format: *active_world_log_format,
        },
        WorldEvent::ClusterMetadataSet { .. } => {
            WorldEventWriteGate::RequiresActiveWorldLogFormat {
                feature: "cluster_metadata",
                min_format: 2,
            }
        }
        WorldEvent::CharacterSnapshot { .. }
        | WorldEvent::MobSpawned { .. }
        | WorldEvent::CharacterMoved { .. }
        | WorldEvent::CharacterRemoved { .. }
        | WorldEvent::CombatSet { .. }
        | WorldEvent::HpSet { .. }
        | WorldEvent::StunSet { .. }
        | WorldEvent::BossStateSet { .. }
        | WorldEvent::AmbientStateSet { .. }
        | WorldEvent::PartyCreated { .. }
        | WorldEvent::PartyMemberSet { .. }
        | WorldEvent::PartyLeaderSet { .. }
        | WorldEvent::PartyDisbanded { .. }
        | WorldEvent::PartyInviteSet { .. }
        | WorldEvent::RngStateSet { .. }
        | WorldEvent::ClockSet { .. }
        | WorldEvent::ScheduledEventSet { .. }
        | WorldEvent::RoomSet { .. }
        | WorldEvent::ClientCommandSeen { .. } => WorldEventWriteGate::RollingSafe,
    }
}

fn default_principal() -> String {
    "acct:".to_string()
}

fn default_true() -> bool {
    true
}

fn default_level() -> u32 {
    1
}

fn default_sex() -> String {
    "none".to_string()
}

fn default_pronouns() -> String {
    "they".to_string()
}

fn default_hp() -> i32 {
    10
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
    #[serde(default = "default_principal")]
    pub principal: String,
    #[serde(default)]
    pub auth_caps: Vec<String>,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub bot_ever: bool,
    #[serde(default)]
    pub bot_ever_since_ms: Option<u64>,
    #[serde(default)]
    pub bot_mode_changed_ms: u64,
    #[serde(default)]
    pub friends: Vec<String>,
    pub room_id: String,
    #[serde(default = "default_true")]
    pub autoassist: bool,
    #[serde(default)]
    pub follow_leader: bool,
    #[serde(default)]
    pub drink_level: u32,
    #[serde(default)]
    pub gold: u32,
    #[serde(default)]
    pub inv: std::collections::BTreeMap<String, u32>,
    #[serde(default)]
    pub quest: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default = "default_level")]
    pub level: u32,
    #[serde(default)]
    pub xp: u32,
    #[serde(default)]
    pub skill_points: u32,
    #[serde(default)]
    pub skills: std::collections::BTreeMap<String, u32>,
    #[serde(default)]
    pub skill_cd_ms: std::collections::BTreeMap<String, u64>,
    #[serde(default)]
    pub race: Option<String>,
    #[serde(default = "default_sex")]
    pub sex: String,
    #[serde(default = "default_pronouns")]
    pub pronouns: String,
    #[serde(default)]
    pub stats: AbilityScoresSnapshot,
    #[serde(default = "default_hp")]
    pub hp: i32,
    #[serde(default = "default_hp")]
    pub max_hp: i32,
    #[serde(default)]
    pub mana: i32,
    #[serde(default)]
    pub max_mana: i32,
    #[serde(default)]
    pub stamina: i32,
    #[serde(default)]
    pub max_stamina: i32,
    #[serde(default)]
    pub last_mana_regen_ms: u64,
    #[serde(default)]
    pub last_stamina_regen_ms: u64,
    #[serde(default)]
    pub pvp_enabled: bool,
    #[serde(default)]
    pub stunned_until_ms: u64,
    #[serde(default)]
    pub equip: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AbilityScoresSnapshot {
    pub str_: i32,
    pub dex: i32,
    pub con: i32,
    pub int_: i32,
    pub wis: i32,
    pub cha: i32,
}

impl Default for AbilityScoresSnapshot {
    fn default() -> Self {
        Self {
            str_: 10,
            dex: 10,
            con: 10,
            int_: 10,
            wis: 10,
            cha: 10,
        }
    }
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

    #[test]
    fn legacy_character_snapshot_defaults_newer_fields() {
        let raw = r#"{
            "t":"CharacterSnapshot",
            "character":{
                "character_id":42,
                "name":"Alice",
                "room_id":"town.gate"
            },
            "reason":"legacy-replay"
        }"#;
        let parsed: WorldLogEntry = serde_json::from_str(raw).unwrap();
        match parsed {
            WorldLogEntry::World(WorldEvent::CharacterSnapshot { character, reason }) => {
                assert_eq!(reason, "legacy-replay");
                assert_eq!(character.character_id, 42);
                assert_eq!(character.name, "Alice");
                assert_eq!(character.principal, "acct:");
                assert!(character.autoassist);
                assert_eq!(character.level, 1);
                assert_eq!(character.sex, "none");
                assert_eq!(character.pronouns, "they");
                assert_eq!(character.stats, AbilityScoresSnapshot::default());
                assert_eq!(character.hp, 10);
                assert_eq!(character.max_hp, 10);
                assert!(character.skills.is_empty());
                assert!(character.equip.is_empty());
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }

    #[test]
    fn character_snapshot_preserves_format2_equipment_slots() {
        let raw = r#"{
            "t":"CharacterSnapshot",
            "character":{
                "character_id":42,
                "name":"Alice",
                "room_id":"town.gate",
                "equip":{"neck":"training charm"}
            },
            "reason":"format2-replay"
        }"#;
        let parsed: WorldLogEntry = serde_json::from_str(raw).unwrap();
        match parsed {
            WorldLogEntry::World(WorldEvent::CharacterSnapshot { character, reason }) => {
                assert_eq!(reason, "format2-replay");
                assert_eq!(
                    character.equip.get("neck").map(String::as_str),
                    Some("training charm")
                );
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }

    #[test]
    fn existing_event_shapes_tolerate_future_optional_fields() {
        let raw = r#"{
            "index":7,
            "term":2,
            "ms":1234,
            "writer_version":"2.0.0",
            "entry":{
                "t":"CharacterMoved",
                "character_id":42,
                "from":"town.gate",
                "to":"guild.fighter",
                "reason":"quest",
                "future_optional_field":{"ignored":true}
            }
        }"#;
        let env: crate::raftlog::RaftEnvelope<WorldLogEntry> = serde_json::from_str(raw).unwrap();
        assert_eq!(env.index, 7);
        assert_eq!(env.term, 2);
        match env.entry {
            WorldLogEntry::World(WorldEvent::CharacterMoved {
                character_id,
                from,
                to,
                reason,
            }) => {
                assert_eq!(character_id, 42);
                assert_eq!(from.as_deref(), Some("town.gate"));
                assert_eq!(to, "guild.fighter");
                assert_eq!(reason, "quest");
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }

    #[test]
    fn unknown_event_variants_are_not_mixed_version_safe() {
        let raw = r#"{"t":"InventoryDelta","character_id":42,"item":"torch","delta":1}"#;
        let err = serde_json::from_str::<WorldLogEntry>(raw).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("did not match any variant") || msg.contains("unknown variant"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn current_world_event_fixtures_are_classified_for_rolling_upgrade() {
        let fixtures = [
            r#"{"t":"CharacterSnapshot","character":{"character_id":1,"name":"Alice","room_id":"town.gate"},"reason":"test"}"#,
            r#"{"t":"MobSpawned","character_id":2,"room_id":"town.gate","name":"rat","hp":3,"max_hp":3,"boss":null}"#,
            r#"{"t":"CharacterMoved","character_id":1,"from":null,"to":"town.gate","reason":"test"}"#,
            r#"{"t":"CharacterRemoved","character_id":1,"room_id":"town.gate","reason":"test"}"#,
            r#"{"t":"CombatSet","character_id":1,"autoattack":true,"target":2,"next_ready_ms":10,"reason":"test"}"#,
            r#"{"t":"HpSet","character_id":1,"hp":9,"reason":"test"}"#,
            r#"{"t":"StunSet","character_id":1,"stunned_until_ms":10,"reason":"test"}"#,
            r#"{"t":"BossStateSet","boss_id":2,"casting_until_ms":10,"seq":1,"present":true,"reason":"test"}"#,
            r#"{"t":"AmbientStateSet","bartender_id":3,"bartender_emote_idx":4,"reason":"test"}"#,
            r#"{"t":"PartyCreated","party_id":1,"leader":1}"#,
            r#"{"t":"PartyMemberSet","party_id":1,"member":2,"present":true}"#,
            r#"{"t":"PartyLeaderSet","party_id":1,"leader":2}"#,
            r#"{"t":"PartyDisbanded","party_id":1}"#,
            r#"{"t":"PartyInviteSet","invitee":2,"party_id":1,"inviter":1,"expires_ms":99,"present":true}"#,
            r#"{"t":"RngStateSet","state":123,"reason":"test"}"#,
            r#"{"t":"ClockSet","now_ms":123,"reason":"test"}"#,
            r#"{"t":"ScheduledEventSet","event":{"due_ms":10,"seq":2,"kind":"CombatAct","character_id":7},"present":true,"reason":"test"}"#,
            r#"{"t":"RoomSet","room_id":"town.gate","room":null,"present":false,"reason":"test"}"#,
            r#"{"t":"ClientCommandSeen","session":"42","command_id":9,"principal":"acct:alice","reason":"test"}"#,
        ];

        for raw in fixtures {
            let entry: WorldLogEntry = serde_json::from_str(raw).unwrap();
            assert_eq!(
                world_log_entry_mixed_version_compatibility(&entry),
                MixedVersionCompatibility::RollingSafe,
                "fixture should be classified before rollout: {raw}"
            );
        }

        let group: WorldLogEntry = serde_json::from_str(
            r#"{"t":"GroupCreate","group_id":1,"kind":"Admin","name":"admins"}"#,
        )
        .unwrap();
        assert_eq!(
            world_log_entry_mixed_version_compatibility(&group),
            MixedVersionCompatibility::RollingSafe
        );
    }

    #[test]
    fn cluster_format_activation_requires_every_voter_not_just_quorum() {
        let aab = [
            ClusterFormatVoter {
                node_id: "n0",
                max_world_log_format: 1,
            },
            ClusterFormatVoter {
                node_id: "n1",
                max_world_log_format: 1,
            },
            ClusterFormatVoter {
                node_id: "n2",
                max_world_log_format: 2,
            },
        ];
        assert_eq!(
            evaluate_world_log_format_activation(2, &aab),
            ClusterFormatActivation::Rejected {
                target_format: 2,
                unsupported_voters: vec!["n0", "n1"],
            }
        );

        let abb = [
            ClusterFormatVoter {
                node_id: "n0",
                max_world_log_format: 1,
            },
            ClusterFormatVoter {
                node_id: "n1",
                max_world_log_format: 2,
            },
            ClusterFormatVoter {
                node_id: "n2",
                max_world_log_format: 2,
            },
        ];
        assert_eq!(
            evaluate_world_log_format_activation(2, &abb),
            ClusterFormatActivation::Rejected {
                target_format: 2,
                unsupported_voters: vec!["n0"],
            }
        );

        let bbb = [
            ClusterFormatVoter {
                node_id: "n0",
                max_world_log_format: 2,
            },
            ClusterFormatVoter {
                node_id: "n1",
                max_world_log_format: 2,
            },
            ClusterFormatVoter {
                node_id: "n2",
                max_world_log_format: 2,
            },
        ];
        assert_eq!(
            evaluate_world_log_format_activation(2, &bbb),
            ClusterFormatActivation::Allowed
        );
    }

    #[test]
    fn persistence_format_writes_require_active_snapshot_and_reader_format() {
        let default_state = ClusterFeatureState::default();
        assert_eq!(
            evaluate_persistence_format_write(
                &default_state,
                PersistenceArtifactKind::WorldSnapshot,
                1
            ),
            PersistenceFormatDecision::Allowed
        );
        assert_eq!(
            evaluate_persistence_format_write(&default_state, PersistenceArtifactKind::LsmtRun, 2),
            PersistenceFormatDecision::Rejected {
                artifact: PersistenceArtifactKind::LsmtRun,
                requested_format: 2,
                active_snapshot_format: 1,
                min_reader_world_log_format: 1,
                reason: "snapshot format is not active",
            }
        );

        let staged_snapshot = ClusterFeatureState {
            active_world_log_format: 2,
            snapshot_format: 2,
            min_reader_world_log_format: 1,
        };
        assert_eq!(
            evaluate_persistence_format_write(
                &staged_snapshot,
                PersistenceArtifactKind::LsmtManifest,
                2
            ),
            PersistenceFormatDecision::Rejected {
                artifact: PersistenceArtifactKind::LsmtManifest,
                requested_format: 2,
                active_snapshot_format: 2,
                min_reader_world_log_format: 1,
                reason: "minimum reader format is too old",
            }
        );

        let activated = ClusterFeatureState {
            active_world_log_format: 2,
            snapshot_format: 2,
            min_reader_world_log_format: 2,
        };
        assert_eq!(
            evaluate_persistence_format_write(&activated, PersistenceArtifactKind::LsmtRun, 2),
            PersistenceFormatDecision::Allowed
        );
    }

    #[test]
    fn cluster_feature_activation_is_feature_gated_metadata() {
        let raw = r#"{"t":"ClusterFeatureSet","active_world_log_format":2,"snapshot_format":2,"min_reader_world_log_format":2,"reason":"all-voters-upgraded"}"#;
        let entry: WorldLogEntry = serde_json::from_str(raw).unwrap();
        assert_eq!(
            world_log_entry_mixed_version_compatibility(&entry),
            MixedVersionCompatibility::RequiresClusterFeature("cluster_feature_activation")
        );
        match &entry {
            WorldLogEntry::World(event) => assert_eq!(
                world_event_write_gate(event),
                WorldEventWriteGate::ClusterFeatureActivation { target_format: 2 }
            ),
            other => panic!("unexpected parse: {other:?}"),
        }
        match entry {
            WorldLogEntry::World(WorldEvent::ClusterFeatureSet {
                active_world_log_format,
                snapshot_format,
                min_reader_world_log_format,
                reason,
            }) => {
                assert_eq!(active_world_log_format, 2);
                assert_eq!(snapshot_format, 2);
                assert_eq!(min_reader_world_log_format, 2);
                assert_eq!(reason, "all-voters-upgraded");
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }

    #[test]
    fn cluster_metadata_is_format2_gated() {
        let raw =
            r#"{"t":"ClusterMetadataSet","key":"rollout.probe","value":"active","reason":"test"}"#;
        let entry: WorldLogEntry = serde_json::from_str(raw).unwrap();
        assert_eq!(
            world_log_entry_mixed_version_compatibility(&entry),
            MixedVersionCompatibility::RequiresClusterFeature("world_log_format_2")
        );
        match &entry {
            WorldLogEntry::World(event) => assert_eq!(
                world_event_write_gate(event),
                WorldEventWriteGate::RequiresActiveWorldLogFormat {
                    feature: "cluster_metadata",
                    min_format: 2,
                }
            ),
            other => panic!("unexpected parse: {other:?}"),
        }
        match entry {
            WorldLogEntry::World(WorldEvent::ClusterMetadataSet { key, value, reason }) => {
                assert_eq!(key, "rollout.probe");
                assert_eq!(value.as_deref(), Some("active"));
                assert_eq!(reason, "test");
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }
}
