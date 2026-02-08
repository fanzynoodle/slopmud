# Protoadventures

Protoadventures are linear, playtestable quest runs (party size 1-5) written as room-by-room beats.

They are intentionally *not* an area file format yet. The goal is to lock in:

- what the player does (quest steps),
- what rooms need to exist (room graph and setpieces),
- what gets taught (mechanics lessons),
- what the engine must eventually support (gates, triggers, spawns).

Later, other agents can translate these into zone/area files once we pick a real content format.

## Constraints

- Do not copy Wizards of the Coast text.
- If we use CC BY 4.0 material (e.g. SRD 5.2.1), keep attribution with the content source and add it to the SRD tabulation ledger.
- Prefer original names, descriptions, and mechanics text.

## Workflow (Short)

- Pick an `adventure_id` in `docs/adventures_todo.md`.
- Create one or more run files at `protoadventures/party_runs/<adventure_id>/run-XX.md` (that is the claim).
- After 3 runs exist, start (or update) `protoadventures/<adventure_id>.md` to distill the runs into a linear room flow.

## Playtesting (Local)

You can load a protoadventure directly into the dev shard as dynamic rooms.

1. Start a local session broker + shard: `just dev-run`
2. Connect (example): `telnet 127.0.0.1 4000`
3. In game:
   - `proto list`
   - `proto <adventure_id>`
   - `proto exit`

This reads `protoadventures/<adventure_id>.md`, instantiates it under a `proto.<adventure_id>.*` room prefix, and teleports you to the start of the `## Room Flow`.

Loader notes (current implementation):

- Each room should be introduced with a `### R_*` heading.
- Exits should be declared with `- exits:` (single-line or multi-line bullets like `- north -> \`R_FOO\``).

To validate structure (dangling exits, missing room headings):

- `python3 scripts/protoadventure_lint.py`

## SRD 5.2.1 (CC BY 4.0) Notes

We keep the SRD 5.2.1 source material and license text in:

- `reference/gaming-systems/dnd5e-srd-5.2.1-cc/`

If you derive anything from SRD 5.2.1 for a protoadventure (monster ability text, item text, rules blurbs, etc):

- add a row to `reference/gaming-systems/dnd5e-srd-5.2.1-cc/TABULATION.md`
- include whatever attribution is required for the derived work (see the SRD's legal section)

Practical rule of thumb:

- Tabulate if you quoted or closely paraphrased SRD phrasing, or copied SRD numbers/tables.
- Do not tabulate generic game concepts you did not copy (HP/AC/advantage, etc).

## File Format (Draft)

Each protoadventure is a single Markdown file with YAML front matter plus structured sections.

## When To Start A Protoadventure

- After 3 party runs exist for an `adventure_id`, start (or update) `protoadventures/<adventure_id>.md`.
- Treat party runs as your source-of-truth for what broke and what was fun; the protoadventure is the "playable linear run" derived from them.

## Definition Of Done (Protoadventure)

Minimum bar:

- YAML front matter filled (`adventure_id`, `zone`, `clusters`, `hubs`, `setpieces`, `level_band`, `party_size`, `expected_runtime_min`)
- `Hook` is 1 paragraph max
- `Inputs (design assumptions)` is 5+ bullets distilled from party runs (things that must be true for the run to feel fair/readable)
- `Quest Line` lists explicit success conditions and target quest keys (if known)
- `Room Flow` has a mostly-linear chain with stable room IDs (`R_*`).
- `Room Flow` includes a `beat` and `exits` for every room.
- `Room Flow` includes at least 3 "teach" callouts across the run.
- `Implementation Notes` includes 5+ bullet TODOs extracted from party runs

### Front matter

- `adventure_id`: stable slug (e.g. `q1-first-day`)
- `area_id`: e.g. `A01`
- `zone`: zone name from `docs/zone_beats.md` (YAML gotcha: if the value contains `: `, quote it, e.g. `zone: "Town: Gaia Gate"`)
- `clusters`: list of cluster IDs (e.g. `CL_NS_ORIENTATION`)
- `hubs`: list of `HUB_*` IDs from `docs/quest_state_model.md`
- `setpieces`: list of `setpiece.*` tags (if any)
- `level_band`: `[min, max]`
- `party_size`: `[min, max]`
- `expected_runtime_min`: rough minutes for a clean run

### Sections

- `Hook`: why you are here (1 paragraph)
- `Inputs (design assumptions)`: short bullet list of constraints learned from party runs (readability, fairness, scaling)
- `Quest Line`: linear steps, with explicit success conditions
- `Room Flow`: the actual room-by-room chain
- `NPCs`: names, roles, dialogue intent
- `Rewards`: what changes (gates, shortcuts, services)
- `Implementation Notes`: triggers, spawn notes, and what to stub first

### Room IDs

Use stable, human-readable IDs so we can later map them into a numeric area file without losing intent:

- Prefix: `R_`
- Include zone shorthand: `NS` (Newbie School), `TOWN`, `MEADOW`, `ORCH`, `SEW`, `QUARRY`, `CHECK`, `HILL`, `RAIL`, `RUST`, `LIB`, etc.
- Example: `R_NS_ORIENT_01`

Each room entry should include:

- `id`
- `cluster`
- `tags` (optional): any `HUB_*` and/or `setpiece.*`
- `exits`: directions and target room ids (keep mostly linear)
- `beat`: what happens here
- `teach`: optional lesson (verbs, aggro, interrupts, backtracking, etc.)

## Source Brief

These protoadventures are derived from:

- `docs/zone_beats.md`
- `docs/area_summary.md`
- `docs/quest_state_model.md`
- `docs/levels.md`
- `docs/adventures_todo.md`
- `docs/adventure_iteration.md`
