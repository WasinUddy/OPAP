# Architecture and integration status

This document describes the repository as it exists today. It is not a claim of
OSCAR parity or a promise that the preview can import therapy data.

## Component boundaries

| Component | Responsibility | Present integration status |
| --- | --- | --- |
| `apps/desktop` | React, TypeScript, Mantine navigation and data visualization | Runs as a browser preview with fabricated data; it does not call Rust APIs |
| `apps/desktop/src-tauri` | Thin Tauri 2 native host, local database setup, stable error envelopes, and bounded source inspection | Stand-alone experimental crate; exposes bootstrap/health/storage/about/source-inspection commands but no session import |
| `crates/opap-service` | Framework-neutral DTOs and application workflows for profiles, opaque source selection, and import-job state | Stand-alone experimental crate; jobs remain blocked because the session importer is unavailable, and the Tauri host does not use this service yet |
| `crates/opap-core` | Portable domain contracts, bounded import-source abstraction, ResMed detection, and identity parsing | Used by the CLI and source-inspection foundations; its ResMed `import` operation is intentionally unsupported |
| `crates/opap-edf` | Filesystem-independent EDF/EDF+ parsing and validation | Tested independently, including WASM compilation; it is not called by `opap-core` yet |
| `crates/opap-storage` | SQLite migrations, constraints, repositories, and atomic session-data replacement | Tested as a library; no real importer currently writes clinical sessions to it |
| `tests/acceptance` | Executable Gherkin scenarios | Covers synthetic ResMed detection and machine identification through the library and CLI only |
| `compat` | Pinned OSCAR-SQL baseline and private conformance-fixture convention | Machine-identity comparison is available; canonical session manifests and session parity remain planned |

The root Cargo workspace contains `opap-core`, `opap-edf`, and `opap-storage`.
The service and Tauri host currently use their own lockfiles and must be tested
with `--manifest-path`.

## What is wired today

```text
Browser preview
  React + Mantine -> fabricated TypeScript data

Identity CLI
  opap-core CLI -> bounded DirectorySource -> ResMed detection/identity

Experimental native host
  Tauri commands -> opap-core source inspection
                 -> opap-storage bootstrap/status

Experimental service
  typed DTOs -> opap-core inspection
             -> opap-storage profiles and blocked job records

Independent parser
  caller-provided bytes -> opap-edf -> validated EDF/EDF+ structures
```

There is no path from a real CPAP card to displayed sessions. In particular,
`opap-edf` is not yet connected to ResMed filename/session grouping, settings,
events, waveform channels, summaries, timezone repair, or SQLite persistence.

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
- The renderer must not receive database handles or unrestricted filesystem
  access. The service's opaque source IDs are the intended design. The current
  experimental Tauri `inspect_source` command still accepts an absolute path,
  so it must not be treated as the final release boundary.
- Profile databases belong in per-user application storage with restrictive
  permissions. Local-first does not protect data from another process running
  as the same user or from a compromised machine; OS login protection and disk
  encryption remain important.
- Logs, error responses, diagnostics, screenshots, exports, and test goldens can
  leak health data even when raw cards are absent. Identifiers and paths must be
  redacted, and nothing is uploaded automatically.

See the [threat model](security/threat-model.md) for release-blocking controls.

## Time and calculation contracts

Device-local time, timezone context, correction provenance, and normalized UTC
must remain distinguishable. OSCAR-compatible day grouping and timestamp repair
cannot be inferred from UTC alone. Clinically meaningful calculations need a
versioned algorithm, named input channels, deterministic tests, and documented
tolerances against the pinned OSCAR baseline.

Until those contracts are implemented and verified, the application must show
the value as unavailable rather than derive or fabricate it.

## WASM boundary

The portable `opap-core` library surface and `opap-edf` are checked for the
`wasm32-unknown-unknown` target. This demonstrates that pure parsing code can be
host-independent; it is not a browser application. A browser release still
needs a separately designed file-selection, persistence, privacy, compatibility,
and support model.
