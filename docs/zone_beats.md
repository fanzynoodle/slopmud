# Gaia Zone Beats (A01) (Draft)

This doc breaks the A01 launch world into zone-sized "area files" and smaller room clusters, so we can draft actual area/room data without inventing the layout from scratch each time.

Related docs:

- World size and room budgets: `docs/room_scale_plan.md`
- High-level area descriptions: `docs/area_summary.md`
- Quest named states and shared hubs: `docs/quest_state_model.md`
- Quest graph: `docs/quest_state_graph.mermaid`
- Room-by-room protoadventures (playtest runs): `protoadventures/README.md`
- Party/bots policy: `docs/party_and_bots.md`
- Overworld portal coords + exit lengths: `docs/overworld_cartesian_layout.md`

## Conventions

- Zone: a ship-together chunk of rooms (a future "area file"). Zones map 1:1 with the A01 budget table.
- Cluster: a sub-area inside a zone that we can build/test in isolation.
- Hub: a specific room cluster that is referenced by quest graphs. Hubs should be stable (we can move details around them later).
- Group-first: any cluster that contains a `setpiece.*` should be assumed to be tuned for parties of 3-5.
  - If fewer than 3 humans are present, we auto-fill with bots to reach a trio.
  - Details are in `docs/party_and_bots.md`.

ID conventions (draft):

- Cluster IDs: `CL_<ZONE>_<NAME>` (stable; referenced by tests/tools later)
- Hub IDs: `HUB_*` as defined in `docs/quest_state_model.md`

Room counts are targets, not promises.

## A01 Zones And Clusters

### Newbie School (L1-L2, 50 rooms)

Primary quests: Q1.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_NS_ORIENTATION | 12 | HUB_NS_ORIENTATION; naming, verbs, badge issuance |
| CL_NS_DORMS | 10 | social/tutorial micro-quests |
| CL_NS_LABS | 16 | HUB_NS_LABS; scripted combat scenarios |
| CL_NS_SIM_YARD | 12 | small group drill; aggro lessons |

### Town: Gaia Gate (L1-L4, 120 rooms)

Primary quests: Q1, Q2, Q3 (start), Q4 (start), Q5 (handoff).

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_TOWN_GATE_PLAZA | 20 | HUB_TOWN_GATE; arrivals, starter kit choice |
| CL_TOWN_JOB_BOARD | 10 | HUB_TOWN_JOB_BOARD; quest faucet and repeatables |
| CL_TOWN_MARKET_ROW | 20 | vendors, repairs, gold sinks |
| CL_TOWN_BANK_CLINIC | 20 | bank/locker, status cures, revive |
| CL_TOWN_MAINT_OFFICE | 10 | HUB_TOWN_MAINT; sewer access |
| CL_TOWN_ALLEYS | 20 | secret fence/backrooms, shortcuts |
| CL_TOWN_EDGE | 20 | outward gates to Meadowline/Orchard/Old Road; placeholder blocked exits (A02+) live here |

### Meadowline (L2-L3, 90 rooms)

Primary quests: Q2 contracts (some).

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_MEADOW_TRAILS | 30 | clear geography; breadcrumb signage back to Town |
| CL_MEADOW_PONDS | 20 | hazards, low-stakes resource nodes |
| CL_MEADOW_PEST_FIELDS | 30 | main grind loop (fast respawn) |
| CL_MEADOW_SEWER_GRATE | 10 | soft gate to Sewers (alternate entry) |

### Scrap Orchard (L2-L3, 90 rooms)

Primary quests: Q2 contracts (some).

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_ORCHARD_GROVE | 30 | salvage groves; basic drops |
| CL_ORCHARD_ROOTWORKS | 20 | tight paths; ambush lessons |
| CL_ORCHARD_DRONE_NESTS | 30 | ranged packs; scrap-mod drops |
| CL_ORCHARD_PIPEWAY | 10 | soft gate to Sewers (alternate entry) |

### Under-Town Sewers (L3-L4, 110 rooms)

