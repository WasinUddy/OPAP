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
| ResMed source handling | Bounded card inventory, card detection, machine identification, and a heuristic DATALOG candidate index | Detection does not parse STR records or establish import readiness; session import deliberately returns `UnsupportedOperation` |
| EDF/EDF+ | A safe, allocation-bounded parser for headers, samples, calibration, and TAL annotations; candidate indexing reads bounded headers | Clinical signals are not imported, and deliberate safety/spec corrections mean parser behavior is not universally identical to OSCAR |
| Local storage | Versioned SQLite migrations and repositories for profiles, machines, sessions, events, waveforms, chunks, and import history | No end-to-end importer populates a user profile |
| Desktop UI | Responsive Mantine Overview, Daily, Import, and Settings/About screens with an explicit browser-demo adapter and a typed native adapter | Therapy views still contain fabricated sample values; no session importer populates them |
| Native/application boundaries | A thin Tauri host delegates bootstrap, profile/source inspection, and blocked import-job workflows to the framework-neutral service | Real source inspection is local and path-opaque, but session import remains unavailable |
| Compatibility tests | Synthetic unit/integration tests, ResMed identification Cucumber scenarios, and an opt-in private conformance layout | A canonical full-session oracle comparison and golden suite remain planned |

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

## Run the native developer host

Install the platform packages listed in the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/), then use the
repository-pinned Tauri CLI; no global CLI installation is needed:

```sh
pnpm --dir apps/desktop install --frozen-lockfile
pnpm --dir apps/desktop run tauri:dev
```

For a non-interactive compile check matching CI, build the host without an
installer or application bundle. The default script uses an optimized Rust
profile; append `:debug` for the faster CI-equivalent profile:

```sh
pnpm --dir apps/desktop run tauri:build
pnpm --dir apps/desktop run tauri:build:debug
```

`pnpm --dir apps/desktop run tauri:bundle` asks the platform packaging tools for
an **unsigned local developer bundle**. Its configuration uses the checked-in
PNG, macOS ICNS, and Windows ICO artwork and points installer packaging at the
GPLv3 license text, but the result is not a signed, notarized, or supported
production release. Before redistribution, verify that the complete license is
physically present in the artifact; installer license metadata alone does not
place it inside every application format. CI only compiles the native host on
macOS with `--no-bundle`; it does not publish an installer.

For an attributable build, set `OPAP_BUILD_REVISION` and
`VITE_OPAP_SOURCE_REVISION` to the same exact 7–40 digit Git revision before
running the command. When either value is absent or invalid, the corresponding
About/source link stays unavailable instead of claiming unverifiable provenance.

Windows builds also still rely on inherited application-data ACLs. Explicit
private DACL, hard-link, and reparse-point enforcement is a release blocker
before OPAP can claim the same local-database hardening it applies on Unix.

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

The application service is part of the root workspace. The Tauri host retains a
stand-alone lockfile, so check it explicitly when changing native code or its
dependencies:

```sh
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --all-targets --all-features -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked
pnpm --dir apps/desktop run tauri:build:debug
```

The Cucumber suite covers ResMed detection/identification and blocked
application-service workflows; it does not execute a therapy-session import.
Private or approved anonymized compatibility cards use the ignored layout
documented in [`compat/README.md`](compat/README.md); never add real patient card
contents to Git.

## OSCAR compatibility scope

The behavioral oracle is
[`CrimsonNape/OSCAR-code`](https://gitlab.com/CrimsonNape/OSCAR-code) commit
`64c5e90a26f91fb15868bcfcccde0c1e1522ac86`, recorded in
[`compat/oscar-code-revision.txt`](compat/oscar-code-revision.txt). The relevant
ResMed/EDF loader files at that revision are byte-identical to OSCAR 1.7.2
commit `c5c7890785b196993c7c67966f024c32929ec5ab`.

Compatibility is intentionally narrower than “OSCAR rewritten.” OPAP corrects
OSCAR's derived JSON family-name truncation, excludes `RMVENT_*` definitions
that exist only in OSCAR-SQL, and applies explicit safety guards to EDF and
analytics behavior. The current session-candidate index is a pre-import
heuristic without OSCAR's STR mask-on/mask-off seeding. Compressed EDF, AEV, and
unknown DATALOG suffixes can be grouped as candidates, but their payloads,
machine type/settings, events, waveforms, and oximetry remain unsupported.
Analytics also omits OSCAR's CPAP-machine-type filter and uses a bounded form
of OSCAR's day-style duration-weighted percentile calculation. See the
[port map](PORTING.md) for exact deviations; no full-session parity claim is
made.

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

The pinned OSCAR-code revision and translated behavior are recorded in
[PORTING.md](PORTING.md) and
[`compat/oscar-code-revision.txt`](compat/oscar-code-revision.txt). All
redistributed builds must preserve the GNU GPL version 3 license, corresponding
source availability, applicable notices, and upstream attribution. See
[COPYING](COPYING) and the
[GPL and attribution policy](docs/legal/gpl-and-attribution.md).
