# Quest State Model (Draft)

This doc defines a lightweight, named-state quest system for Gaia, and connects quest progression to area design (rooms, blocked exits, spawn variants, and NPC dialogue).

The point: we want a small set of stable "graph points" (hubs + setpieces) that connect multiple quest lines, so the world layout stays coherent even as content expands.

## Conventions

### State Keys

- Namespace: `q.<quest_id>.<key>`
- Key types:
  - `enum`: one of a fixed set of strings
  - `bool`: `0/1`
  - `counter`: integer with a bounded range
- Storage: per-character, persisted.

### Gating Keys

Use explicit, boring gates to control access and keep secrets server-side:

- `gate.<area_id>` bool: e.g. `gate.A01` (always true), `gate.A12` (future).
- `gate.<zone_id>` bool: e.g. `gate.sewers.shortcut_to_quarry`.
- `rep.<faction>` counter: e.g. `rep.civic`, `rep.industrial`, `rep.green`.

### Design Rules

- Named quest states should correspond to real world locations or interactions.
  - If a state does not map to a place, it probably should not exist.
- Every quest has:
  - 1 hub entry point (usually in Town),
  - 1-2 "setpieces" (dungeon room clusters, elites, puzzles),
  - 1 unlock (gate, service, shortcut, or travel).
- Branches should re-converge unless we are intentionally creating permanent divergence.
- Group-first by default:
  - Story beats, bosses, and mechanical objectives are tuned for a party of 3-5 ("trio to quintet").
  - If fewer than 3 humans are in the party at the setpiece boundary, we auto-fill with Gaia assist bots until the party reaches 3 total.
  - If humans join mid-run, bots should step out cleanly (no loot claims, no permanent progression).

## Quest Graph Points (shared hubs)

These are "connective tissue" points that link multiple quest lines and will drive area layout.

| Hub ID | Zone (A01) | Location | Purpose | Used by |
| --- | --- | --- | --- | --- |
| HUB_NS_ORIENTATION | Newbie School | Orientation Wing | start, identity, verbs | Q1 |
| HUB_NS_LABS | Newbie School | Combat Labs | combat tutorial | Q1 |
| HUB_TOWN_GATE | Town: Gaia Gate | arrivals plaza | arrivals, starter kit | Q1, Q2 |
| HUB_TOWN_JOB_BOARD | Town: Gaia Gate | job board square | quest faucet + dailies | Q2, Q3, Q4, Q5 |
| HUB_TOWN_MAINT | Town: Gaia Gate | maintenance office | sewer access + valves quest | Q3 |
| HUB_SEWERS_JUNCTION | Under-Town Sewers | junction node | valve routing + shortcut | Q3, Q4 |
| HUB_QUARRY_FOREMAN | Quarry | foreman shack | quarry storyline | Q4 |
| HUB_CHECKPOINT_GATEHOUSE | Old Road Checkpoint | gatehouse | travel gate to midgame | Q4, Q5 |
| HUB_HILLFORT_COMMAND | Hillfort Ruins | command bunker | patrols + safe travel | Q6 |
| HUB_RAIL_TERMINAL | Rail Spur | terminal shed | travel + world events | Q6 |
| HUB_RUSTWOOD_RANGER | Rustwood | ranger camp | navigation + hunts | Q6, Q5 |
| HUB_LIB_ANTECHAMBER | Sunken Library | antechamber | index plates quest | Q5 |
| HUB_FACTORY_SWITCHROOM | Factory District | switchroom | sabotage storyline | Q7 |
| HUB_RES_PUMP_STATION | Reservoir | pump station | treaty + control storyline | Q8 |
| HUB_UNDERRAIL_PLATFORM | The Underrail | platform | deep travel hub | Q9 |
| HUB_SKYBRIDGE_ANCHORHOUSE | Skybridge | anchorhouse | traversal gate + anchors | Q10 |
| HUB_WASTES_OUTPOST | Glass Wastes | outpost | survival gate + routes | Q11 |
| HUB_GARDENS_GREENHOUSE | Crater Gardens | greenhouse | bio-contract storyline | Q12 |
| HUB_CORE_GATE | The Core | gate ring | raid-lite entry hub | Q13 |
| HUB_SEAM_EDGE | The Seam | edge station | endgame hub | Q14 |

