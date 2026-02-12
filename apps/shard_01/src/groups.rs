#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Capability {
    // Broad.
    AdminAll,

    // Raft visibility.
    RaftTail,
    RaftWatch,

    // World / dev.
    WorldWarp,
    WorldProtoLoad,

    // Group management.
    GroupCreate,
    GroupMemberAdd,
    GroupMemberRemove,
    GroupRoleSet,
    GroupPolicySet,
    GroupRoleCapsSet,
}

impl Capability {
    pub const ALL: &'static [Capability] = &[
        Capability::AdminAll,
        Capability::RaftTail,
        Capability::RaftWatch,
        Capability::WorldWarp,
        Capability::WorldProtoLoad,
        Capability::GroupCreate,
        Capability::GroupMemberAdd,
        Capability::GroupMemberRemove,
        Capability::GroupRoleSet,
        Capability::GroupPolicySet,
        Capability::GroupRoleCapsSet,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Capability::AdminAll => "admin.all",
            Capability::RaftTail => "raft.tail",
            Capability::RaftWatch => "raft.watch",
            Capability::WorldWarp => "world.warp",
            Capability::WorldProtoLoad => "world.proto.load",
            Capability::GroupCreate => "group.create",
            Capability::GroupMemberAdd => "group.member.add",
            Capability::GroupMemberRemove => "group.member.remove",
            Capability::GroupRoleSet => "group.role.set",
            Capability::GroupPolicySet => "group.policy.set",
            Capability::GroupRoleCapsSet => "group.rolecaps.set",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "admin" | "admin.all" => Some(Capability::AdminAll),
            "raft.tail" => Some(Capability::RaftTail),
            "raft.watch" => Some(Capability::RaftWatch),
            "world.warp" => Some(Capability::WorldWarp),
            "world.proto.load" | "world.proto" | "proto.load" => Some(Capability::WorldProtoLoad),
            "group.create" => Some(Capability::GroupCreate),
            "group.member.add" | "group.add" => Some(Capability::GroupMemberAdd),
            "group.member.remove" | "group.remove" => Some(Capability::GroupMemberRemove),
            "group.role.set" | "group.role" => Some(Capability::GroupRoleSet),
            "group.policy.set" | "group.policy" => Some(Capability::GroupPolicySet),
            "group.rolecaps.set" | "group.rolecaps" => Some(Capability::GroupRoleCapsSet),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum GroupRole {
    Owner,
    Officer,
    Member,
    Guest,
}

impl GroupRole {
    pub const ALL: &'static [GroupRole] = &[
        GroupRole::Owner,
        GroupRole::Officer,
        GroupRole::Member,
        GroupRole::Guest,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            GroupRole::Owner => "owner",
            GroupRole::Officer => "officer",
            GroupRole::Member => "member",
            GroupRole::Guest => "guest",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "owner" | "leader" => Some(GroupRole::Owner),
            "officer" | "mod" => Some(GroupRole::Officer),
            "member" => Some(GroupRole::Member),
            "guest" => Some(GroupRole::Guest),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GroupKind {
    Admin,
    Guild,
    Class { class: String },
    Custom,
}

impl GroupKind {
    pub fn as_str(&self) -> String {
        match self {
            GroupKind::Admin => "admin".to_string(),
            GroupKind::Guild => "guild".to_string(),
            GroupKind::Custom => "custom".to_string(),
            GroupKind::Class { class } => format!("class:{class}"),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim().to_ascii_lowercase();
        if t == "admin" {
            return Some(GroupKind::Admin);
        }
        if t == "guild" {
            return Some(GroupKind::Guild);
        }
        if t == "custom" {
            return Some(GroupKind::Custom);
        }
        if let Some(rest) = t.strip_prefix("class:") {
            let class = rest.trim();
            if class.is_empty() {
                return None;
            }
            return Some(GroupKind::Class {
                class: class.to_string(),
            });
        }
        None
    }
}

#[derive(Clone, Debug)]
pub struct Group {
    pub id: u64,
    pub kind: GroupKind,
    pub name: String,
    pub members: HashMap<String, GroupRole>, // principal (lowercase name) -> role
    pub policies: HashMap<String, String>,
    pub role_caps: HashMap<GroupRole, HashSet<Capability>>,
}

impl Group {
    pub fn new(id: u64, kind: GroupKind, name: String) -> Self {
        let mut g = Self {
            id,
            kind,
            name,
            members: HashMap::new(),
            policies: HashMap::new(),
            role_caps: HashMap::new(),
        };
        g.apply_default_role_caps();
        g
    }

