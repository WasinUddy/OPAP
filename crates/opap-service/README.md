# opap-service

`opap-service` is OPAP's framework-neutral application boundary. It converts
the importer and SQLite storage crates into versioned, serializable requests,
responses, and stable API errors. Native shells such as Tauri should expose
this service rather than putting business rules in UI commands.

API schema version 2 removes caller-supplied import request keys. Hosts must
use the version reported by bootstrap and reject incompatible renderers.

The current service supports application bootstrap, profile management,
ResMed source inspection, and durable preparation/cancellation of import jobs.
Prepared jobs remain explicitly blocked while ResMed session parsing is not
implemented in `opap-core`; this crate never reports fabricated import
success.

Native folder paths never enter serializable requests, responses, or import
history. Inspection captures a directory capability in the service and returns
an opaque process-local source ID plus a redacted label. The web view can use
that ID but cannot choose or recover arbitrary filesystem paths.

Device display metadata is treated as untrusted removable-media input. The
service emits only a fixed importer-owned brand and allowlisted, service-owned
family labels after bounded validation. Raw model and product-code text never
crosses the serialized boundary; unsafe values use generic or empty fallbacks.
Import request keys are opaque and generated only by the service as
`opap-request:` followed by 32 random lowercase hexadecimal characters.
Renderers cannot supply request keys. Repeated preparation of the same profile
and process-local source handle is idempotent. Request keys remain internal
storage details and are never serialized, including for historical jobs.

## Transitional job representation

Storage now persists explicit blocked, running, completed, failed, and
cancelled states. Prepared jobs start as `blocked`; cancellation is a real
atomic state transition rather than an encoded failure.

The prepared-job flow is still transitional and **not executor-safe**. No
import worker may be enabled until a source fingerprint is persisted and
revalidated, recovery and progress are exercised end to end, and core session
parsing is complete. Opaque directory capabilities are intentionally
process-local and must be selected again after restart before future execution.

Copyright (C) 2026 OPAP contributors. Licensed under GPL-3.0-only.
