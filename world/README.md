# World Planning Data

This folder holds machine-readable planning data derived from the docs in `docs/`.

Current outputs:

- `world/overworld.yaml`: zones + portals + exit lengths exported from `docs/overworld_cartesian_layout.md`
- `world/overworld_pairs.tsv`: edge list with endpoint coords + `len`
- `world/zones/*.yaml`: per-zone shape stubs (bounds + portals + clusters) aligned with `docs/zone_beats.md`

These are draft authoring aids, not a final "area file" format yet.

Regenerate / validate:

- `just overworld-export`
- `just zones-stubgen`
- `just zones-annotate-proto`
- `just world-validate`