## Q1: First Day on Gaia (Levels 1-2)

Primary goal: onboarding. Teaches verbs, combat loop, recovery, and gets you to Town.

Target party: solo (bots disabled here).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q1_first_day.state` | enum | `unstarted`, `orientation`, `badge`, `verbs`, `combat`, `town_pass`, `complete` | main progression |
| `q.q1_first_day.kit_choice` | enum | `unpicked`, `human`, `robot` | starter kit theme |

State descriptions (graph nodes):

| State | Where | Set by | Unlock |
| --- | --- | --- | --- |
| `orientation` | HUB_NS_ORIENTATION | talk to instructor | basic commands unlocked |
| `badge` | HUB_NS_ORIENTATION | issue badge | `gate.town_entry=1` (soft) |
| `verbs` | HUB_NS_ORIENTATION | 3 verb drills | none (confidence) |
| `combat` | HUB_NS_LABS | clear lab scenario | access to Town gate escort |
| `town_pass` | HUB_TOWN_GATE | pick kit + accept rules | `gate.town_services=1` |
| `complete` | HUB_TOWN_GATE | finish | Q2 offered |

## Q2: The Job Board Never Sleeps (Levels 1-4)

Primary goal: keep the world moving. Gives repeatables, introduces factions, and points at Sewers + Quarry.

Target party: 1-3 (solo-friendly; bots optional).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q2_job_board.state` | enum | `unstarted`, `contract_1`, `contract_2`, `contract_3`, `choice`, `repeatables`, `complete` | main progression |
| `q.q2_job_board.contracts_done` | counter | `0..3` | how many small contracts completed |
| `q.q2_job_board.faction` | enum | `unset`, `civic`, `industrial`, `green` | primary contact |
| `q.q2_job_board.repeatables_unlocked` | bool | `0/1` | dailies/bounties enabled |

Branching note:

- Faction choice changes flavor rewards and which NPC calls you first.
- All main content should remain accessible eventually (no permanent lockout).

Unlocks:

- `gate.job_board.repeatables=1`
- `gate.sewers.entry=1` (via civic/industrial)
- `gate.quarry.entry=1` (via industrial/green)

## Q3: Sewer Valves (Levels 3-4)

Primary goal: first dungeon loop with a mechanical objective (valves), then a boss.

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q3_sewer_valves.state` | enum | `unstarted`, `valves`, `boss`, `resolved`, `complete` | main progression |
| `q.q3_sewer_valves.valves_opened` | counter | `0..3` | valves toggled |
| `q.q3_sewer_valves.drone_rescued` | bool | `0/1` | optional rescue |
| `gate.sewers.shortcut_to_quarry` | bool | `0/1` | permanent shortcut |

Unlocks:

- `gate.sewers.shortcut_to_quarry=1`
- Sewer vendor unlock (parts, antidotes)

## Q4: Quarry Rights (Levels 4-6)

Primary goal: introduce heavier hits, interrupts, and a meaningful choice (rep).

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q4_quarry_rights.state` | enum | `unstarted`, `clear_dig`, `choice`, `elite`, `complete` | main progression |
| `q.q4_quarry_rights.side` | enum | `unset`, `union`, `foreman` | rep alignment |
| `gate.checkpoint.access` | bool | `0/1` | allows midgame travel |

Unlocks:

- `gate.checkpoint.access=1`
- `rep.industrial` or `rep.civic` bump depending on choice

## Q5: The Sunken Index (Levels 6-8)

Primary goal: teach multi-zone navigation and unlock your first big midgame branch.

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q5_sunken_index.state` | enum | `unstarted`, `plates`, `decode`, `choose_unlock`, `complete` | main progression |
| `q.q5_sunken_index.plates` | counter | `0..4` | plates collected |
| `q.q5_sunken_index.first_unlock` | enum | `unset`, `factory`, `reservoir` | which district opens first |
| `gate.factory.entry` | bool | `0/1` | factory access |
| `gate.reservoir.entry` | bool | `0/1` | reservoir access |

Unlocks:

- `gate.factory.entry=1` OR `gate.reservoir.entry=1` (first choice)
- later: both become true (catch-up)

## Q6: Hillfort Signal (Levels 5-7)

Primary goal: introduce patrols, limited safe travel, and the first "world event" style loop (Rail Spur).

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q6_hillfort_signal.state` | enum | `unstarted`, `enter`, `pylons`, `boss`, `resolved`, `complete` | main progression |
| `q.q6_hillfort_signal.pylons_lit` | counter | `0..3` | how many ward pylons re-lit |
| `gate.hillfort.safehouse` | bool | `0/1` | enables a safe rest node in Hillfort |
| `gate.rail_spur.pass` | bool | `0/1` | enables non-hostile rail service events |

