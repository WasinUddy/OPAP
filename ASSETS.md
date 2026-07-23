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
`apps/desktop/pnpm-lock.yaml`; discovered dependency license and notice text is
included in [`THIRD_PARTY_LICENSES.txt`](THIRD_PARTY_LICENSES.txt). Before
distributing a build, inspect the produced artifact, pinned supplemental source
evidence, and any explicitly unresolved exception in that generated file. The
release gate remains blocked while an exception exists; this focused asset
ledger is not an exhaustive third-party notice or a legal-compliance claim.
