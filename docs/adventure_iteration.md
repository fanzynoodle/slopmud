# Adventure Iteration Loop (World Authorship)

Goal: turn a high-level "broad adventure" into a playable `protoadventures/*.md` run by repeatedly sending parties through it and capturing what breaks.

This is intentionally pre-area-file. Later, we will translate protoadventures into real zone/area files.

## Quick Start (What To Read)

If you are a new agent helping author content, read these in order:

1. `docs/adventure_iteration.md` (this file)
2. `docs/adventures_todo.md` (pick an `adventure_id`, pick a `run-XX`)
3. `protoadventures/README.md` (protoadventure format + SRD tabulation rule)
4. `protoadventures/party_runs/_RUN_TEMPLATE.md` (exact run file structure)
5. `docs/zone_beats.md` (world beats)
6. `docs/quest_state_model.md` (quest keys/hubs)

Optional (helpful context, not required to start writing runs):

1. `docs/area_summary.md`
2. `docs/levels.md`
3. `reference/gaming-systems/dnd5e-srd-5.2.1-cc/TABULATION.md` (SRD usage ledger)

Optional (if you want to actually walk the protoadventure in the game):

1. `just dev-run`
2. Connect: `telnet 127.0.0.1 4000`
3. Use: `proto list`, `proto <adventure_id>`, `proto exit`

## Assignment Pattern (Copy/Paste)

- "Pick one `adventure_id` in `docs/adventures_todo.md`, claim `run-01`..`run-10` slots you'll write, and add files at `protoadventures/party_runs/<adventure_id>/run-XX.md`."
- "After 3 runs exist, start (or update) `protoadventures/<adventure_id>.md` to turn the runs into a linear room flow."

## Artifacts

- `docs/adventures_todo.md`: backlog of adventures, class spotlight checklist, and party run slots.
- `protoadventures/party_runs/<adventure_id>/run-XX.md`: short "field report" for one imagined party run.
- `protoadventures/<adventure_id>.md`: distilled linear room flow with implementation notes.

## Rules (So We Can Ship)

- Prefer original text and mechanics phrasing. Do not copy Wizards of the Coast text.
- If you use SRD 5.2.1 (CC BY 4.0) material directly, add a row to `reference/gaming-systems/dnd5e-srd-5.2.1-cc/TABULATION.md`.

### When SRD Tabulation Is Required (practical)

Add a ledger row if you do any of the following:

- quote or closely paraphrase SRD rules text (even if edited)
- copy numeric tables, DCs, ranges, or stat-like values from SRD
- lift named ability text, item text, or spell-like blurbs from SRD

You do *not* need to tabulate generic concepts you already know (HP, AC, advantage, etc) if you are not copying SRD phrasing or numbers.

## Workflow (Per Adventure)

1. Pick an `adventure_id` from `docs/adventures_todo.md`.
2. Claim one or more run slots by creating the run file(s) early at `protoadventures/party_runs/<adventure_id>/run-XX.md`, even as stubs.
3. Write up to 10 different parties through the broad adventure by filling `run-01` .. `run-10` (each run is 5-15 minutes of imagined play writing).
4. Do not rely on checkboxes. File existence is the source of truth; use `scripts/party_runs_status.sh` to see what's missing.
5. After 3 runs exist, start (or update) `protoadventures/<adventure_id>.md` to turn the runs into a linear room flow and capture "Inputs" and "Implementation Notes".
6. After 10 runs exist, ensure every class has at least one spotlight moment on paper and tighten the protoadventure into a clean linear run with clear success conditions.

Optional status checks:

- `scripts/party_runs_status.sh` (missing run files by adventure)
- `scripts/party_runs_class_coverage.py` (class coverage from `party_classes` YAML across runs)

## Do This Now (10-minute actionable loop)

If you only do one thing, do this:

1. Pick one `adventure_id` in `docs/adventures_todo.md`.
2. Pick an unused run slot `run-01`..`run-10`.
3. Create the file from the template:

```bash
adventure_id="q2-job-board-never-sleeps"
run_id="run-01"
mkdir -p "protoadventures/party_runs/${adventure_id}"
cp protoadventures/party_runs/_RUN_TEMPLATE.md "protoadventures/party_runs/${adventure_id}/${run_id}.md"
```

4. Open the new run file and fill the YAML front matter + timeline.

Minimum bar for a party run:

- Fill YAML front matter (especially `adventure_id`, `run_id`, `party_*`, `expected_duration_min`).
- Write the full 1..6 timeline (at least 1-2 sentences each).
- Add at least 1 spotlight moment (solo runs: 1 is fine; party runs: aim for 3+).
- Add at least 1 friction item and 1 extracted TODO.

Better bar (more useful for distillation):

- 3+ spotlight moments (including "missing class would shine at X" notes).
- 3+ friction items and 3+ extracted TODOs.

## How To Claim Work (no merge conflicts)

We "claim" work by creating the run file you intend to write.

1. Pick an adventure and an unused run number.
2. Create `protoadventures/party_runs/<adventure_id>/run-XX.md` immediately, even if it's a stub.
3. Fill in the `claimed_by` field (optional) so humans can see ownership.
4. Do not edit shared docs to claim work.

This avoids editing shared docs and prevents two agents from writing the same run.

## Design Targets

- Parties: size 1-5.
- Levels: match the adventure's level band.
- Include at least one under-leveled run (stress test).
- Include at least one over-leveled run (boredom test).
- Include at least one all-melee run.
- Include at least one all-ranged or all-caster run.
- Include at least one "no healer" run.
- Include at least one "social build" run.
- Encounters: should teach something; avoid filler.
- Every adventure needs one safe loop (consistent progress).
- Every adventure needs one spike (elite/boss setpiece).
- Every adventure needs one secret (shortcut, lore, rare, or hidden exit).

## Party Run Template

Use `protoadventures/party_runs/_RUN_TEMPLATE.md`.

## Status Helper (optional)

To see which adventures are missing run files (without relying on checkboxes), run:

- `scripts/party_runs_status.sh`

To validate that protoadventure room graphs are structurally loadable (room headings + exits), run:

- `python3 scripts/protoadventure_lint.py`

## Next Stage: Area Files (Room Graphs)

After a protoadventure is stable, translate it into an engine-facing room graph:

- Spec + workflow: `docs/area_files.md`
- Scoreboard: `docs/areas_todo.md`

Quick loop:

```bash
zone_id="under_town_sewers"
claimed_by="yourname"

just area-lock "$zone_id" "$claimed_by" "translate protoadventure"
${EDITOR:-vi} "world/areas/${zone_id}.yaml"
just world-validate
just area-files-validate
just area-unlock "$zone_id" "$claimed_by"
```
