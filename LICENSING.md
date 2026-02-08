# Licensing

This repository contains source code, original game content, and third-party reference material.
They are licensed separately.

## Source Code: MIT OR 0BSD

Unless a file or directory says otherwise, the source code in this repository is dual-licensed under
your choice of:

- MIT License (see `LICENSE`)
- BSD Zero Clause License (0BSD) (see `LICENSE-0BSD`)

SPDX identifier: `MIT OR 0BSD`.

## Original Content: MIT OR 0BSD OR CC BY-SA 4.0

Unless a file or directory says otherwise, original non-code content in this repository (docs,
worldbuilding text, protoadventures, data, etc.) is tri-licensed under your choice of:

- MIT License (see `LICENSE`)
- BSD Zero Clause License (0BSD) (see `LICENSE-0BSD`)
- Creative Commons Attribution-ShareAlike 4.0 International (CC BY-SA 4.0):
  https://creativecommons.org/licenses/by-sa/4.0/legalcode

If you use the CC BY-SA option, attribute as: "slopmud contributors" (or "fanzynoodle and
contributors").

## Third-Party Material: Various (Keep Original Licenses)

Some paths include third-party material. Those files keep their original licenses and are not
re-licensed by the terms above. Look for license/notice files in the relevant directory (and/or
per-file headers).

Common third-party/reference paths:

- `reference/mud-codebases/**`
- `reference/game-engines/**`
- `reference/gaming-systems/**`

### D&D SRD (CC BY 4.0): What We Can/Can't Use

This repo vendors D&D System Reference Document (SRD) material that Wizards of the Coast LLC has
released under Creative Commons Attribution 4.0 (CC BY 4.0).

SRD copies in this repo:

- SRD 5.2.1 (official): `reference/gaming-systems/dnd5e-srd-5.2.1-cc/`

Project policy (based on `docs/adventure_iteration.md`, `protoadventures/README.md`, and the SRD
legal section):

- OK: Use SRD material when needed, as permitted by CC BY 4.0, with the required attribution
  statement included in the published work.
- OK: Use generic RPG concepts you already know (HP/AC/advantage/etc.) without copying SRD prose or
  numbers.
- Not OK (project policy): Copy Wizards of the Coast text outside the SRD.
- Required when using SRD text/numbers: Add a row to
  `reference/gaming-systems/dnd5e-srd-5.2.1-cc/TABULATION.md` (ledger of SRD-derived usages).
- Currently, the SRD 5.2.1 attribution statement is included in `docs/area_summary.md` and
  `docs/levels.md` (see `reference/gaming-systems/dnd5e-srd-5.2.1-cc/TABULATION.md`, row
  `SRD-ATTR-001`).
- Attribution: The SRD legal section provides a specific required attribution statement and asks you
  not to add any other attribution to Wizards beyond that statement. It also says you may describe
  your work as "compatible with fifth edition" or "5E compatible."

SRD 5.2.1 required attribution statement (verbatim):

```text
This work includes material from the System Reference Document 5.2.1 ("SRD 5.2.1") by Wizards of the Coast LLC, available at https://www.dndbeyond.com/srd. The SRD 5.2.1 is licensed under the Creative Commons Attribution 4.0 International License, available at https://creativecommons.org/licenses/by/4.0/legalcode.
```

### OGL Reference Material (Not CC BY)

This repo also includes Open Game License (OGL) reference material (for example
`reference/gaming-systems/opend6/`). If you plan to ship
OGL-derived text, you must comply with the OGL terms included with those works (including Product
Identity restrictions). slopmud treats these as reference-only unless explicitly stated otherwise.
