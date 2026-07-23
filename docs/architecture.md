# Architecture and integration status

This document describes the repository as it exists today. It is not a claim of
OSCAR parity or a promise that the desktop preview can import therapy data.

## Component boundaries

| Component | Responsibility | Present integration status |
| --- | --- | --- |
| `apps/desktop` | React, TypeScript, Mantine navigation and data visualization | Selects an explicitly fabricated adapter in a browser and a typed command adapter in Tauri; real therapy query APIs are unavailable |
| `apps/desktop/src-tauri` | Thin Tauri 2 native host, native folder picker, local database setup, and allowlisted commands | Delegates to `opap-service` and keeps source paths native; no session-import executor is exposed |
| `crates/opap-service` | Framework-neutral DTOs and application workflows for profiles, opaque source selection, and import-job state | Used by the native host and tested in the root workspace; jobs remain blocked because durable import execution is not implemented and `session_import` remains `false` |
| `crates/opap-core` | Portable domain contracts, bounded import-source abstraction, ResMed detection, identity parsing, candidate indexing, and partial session import | A direct library caller can import validated uncompressed BRP waveforms with explicit clock context; other ResMed payloads are not decoded |
| `crates/opap-edf` | Filesystem-independent EDF/EDF+ parsing and validation | Tested independently, including WASM compilation; `opap-core` uses it for bounded candidate headers and complete uncompressed BRP signal decoding |
| `crates/opap-storage` | SQLite migrations, constraints, repositories, and atomic session-data replacement | Tested as a library; no durable service workflow currently writes core BRP import reports to a user profile |
| `tests/acceptance` | Executable Gherkin scenarios | Covers synthetic ResMed detection, machine identification, partial BRP import, and privacy-safe service job workflows |
| `compat` | Pinned OSCAR-code differential manifest, comparator, and private oracle workflow | Synthetic v1 manifest comparisons are available; real-card adapters and full-session goldens remain external/planned |

The root Cargo workspace contains the portable domain, parsing, analytics,
channel-registry, storage, and service crates. The platform-specific Tauri host
retains its own lockfile and must be tested with `--manifest-path`.

## What is wired today

```text
Browser preview
  React + Mantine -> explicit demo client -> fabricated in-memory data

Native desktop
  React + Mantine -> typed Tauri client -> allowlisted Tauri commands
                  -> opap-service -> bounded opap-core source inspection
                                  -> opap-storage profiles/blocked jobs

Identity CLI
  opap-core CLI -> bounded DirectorySource -> ResMed detection/identity

Core library import
  caller-provided ImportSource + explicit fixed-offset clock context
    -> ResmedImporter discovery/candidate index
    -> opap-edf validation + affine calibration
    -> bounded partial BRP sessions with stable opaque keys and warnings
```

There is no path from a real CPAP card to persisted and displayed sessions.
Only validated uncompressed BRP waveforms are connected at the direct core
library boundary. Supported flow is normalized to L/min and source
calibration/timestamp provenance is retained. STR intervals/settings, PLD, EVE,
CSL, SAD/SA2 payload decoding, compressed BRP, durable service execution,
native import-job capability, and real UI therapy queries remain unavailable.
The browser adapter continues to provide explicitly fabricated demo data.

## Intended end-to-end shape

The target desktop boundary is:

```text
React + Mantine
  -> allowlisted, typed Tauri commands
  -> framework-neutral application service and cancellable jobs
  -> device importer + pure parsers
  -> transactional SQLite repositories
```

Large waveform arrays should be fetched in bounded time ranges and downsampled
for the chart viewport; they should not cross the native bridge as one complete
session payload. Import work should run outside the renderer, persist explicit
job phases, and commit each complete session atomically and idempotently.

## Trust and privacy boundaries

- A selected card and every file on it are untrusted input. Native sources must
  enforce path confinement, inventory limits, per-read limits, checked numeric
  conversions, and no writes to the source.
- The renderer does not receive database handles or unrestricted filesystem
  access. A pathless native picker passes the selected path directly to the
  service, which returns a process-local opaque source ID and redacted source
  metadata. Import execution must revalidate the source before it is enabled.
- Profile databases belong in per-user application storage with restrictive
  permissions. Local-first does not protect data from another process running
  as the same user or from a compromised machine; OS login protection and disk
  encryption remain important.
- Logs, error responses, diagnostics, screenshots, exports, and test goldens can
  leak health data even when raw cards are absent. Identifiers and paths must be
  redacted, and nothing is uploaded automatically.

See the [threat model](security/threat-model.md) for release-blocking controls.

## Native packaging boundary

The exact Tauri CLI is pinned in the frontend lockfile. CI runs host formatting,
strict Clippy, tests, and a macOS `tauri build --debug --no-bundle` compile. It
does not create, sign, notarize, or publish a production installer.

Local developer bundles use checked-in PNG, ICNS, and ICO assets and remain
unsigned development artifacts. Every redistributed format must be inspected to
ensure the complete GPLv3 text is physically included; installer `licenseFile`
metadata alone does not make it an application resource. Unix database setup
enforces private permissions, no-follow opening, and hard-link rejection;
equivalent Windows DACL, hard-link, and reparse-point enforcement remains a
release blocker and must be validated on Windows before making a cross-platform
privacy-hardening claim.

## Time and calculation contracts

Device-local time, timezone context, correction provenance, and normalized UTC
must remain distinguishable. The BRP slice requires an explicit fixed UTC
offset, device-local reference time, and clock correction from its caller; it
does not consult or guess from the host timezone. OSCAR-compatible STR day
grouping and broader timestamp repair are not implemented. Clinically
meaningful calculations need a versioned algorithm, named input channels,
deterministic tests, and documented tolerances against the pinned OSCAR
baseline.

Until the remaining contracts are implemented and verified, the native
application must show the value as unavailable rather than derive or fabricate
it. Fabricated browser-preview values must remain explicitly labeled as demo
data.

## WASM boundary

The portable `opap-core` library surface and `opap-edf` are checked for the
`wasm32-unknown-unknown` target. This demonstrates that pure parsing code can be
host-independent; it is not a browser application. A browser release still
needs a separately designed file-selection, persistence, privacy, compatibility,
and support model.