State descriptions (graph nodes):

| State | Where | Set by | Unlock |
| --- | --- | --- | --- |
| `enter` | HUB_HILLFORT_COMMAND | accept patrol briefing | Hillfort opens as a loop |
| `pylons` | Hillfort Ruins | light 3 pylons | adds "safe path" signage |
| `boss` | Hillfort Ruins | enter banner hall | boss encounter |
| `resolved` | HUB_HILLFORT_COMMAND | report in | `gate.hillfort.safehouse=1` |
| `complete` | HUB_RAIL_TERMINAL | receive rail pass | `gate.rail_spur.pass=1` |

## Q7: Conveyor War (Factory District) (Levels 7-10)

Primary goal: teach tight-room tactics (line-of-sight, interrupts) and unlock The Underrail.

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q7_conveyor_war.state` | enum | `unstarted`, `entry`, `shutdown`, `boss`, `complete` | main progression |
| `q.q7_conveyor_war.lines_disabled` | counter | `0..3` | how many lines shut down |
| `gate.underrail.entry` | bool | `0/1` | allows access to The Underrail |
| `gate.factory.shortcut_to_reservoir` | bool | `0/1` | factory-to-reservoir shortcut |

Unlocks:

- `gate.underrail.entry=1` (via freight elevator)
- `gate.factory.shortcut_to_reservoir=1` (late in quest)

State descriptions (graph nodes):

| State | Where | Set by | Unlock |
| --- | --- | --- | --- |
| `entry` | HUB_FACTORY_SWITCHROOM | accept sabotage order | factory loop enabled |
| `shutdown` | Factory District | disable 3 conveyor lines | boss arena access |
| `boss` | Factory District | enter foreman arena | none |
| `complete` | HUB_FACTORY_SWITCHROOM | report in | `gate.underrail.entry=1`, `gate.factory.shortcut_to_reservoir=1` |

## Q8: Flow Treaty (Reservoir) (Levels 8-11)

Primary goal: introduce resource-control mechanics and unlock The Underrail (alternate entry).

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q8_flow_treaty.state` | enum | `unstarted`, `entry`, `controls`, `boss`, `complete` | main progression |
| `q.q8_flow_treaty.controls_restored` | counter | `0..3` | pump/valve control nodes restored |
| `gate.underrail.entry` | bool | `0/1` | allows access to The Underrail |
| `gate.reservoir.shortcut_to_factory` | bool | `0/1` | reservoir-to-factory shortcut |

Unlocks:

- `gate.underrail.entry=1` (via pump tunnel)
- `gate.reservoir.shortcut_to_factory=1` (late in quest)

State descriptions (graph nodes):

| State | Where | Set by | Unlock |
| --- | --- | --- | --- |
| `entry` | HUB_RES_PUMP_STATION | accept control-room request | reservoir loop enabled |
| `controls` | Reservoir | restore 3 control nodes | boss chamber access |
| `boss` | Reservoir | enter regulator chamber | none |
| `complete` | HUB_RES_PUMP_STATION | report in | `gate.underrail.entry=1`, `gate.reservoir.shortcut_to_factory=1` |

## Q9: Beacon Protocol (The Underrail) (Levels 9-12)

