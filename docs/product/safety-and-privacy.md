# Product safety and privacy

## Medical-use boundary

OPAP organizes and visualizes data recorded by therapy devices. It does not
diagnose disease, prescribe treatment, recommend pressure changes, replace a
clinician, verify that a device is functioning safely, or provide emergency
monitoring. The application and exported reports MUST display a concise version
of this boundary and direct urgent concerns to an appropriate medical service.

Derived values MUST name their calculation and source channel where practical.
Unsupported, missing, corrupt, or ambiguous data is shown as unavailable or
flagged; it MUST NOT be silently fabricated or presented with false precision.
Changing a clinically meaningful calculation requires compatibility fixtures,
review, release notes, and a visible calculation/version identifier.

## Telemetry and networking

Analytics, usage telemetry, crash uploads, and diagnostic uploads are off by
default. No identifier or health-derived value is collected merely by launching,
importing, or viewing data. Any future telemetry MUST:

- be separately opt-in, revocable, and explained before collection;
- minimize fields and retention, and never contain raw therapy data, filenames,
  free text, machine serial numbers, precise therapy timestamps, or profile IDs;
- provide a preview for user-initiated diagnostic submissions; and
- keep core functionality available when consent is declined or withdrawn.

Automatic update checks, remote fonts, and other network calls require clear
documentation and a setting. Release tests MUST be able to run the app with
network access blocked without loss of core functionality.

## Backup, restore, and export

- Backup and export are explicit user actions with a chosen destination. OPAP
  does not silently mirror the source card or database to cloud folders.
- A backup contains the database, schema/version manifest, and integrity
  checksums. Creation uses a consistent database snapshot and either completes
  atomically or leaves the previous backup untouched.
- Restore validates format, version, checksums, and free space before replacing
  data. It creates a recoverable pre-restore snapshot and reports any skipped or
  incompatible content.
- Reports, CSV, JSON, and unencrypted backup formats MUST be labeled as
  containing sensitive health data before creation. File extensions MUST NOT
  imply encryption when none is provided.
- Exports include their OPAP calculation/schema version and timezone context so
  they can be interpreted later. Export failures must not damage source data.
- Deleting a profile removes OPAP's references and local files after explicit
  confirmation, but the UI MUST explain that secure erasure is not guaranteed
  on SSDs and does not remove user-created backups or exports.

Before the first stable release, backup/restore round trips and export redaction
options are acceptance-tested with synthetic profiles.
