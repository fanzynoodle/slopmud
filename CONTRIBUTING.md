# Contributing

If you contribute code, docs, game text, data, lore, quests, items, NPCs, logs, or anything else, you agree to the terms below.

## License

- Source code is dual-licensed: MIT OR 0BSD (see `LICENSE` and `LICENSE-0BSD`).
- Original non-code content is licensed separately. Unless a file says otherwise, it is tri-licensed:
  MIT OR 0BSD OR CC BY-SA 4.0 (see `LICENSING.md`).
- Your contributions are licensed under the project defaults above, and additionally licensed to
  **fanzynoodle** (so they can use/relicense/commercialize them under any terms).

If you don't want that, don't contribute.

## Third-Party Material

Only contribute material you have the rights to submit.

slopmud is inspired by tabletop RPGs (including D&D 4e), but we do not accept or ship copyrighted rules text from Wizards of the Coast.

If you contribute CC BY 4.0 material (or anything else with attribution requirements), include the required attribution and a source link.

If you use SRD 5.2.1 (CC BY 4.0) material, add an entry to `reference/gaming-systems/dnd5e-srd-5.2.1-cc/TABULATION.md`.

## Protoadventure Writing Workflow

Read these, in this order:

1. `docs/adventure_iteration.md` (the workflow: broad adventure -> 10 party runs -> protoadventure)
2. `docs/adventures_todo.md` (pick an `adventure_id`, claim a `run-XX`)
3. `protoadventures/README.md` (protoadventure format + SRD tabulation rule)
4. `protoadventures/party_runs/_RUN_TEMPLATE.md` (exact run file structure)
5. `docs/zone_beats.md` and `docs/quest_state_model.md` (world beats + quest keys/hubs)

Assignment pattern:

- Pick one `adventure_id` in `docs/adventures_todo.md`, claim run slots by creating files at `protoadventures/party_runs/<adventure_id>/run-XX.md`, and optionally set `claimed_by` in YAML.
- After 3 runs exist, start (or update) `protoadventures/<adventure_id>.md` to turn the runs into a linear room flow.
- If you complete a run, optionally flip its checkbox to `[x]` in `docs/adventures_todo.md` (minimal edit).

Rule callout:

- Do not copy Wizards of the Coast text. If you directly use SRD 5.2.1 material, add an entry to `reference/gaming-systems/dnd5e-srd-5.2.1-cc/TABULATION.md`.

## Local Playtest + Validation (Optional)

- Load a protoadventure in-game: `just dev-run`, connect via `telnet 127.0.0.1 4000`, then `proto list` / `proto <adventure_id>` / `proto exit`.
- Validate protoadventure structure: `just proto-lint` (checks for missing room headings and dangling exits).

## Area / Overworld Authoring Workflow (Parallel-Friendly)

This is the next layer after protoadventures: draft zone "shape" YAML and keep it consistent with the overworld portal/length spec.

Read these, in this order:

1. `docs/area_iteration.md` (workflow + lock rules)
2. `docs/areas_todo.md` (pick a zone)
3. `docs/zone_beats.md` (official cluster IDs)
4. `docs/overworld_cartesian_layout.md` (portals + exit lengths)

Rules:

- Take a zone lock before editing `world/zones/<zone_id>.yaml`.
- Lock files under `locks/` are local coordination and should not be committed.
- Keep cluster IDs aligned with `docs/zone_beats.md` (avoid inventing new `CL_*` IDs in protoadventures).

Useful commands:

- Export + validate overworld spec: `just overworld-export && just overworld-validate`
- Generate stub zone shapes: `just zones-stubgen`
- Validate all zone shapes: `just areas-validate`
- Take/release a zone lock: `just area-lock <zone_id> <you>` / `just area-unlock <zone_id> <you>`

## Area Files (Room Graphs)

After a zone is shaped, translate it into an engine-facing room graph:

- Spec: `docs/area_files.md`
- Files: `world/areas/<zone_id>.yaml`

Validate:

- `just world-validate`
- `just area-files-validate`
