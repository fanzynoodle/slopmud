# Party Runs

Each file in `protoadventures/party_runs/` is a short "imagined play" report for a specific party attempting a broad adventure.

Why: multiple agents can generate runs in parallel, and later we distill the results into a single `protoadventures/<adventure_id>.md`.

## Naming

- Directory: `protoadventures/party_runs/<adventure_id>/`
- Files: `run-01.md` .. `run-10.md`

## Template

Copy `protoadventures/party_runs/_RUN_TEMPLATE.md`.

## Claiming

To avoid two agents writing the same run:

- create the `run-XX.md` file as a stub first
- fill in `claimed_by` in YAML (optional but recommended)

If you complete a run, you can optionally flip its checkbox to `[x]` in `docs/adventures_todo.md` (minimal edit).
