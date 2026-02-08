//! Manually written FlatBuffers schema bindings for rooms.
//!
//! Schema (rooms.fbs):
//! table Exit { dir:string; to:string; }
//! table Room { id:string; name:string; description:string; exits:[Exit]; area:string; }
//! table Area { id:string; name:string; rooms:[Room]; }
//! table World { areas:[Area]; }
//! root_type World;

use flatbuffers::FlatBufferBuilder;
use flatbuffers::Follow;
use flatbuffers::ForwardsUOffset;
use flatbuffers::Table;
use flatbuffers::TableUnfinishedWIPOffset;
use flatbuffers::VOffsetT;
use flatbuffers::Vector;
use flatbuffers::WIPOffset;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Exit<'a> {
    _tab: Table<'a>,
}

impl<'a> Follow<'a> for Exit<'a> {
    type Inner = Exit<'a>;
    unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        Exit {
            _tab: unsafe { Table::new(buf, loc) },
        }
    }
}

impl<'a> Exit<'a> {
    const VT_DIR: VOffsetT = 4;
    const VT_TO: VOffsetT = 6;

    pub fn dir(&self) -> Option<&'a str> {
        unsafe {
            self._tab
                .get::<ForwardsUOffset<&'a str>>(Self::VT_DIR, None)
        }
    }

    pub fn to(&self) -> Option<&'a str> {
        unsafe { self._tab.get::<ForwardsUOffset<&'a str>>(Self::VT_TO, None) }
    }
}

pub struct ExitArgs<'a> {
    pub dir: Option<WIPOffset<&'a str>>,
    pub to: Option<WIPOffset<&'a str>>,
}

impl<'a> Default for ExitArgs<'a> {
    fn default() -> Self {
        Self {
            dir: None,
            to: None,
        }
    }
}

pub struct ExitBuilder<'a: 'b, 'b> {
    fbb: &'b mut FlatBufferBuilder<'a>,
    start: WIPOffset<TableUnfinishedWIPOffset>,
}

impl<'a: 'b, 'b> ExitBuilder<'a, 'b> {
    pub fn new(fbb: &'b mut FlatBufferBuilder<'a>) -> Self {
        let start = fbb.start_table();
        Self { fbb, start }
    }

    pub fn add_dir(&mut self, dir: WIPOffset<&'b str>) {
        self.fbb.push_slot_always(Exit::VT_DIR, dir);
    }

    pub fn add_to(&mut self, to: WIPOffset<&'b str>) {
        self.fbb.push_slot_always(Exit::VT_TO, to);
    }

    pub fn finish(self) -> WIPOffset<Exit<'a>> {
        let o = self.fbb.end_table(self.start);
        WIPOffset::new(o.value())
    }
}

pub fn create_exit<'a: 'b, 'b>(
    fbb: &'b mut FlatBufferBuilder<'a>,
    args: &ExitArgs<'b>,
) -> WIPOffset<Exit<'a>> {
    let mut builder = ExitBuilder::new(fbb);
    if let Some(dir) = args.dir {
        builder.add_dir(dir);
    }
    if let Some(to) = args.to {
        builder.add_to(to);
    }
    builder.finish()
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Room<'a> {
    _tab: Table<'a>,
}

impl<'a> Follow<'a> for Room<'a> {
    type Inner = Room<'a>;
    unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        Room {
            _tab: unsafe { Table::new(buf, loc) },
        }
    }
}

impl<'a> Room<'a> {
    const VT_ID: VOffsetT = 4;
    const VT_NAME: VOffsetT = 6;
    const VT_DESC: VOffsetT = 8;
    const VT_EXITS: VOffsetT = 10;
    const VT_AREA: VOffsetT = 12;

    pub fn id(&self) -> Option<&'a str> {
        unsafe { self._tab.get::<ForwardsUOffset<&'a str>>(Self::VT_ID, None) }
    }

    pub fn name(&self) -> Option<&'a str> {
        unsafe {
            self._tab
                .get::<ForwardsUOffset<&'a str>>(Self::VT_NAME, None)
        }
    }

    pub fn description(&self) -> Option<&'a str> {
        unsafe {
            self._tab
                .get::<ForwardsUOffset<&'a str>>(Self::VT_DESC, None)
        }
    }

    pub fn exits(&self) -> Option<Vector<'a, ForwardsUOffset<Exit<'a>>>> {
        unsafe {
            self._tab
                .get::<ForwardsUOffset<Vector<'a, ForwardsUOffset<Exit<'a>>>>>(Self::VT_EXITS, None)
        }
    }

    #[allow(dead_code)]
    pub fn area(&self) -> Option<&'a str> {
        unsafe {
            self._tab
                .get::<ForwardsUOffset<&'a str>>(Self::VT_AREA, None)
        }
    }
}

pub struct RoomArgs<'a> {
    pub id: Option<WIPOffset<&'a str>>,
    pub name: Option<WIPOffset<&'a str>>,
    pub description: Option<WIPOffset<&'a str>>,
    pub exits: Option<WIPOffset<Vector<'a, ForwardsUOffset<Exit<'a>>>>>,
    pub area: Option<WIPOffset<&'a str>>,
}

impl<'a> Default for RoomArgs<'a> {
    fn default() -> Self {
        Self {
            id: None,
            name: None,
            description: None,
            exits: None,
            area: None,
        }
    }
}

pub struct RoomBuilder<'a: 'b, 'b> {
    fbb: &'b mut FlatBufferBuilder<'a>,
    start: WIPOffset<TableUnfinishedWIPOffset>,
}

impl<'a: 'b, 'b> RoomBuilder<'a, 'b> {
    pub fn new(fbb: &'b mut FlatBufferBuilder<'a>) -> Self {
        let start = fbb.start_table();
        Self { fbb, start }
    }

    pub fn add_id(&mut self, id: WIPOffset<&'b str>) {
        self.fbb.push_slot_always(Room::VT_ID, id);
    }

    pub fn add_name(&mut self, name: WIPOffset<&'b str>) {
        self.fbb.push_slot_always(Room::VT_NAME, name);
    }

    pub fn add_description(&mut self, description: WIPOffset<&'b str>) {
        self.fbb.push_slot_always(Room::VT_DESC, description);
    }

    pub fn add_exits(&mut self, exits: WIPOffset<Vector<'b, ForwardsUOffset<Exit<'b>>>>) {
        self.fbb.push_slot_always(Room::VT_EXITS, exits);
    }

    pub fn add_area(&mut self, area: WIPOffset<&'b str>) {
        self.fbb.push_slot_always(Room::VT_AREA, area);
    }

    pub fn finish(self) -> WIPOffset<Room<'a>> {
        let o = self.fbb.end_table(self.start);
        WIPOffset::new(o.value())
    }
}