Primary quests: Q3, Q4 (via shortcut), Q2 repeatables.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_SEWERS_ENTRY | 15 | entry tunnels from Town |
| CL_SEWERS_JUNCTION | 20 | HUB_SEWERS_JUNCTION; routing node |
| CL_SEWERS_VALVE_RUN | 35 | Q3 valves (`setpiece.q3.valve_room_1..3`) |
| CL_SEWERS_SLUDGE_LOOPS | 20 | grind loop + status hazards |
| CL_SEWERS_GREASE_KING | 20 | boss arena (`setpiece.q3.grease_king_arena`) |

### Quarry (L4-L6, 70 rooms)

Primary quests: Q4.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_QUARRY_FOOTHILLS | 15 | entrances from Sewers and Old Road |
| CL_QUARRY_WORKS | 15 | HUB_QUARRY_FOREMAN; staging and vendors |
| CL_QUARRY_PITS | 20 | grind loop; heavier hits |
| CL_QUARRY_ILLEGAL_DIG | 20 | setpiece for Q4 (`setpiece.q4.illegal_dig_site`) |

### Old Road Checkpoint (L4-L6, 30 rooms)

Primary quests: Q4; main travel gate to midgame.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_CHECKPOINT_GATEHOUSE | 15 | HUB_CHECKPOINT_GATEHOUSE |
| CL_CHECKPOINT_ROAD | 10 | patrols/tolls; placeholder blocked exits (A11+) can live here |
| CL_CHECKPOINT_AMBUSH_SPUR | 5 | one small spike setpiece for travel tutoring |

### Hillfort Ruins (L4-L7, 70 rooms)

Primary quests: Q6 (optional but recommended).

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_HILLFORT_APPROACH | 20 | approach paths; mixed packs |
| CL_HILLFORT_COMMAND_BUNKER | 10 | HUB_HILLFORT_COMMAND; rest/safehouse unlock |
| CL_HILLFORT_COURTYARDS | 20 | pylon routes (`setpiece.q6.pylon_1..3`) |
| CL_HILLFORT_BANNER_HALL | 20 | boss (`setpiece.q6.banner_hall`) |

### Rail Spur (L5-L7, 50 rooms)

Primary quests: Q6 (handoff); world events live here.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_RAIL_TERMINAL | 15 | HUB_RAIL_TERMINAL; travel + event dispatcher |
| CL_RAIL_YARDS | 15 | combat-with-geometry; moving hazards |
| CL_RAIL_LINE | 20 | event nodes (ambush, runaway cargo bot) |

### Rustwood (L5-L8, 120 rooms)

Primary quests: Q5 (pathing into Library); side hunts.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_RUSTWOOD_RANGER_CAMP | 10 | HUB_RUSTWOOD_RANGER; navigation hints |
| CL_RUSTWOOD_TRAILS | 40 | long loops; stealth predators |
| CL_RUSTWOOD_PYLONS | 30 | sightline puzzles; mixed packs |
| CL_RUSTWOOD_GROVES | 40 | grind loop; rare spawns |

### Sunken Library (L6-L9, 110 rooms)

Primary quests: Q5.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_LIB_ANTECHAMBER | 10 | HUB_LIB_ANTECHAMBER; quest intake |
| CL_LIB_STACKS | 40 | plate shrines (`setpiece.q5.plate_shrine_1..4`) |
| CL_LIB_FLOODED_WINGS | 30 | navigation + debuffs |
| CL_LIB_DECODER_CHAMBER | 30 | decoder setpiece (`setpiece.q5.decoder_chamber`) |

### Factory District (Outskirts) (L7-L10, 120 rooms)

Primary quests: Q7.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_FACTORY_GATE | 20 | entry streets; line-of-sight tutoring |
| CL_FACTORY_SWITCHROOM | 10 | HUB_FACTORY_SWITCHROOM |
| CL_FACTORY_CONVEYORS | 40 | shutdown setpieces (`setpiece.q7.line_shutdown_1..3`) |
| CL_FACTORY_FOUNDRY | 30 | hazards + interrupts |
| CL_FACTORY_FOREMAN_ARENA | 20 | boss (`setpiece.q7.foreman_clank_arena`) |