Primary goal: deep traversal dungeon; unlock Skybridge travel.

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q9_beacon_protocol.state` | enum | `unstarted`, `platform`, `beacons`, `boss`, `complete` | main progression |
| `q.q9_beacon_protocol.beacons_lit` | counter | `0..4` | beacons restored |
| `gate.skybridge.entry` | bool | `0/1` | allows access to Skybridge |

Unlocks:

- `gate.skybridge.entry=1`

State descriptions (graph nodes):

| State | Where | Set by | Unlock |
| --- | --- | --- | --- |
| `platform` | HUB_UNDERRAIL_PLATFORM | accept beacon order | underrail loop enabled |
| `beacons` | The Underrail | light 4 beacons | boss arena access |
| `boss` | The Underrail | enter warden arena | none |
| `complete` | HUB_UNDERRAIL_PLATFORM | report in | `gate.skybridge.entry=1` |

## Q10: Anchor the Wind (Skybridge) (Levels 11-13)

Primary goal: vertical traversal pressure; unlock the Glass Wastes.

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q10_anchor_wind.state` | enum | `unstarted`, `anchors`, `rescue`, `boss`, `complete` | main progression |
| `q.q10_anchor_wind.anchors_calibrated` | counter | `0..3` | anchors fixed |
| `gate.glass_wastes.entry` | bool | `0/1` | allows access to Glass Wastes |

Unlocks:

- `gate.glass_wastes.entry=1`

State descriptions (graph nodes):

| State | Where | Set by | Unlock |
| --- | --- | --- | --- |
| `anchors` | HUB_SKYBRIDGE_ANCHORHOUSE | accept calibration job | anchor routes enabled |
| `rescue` | Skybridge | complete rescue objective | boss arena access |
| `boss` | Skybridge | enter span setpiece | none |
| `complete` | HUB_SKYBRIDGE_ANCHORHOUSE | report in | `gate.glass_wastes.entry=1` |

## Q11: Stormglass Route (Glass Wastes) (Levels 12-15)

Primary goal: long loops, navigation, and a big storm setpiece; unlock Crater Gardens.

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q11_stormglass_route.state` | enum | `unstarted`, `outpost`, `waypoints`, `storm`, `complete` | main progression |
| `q.q11_stormglass_route.waypoints_marked` | counter | `0..3` | route waypoints marked |
| `gate.crater_gardens.entry` | bool | `0/1` | allows access to Crater Gardens |

Unlocks:

- `gate.crater_gardens.entry=1`

State descriptions (graph nodes):

| State | Where | Set by | Unlock |
| --- | --- | --- | --- |
| `outpost` | HUB_WASTES_OUTPOST | accept route contract | outpost services |
| `waypoints` | Glass Wastes | mark 3 waypoints | storm event enabled |
| `storm` | Glass Wastes | survive the storm eye | none |
| `complete` | HUB_WASTES_OUTPOST | report in | `gate.crater_gardens.entry=1` |

## Q12: Bloom Contract (Crater Gardens) (Levels 13-16)

Primary goal: biome-flavored combat and a meaningful faction choice; unlock The Core.

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q12_bloom_contract.state` | enum | `unstarted`, `samples`, `choice`, `boss`, `complete` | main progression |
| `q.q12_bloom_contract.samples` | counter | `0..3` | bloom samples recovered |
| `q.q12_bloom_contract.side` | enum | `unset`, `green`, `industrial` | contract alignment |
| `gate.core.entry` | bool | `0/1` | allows access to The Core |

Unlocks:

- `gate.core.entry=1`

State descriptions (graph nodes):

| State | Where | Set by | Unlock |
| --- | --- | --- | --- |
| `samples` | HUB_GARDENS_GREENHOUSE | accept bio-contract | sample routes enabled |
| `choice` | HUB_GARDENS_GREENHOUSE | pick a side | sets `q.q12_bloom_contract.side` |
| `boss` | Crater Gardens | enter root engine arena | none |
| `complete` | HUB_GARDENS_GREENHOUSE | report in | `gate.core.entry=1` |

## Q13: Heart of the Machine (The Core) (Levels 15-17)

