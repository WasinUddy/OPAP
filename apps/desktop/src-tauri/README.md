# OPAP native host

This Tauri 2 crate is a thin, local-only adapter around `opap-service`. It
exposes these commands:

- `about`
- `app_bootstrap`
- `profile_list`
- `profile_create`
- `source_select`
- `import_prepare`
- `import_jobs`
- `import_cancel`

`source_select` accepts no arguments. It opens the operating system's folder
picker and passes the selected path directly to the service; neither its input
nor its result contains a filesystem path. Cancellation returns `null`.
Successful inspection returns the canonical service `SourceInspection`, which
contains a process-local opaque `source_id` and a redacted serial suffix.

Storage is created as `opap.sqlite3` inside Tauri's application data directory.
On Unix, the host applies mode `0700` to that directory and `0600` to the
database. It rejects symlinked and multiply-linked database files, uses
no-follow opens, and protects SQLite WAL sidecars through the private directory.

All fallible commands return `opap-service::ApiError`. Frontends should branch
on its stable `code`, never on the human-readable message. Clinical and workflow
DTOs are never redefined in this host.

No command currently imports or synthesizes clinical sessions. Prepared jobs
remain explicitly blocked with `session_parser_not_implemented`; no command for
running a session import is exposed.

Native bundles include the repository's complete GPLv3 text and platform icon
formats. Release builds should set `OPAP_BUILD_REVISION` to the exact 7-40 digit
Git commit so About can link to the matching source; development builds omit the
revision instead of claiming provenance they do not have.

This crate retains a nested `[workspace]` and lockfile so its platform-heavy
Tauri dependency graph stays isolated from the root Rust workspace. The shared
`opap-service` crate is a root-workspace member and is consumed here through an
exact-version path dependency. Dedicated native-host CI validates this boundary.
The native host's minimum Rust version is 1.88 because its patched plist/XML
dependency chain requires that release.
