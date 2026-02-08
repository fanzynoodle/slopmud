---
adventure_id: side-alley-fence
area_id: A01
zone: Town Alleys (Black Market)
clusters:
  - CL_TOWN_ALLEYS
hubs: []
setpieces: []
level_band: [2, 5]
party_size: [1, 5]
expected_runtime_min: 30
---

# Side Quest: Alley Fence (Town Alleys) (L2-L5, Party 1-5)

## Hook

You keep finding odd loot that regular vendors won't buy. The town has an answer: a fence in the alleys, reachable only if you follow the right symbols and don't start trouble.

This quest is both a discovery run (learn the alley symbol language) and a morality test (profit vs harm). Finish it to unlock an odd-loot buy/sell vendor and (optionally) better pricing tiers.

Inputs (design assumptions):

- Symbol language must be consistent so discovery feels fair (no random guessing).
- Provide a safe refusal branch for shady favors and at least one clean-favor chain to earn trusted status without moral compromise.
- Add a simple, loud "alley heat" meter so loud choices have clear consequences.
- Rooftop routes need visible reconnection points so players do not feel lost.
- Trusted status should have explicit requirements (favor count + heat cap) and explicit pricing tier feedback.
- Vendor unlock should be explicit (what changed, where to find it, what it buys/sells).

## Quest Line

Success conditions:

- find the fence
- complete one favor without escalating the whole block
- earn vendor access (base) or trusted pricing (optional)

Optional outcomes:

- "trusted" status for better prices and a shortcut entry
- a nonlethal path that reduces alley heat

## Room Flow

This should be mostly town traversal and social/stealth beats, not a dungeon crawl.

### R_TOWN_ALLEY_01 (entry)

- beat: rumor points you to a symbol: three slashes inside a circle.
- teach: symbol language is learnable; you're not supposed to guess randomly
- exits: north -> `R_TOWN_ALLEY_02`

### R_TOWN_ALLEY_02 (checkpoint)

- beat: gang checkpoint. They demand toll or trouble.
- teach: nonlethal and social are valid solutions; "heat" exists
- outcomes:
  - talk through (bard/paladin/cleric)
  - bypass (rogue/monk rooftop or lockpick)
  - controlled violence (fighter/barbarian) with heat increase
- exits: east -> `R_TOWN_SERVICE_DOOR_01`, up -> `R_TOWN_ROOFTOPS_01`, back -> `R_TOWN_ALLEY_01`

### R_TOWN_SERVICE_DOOR_01 (bypass)

- beat: a locked service door that bypasses the checkpoint crowd.
- teach: stealth routes have clear payoffs
- exits: east -> `R_TOWN_ALLEY_03`, back -> `R_TOWN_ALLEY_02`

### R_TOWN_ROOFTOPS_01 (rooftop route)

- beat: ladders and gaps; clear reconnection points back to alley level.
- teach: movement mastery route for monks/rogues
- exits: drop -> `R_TOWN_ALLEY_03`, back -> `R_TOWN_ALLEY_02`

### R_TOWN_ALLEY_03 (warded backroom door)

- beat: a door with a warded knocker. Wrong touch triggers an alert.
- teach: traps are about tells and choices
- solutions:
  - wizard disarms/dispel
  - rogue finds the safe contact points
  - cleric/paladin can refuse and request a clean favor alternative
- exits: east -> `R_TOWN_FENCE_BACKROOM_01`, back -> `R_TOWN_ALLEY_02`

### R_TOWN_FENCE_BACKROOM_01 (the Fence)

- beat: meet the Fence. A single question frames the morality test.
- teach: you can draw a line and still progress
- quest: grant base vendor access
- exits: back -> `R_TOWN_ALLEY_03`, end -> `R_TOWN_FENCE_REWARD_01`

### Favor Setpieces (pick 1)

Favors should be modular and class-friendly:

- Delivery favor: carry a sealed pouch; do not open; solve via route/stealth.
- Courier tail: follow a runner and mark a chalk sign to prove you tracked them.
- Debt ledger: destroy a ledger without hurting anyone (nonlethal path).

Each favor has:

- a clean path (low heat, slower)
- a dirty path (faster, higher heat)

### R_TOWN_FENCE_REWARD_01

- beat: fence grants access and optionally "trusted" status.
- rewards:
  - unlock odd-loot vendor
  - trusted tier unlock (better prices, attic door shortcut)
- exits: back -> `R_TOWN_ALLEY_01`

## NPCs

- The Fence: calm, practical, and observant; cares about heat and reliability.
- Alley Shakedown Lead: tests if you escalate.
- Victim NPC (optional): gives the cleric/paladin moment and sets "no harm" branch.

## Rewards

- Base: odd-loot buy/sell unlocked
- Optional: trusted pricing tier + shortcut entry (attic door)

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/side-alley-fence/`):

- Symbol language must be consistent so discovery feels fair.
- Provide a safe refusal branch for shady favors.
- Add a simple "alley heat" meter to make loud choices have clear consequences.
- Rooftop routes need visible reconnection points, or players feel lost.
- Trusted status should have explicit requirements (favor count + heat cap) and explicit pricing tier feedback.
- Provide at least one clean-favor chain that can earn trusted status without moral compromise.