Primary goal: raid-lite multi-stage boss; unlock The Seam.

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q13_heart_machine.state` | enum | `unstarted`, `keypieces`, `gate`, `boss`, `complete` | main progression |
| `q.q13_heart_machine.keypieces` | counter | `0..3` | keypieces assembled |
| `gate.seam.entry` | bool | `0/1` | allows access to The Seam |

Unlocks:

- `gate.seam.entry=1`

State descriptions (graph nodes):

| State | Where | Set by | Unlock |
| --- | --- | --- | --- |
| `keypieces` | HUB_CORE_GATE | accept core entry protocol | keypiece hunt begins |
| `gate` | HUB_CORE_GATE | insert 3 keypieces | boss arena access |
| `boss` | The Core | enter auditor arena | none |
| `complete` | HUB_CORE_GATE | report in | `gate.seam.entry=1` |

## Q14: Stabilize the Boundary (The Seam) (Levels 17-20)

Primary goal: endgame loop; sets up future seam expansions (A41+ placeholders).

Target party: 3-5 (auto-fill bots to 3).

State keys:

| Key | Type | Values | Meaning |
| --- | --- | --- | --- |
| `q.q14_stabilize_boundary.state` | enum | `unstarted`, `rifts`, `choice`, `final`, `complete` | main progression |
| `q.q14_stabilize_boundary.rifts` | counter | `0..4` | seam rifts stabilized |
| `q.q14_stabilize_boundary.reinforce` | enum | `unset`, `red_shift`, `blue_noise`, `glass_choir` | which seam region is reinforced |
| `gate.A41` | bool | `0/1` | future seam area gate (placeholder) |
| `gate.A42` | bool | `0/1` | future seam area gate (placeholder) |
| `gate.A43` | bool | `0/1` | future seam area gate (placeholder) |

State descriptions (graph nodes):

| State | Where | Set by | Unlock |
| --- | --- | --- | --- |
| `rifts` | HUB_SEAM_EDGE | accept stabilization work | rift loop enabled |
| `choice` | HUB_SEAM_EDGE | choose reinforcement target | sets `q.q14_stabilize_boundary.reinforce` |
| `final` | The Seam | enter stabilizer nexus | none |
| `complete` | HUB_SEAM_EDGE | report in | future seam gates can toggle (A41+) |

## Cross-Quest Links (the "world spine")

These are the edges that stitch the quest graphs into the area graph:

- `q.q1_first_day.state=complete` enables Q2 at `HUB_TOWN_JOB_BOARD`.
- Q2 enables Q3/Q4 entry gates.
- `q.q3_sewer_valves.state=complete` enables `gate.sewers.shortcut_to_quarry`.
- `q.q4_quarry_rights.state=complete` enables `gate.checkpoint.access`.
- `q.q5_sunken_index.state=complete` enables midgame branch entry (`gate.factory.entry` and/or `gate.reservoir.entry`).
- Q6 is optional but recommended after Q4 (`gate.rail_spur.pass` is quality-of-life).
- Q7/Q8 should converge on `gate.underrail.entry=1`.
- `q.q9_beacon_protocol.state=complete` enables `gate.skybridge.entry=1`.
- `q.q10_anchor_wind.state=complete` enables `gate.glass_wastes.entry=1`.
- `q.q11_stormglass_route.state=complete` enables `gate.crater_gardens.entry=1`.
- `q.q12_bloom_contract.state=complete` enables `gate.core.entry=1`.
- `q.q13_heart_machine.state=complete` enables `gate.seam.entry=1`.

## Areas As Quest Setpieces

When we build area files, we should explicitly tag room clusters as setpieces:

- `setpiece.q3.valve_room_1..3`
- `setpiece.q3.grease_king_arena`
- `setpiece.q4.illegal_dig_site`
- `setpiece.q4.breaker7_arena`
- `setpiece.q5.plate_shrine_1..4`
- `setpiece.q5.decoder_chamber`
- `setpiece.q6.pylon_1..3`
- `setpiece.q6.banner_hall`
- `setpiece.q7.line_shutdown_1..3`
- `setpiece.q7.foreman_clank_arena`
- `setpiece.q8.control_node_1..3`
- `setpiece.q8.flow_regulator_chamber`
- `setpiece.q9.beacon_1..4`
- `setpiece.q9.platform_warden_arena`
- `setpiece.q10.anchor_1..3`
- `setpiece.q10.wind_marshal_span`
- `setpiece.q11.storm_eye_event`
- `setpiece.q12.bloom_grove_1..3`
- `setpiece.q12.root_engine_arena`
- `setpiece.q13.lock_1_precision`
- `setpiece.q13.lock_2_geometry`
- `setpiece.q13.lock_3_sustain`
- `setpiece.q13.core_auditor_arena`
- `setpiece.q14.rift_1..4`
- `setpiece.q14.stabilizer_nexus`

These tags will later help with:

- spawn variants (pre/post completion),
- fast travel / breadcrumbs,
- debugging ("why is player stuck?").