    fn apply_default_role_caps(&mut self) {
        // Intentionally conservative defaults; admins can override via raft log.
        for r in GroupRole::ALL {
            self.role_caps.entry(*r).or_default();
        }

        match self.kind {
            GroupKind::Admin => {
                // Any member of the admin group is a full admin for now.
                let mut caps = HashSet::new();
                caps.insert(Capability::AdminAll);
                caps.insert(Capability::RaftTail);
                caps.insert(Capability::RaftWatch);
                caps.insert(Capability::WorldWarp);
                caps.insert(Capability::WorldProtoLoad);
                caps.insert(Capability::GroupCreate);
                caps.insert(Capability::GroupMemberAdd);
                caps.insert(Capability::GroupMemberRemove);
                caps.insert(Capability::GroupRoleSet);
                caps.insert(Capability::GroupPolicySet);
                caps.insert(Capability::GroupRoleCapsSet);
                self.role_caps.insert(GroupRole::Owner, caps.clone());
                self.role_caps.insert(GroupRole::Officer, caps.clone());
                self.role_caps.insert(GroupRole::Member, caps.clone());
            }
            GroupKind::Guild | GroupKind::Custom | GroupKind::Class { .. } => {
                // Owners/officers can manage membership/policies by default.
                for r in [GroupRole::Owner, GroupRole::Officer] {
                    let c = self.role_caps.entry(r).or_default();
                    c.insert(Capability::GroupMemberAdd);
                    c.insert(Capability::GroupMemberRemove);
                    c.insert(Capability::GroupRoleSet);
                    c.insert(Capability::GroupPolicySet);
                    c.insert(Capability::GroupRoleCapsSet);
                }
            }
        }
    }

    pub fn caps_for_role(&self, role: GroupRole) -> HashSet<Capability> {
        self.role_caps.get(&role).cloned().unwrap_or_default()
    }

    pub fn implied_role_for_class(&self, class: &str) -> Option<GroupRole> {
        match &self.kind {
            GroupKind::Class { class: want } => {
                if want.eq_ignore_ascii_case(class) {
                    Some(GroupRole::Member)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t")]
pub enum GroupLogEntry {
    GroupCreate {
        group_id: u64,
        kind: GroupKind,
        name: String,
    },
    GroupMemberSet {
        group_id: u64,
        member: String,
        role: Option<GroupRole>, // None means remove.
    },
    GroupPolicySet {
        group_id: u64,
        key: String,
        value: Option<String>, // None means delete.
    },
    GroupRoleCapsSet {
        group_id: u64,
        role: GroupRole,
        caps: Vec<Capability>, // absolute set
    },
}

#[derive(Clone, Debug, Default)]
pub struct GroupStore {
    pub groups: HashMap<u64, Group>,
    pub group_ids_by_name: HashMap<String, u64>, // lowercase name -> id
}

impl GroupStore {
    pub fn apply(&mut self, e: &GroupLogEntry) {
        match e {
            GroupLogEntry::GroupCreate {
                group_id,
                kind,
                name,
            } => {
                let g = Group::new(*group_id, kind.clone(), name.clone());
                self.group_ids_by_name
                    .insert(name.to_ascii_lowercase(), *group_id);
                self.groups.insert(*group_id, g);
            }
            GroupLogEntry::GroupMemberSet {
                group_id,
                member,
                role,
            } => {
                let Some(g) = self.groups.get_mut(group_id) else {
                    return;
                };
                let key = member.trim().to_ascii_lowercase();
                if key.is_empty() {
                    return;
                }
                match role {
                    Some(r) => {
                        g.members.insert(key, *r);
                    }
                    None => {
                        g.members.remove(&key);
                    }
                }
            }
            GroupLogEntry::GroupPolicySet {
                group_id,
                key,
                value,
            } => {
                let Some(g) = self.groups.get_mut(group_id) else {
                    return;
                };
                let k = key.trim().to_ascii_lowercase();
                if k.is_empty() {
                    return;
                }
                match value {
                    Some(v) => {
                        g.policies.insert(k, v.clone());
                    }
                    None => {
                        g.policies.remove(&k);
                    }
                }
            }
            GroupLogEntry::GroupRoleCapsSet {
                group_id,
                role,
                caps,
            } => {
                let Some(g) = self.groups.get_mut(group_id) else {
                    return;
                };
                let mut set = HashSet::new();
                for c in caps {
                    set.insert(*c);
                }
                g.role_caps.insert(*role, set);
            }
        }
    }

    pub fn group_by_name(&self, name: &str) -> Option<&Group> {
        let key = name.trim().to_ascii_lowercase();
        let id = *self.group_ids_by_name.get(&key)?;
        self.groups.get(&id)
    }

    pub fn effective_caps_for_principal(
        &self,
        principal: &str,
        class: &str,
    ) -> HashSet<Capability> {
        let key = principal.trim().to_ascii_lowercase();
        let mut out = HashSet::new();
        for g in self.groups.values() {
            let mut role_opt = g.members.get(&key).copied();
            if role_opt.is_none() {
                role_opt = g.implied_role_for_class(class);
            }
            let Some(role) = role_opt else {
                continue;
            };
            out.extend(g.caps_for_role(role));
        }
        out
    }

    pub fn role_for_principal_in_group(
        &self,
        group_id: u64,
        principal: &str,
        class: &str,
    ) -> Option<GroupRole> {
        let g = self.groups.get(&group_id)?;
        let key = principal.trim().to_ascii_lowercase();
        let mut role_opt = g.members.get(&key).copied();
        if role_opt.is_none() {
            role_opt = g.implied_role_for_class(class);
        }
        role_opt
    }

    pub fn caps_for_principal_in_group(
        &self,
        group_id: u64,
        principal: &str,
        class: &str,
    ) -> HashSet<Capability> {
        let Some(role) = self.role_for_principal_in_group(group_id, principal, class) else {
            return HashSet::new();
        };
        self.groups
            .get(&group_id)
            .map(|g| g.caps_for_role(role))
            .unwrap_or_default()
    }
}
