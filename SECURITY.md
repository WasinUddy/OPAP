# Security policy

OPAP handles untrusted device files and potentially sensitive health data.
Responsible reports about parser safety, path confinement, local data exposure,
native-command scope, dependencies, or high-confidence clinical correctness are
welcome.

## Supported versions

OPAP is a pre-release project with no supported binary release. Security fixes
are made on the current `main` branch; older commits and preview artifacts do
not receive separate support.

## Report privately

Use GitHub's
[private vulnerability report](https://github.com/WasinUddy/OPAP/security/advisories/new)
when it is available. Do not open a public issue with exploit details, patient
information, a real CPAP file, a database, or a sensitive log.

If the private form is unavailable, open a minimal issue asking a maintainer for
a private reporting channel. Include no vulnerability details or sensitive data
in that issue.

A useful private report contains:

- the affected commit and component;
- impact and realistic attack preconditions;
- minimal reproduction steps using synthetic data;
- expected and observed behavior;
- relevant operating system/toolchain details; and
- a suggested mitigation, if known.

Do not collect patient data to demonstrate a vulnerability. Do not test against
someone else's device, account, files, or computer.

## Clinical-correctness reports

A deterministic error that could plausibly cause a user to misinterpret therapy
data may be handled privately like a vulnerability. Use a synthetic or
thoroughly minimized reproduction and identify the source channel, timestamp
context, calculation version, expected result, and comparison oracle. General
feature requests and non-sensitive display bugs can use the public issue tracker.

OPAP remains a developer preview and is not a medical device, diagnosis system,
remote monitor, or emergency service. For an urgent health concern, contact a
qualified clinician or local emergency service rather than relying on this
project or its maintainers.

## What to expect

Maintainers will validate the report, determine affected versions and severity,
and coordinate remediation and disclosure when practicable. Please allow time
for a fix and release plan before public disclosure. Credit is offered when
requested and safe to provide.

The project cannot promise a bounty, a fixed response deadline, or support for
production use at this stage.

## Security model and limitations

The required controls and explicit exclusions are documented in the
[threat model](docs/security/threat-model.md). In summary:

- device files, source directories, imports, and exports are untrusted;
- core functionality is intended to remain local and offline, with telemetry off
  by default;
- local-first storage does not protect an already compromised machine or data
  shared in an unencrypted export; and
- raw patient fixtures and derived goldens must never be attached to reports or
  committed to the repository.
