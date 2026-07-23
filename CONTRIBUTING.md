# Contributing to OPAP

Thank you for helping build OPAP. The project is still establishing its import
and compatibility foundations, so small changes with explicit boundaries and
tests are much easier to review than broad ports.

## Before you start

1. Read the [current status](README.md#current-status),
   [architecture](docs/architecture.md), and [roadmap](docs/roadmap.md).
2. Search existing issues before opening a new one. For a larger feature or a
   compatibility change, discuss the behavior and acceptance evidence first.
3. Never attach a real CPAP card, patient database, log, screenshot, report, or
   golden output to an issue or pull request.
4. Report vulnerabilities and potentially dangerous clinical-correctness defects
   using [SECURITY.md](SECURITY.md).

By contributing, you agree that your contribution is distributed under
GPL-3.0-only and that you have the right to submit it. Do not copy proprietary
vendor code or documentation.

## Set up the repository

Install Rust stable with `rustfmt` and Clippy, plus Node.js 22 and pnpm 11.15.1
for frontend work. Native-host changes also need the platform packages from the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/). Then run:

```sh
cargo test --workspace --all-targets --all-features --locked
pnpm --dir apps/desktop install --frozen-lockfile
pnpm --dir apps/desktop test:unit
```

The complete commands, including the stand-alone Tauri checks, are in the
[development setup](README.md#development-setup).

## Design and implementation rules

### Rust import and domain code

- Treat all device data as hostile. Bound recursion, entry counts, file reads,
  decoded counts, allocations, decompression, and arithmetic before allocating
  or indexing.
- Keep filesystem access outside pure parsers. `ImportSource` is the portability
  boundary; `opap-edf` consumes caller-provided bytes.
- Avoid `unsafe` in parser code. A proposed exception needs a documented safety
  argument, tests, and focused review.
- Preserve raw device-local time and correction/timezone provenance whenever a
  normalized timestamp is produced.
- Reject non-finite clinical values at the boundary. Missing and invalid values
  are not zero.
- Make imports repeatable and transactional. Reimporting the same logical
  session must replace stale child data rather than append duplicates.

### OSCAR compatibility work

- Use the OSCAR-code revision in `compat/oscar-code-revision.txt` as the
  behavioral oracle. Do not silently substitute OSCAR-SQL behavior; its
  SQL-only `RMVENT_*` definitions are outside the pinned compatibility set.
- Record the upstream subsystem and material deviations in [PORTING.md](PORTING.md)
  when translating behavior.
- Add focused synthetic tests for each branch. Session-level ports also require
  a versioned compatibility manifest and exact comparisons or documented
  field-specific tolerances.
- Label safety corrections and intentionally stricter behavior as deviations,
  not parity. Current examples include robust derived JSON family recognition,
  bounded EDF parsing, and guarded analytics edge cases.
- Do not present the DATALOG candidate index as an imported session or OSCAR
  session parity. It lacks STR mask-on/mask-off seeding; compressed EDF, AEV,
  and unknown DATALOG suffixes may be grouped as candidates, but their payloads
  are not decoded or imported.
- Do not treat OSCAR's existing YAML generator as a complete oracle: its public
  checkout has no patient fixture corpus, it truncates long arrays, and some
  timestamps depend on the host timezone.

### Storage and service work

- Change the schema only through numbered migrations. Test opening a fresh
  database and upgrading every supported prior schema.
- Keep foreign keys enabled and multi-table changes in one transaction.
- Expose stable typed errors and DTOs; clients must not branch on human-readable
  error text.
- Do not enable an import worker while jobs lack durable recovery, cancellation,
  source fingerprinting, and an implemented session parser.

### UI and native-host work

- Use Mantine components and the shared theme before adding custom controls.
- Keep keyboard, screen-reader, narrow-window, loading, empty, and error states
  part of the feature, not later cleanup.
- Label fabricated data and demo behavior unambiguously. A preview must never
  look like a successful real import.
- Keep privileged work in allowlisted native commands. Do not expose arbitrary
  filesystem or database access to the renderer.
- Range-load and viewport-downsample waveforms instead of copying an entire
  night into React state.

## Tests expected with a change

| Change | Minimum evidence |
| --- | --- |
| Pure parser or domain behavior | Unit tests including malformed and boundary input; WASM check when portable code changes |
| Filesystem source | Symlink/traversal, size, count, depth, and read-limit tests |
| Storage migration/repository | Fresh and upgraded database integration tests, constraint failures, and rollback/reimport coverage |
| User-visible workflow | Component/integration tests and a focused Cucumber scenario once the workflow is truly connected |
| OSCAR-derived behavior | Synthetic branch tests plus differential conformance evidence where fixtures are legally available |
| UI layout | Unit tests plus manual keyboard and representative narrow/wide viewport review |

Run before requesting review:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --manifest-path tests/acceptance/Cargo.toml --locked
pnpm --dir apps/desktop lint
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop test:unit
pnpm --dir apps/desktop build
```

Also run the relevant stand-alone manifest tests when changing
`apps/desktop/src-tauri`:

```sh
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --all-targets --all-features -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --all-targets --all-features
pnpm --dir apps/desktop run tauri:build:debug
```

The last command uses the exact Tauri CLI in `pnpm-lock.yaml` and deliberately
skips bundling. `pnpm --dir apps/desktop run tauri:bundle` requests an unsigned
local developer bundle, not a production or release-signing workflow. Preserve
the checked-in PNG/ICNS/ICO assets and verify the complete GPLv3 text is
physically included in every redistributed artifact; installer `licenseFile`
metadata alone is insufficient. Windows privacy hardening remains incomplete
until private ACL, hard-link, and reparse-point behavior is implemented and
exercised on a Windows CI runner.

Any build intended for redistribution must compile the native and renderer
About metadata from the same reviewed revision by setting
`OPAP_BUILD_REVISION` and `VITE_OPAP_SOURCE_REVISION` to that exact Git hash.

## Fixtures and privacy

Normal tests must use synthetic public fixtures. Approved private conformance
cards live outside Git under `testdata/private` and are opt-in. Follow the
[fixture anonymization policy](docs/security/fixture-anonymization.md) even for
parsed JSON or YAML: a transformed output can still identify someone.

Use fictional names, reserved serial numbers, and synthetic dates in examples.
Do not paste local absolute paths or database contents into test snapshots,
commit messages, CI logs, or review comments.

## Pull requests

- Keep commits focused and describe the user-visible or compatibility behavior,
  not only the files changed.
- Explain safety limits, data migrations, and OSCAR deviations in the pull
  request body.
- State which commands ran and which checks were not available locally.
- Update README, architecture, port map, policy, and API documentation when a
  boundary or support claim changes.
- Do not update a golden result merely to make a failing comparison pass; explain
  and review the behavioral change.
