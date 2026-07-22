# OPAP

OPAP is an early-stage, local-first CPAP data viewer. The project is rebuilding
selected OSCAR behavior in safe Rust and pairing it with a minimal React and
Mantine desktop interface.

> [!IMPORTANT]
> OPAP is a developer preview. It does **not** import real therapy sessions yet,
> does **not** have full OSCAR compatibility, and must not be used to diagnose,
> monitor, or change treatment. The screens currently display fabricated sample
> data.

## Current status

The repository contains tested foundations, not a user-ready CPAP application:

| Area | What works now | Important limit |
| --- | --- | --- |
| ResMed source handling | Bounded card inventory, card detection, and machine identification from `Identification.tgt` or `Identification.json` | Session import deliberately returns `UnsupportedOperation` |
| EDF/EDF+ | A safe, allocation-bounded parser for headers, samples, calibration, and TAL annotations | It is not connected to the ResMed session importer |
| Local storage | Versioned SQLite migrations and repositories for profiles, machines, sessions, events, waveforms, chunks, and import history | No end-to-end importer populates a user profile |
| Desktop UI | Responsive Mantine Overview, Daily, Import, and Settings/About preview screens | All therapy values and the import flow are fabricated; settings are not saved |
| Native/application boundaries | Experimental Tauri host and framework-neutral service APIs for bootstrap, profile/source inspection, and blocked import jobs | They are stand-alone foundations and are not wired into the preview UI |
| Compatibility tests | Synthetic unit/integration tests, ResMed identification Cucumber scenarios, and an opt-in private conformance layout | No session-level OSCAR golden parity suite exists yet |

See the [architecture and integration status](docs/architecture.md), the
[OSCAR port map](PORTING.md), and the [roadmap](docs/roadmap.md) for the exact
boundaries.

## Preview the interface

The browser preview requires Node.js 22 and pnpm 11.15.1 (the versions used by
CI):

```sh
cd apps/desktop
pnpm install --frozen-lockfile
pnpm dev
```

Open <http://localhost:5173>. This preview does not open a device folder, read a
CPAP card, write SQLite data, or make medical calculations.

## Inspect a ResMed card identity

The current CLI only detects a card and reads its machine identity:

```sh
cargo run -p opap-core -- detect /path/to/card
cargo run -p opap-core -- machine-info /path/to/card
```

Treat the resulting serial number and source path as sensitive. These commands
do not import EDF sessions, events, settings, summaries, or waveforms, and they
never write to the source card.

## Development setup

Requirements:

- Rust stable with `rustfmt` and Clippy; the workspace's minimum supported Rust
  version is 1.85 and `rust-toolchain.toml` selects stable.
- Node.js 22 and pnpm 11.15.1 for the frontend.
- Platform dependencies from the
  [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) only when
  working on the experimental native host.

From the repository root, run the same primary checks as CI:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --manifest-path tests/acceptance/Cargo.toml --locked

pnpm --dir apps/desktop install --frozen-lockfile
pnpm --dir apps/desktop lint
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop test:unit
pnpm --dir apps/desktop build
```

The application service and Tauri host currently remain stand-alone crates, so
check them explicitly when changing their code:

```sh
cargo test --manifest-path crates/opap-service/Cargo.toml --locked
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked
```

The Cucumber suite covers ResMed detection and identification only. Private or
approved anonymized compatibility cards use the ignored layout documented in
[`compat/README.md`](compat/README.md); never add real patient card contents to
Git.

## Project principles

- **Local first:** core viewing functionality must work offline, with telemetry
  and diagnostic uploads off by default.
- **No fabricated clinical results:** missing or unsupported data is unavailable
  or explicitly flagged, never silently synthesized.
- **Safe parsing:** device files are untrusted; reads, allocations, paths, and
  decoded structures must be bounded and tested with malformed input.
- **Behavior before redesign:** OSCAR is a pinned behavioral oracle for selected
  compatible behavior, while the production implementation remains idiomatic
  Rust with typed boundaries.
- **Small, testable ports:** parser, domain, storage, service, native host, and UI
  remain separable so each can be verified independently.

Read [CONTRIBUTING.md](CONTRIBUTING.md) before proposing a change. Security and
clinical-correctness concerns should follow [SECURITY.md](SECURITY.md), not a
public issue containing sensitive details.

## Privacy and medical-use boundary

CPAP cards, databases, logs, screenshots, and parsed outputs can contain health
information, stable identifiers, and precise timestamps. Only synthetic or
explicitly approved and reviewed anonymized fixtures belong in the repository.
The full policies cover the [threat model](docs/security/threat-model.md),
[fixture anonymization](docs/security/fixture-anonymization.md), and
[product safety and privacy](docs/product/safety-and-privacy.md).

OPAP organizes and visualizes device-recorded information. It is not a medical
device, diagnosis system, remote monitor, emergency service, or substitute for
a qualified clinician.

## Licensing and attribution

OPAP is a GPLv3 derivative of OSCAR and SleepyHead. Portions are based on the
free and open-source software SleepyHead, developed and copyrighted by Mark
Watkins, 2011–2018. Portions of OSCAR are copyright the OSCAR Team.

The pinned OSCAR-SQL revision and translated behavior are recorded in
[PORTING.md](PORTING.md) and
[`compat/oscar-sql-revision.txt`](compat/oscar-sql-revision.txt). All
redistributed builds must preserve the GNU GPL version 3 license, corresponding
source availability, applicable notices, and upstream attribution. See
[COPYING](COPYING) and the
[GPL and attribution policy](docs/legal/gpl-and-attribution.md).