### The Reservoir (L8-L11, 110 rooms)

Primary quests: Q8.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_RES_PUMP_STATION | 10 | HUB_RES_PUMP_STATION |
| CL_RES_SHORES | 30 | wide open pulls; ranged threats |
| CL_RES_TUNNELS | 30 | traversal + ambush |
| CL_RES_CONTROL_NODES | 20 | controls (`setpiece.q8.control_node_1..3`) |
| CL_RES_FLOW_REGULATOR | 20 | boss (`setpiece.q8.flow_regulator_chamber`) |

### The Underrail (L9-L12, 120 rooms)

Primary quests: Q9.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_UNDERRAIL_PLATFORM | 15 | HUB_UNDERRAIL_PLATFORM |
| CL_UNDERRAIL_TUNNELS | 45 | beacon runs (`setpiece.q9.beacon_1..4`) |
| CL_UNDERRAIL_SWITCHYARD | 30 | long loops; patrol timing |
| CL_UNDERRAIL_WARDEN_ARENA | 30 | boss (`setpiece.q9.platform_warden_arena`) |

### Skybridge (L11-L13, 100 rooms)

Primary quests: Q10.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_SKY_ANCHORHOUSE | 10 | HUB_SKYBRIDGE_ANCHORHOUSE |
| CL_SKY_SPANS | 50 | traversal pressure; knockback |
| CL_SKY_ANCHORS | 20 | anchor repairs (`setpiece.q10.anchor_1..3`) |
| CL_SKY_WIND_MARSHAL | 20 | boss (`setpiece.q10.wind_marshal_span`) |

### Glass Wastes (L12-L15, 160 rooms)

Primary quests: Q11.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_WASTES_OUTPOST | 10 | HUB_WASTES_OUTPOST |
| CL_WASTES_DUNES | 60 | long loops; attrition |
| CL_WASTES_GLASSFIELDS | 50 | navigation; ranged pressure |
| CL_WASTES_STORM_EYE | 40 | storm setpiece (`setpiece.q11.storm_eye_event`) |

### Crater Gardens (L13-L16, 130 rooms)

Primary quests: Q12.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_GARDENS_GREENHOUSE | 10 | HUB_GARDENS_GREENHOUSE |
| CL_GARDENS_WILDS | 50 | mixed packs; terrain funnels |
| CL_GARDENS_BLOOM_GROVES | 40 | sample runs (`setpiece.q12.bloom_grove_1..3`) |
| CL_GARDENS_ROOT_ENGINE | 30 | boss (`setpiece.q12.root_engine_arena`) |

### The Core (L15-L17, 150 rooms)

Primary quests: Q13.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_CORE_GATE_RING | 15 | HUB_CORE_GATE; gate + staging |
| CL_CORE_APPROACH | 35 | ramp-up; add waves |
| CL_CORE_INNERWORKS | 50 | puzzle-ish traversal; hazards |
| CL_CORE_AUDITOR_ARENA | 50 | boss (`setpiece.q13.core_auditor_arena`) |

### The Seam (L17-L20, 200 rooms)

Primary quests: Q14.

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_SEAM_EDGE_STATION | 15 | HUB_SEAM_EDGE |
| CL_SEAM_RIFTS | 80 | rift runs (`setpiece.q14.rift_1..4`) |
| CL_SEAM_MAZE | 60 | endgame loops; reality glitches |
| CL_SEAM_STABILIZER_NEXUS | 45 | finale (`setpiece.q14.stabilizer_nexus`) |

### Arena (L?, 2 rooms)

Primary quests: none (sandbox).

| Cluster ID | Rooms | Notes |
| --- | ---:| --- |
| CL_ARENA_PIT | 2 | isolated fight pit; no overworld portals yet |