pub fn create_room<'a: 'b, 'b>(
    fbb: &'b mut FlatBufferBuilder<'a>,
    args: &RoomArgs<'b>,
) -> WIPOffset<Room<'a>> {
    let mut builder = RoomBuilder::new(fbb);
    if let Some(id) = args.id {
        builder.add_id(id);
    }
    if let Some(name) = args.name {
        builder.add_name(name);
    }
    if let Some(description) = args.description {
        builder.add_description(description);
    }
    if let Some(exits) = args.exits {
        builder.add_exits(exits);
    }
    if let Some(area) = args.area {
        builder.add_area(area);
    }
    builder.finish()
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Area<'a> {
    _tab: Table<'a>,
}

impl<'a> Follow<'a> for Area<'a> {
    type Inner = Area<'a>;
    unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        Area {
            _tab: unsafe { Table::new(buf, loc) },
        }
    }
}

impl<'a> Area<'a> {
    const VT_ID: VOffsetT = 4;
    const VT_NAME: VOffsetT = 6;
    const VT_ROOMS: VOffsetT = 8;

    pub fn id(&self) -> Option<&'a str> {
        unsafe { self._tab.get::<ForwardsUOffset<&'a str>>(Self::VT_ID, None) }
    }

    pub fn name(&self) -> Option<&'a str> {
        unsafe {
            self._tab
                .get::<ForwardsUOffset<&'a str>>(Self::VT_NAME, None)
        }
    }

    pub fn rooms(&self) -> Option<Vector<'a, ForwardsUOffset<Room<'a>>>> {
        unsafe {
            self._tab
                .get::<ForwardsUOffset<Vector<'a, ForwardsUOffset<Room<'a>>>>>(Self::VT_ROOMS, None)
        }
    }
}

pub struct AreaArgs<'a> {
    pub id: Option<WIPOffset<&'a str>>,
    pub name: Option<WIPOffset<&'a str>>,
    pub rooms: Option<WIPOffset<Vector<'a, ForwardsUOffset<Room<'a>>>>>,
}

impl<'a> Default for AreaArgs<'a> {
    fn default() -> Self {
        Self {
            id: None,
            name: None,
            rooms: None,
        }
    }
}

pub struct AreaBuilder<'a: 'b, 'b> {
    fbb: &'b mut FlatBufferBuilder<'a>,
    start: WIPOffset<TableUnfinishedWIPOffset>,
}

impl<'a: 'b, 'b> AreaBuilder<'a, 'b> {
    pub fn new(fbb: &'b mut FlatBufferBuilder<'a>) -> Self {
        let start = fbb.start_table();
        Self { fbb, start }
    }

    pub fn add_id(&mut self, id: WIPOffset<&'b str>) {
        self.fbb.push_slot_always(Area::VT_ID, id);
    }

    pub fn add_name(&mut self, name: WIPOffset<&'b str>) {
        self.fbb.push_slot_always(Area::VT_NAME, name);
    }

    pub fn add_rooms(&mut self, rooms: WIPOffset<Vector<'b, ForwardsUOffset<Room<'b>>>>) {
        self.fbb.push_slot_always(Area::VT_ROOMS, rooms);
    }

    pub fn finish(self) -> WIPOffset<Area<'a>> {
        let o = self.fbb.end_table(self.start);
        WIPOffset::new(o.value())
    }
}

pub fn create_area<'a: 'b, 'b>(
    fbb: &'b mut FlatBufferBuilder<'a>,
    args: &AreaArgs<'b>,
) -> WIPOffset<Area<'a>> {
    let mut builder = AreaBuilder::new(fbb);
    if let Some(id) = args.id {
        builder.add_id(id);
    }
    if let Some(name) = args.name {
        builder.add_name(name);
    }
    if let Some(rooms) = args.rooms {
        builder.add_rooms(rooms);
    }
    builder.finish()
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct World<'a> {
    _tab: Table<'a>,
}

impl<'a> Follow<'a> for World<'a> {
    type Inner = World<'a>;
    unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        World {
            _tab: unsafe { Table::new(buf, loc) },
        }
    }
}

impl<'a> World<'a> {
    const VT_AREAS: VOffsetT = 4;

    pub fn areas(&self) -> Option<Vector<'a, ForwardsUOffset<Area<'a>>>> {
        unsafe {
            self._tab
                .get::<ForwardsUOffset<Vector<'a, ForwardsUOffset<Area<'a>>>>>(Self::VT_AREAS, None)
        }
    }
}

pub struct WorldArgs<'a> {
    pub areas: Option<WIPOffset<Vector<'a, ForwardsUOffset<Area<'a>>>>>,
}

impl<'a> Default for WorldArgs<'a> {
    fn default() -> Self {
        Self { areas: None }
    }
}

pub struct WorldBuilder<'a: 'b, 'b> {
    fbb: &'b mut FlatBufferBuilder<'a>,
    start: WIPOffset<TableUnfinishedWIPOffset>,
}

impl<'a: 'b, 'b> WorldBuilder<'a, 'b> {
    pub fn new(fbb: &'b mut FlatBufferBuilder<'a>) -> Self {
        let start = fbb.start_table();
        Self { fbb, start }
    }

    pub fn add_areas(&mut self, areas: WIPOffset<Vector<'b, ForwardsUOffset<Area<'b>>>>) {
        self.fbb.push_slot_always(World::VT_AREAS, areas);
    }

    pub fn finish(self) -> WIPOffset<World<'a>> {
        let o = self.fbb.end_table(self.start);
        WIPOffset::new(o.value())
    }
}

pub fn create_world<'a: 'b, 'b>(
    fbb: &'b mut FlatBufferBuilder<'a>,
    args: &WorldArgs<'b>,
) -> WIPOffset<World<'a>> {
    let mut builder = WorldBuilder::new(fbb);
    if let Some(areas) = args.areas {
        builder.add_areas(areas);
    }
    builder.finish()
}

