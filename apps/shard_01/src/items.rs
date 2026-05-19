#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Small,
    Medium,
    Large,
}

impl Size {
    pub fn as_str(self) -> &'static str {
        match self {
            Size::Small => "small",
            Size::Medium => "medium",
            Size::Large => "large",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipSlot {
    Wield,
    Shield,
    Head,
    Neck,
    Body,
    Legs,
    Feet,
    Hands,
    Arms,
}

impl EquipSlot {
    pub fn all() -> &'static [EquipSlot] {
        &[
            EquipSlot::Wield,
            EquipSlot::Shield,
            EquipSlot::Head,
            EquipSlot::Neck,
            EquipSlot::Body,
            EquipSlot::Arms,
            EquipSlot::Hands,
            EquipSlot::Legs,
            EquipSlot::Feet,
        ]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EquipSlot::Wield => "wield",
            EquipSlot::Shield => "shield",
            EquipSlot::Head => "head",
            EquipSlot::Neck => "neck",
            EquipSlot::Body => "body",
            EquipSlot::Legs => "legs",
            EquipSlot::Feet => "feet",
            EquipSlot::Hands => "hands",
            EquipSlot::Arms => "arms",
        }
    }

    pub fn parse(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "wield" | "weapon" | "mainhand" | "main-hand" => Some(EquipSlot::Wield),
            "shield" | "offhand" | "off-hand" => Some(EquipSlot::Shield),
            "head" | "helm" | "helmet" => Some(EquipSlot::Head),
            "neck" | "amulet" | "necklace" | "pendant" | "charm" => Some(EquipSlot::Neck),
            "body" | "torso" | "chest" | "armor" | "armour" => Some(EquipSlot::Body),
            "legs" | "pants" | "trousers" => Some(EquipSlot::Legs),
            "feet" | "boots" | "shoes" => Some(EquipSlot::Feet),
            "hands" | "hand" | "gloves" | "gauntlets" => Some(EquipSlot::Hands),
            "arms" | "sleeves" => Some(EquipSlot::Arms),
            _ => None,
        }
    }

    pub fn min_world_log_format(self) -> u32 {
        match self {
            EquipSlot::Neck => 2,
            _ => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmorClass {
    Cloth,
    Leather,
    Mail,
    Plate,
    Shield,
}

impl ArmorClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ArmorClass::Cloth => "cloth",
            ArmorClass::Leather => "leather",
            ArmorClass::Mail => "mail",
            ArmorClass::Plate => "plate",
            ArmorClass::Shield => "shield",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WeaponDef {
    pub dmg_min: i32,
    pub dmg_max: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct ArmorDef {
    pub slot: EquipSlot,
    pub armor_value: i32,
    pub class: ArmorClass,
}

#[derive(Debug, Clone, Copy)]
pub struct AccessoryDef {
    pub slot: EquipSlot,
}

#[derive(Debug, Clone, Copy)]
pub struct ConsumableDef {
    pub heal: i32,
}

#[derive(Debug, Clone, Copy)]
pub enum ItemKind {
    Weapon(WeaponDef),
    Armor(ArmorDef),
    Accessory(AccessoryDef),
    Consumable(ConsumableDef),
    Misc,
}

#[derive(Debug, Clone, Copy)]
pub struct ItemDef {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub size: Option<Size>,
    pub kind: ItemKind,
    pub description: &'static str,
}

impl ItemDef {
    pub fn matches_token(&self, token: &str) -> bool {
        let t = token.trim().to_ascii_lowercase();
        if t.is_empty() {
            return false;
        }
        let name_lc = self.name.to_ascii_lowercase();
        if name_lc == t || name_lc.starts_with(&t) {
            return true;
        }
        self.aliases.iter().any(|a| a.eq_ignore_ascii_case(&t))
    }

    pub fn equip_slot(&self) -> Option<EquipSlot> {
        match self.kind {
            ItemKind::Weapon(_) => Some(EquipSlot::Wield),
            ItemKind::Armor(a) => Some(a.slot),
            ItemKind::Accessory(a) => Some(a.slot),
            ItemKind::Consumable(_) | ItemKind::Misc => None,
        }
    }
}

static ITEMS: [ItemDef; 12] = [
    ItemDef {
        name: "stenchpouch",
        aliases: &["stench", "pouch", "stench pouch"],
        size: None,
        kind: ItemKind::Misc,
        description: "a waxed pouch of compressed stench.\n\nit is unpleasantly warm.\n",
    },
    ItemDef {
        name: "field bandage",
        aliases: &["bandage", "bandages", "field", "heal", "healing"],
        size: None,
        kind: ItemKind::Consumable(ConsumableDef { heal: 6 }),
        description: "a disposable field bandage.\n\nyou can `use` it.\n",
    },
    ItemDef {
        name: "training charm",
        aliases: &["charm", "amulet", "necklace", "pendant"],
        size: None,
        kind: ItemKind::Accessory(AccessoryDef {
            slot: EquipSlot::Neck,
        }),
        description: "a cord charm issued when the guilds start tracking neck gear.\n",
    },
    ItemDef {
        name: "practice sword (small)",
        aliases: &["sword", "practice sword", "blade"],
        size: Some(Size::Small),
        kind: ItemKind::Weapon(WeaponDef {
            dmg_min: 2,
            dmg_max: 5,
        }),
        description: "a training sword with a dull edge.\n",
    },
    ItemDef {
        name: "practice sword (medium)",
        aliases: &["sword", "practice sword", "blade"],
        size: Some(Size::Medium),
        kind: ItemKind::Weapon(WeaponDef {
            dmg_min: 2,
            dmg_max: 5,
        }),
        description: "a training sword with a dull edge.\n",
    },
    ItemDef {
        name: "wooden buckler (small)",
        aliases: &["buckler", "shield"],
        size: Some(Size::Small),
        kind: ItemKind::Armor(ArmorDef {
            slot: EquipSlot::Shield,
            armor_value: 1,
            class: ArmorClass::Shield,
        }),
        description: "a light wooden buckler.\n",
    },
    ItemDef {
        name: "wooden buckler (medium)",
        aliases: &["buckler", "shield"],
        size: Some(Size::Medium),
        kind: ItemKind::Armor(ArmorDef {
            slot: EquipSlot::Shield,
            armor_value: 1,
            class: ArmorClass::Shield,
        }),
        description: "a light wooden buckler.\n",
    },
    ItemDef {
        name: "training tunic (small)",
        aliases: &["tunic", "shirt", "armor", "armour", "training tunic"],
        size: Some(Size::Small),
        kind: ItemKind::Armor(ArmorDef {
            slot: EquipSlot::Body,
            armor_value: 2,
            class: ArmorClass::Cloth,
        }),
        description: "a padded training tunic.\n",
    },
    ItemDef {
        name: "training tunic (medium)",
        aliases: &["tunic", "shirt", "armor", "armour", "training tunic"],
        size: Some(Size::Medium),
        kind: ItemKind::Armor(ArmorDef {
            slot: EquipSlot::Body,
            armor_value: 2,
            class: ArmorClass::Cloth,
        }),
        description: "a padded training tunic.\n",
    },
    ItemDef {
        name: "training boots (small)",
        aliases: &["boots", "shoes", "training boots"],
        size: Some(Size::Small),
        kind: ItemKind::Armor(ArmorDef {
            slot: EquipSlot::Feet,
            armor_value: 1,
            class: ArmorClass::Leather,
        }),
        description: "scuffed training boots.\n",
    },
    ItemDef {
        name: "training boots (medium)",
        aliases: &["boots", "shoes", "training boots"],
        size: Some(Size::Medium),
        kind: ItemKind::Armor(ArmorDef {
            slot: EquipSlot::Feet,
            armor_value: 1,
            class: ArmorClass::Leather,
        }),
        description: "scuffed training boots.\n",
    },
    ItemDef {
        name: "training gloves",
        aliases: &["gloves", "gauntlets", "hands"],
        size: None,
        kind: ItemKind::Armor(ArmorDef {
            slot: EquipSlot::Hands,
            armor_value: 1,
            class: ArmorClass::Leather,
        }),
        description: "thin gloves meant to keep you from leaving blood on the rack.\n",
    },
];

pub fn find_item_def(name: &str) -> Option<&'static ItemDef> {
    let t = name.trim();
    if t.is_empty() {
        return None;
    }
    ITEMS.iter().find(|d| d.name.eq_ignore_ascii_case(t))
}

pub fn all_item_defs() -> &'static [ItemDef] {
    &ITEMS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neck_slot_is_format2_accessory() {
        assert_eq!(EquipSlot::parse("neck"), Some(EquipSlot::Neck));
        assert_eq!(EquipSlot::parse("amulet"), Some(EquipSlot::Neck));
        assert_eq!(EquipSlot::Neck.as_str(), "neck");
        assert_eq!(EquipSlot::Neck.min_world_log_format(), 2);
        assert_eq!(EquipSlot::Wield.min_world_log_format(), 1);

        let charm = find_item_def("training charm").unwrap();
        assert_eq!(charm.equip_slot(), Some(EquipSlot::Neck));
        assert!(matches!(charm.kind, ItemKind::Accessory(_)));
    }
}
