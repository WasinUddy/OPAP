# Threat model

## Protected assets

- Raw SD-card files and imported session data
- Identity, profile, machine serial number, and therapy timestamps
- Derived metrics, notes, reports, backups, exports, and test fixtures
- Database integrity and parser correctness

The user-controlled CPAP card, imported files, export destinations, and files
opened from elsewhere are untrusted inputs. The native application, packaged
Rust core, local database, and renderer/native bridge are trusted only after
their release integrity and permissions have been validated.

## Primary threats and required controls

| Threat | Required control |
| --- | --- |
| Malformed device files cause code execution, denial of service, or excessive allocation | Bounds-check every length/count, reject impossible timestamps and sizes, avoid `unsafe` in parsers, cap decompression/allocation, and fuzz parser entry points. |
| Import escapes the selected card directory through links or crafted paths | Canonicalize roots, reject traversal, do not follow links outside the selected root, and never write to the source card. |
| Duplicate or partial imports corrupt results | Use transactions, stable session identities, idempotent re-import, schema constraints, and rollback on failure. |
| Renderer compromise reaches arbitrary files or commands | Expose an allowlisted, typed native API; keep raw paths and database handles out of the web view; use a restrictive content-security policy. |
| Sensitive data leaks through logs, crashes, analytics, or screenshots | Redact identifiers and paths, keep telemetry off, exclude data payloads from diagnostics, and require preview/consent before any diagnostic bundle leaves the device. |
| Another local user reads records | Use per-user storage and restrictive file permissions; clearly recommend OS login and full-disk encryption. |
| Export or backup is disclosed | Warn that exports contain health data, make destinations explicit, never upload automatically, and document whether each format is encrypted. |
| Supply-chain compromise alters releases | Pin dependencies, audit licenses/advisories, protect release signing keys, publish checksums, and build from reviewed source. |
| Incorrect calculations influence care | Differential-test against the pinned OSCAR baseline, show source data and calculation provenance, and enforce the medical-use boundary. |

## Out of scope

OPAP does not claim to protect data on an already compromised device, from a
user who can access the same OS account, or after the user shares an
unencrypted export. It is not a medical device, diagnosis system, remote
monitor, or emergency-alert service.

## Security gate

Every new parser requires malformed-input unit tests and fuzz coverage. Every
new bridge command requires an authorization/scope review. A release is blocked
by known arbitrary-code execution, path traversal, unintended network transfer,
silent data loss, or high-confidence clinical-calculation defects.
