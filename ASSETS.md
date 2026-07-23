# OPAP visual asset provenance

This ledger covers visual assets checked into the OPAP repository. It is not a
software bill of materials and does not replace dependency-license reports.

| Asset | Origin and relationship | Copyright and license |
| --- | --- | --- |
| `apps/desktop/src/assets/sleeping-breath.svg` | Original hand-drawn OPAP interface artwork, first added to this repository in 2026 | Copyright (c) 2026 OPAP contributors; GPL-3.0-only |
| `apps/desktop/src-tauri/icons/icon.svg` | Original OPAP application-icon source artwork, first added to this repository in 2026 | Copyright (c) 2026 OPAP contributors; GPL-3.0-only |
| `apps/desktop/src-tauri/icons/icon.png`, `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, and `icon.ico` | Raster and platform-format derivatives of `icon.svg` | Copyright (c) 2026 OPAP contributors; GPL-3.0-only |

No OSCAR or SleepyHead logo, screenshot, documentation image, or patient data
is included in the assets listed above.

The interface also renders icons from the `lucide-react` package (ISC) and uses
Mantine packages (MIT). Those packages are version-pinned by
`apps/desktop/pnpm-lock.yaml`; their own distributed license files and the
automated dependency-license report remain authoritative. Before distributing
a build, inspect the produced artifact rather than treating this focused ledger
as an exhaustive third-party notice file.
