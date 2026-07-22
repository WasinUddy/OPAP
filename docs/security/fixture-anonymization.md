# Test-fixture anonymization policy

CPAP files can identify a person through names, serial numbers, timestamps,
notes, embedded headers, or combinations of otherwise ordinary fields. Simply
renaming a directory is not anonymization.

## Repository policy

- Public fixtures MUST be synthetic, or anonymized with documented permission
  for public redistribution. Prefer synthetic fixtures for every new test.
- Raw patient cards, screenshots, databases, logs, reports, and golden files
  derived from them MUST NOT be committed, attached to issues, or uploaded to
  CI artifacts.
- Private conformance fixtures stay outside Git in the ignored
  `testdata/private` layout described by `compat/README.md`. Store and transfer
  them only through an approved encrypted location with access limited to
  maintainers who need them.
- Golden outputs have the same sensitivity as their inputs until reviewed; a
  parsed JSON or YAML file is not automatically anonymous.

## Anonymization checklist

Before a fixture may become public, a reviewer other than its author MUST
confirm that it:

1. Removes names, birth dates, patient and clinician identifiers, free text,
   addresses, account references, and original filenames.
2. Replaces machine serial numbers and stable device identifiers with reserved
   synthetic values, consistently across all files.
3. Shifts all dates and times by one secret random offset while preserving only
   intervals needed by the test; record neither the original dates nor the
   offset in Git.
4. Removes unused sessions and signal ranges, thumbnails, filesystem metadata,
   and vendor fields not required by the asserted behavior.
5. Scans both decoded content and binary strings for the removed values, then
   verifies that the minimized fixture still exercises its named test.
6. Records provenance as `synthetic` or `anonymized-with-permission`, the fields
   transformed, and the review date in a sidecar manifest without personal data.

Anonymization is irreversible for repository purposes. If a later discovery
shows a fixture may identify someone, remove it from current history and release
artifacts, rotate any exposed identifiers where possible, and perform a focused
incident review. Git history rewriting requires maintainer coordination.

Unit, integration, and Cucumber acceptance tests in normal CI MUST use only
public-safe fixtures. Private differential tests are opt-in and MUST skip with
a clear message when the external corpus is absent.