pub fn build_world_buffer() -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    // ---------------- Areas (A01-ish) ----------------
    let area_town_id = fbb.create_string("a01_town_gaia_gate");
    let area_town_name = fbb.create_string("Town: Gaia Gate");

    let area_class_halls_id = fbb.create_string("a01_class_halls");
    let area_class_halls_name = fbb.create_string("Class Halls");

    let area_school_id = fbb.create_string("a01_newbie_school");
    let area_school_name = fbb.create_string("Newbie School");

    let area_hunt_id = fbb.create_string("a01_hunting_grounds");
    let area_hunt_name = fbb.create_string("Hunting Grounds");

    let area_sewers_id = fbb.create_string("a01_sewers");
    let area_sewers_name = fbb.create_string("Under-Town Sewers");

    let area_quarry_id = fbb.create_string("a01_quarry");
    let area_quarry_name = fbb.create_string("Quarry");

    // ---------------- Rooms ----------------
    let room_town_gate_id = fbb.create_string("town.gate");
    let room_town_gate_name = fbb.create_string("Gaia Gate");
    let room_town_gate_desc = fbb.create_string(
        "A bright arrivals plaza hums with low voices and distant machinery. A painted sign reads: GAIA: HUMAN + ROBOT.",
    );

    // Overworld portal stubs (until Town becomes a full area file).
    let room_portal_town_ns_id = fbb.create_string("P_TOWN_NS");
    let room_portal_town_ns_name = fbb.create_string("Gate Security Arch");
    let room_portal_town_ns_desc = fbb.create_string(
        "A quiet security archway and a strip of clean floor that feels like a border. The city smells like food. The school smells like bleach.",
    );
    let room_portal_ns_town_id = fbb.create_string("P_NS_TOWN");

    let room_town_class_row_id = fbb.create_string("town.class_row");
    let room_town_class_row_name = fbb.create_string("Hall Row");
    let room_town_class_row_desc = fbb.create_string(
        "A long colonnade lined with banners for each calling. Doors bear simple sigils and the scent of oil, incense, and ink. A sign reads: CHOOSE A HALL BY NAME.",
    );

    let room_tavern_id = fbb.create_string("town.tavern");
    let room_tavern_name = fbb.create_string("The Tavern");
    let room_tavern_desc = fbb.create_string(
        "Warm light, cheap wood, and a hard-earned quiet. A chalkboard reads: NO SOLICITING. NO SPAM. YES STORIES.",
    );

    let room_graveyard_id = fbb.create_string("town.graveyard");
    let room_graveyard_name = fbb.create_string("The Graveyard");
    let room_graveyard_desc = fbb.create_string(
        "A small fenced yard of dark soil and pale stones. The air tastes like rain and old names. A narrow path leads back toward the living.",
    );

    let room_school_orientation_id = fbb.create_string("newbie_school.orientation");
    let room_school_orientation_name = fbb.create_string("Orientation Wing");
    let room_school_orientation_desc = fbb.create_string(
        "Sterile halls, clean signage, and a gentle training hum. A corridor leads back toward the city gate.\n\nA small Hall of Heroes sits off to one side. Two stone sentinels stand guard: Hatchet (chipped blade) and Javelin (point held outward). A third name is freshly cut into the stone: Aradune.",
    );

    let room_school_first_fight_id = fbb.create_string("newbie_school.first_fight");
    let room_school_first_fight_name = fbb.create_string("First Fight Ring");
    let room_school_first_fight_desc = fbb.create_string(
        "A small sparring ring with scuffed floors and neat chalk lines. A training placard reads: KILL STENCHWORM.\n\nA drain in the corner smells like a dare.",
    );

    let room_meadow_id = fbb.create_string("meadowline.trails");
    let room_meadow_name = fbb.create_string("Meadowline Trails");
    let room_meadow_desc = fbb.create_string(
        "A safe loop of trails with clear sightlines. You can hear the town behind you and the wild ahead.",
    );

    let room_orchard_id = fbb.create_string("scrap_orchard.grove");
    let room_orchard_name = fbb.create_string("Scrap Orchard Grove");
    let room_orchard_desc = fbb.create_string(
        "Half-buried metal ribs and old cabling twist through roots. Drones flicker in the branches like lanterns.",
    );

    let room_sewers_from_town_id = fbb.create_string("sewers.from_town");
    let room_sewers_from_town_name = fbb.create_string("Town Maintenance Ladder");
    let room_sewers_from_town_desc = fbb.create_string(
        "A rusted ladder rises into the town's maintenance office. Fresh paint stripes and bolted arrows point deeper into the tunnels.",
    );

    let room_sewers_from_meadow_id = fbb.create_string("sewers.from_meadow");
    let room_sewers_from_meadow_name = fbb.create_string("Meadowline Grate");
    let room_sewers_from_meadow_desc = fbb.create_string(
        "A low grate lets in grass-scented air. Water trickles along the channel, and every sound turns into an echo.",
    );

    let room_sewers_from_orchard_id = fbb.create_string("sewers.from_orchard");
    let room_sewers_from_orchard_name = fbb.create_string("Orchard Pipeway");
    let room_sewers_from_orchard_desc = fbb.create_string(
        "A tilted pipeway hums under the roots. The air smells faintly of coolant and wet metal. Loose cabling vanishes into the dark.",
    );

    let room_sewers_junction_id = fbb.create_string("sewers.junction");
    let room_sewers_junction_name = fbb.create_string("Sewers Junction");
    let room_sewers_junction_desc = fbb.create_string(
        "A wide junction chamber with bolted signage. Chalk marks show valve progress. Routes branch toward valves, sludge loops, and a sealed quarry gate.",
    );

    let room_sewers_valves_id = fbb.create_string("sewers.valves");
    let room_sewers_valves_name = fbb.create_string("Valve Access Tunnels");
    let room_sewers_valves_desc = fbb.create_string(
        "Three marked service spokes lead to valve rooms. The air is warmer here and the pipes answer with a slow ticking.",
    );

    let room_sewers_valve_1_id = fbb.create_string("sewers.valve_1");
    let room_sewers_valve_1_name = fbb.create_string("Valve Room One");
    let room_sewers_valve_1_desc = fbb.create_string(
        "A heavy wheel sits on a stubborn seal. The floor is slick with old grease and boot prints that never quite lead straight.",
    );

    let room_sewers_valve_2_id = fbb.create_string("sewers.valve_2");
    let room_sewers_valve_2_name = fbb.create_string("Valve Room Two");
    let room_sewers_valve_2_desc = fbb.create_string(
        "A split pipe hisses softly into a drain. The valve housing is caked with mineral crust and something that looks like tar.",
    );

    let room_sewers_valve_3_id = fbb.create_string("sewers.valve_3");
    let room_sewers_valve_3_name = fbb.create_string("Valve Room Three");
    let room_sewers_valve_3_desc = fbb.create_string(
        "A narrow service bay with a grated catwalk. The valve wheel is reachable, but the room makes every movement feel cramped.",
    );

    let room_sewers_sludge_id = fbb.create_string("sewers.sludge");
    let room_sewers_sludge_name = fbb.create_string("Sludge Loops");
    let room_sewers_sludge_desc = fbb.create_string(
        "A looping run of low tunnels where the sludge never fully drains. Hazard stripes mark safer footing and occasional dry pockets.",
    );

    let room_sewers_drone_extract_id = fbb.create_string("sewers.drone_extract");
    let room_sewers_drone_extract_name = fbb.create_string("Collapsed Drone Bay");
    let room_sewers_drone_extract_desc = fbb.create_string(
        "A maintenance drone blinks from under debris. Its casing is dented, its light frantic. The way out is narrow and messy.",
    );

    let room_sewers_safe_pocket_id = fbb.create_string("sewers.safe_pocket");
    let room_sewers_safe_pocket_name = fbb.create_string("Dry Maintenance Alcove");
    let room_sewers_safe_pocket_desc = fbb.create_string(
        "A dry alcove with a bolted bench and a humming panel. The air is clearer here. You can safely pause and plan.",
    );

    let room_sewers_keycard_door_id = fbb.create_string("sewers.keycard_door");
    let room_sewers_keycard_door_name = fbb.create_string("Keycard Door");
    let room_sewers_keycard_door_desc = fbb.create_string(
        "A reinforced door with a dead reader. Someone has scratched: 'CARD STILL WORKS, IF YOU FIND IT.'",
    );

    let room_sewers_grease_approach_id = fbb.create_string("sewers.grease_approach");
    let room_sewers_grease_approach_name = fbb.create_string("Grease King Approach");
    let room_sewers_grease_approach_desc = fbb.create_string(
        "The tunnels widen and the stink thickens. A low vibration travels through the stone like distant laughter.",
    );

    let room_sewers_grease_arena_id = fbb.create_string("sewers.grease_arena");
    let room_sewers_grease_arena_name = fbb.create_string("Grease King Arena");
    let room_sewers_grease_arena_desc = fbb.create_string(
        "A round chamber coated in slick residue. Broken pipes drip in uneven cadence. Something watches from the shine.",
    );

    let room_sewers_to_quarry_id = fbb.create_string("sewers.to_quarry");
    let room_sewers_to_quarry_name = fbb.create_string("Quarry Gate Tunnel");
    let room_sewers_to_quarry_desc = fbb.create_string(
        "A sealed maintenance gate is stamped: QUARRY. A side tunnel runs back toward the junction, away from the colder air beyond.",
    );

    let room_quarry_foothills_id = fbb.create_string("quarry.foothills");
    let room_quarry_foothills_name = fbb.create_string("Quarry Foothills");
    let room_quarry_foothills_desc = fbb.create_string(
        "Cold air carries grit and distant hammering. Work lights flicker across broken stone. The quarry yawns ahead.",
    );

    let room_class_fighter_id = fbb.create_string("class_halls.fighter");
    let room_class_fighter_name = fbb.create_string("Anvil Keep");
    let room_class_fighter_desc = fbb.create_string(
        "Hammer rings on steel. Kera Forgefront runs the shop; Captain Rhune drills the line. Sable Recruiter and Holt Veteran take names for work.",
    );

    let room_class_rogue_id = fbb.create_string("class_halls.rogue");
    let room_class_rogue_name = fbb.create_string("Shadow Exchange");
    let room_class_rogue_desc = fbb.create_string(
        "Low light, clean edges, and a ledger that never leaves the table. Lilt Fence keeps the shop; Mistcut trains. Nix and Echo Glass trade jobs.",
    );

    let room_class_cleric_id = fbb.create_string("class_halls.cleric");
    let room_class_cleric_name = fbb.create_string("Sanctum of Quiet");
    let room_class_cleric_desc = fbb.create_string(
        "Incense and soft voices. Sister Vell keeps relics; Canon Hara teaches rites. Brother Piers offers pilgrim tasks.",
    );

    let room_class_wizard_id = fbb.create_string("class_halls.wizard");
    let room_class_wizard_name = fbb.create_string("Arcane Annex");
    let room_class_wizard_desc = fbb.create_string(
        "Shelves of ink and brass, chalk on every surface. Mira Quill minds the shop; Archmage Sel instructs. Sela Archivist and Orin Scribe post commissions.",
    );

    let room_class_ranger_id = fbb.create_string("class_halls.ranger");
    let room_class_ranger_name = fbb.create_string("Greenwatch Lodge");
    let room_class_ranger_desc = fbb.create_string(
        "Weathered beams, maps, and bow racks. Pine Flint supplies the hall; Tracker Mae drills. Jory Scout and Kestrel Pathfinder have trailwork.",
    );

    let room_class_paladin_id = fbb.create_string("class_halls.paladin");
    let room_class_paladin_name = fbb.create_string("Oathbound Court");
    let room_class_paladin_desc = fbb.create_string(
        "Bright banners and clean stone. Rhea Sunsteel runs the armory; Justicar Hal trains. Lumen Vowkeeper and Alden Oathbound stand by for vows.",
    );

    let room_class_bard_id = fbb.create_string("class_halls.bard");
    let room_class_bard_name = fbb.create_string("Lilt House");
    let room_class_bard_desc = fbb.create_string(
        "Strings on the walls and a gentle crowd. Caro Strings keeps the shop; Maestra Jun teaches. Piper Vale and Tess Chronicler need stories.",
    );

    let room_class_druid_id = fbb.create_string("class_halls.druid");
    let room_class_druid_name = fbb.create_string("Rootsong Grotto");
    let room_class_druid_desc = fbb.create_string(
        "Living stone and damp earth. Iri Moss trades herbs; Grovecaller Olan trains. Bracken and Fern Watcher seek helpers.",
    );

    let room_class_barbarian_id = fbb.create_string("class_halls.barbarian");
    let room_class_barbarian_name = fbb.create_string("Ironhowl Yard");
    let room_class_barbarian_desc = fbb.create_string(
        "A ring of scored wood and heavy chains. Krag Stonefury sells gear; Warchief Una trains. Rok Loud and Mira Flint call for bolds.",
    );

    let room_class_warlock_id = fbb.create_string("class_halls.warlock");
    let room_class_warlock_name = fbb.create_string("Pact Lantern");
    let room_class_warlock_desc = fbb.create_string(
        "Low flames and quiet bargains. Vesh Cinder handles the shop; Pactmaster Lira teaches. Hask Bound and Nyla Whisper offer contracts.",
    );

    let room_class_sorcerer_id = fbb.create_string("class_halls.sorcerer");
    let room_class_sorcerer_name = fbb.create_string("Stormblood Loft");
    let room_class_sorcerer_desc = fbb.create_string(
        "Crackling air and glass rods. Nira Sparkglass sells reagents; Wildcaster Joss trains. Fenn Unstable and Risa Flux need help.",
    );

    let room_class_monk_id = fbb.create_string("class_halls.monk");
    let room_class_monk_name = fbb.create_string("Stillwater Cloister");
    let room_class_monk_desc = fbb.create_string(
        "Quiet mats and a running fountain. Toma Quiethands keeps the shop; Master Sen trains. Ili Swift and Pema Still watch for recruits.",
    );

    // ---------------- Directions ----------------
    let dir_north = fbb.create_string("north");
    let dir_south = fbb.create_string("south");
    let dir_east = fbb.create_string("east");
    let dir_west = fbb.create_string("west");
    let dir_up = fbb.create_string("up");
    let dir_down = fbb.create_string("down");
    let dir_tavern = fbb.create_string("tavern");
    let dir_back = fbb.create_string("back");
    let dir_graveyard = fbb.create_string("graveyard");
    let dir_hall = fbb.create_string("hall");
    let dir_barbarian = fbb.create_string("barbarian");
    let dir_bard = fbb.create_string("bard");
    let dir_cleric = fbb.create_string("cleric");
    let dir_druid = fbb.create_string("druid");
    let dir_fighter = fbb.create_string("fighter");
    let dir_monk = fbb.create_string("monk");
    let dir_paladin = fbb.create_string("paladin");
    let dir_ranger = fbb.create_string("ranger");
    let dir_rogue = fbb.create_string("rogue");
    let dir_sorcerer = fbb.create_string("sorcerer");
    let dir_warlock = fbb.create_string("warlock");
    let dir_wizard = fbb.create_string("wizard");
    let dir_junction = fbb.create_string("junction");
    let dir_town = fbb.create_string("town");
    let dir_meadow = fbb.create_string("meadow");
    let dir_orchard = fbb.create_string("orchard");
    let dir_valves = fbb.create_string("valves");
    let dir_v1 = fbb.create_string("v1");
    let dir_v2 = fbb.create_string("v2");
    let dir_v3 = fbb.create_string("v3");
    let dir_sludge = fbb.create_string("sludge");
    let dir_drone = fbb.create_string("drone");
    let dir_safe = fbb.create_string("safe");
    let dir_keycard = fbb.create_string("keycard");
    let dir_boss = fbb.create_string("boss");
    let dir_arena = fbb.create_string("arena");
    let dir_quarry = fbb.create_string("quarry");
    let dir_sewers = fbb.create_string("sewers");

    // ---------------- Exits ----------------
    // Town gate exits.
    let exit_town_tavern = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_tavern),
            to: Some(room_tavern_id),
        },
    );
    let exit_town_south = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_south),
            to: Some(room_portal_town_ns_id),
        },
    );
    let exit_town_east = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_east),
            to: Some(room_meadow_id),
        },
    );
    let exit_town_north = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_north),
            to: Some(room_orchard_id),
        },
    );
    let exit_town_down = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_down),
            to: Some(room_sewers_from_town_id),
        },
    );
    let exit_town_graveyard = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_graveyard),
            to: Some(room_graveyard_id),
        },
    );
    let exit_town_hall = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_hall),
            to: Some(room_town_class_row_id),
        },
    );
    let exits_town_gate = fbb.create_vector(&[
        exit_town_tavern,
        exit_town_south,
        exit_town_east,
        exit_town_north,
        exit_town_down,
        exit_town_graveyard,
        exit_town_hall,
    ]);

    // Town <-> Newbie School portal exits.
    let exit_portal_town_ns_north = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_north),
            to: Some(room_town_gate_id),
        },
    );
    let exit_portal_town_ns_south = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_south),
            to: Some(room_portal_ns_town_id),
        },
    );
    let exits_portal_town_ns = fbb.create_vector(&[
        exit_portal_town_ns_north,
        exit_portal_town_ns_south,
    ]);

    // Tavern exits.
    let exit_tavern_back = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_back),
            to: Some(room_town_gate_id),
        },
    );
    let exits_tavern = fbb.create_vector(&[exit_tavern_back]);

    // Class row exits.
    let exit_class_row_back = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_back),
            to: Some(room_town_gate_id),
        },
    );
    let exit_class_row_barbarian = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_barbarian),
            to: Some(room_class_barbarian_id),
        },
    );
    let exit_class_row_bard = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_bard),
            to: Some(room_class_bard_id),
        },
    );
    let exit_class_row_cleric = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_cleric),
            to: Some(room_class_cleric_id),
        },
    );
    let exit_class_row_druid = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_druid),
            to: Some(room_class_druid_id),
        },
    );
    let exit_class_row_fighter = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_fighter),
            to: Some(room_class_fighter_id),
        },
    );
    let exit_class_row_monk = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_monk),
            to: Some(room_class_monk_id),
        },
    );
    let exit_class_row_paladin = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_paladin),
            to: Some(room_class_paladin_id),
        },
    );
    let exit_class_row_ranger = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_ranger),
            to: Some(room_class_ranger_id),
        },
    );
    let exit_class_row_rogue = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_rogue),
            to: Some(room_class_rogue_id),
        },
    );
    let exit_class_row_sorcerer = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_sorcerer),
            to: Some(room_class_sorcerer_id),
        },
    );
    let exit_class_row_warlock = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_warlock),
            to: Some(room_class_warlock_id),
        },
    );
    let exit_class_row_wizard = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_wizard),
            to: Some(room_class_wizard_id),
        },
    );
    let exits_class_row = fbb.create_vector(&[
        exit_class_row_back,
        exit_class_row_barbarian,
        exit_class_row_bard,
        exit_class_row_cleric,
        exit_class_row_druid,
        exit_class_row_fighter,
        exit_class_row_monk,
        exit_class_row_paladin,
        exit_class_row_ranger,
        exit_class_row_rogue,
        exit_class_row_sorcerer,
        exit_class_row_warlock,
        exit_class_row_wizard,
    ]);

    // Class hall exits.
    let exit_class_hall_back = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_back),
            to: Some(room_town_class_row_id),
        },
    );
    let exits_class_hall = fbb.create_vector(&[exit_class_hall_back]);

    // Graveyard exits.
    let exit_graveyard_up = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_up),
            to: Some(room_town_gate_id),
        },
    );
    let exits_graveyard = fbb.create_vector(&[exit_graveyard_up]);

    // School exits.
    let exit_school_north = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_north),
            to: Some(room_town_gate_id),
        },
    );
    let exit_school_east = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_east),
            to: Some(room_school_first_fight_id),
        },
    );
    let exits_school_orientation = fbb.create_vector(&[exit_school_north, exit_school_east]);

    // First fight exits.
    let exit_first_fight_west = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_west),
            to: Some(room_school_orientation_id),
        },
    );
    let exits_first_fight = fbb.create_vector(&[exit_first_fight_west]);

    // Meadow exits.
    let exit_meadow_west = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_west),
            to: Some(room_town_gate_id),
        },
    );
    let exit_meadow_down = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_down),
            to: Some(room_sewers_from_meadow_id),
        },
    );
    let exits_meadow = fbb.create_vector(&[exit_meadow_west, exit_meadow_down]);

    // Orchard exits.
    let exit_orchard_south = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_south),
            to: Some(room_town_gate_id),
        },
    );
    let exit_orchard_down = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_down),
            to: Some(room_sewers_from_orchard_id),
        },
    );
    let exits_orchard = fbb.create_vector(&[exit_orchard_south, exit_orchard_down]);

    // Sewers: entry portals.
    let exit_sewers_from_town_up = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_up),
            to: Some(room_town_gate_id),
        },
    );
    let exit_sewers_from_town_junction = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_junction),
            to: Some(room_sewers_junction_id),
        },
    );
    let exits_sewers_from_town =
        fbb.create_vector(&[exit_sewers_from_town_up, exit_sewers_from_town_junction]);

    let exit_sewers_from_meadow_up = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_up),
            to: Some(room_meadow_id),
        },
    );
    let exit_sewers_from_meadow_junction = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_junction),
            to: Some(room_sewers_junction_id),
        },
    );
    let exits_sewers_from_meadow =
        fbb.create_vector(&[exit_sewers_from_meadow_up, exit_sewers_from_meadow_junction]);

    let exit_sewers_from_orchard_up = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_up),
            to: Some(room_orchard_id),
        },
    );
    let exit_sewers_from_orchard_junction = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_junction),
            to: Some(room_sewers_junction_id),
        },
    );
    let exits_sewers_from_orchard =
        fbb.create_vector(&[exit_sewers_from_orchard_up, exit_sewers_from_orchard_junction]);

    // Sewers: junction hub.
    let exit_sewers_junction_town = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_town),
            to: Some(room_sewers_from_town_id),
        },
    );
    let exit_sewers_junction_meadow = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_meadow),
            to: Some(room_sewers_from_meadow_id),
        },
    );
    let exit_sewers_junction_orchard = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_orchard),
            to: Some(room_sewers_from_orchard_id),
        },
    );
    let exit_sewers_junction_valves = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_valves),
            to: Some(room_sewers_valves_id),
        },
    );
    let exit_sewers_junction_sludge = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_sludge),
            to: Some(room_sewers_sludge_id),
        },
    );
    let exit_sewers_junction_boss = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_boss),
            to: Some(room_sewers_grease_approach_id),
        },
    );
    let exit_sewers_junction_quarry = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_quarry),
            to: Some(room_sewers_to_quarry_id),
        },
    );
    let exits_sewers_junction = fbb.create_vector(&[
        exit_sewers_junction_town,
        exit_sewers_junction_meadow,
        exit_sewers_junction_orchard,
        exit_sewers_junction_valves,
        exit_sewers_junction_sludge,
        exit_sewers_junction_boss,
        exit_sewers_junction_quarry,
    ]);

    // Sewers: valves spoke hub.
    let exit_sewers_valves_junction = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_junction),
            to: Some(room_sewers_junction_id),
        },
    );
    let exit_sewers_valves_v1 = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_v1),
            to: Some(room_sewers_valve_1_id),
        },
    );
    let exit_sewers_valves_v2 = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_v2),
            to: Some(room_sewers_valve_2_id),
        },
    );
    let exit_sewers_valves_v3 = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_v3),
            to: Some(room_sewers_valve_3_id),
        },
    );
    let exits_sewers_valves = fbb.create_vector(&[
        exit_sewers_valves_junction,
        exit_sewers_valves_v1,
        exit_sewers_valves_v2,
        exit_sewers_valves_v3,
    ]);

    let exit_sewers_valve_1_valves = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_valves),
            to: Some(room_sewers_valves_id),
        },
    );
    let exits_sewers_valve_1 = fbb.create_vector(&[exit_sewers_valve_1_valves]);

    let exit_sewers_valve_2_valves = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_valves),
            to: Some(room_sewers_valves_id),
        },
    );
    let exits_sewers_valve_2 = fbb.create_vector(&[exit_sewers_valve_2_valves]);

    let exit_sewers_valve_3_valves = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_valves),
            to: Some(room_sewers_valves_id),
        },
    );
    let exits_sewers_valve_3 = fbb.create_vector(&[exit_sewers_valve_3_valves]);

    // Sewers: sludge loop hub + optional wing.
    let exit_sewers_sludge_junction = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_junction),
            to: Some(room_sewers_junction_id),
        },
    );
    let exit_sewers_sludge_drone = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_drone),
            to: Some(room_sewers_drone_extract_id),
        },
    );
    let exit_sewers_sludge_safe = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_safe),
            to: Some(room_sewers_safe_pocket_id),
        },
    );
    let exit_sewers_sludge_keycard = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_keycard),
            to: Some(room_sewers_keycard_door_id),
        },
    );
    let exits_sewers_sludge = fbb.create_vector(&[
        exit_sewers_sludge_junction,
        exit_sewers_sludge_drone,
        exit_sewers_sludge_safe,
        exit_sewers_sludge_keycard,
    ]);

    let exit_sewers_drone_sludge = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_sludge),
            to: Some(room_sewers_sludge_id),
        },
    );
    let exits_sewers_drone_extract = fbb.create_vector(&[exit_sewers_drone_sludge]);

    let exit_sewers_safe_sludge = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_sludge),
            to: Some(room_sewers_sludge_id),
        },
    );
    let exits_sewers_safe_pocket = fbb.create_vector(&[exit_sewers_safe_sludge]);

    let exit_sewers_keycard_sludge = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_sludge),
            to: Some(room_sewers_sludge_id),
        },
    );
    let exit_sewers_keycard_junction = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_junction),
            to: Some(room_sewers_junction_id),
        },
    );
    let exits_sewers_keycard_door =
        fbb.create_vector(&[exit_sewers_keycard_sludge, exit_sewers_keycard_junction]);

    // Sewers: Grease King path.
    let exit_sewers_grease_approach_junction = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_junction),
            to: Some(room_sewers_junction_id),
        },
    );
    let exit_sewers_grease_approach_arena = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_arena),
            to: Some(room_sewers_grease_arena_id),
        },
    );
    let exits_sewers_grease_approach =
        fbb.create_vector(&[exit_sewers_grease_approach_junction, exit_sewers_grease_approach_arena]);

    let exit_sewers_grease_arena_back = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_back),
            to: Some(room_sewers_grease_approach_id),
        },
    );
    let exits_sewers_grease_arena = fbb.create_vector(&[exit_sewers_grease_arena_back]);

    // Sewers: portal to Quarry.
    let exit_sewers_to_quarry_junction = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_junction),
            to: Some(room_sewers_junction_id),
        },
    );
    let exit_sewers_to_quarry_quarry = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_quarry),
            to: Some(room_quarry_foothills_id),
        },
    );
    let exits_sewers_to_quarry =
        fbb.create_vector(&[exit_sewers_to_quarry_junction, exit_sewers_to_quarry_quarry]);

    // Quarry foothills stub.
    let exit_quarry_foothills_sewers = create_exit(
        &mut fbb,
        &ExitArgs {
            dir: Some(dir_sewers),
            to: Some(room_sewers_to_quarry_id),
        },
    );
    let exits_quarry_foothills = fbb.create_vector(&[exit_quarry_foothills_sewers]);

    // ---------------- Room objects ----------------
    let room_town_gate = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_town_gate_id),
            name: Some(room_town_gate_name),
            description: Some(room_town_gate_desc),
            exits: Some(exits_town_gate),
            area: Some(area_town_id),
        },
    );

    let room_portal_town_ns = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_portal_town_ns_id),
            name: Some(room_portal_town_ns_name),
            description: Some(room_portal_town_ns_desc),
            exits: Some(exits_portal_town_ns),
            area: Some(area_town_id),
        },
    );

    let room_tavern = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_tavern_id),
            name: Some(room_tavern_name),
            description: Some(room_tavern_desc),
            exits: Some(exits_tavern),
            area: Some(area_town_id),
        },
    );

    let room_town_class_row = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_town_class_row_id),
            name: Some(room_town_class_row_name),
            description: Some(room_town_class_row_desc),
            exits: Some(exits_class_row),
            area: Some(area_town_id),
        },
    );

    let room_graveyard = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_graveyard_id),
            name: Some(room_graveyard_name),
            description: Some(room_graveyard_desc),
            exits: Some(exits_graveyard),
            area: Some(area_town_id),
        },
    );

    let room_school_orientation = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_school_orientation_id),
            name: Some(room_school_orientation_name),
            description: Some(room_school_orientation_desc),
            exits: Some(exits_school_orientation),
            area: Some(area_school_id),
        },
    );

    let room_school_first_fight = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_school_first_fight_id),
            name: Some(room_school_first_fight_name),
            description: Some(room_school_first_fight_desc),
            exits: Some(exits_first_fight),
            area: Some(area_school_id),
        },
    );

    let room_meadow = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_meadow_id),
            name: Some(room_meadow_name),
            description: Some(room_meadow_desc),
            exits: Some(exits_meadow),
            area: Some(area_hunt_id),
        },
    );

    let room_orchard = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_orchard_id),
            name: Some(room_orchard_name),
            description: Some(room_orchard_desc),
            exits: Some(exits_orchard),
            area: Some(area_hunt_id),
        },
    );

    let room_sewers_from_town = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_from_town_id),
            name: Some(room_sewers_from_town_name),
            description: Some(room_sewers_from_town_desc),
            exits: Some(exits_sewers_from_town),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_from_meadow = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_from_meadow_id),
            name: Some(room_sewers_from_meadow_name),
            description: Some(room_sewers_from_meadow_desc),
            exits: Some(exits_sewers_from_meadow),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_from_orchard = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_from_orchard_id),
            name: Some(room_sewers_from_orchard_name),
            description: Some(room_sewers_from_orchard_desc),
            exits: Some(exits_sewers_from_orchard),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_junction = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_junction_id),
            name: Some(room_sewers_junction_name),
            description: Some(room_sewers_junction_desc),
            exits: Some(exits_sewers_junction),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_valves = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_valves_id),
            name: Some(room_sewers_valves_name),
            description: Some(room_sewers_valves_desc),
            exits: Some(exits_sewers_valves),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_valve_1 = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_valve_1_id),
            name: Some(room_sewers_valve_1_name),
            description: Some(room_sewers_valve_1_desc),
            exits: Some(exits_sewers_valve_1),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_valve_2 = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_valve_2_id),
            name: Some(room_sewers_valve_2_name),
            description: Some(room_sewers_valve_2_desc),
            exits: Some(exits_sewers_valve_2),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_valve_3 = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_valve_3_id),
            name: Some(room_sewers_valve_3_name),
            description: Some(room_sewers_valve_3_desc),
            exits: Some(exits_sewers_valve_3),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_sludge = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_sludge_id),
            name: Some(room_sewers_sludge_name),
            description: Some(room_sewers_sludge_desc),
            exits: Some(exits_sewers_sludge),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_drone_extract = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_drone_extract_id),
            name: Some(room_sewers_drone_extract_name),
            description: Some(room_sewers_drone_extract_desc),
            exits: Some(exits_sewers_drone_extract),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_safe_pocket = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_safe_pocket_id),
            name: Some(room_sewers_safe_pocket_name),
            description: Some(room_sewers_safe_pocket_desc),
            exits: Some(exits_sewers_safe_pocket),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_keycard_door = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_keycard_door_id),
            name: Some(room_sewers_keycard_door_name),
            description: Some(room_sewers_keycard_door_desc),
            exits: Some(exits_sewers_keycard_door),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_grease_approach = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_grease_approach_id),
            name: Some(room_sewers_grease_approach_name),
            description: Some(room_sewers_grease_approach_desc),
            exits: Some(exits_sewers_grease_approach),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_grease_arena = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_grease_arena_id),
            name: Some(room_sewers_grease_arena_name),
            description: Some(room_sewers_grease_arena_desc),
            exits: Some(exits_sewers_grease_arena),
            area: Some(area_sewers_id),
        },
    );

    let room_sewers_to_quarry = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_sewers_to_quarry_id),
            name: Some(room_sewers_to_quarry_name),
            description: Some(room_sewers_to_quarry_desc),
            exits: Some(exits_sewers_to_quarry),
            area: Some(area_sewers_id),
        },
    );

    let room_quarry_foothills = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_quarry_foothills_id),
            name: Some(room_quarry_foothills_name),
            description: Some(room_quarry_foothills_desc),
            exits: Some(exits_quarry_foothills),
            area: Some(area_quarry_id),
        },
    );

    let room_class_fighter = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_fighter_id),
            name: Some(room_class_fighter_name),
            description: Some(room_class_fighter_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    let room_class_rogue = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_rogue_id),
            name: Some(room_class_rogue_name),
            description: Some(room_class_rogue_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    let room_class_cleric = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_cleric_id),
            name: Some(room_class_cleric_name),
            description: Some(room_class_cleric_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    let room_class_wizard = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_wizard_id),
            name: Some(room_class_wizard_name),
            description: Some(room_class_wizard_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    let room_class_ranger = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_ranger_id),
            name: Some(room_class_ranger_name),
            description: Some(room_class_ranger_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    let room_class_paladin = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_paladin_id),
            name: Some(room_class_paladin_name),
            description: Some(room_class_paladin_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    let room_class_bard = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_bard_id),
            name: Some(room_class_bard_name),
            description: Some(room_class_bard_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    let room_class_druid = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_druid_id),
            name: Some(room_class_druid_name),
            description: Some(room_class_druid_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    let room_class_barbarian = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_barbarian_id),
            name: Some(room_class_barbarian_name),
            description: Some(room_class_barbarian_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    let room_class_warlock = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_warlock_id),
            name: Some(room_class_warlock_name),
            description: Some(room_class_warlock_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    let room_class_sorcerer = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_sorcerer_id),
            name: Some(room_class_sorcerer_name),
            description: Some(room_class_sorcerer_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    let room_class_monk = create_room(
        &mut fbb,
        &RoomArgs {
            id: Some(room_class_monk_id),
            name: Some(room_class_monk_name),
            description: Some(room_class_monk_desc),
            exits: Some(exits_class_hall),
            area: Some(area_class_halls_id),
        },
    );

    // ---------------- Area objects ----------------
    // Keep town first so Rooms::load picks town.gate as the start room.
    let rooms_town = fbb.create_vector(&[
        room_town_gate,
        room_portal_town_ns,
        room_town_class_row,
        room_tavern,
        room_graveyard,
    ]);
    let rooms_class_halls = fbb.create_vector(&[
        room_class_barbarian,
        room_class_bard,
        room_class_cleric,
        room_class_druid,
        room_class_fighter,
        room_class_monk,
        room_class_paladin,
        room_class_ranger,
        room_class_rogue,
        room_class_sorcerer,
        room_class_warlock,
        room_class_wizard,
    ]);
    let rooms_school = fbb.create_vector(&[room_school_orientation, room_school_first_fight]);
    let rooms_hunt = fbb.create_vector(&[room_meadow, room_orchard]);
    let rooms_sewers = fbb.create_vector(&[
        room_sewers_from_town,
        room_sewers_from_meadow,
        room_sewers_from_orchard,
        room_sewers_junction,
        room_sewers_valves,
        room_sewers_valve_1,
        room_sewers_valve_2,
        room_sewers_valve_3,
        room_sewers_sludge,
        room_sewers_drone_extract,
        room_sewers_safe_pocket,
        room_sewers_keycard_door,
        room_sewers_grease_approach,
        room_sewers_grease_arena,
        room_sewers_to_quarry,
    ]);
    let rooms_quarry = fbb.create_vector(&[room_quarry_foothills]);

    let area_town = create_area(
        &mut fbb,
        &AreaArgs {
            id: Some(area_town_id),
            name: Some(area_town_name),
            rooms: Some(rooms_town),
        },
    );
    let area_class_halls = create_area(
        &mut fbb,
        &AreaArgs {
            id: Some(area_class_halls_id),
            name: Some(area_class_halls_name),
            rooms: Some(rooms_class_halls),
        },
    );
    let area_school = create_area(
        &mut fbb,
        &AreaArgs {
            id: Some(area_school_id),
            name: Some(area_school_name),
            rooms: Some(rooms_school),
        },
    );
    let area_hunt = create_area(
        &mut fbb,
        &AreaArgs {
            id: Some(area_hunt_id),
            name: Some(area_hunt_name),
            rooms: Some(rooms_hunt),
        },
    );
    let area_sewers = create_area(
        &mut fbb,
        &AreaArgs {
            id: Some(area_sewers_id),
            name: Some(area_sewers_name),
            rooms: Some(rooms_sewers),
        },
    );
    let area_quarry = create_area(
        &mut fbb,
        &AreaArgs {
            id: Some(area_quarry_id),
            name: Some(area_quarry_name),
            rooms: Some(rooms_quarry),
        },
    );

    let areas = fbb.create_vector(&[
        area_town,
        area_class_halls,
        area_school,
        area_hunt,
        area_sewers,
        area_quarry,
    ]);
    let world = create_world(&mut fbb, &WorldArgs { areas: Some(areas) });
    fbb.finish(world, None);

    fbb.finished_data().to_vec()
}
