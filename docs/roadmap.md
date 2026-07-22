# Development roadmap and acceptance gates

Each milestone is complete only when its gate passes on supported desktop
platforms. Functional tests use synthetic public fixtures; private OSCAR
conformance data remains outside Git.

## Status snapshot (2026-07-23)

No milestone below is complete yet, and there is no supported desktop release.
The current repository has:

- the Rust workspace, pinned OSCAR-SQL provenance, GPL/privacy policies, and CI;
- bounded ResMed card detection and machine identification, but no session
  importer;
- an independent hardened EDF/EDF+ parser that is not yet connected to ResMed
  sessions;
- versioned transactional SQLite storage without an end-to-end clinical import;
- a responsive Mantine interface populated only by clearly labeled fabricated
  data; and
- experimental service and Tauri boundaries that are not connected to that UI
  and cannot execute session imports.

The present wiring and missing links are detailed in
[Architecture and integration status](architecture.md).

## 0. Foundation

Establish the Rust workspace, pinned OSCAR-SQL baseline, GPL notices, CI, and
local-first policies.

Gate: formatting, linting, unit tests, dependency/license checks, and a clean
build pass from a fresh checkout; CI contains no private fixture or health data.

## 1. Behavioral compatibility harness

Define a versioned canonical representation for machine information, sessions,
settings, events, summaries, and signals. Generate golden results through the
pinned OSCAR C++ baseline, normalize both implementations, and compare exact
fields or documented per-field floating-point tolerances.

Gate: synthetic fixtures run in CI; private fixtures run locally when configured;
golden updates require an explained review; repeated runs are deterministic.

## 2. ResMed importer

Port detection, machine identity, EDF/session parsing, settings, respiratory
events, waveforms, summaries, timezone handling, and duplicate detection into
safe Rust slices. Keep parsers independent from UI and storage so pure portions
can later target WASM.

Gate: malformed-input unit and fuzz tests pass; differential results match the
OSCAR baseline; importing the same card twice is idempotent; cancellation or a
bad file cannot leave a partial session.

## 3. Local profile database

Add versioned SQLite migrations, transactional imports, profile lifecycle,
query APIs, backup/restore, and CSV/JSON/report export with provenance metadata.

Gate: migration and rollback tests cover every schema version; simulated crashes
preserve consistency; backup/restore round trips exactly; deletion and export
warnings match the privacy policy.

## 4. Desktop application

Deliver a minimal light-theme interface using Mantine: profile setup, card
import and progress, Daily charts, Overview trends, reports/export, settings,
About/legal notices, keyboard navigation, and purposeful hand-drawn SVG accents.
The native bridge is typed and allowlisted; waveform data is range-loaded rather
than copied wholesale into React state.

Gate: unit and integration suites pass; Cucumber acceptance scenarios cover
first run, import, duplicate import, daily review, export, backup/restore, corrupt
input, and offline use; accessibility and representative waveform performance
budgets pass on each supported OS.

## 5. Release hardening

Complete threat-model controls, dependency and license review, signed packaging,
checksums, data migration rehearsal, user documentation, and calculation/version
release notes. Telemetry remains absent or explicitly opt-in and content-free.

Gate: no release-blocking security or correctness defects; a clean machine can
install, operate offline, import a synthetic card, restore a backup, and verify
an exported report; source and attribution requirements pass the release check.

## Later milestones

Add device families one at a time through the same compatibility and parser
gates. A browser/WASM build may ship only after its filesystem, persistence,
consent, and browser support are separately specified; it must reuse pure Rust
parsers and may not weaken the local-first guarantees.
