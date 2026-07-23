# OPAP project policies

These documents define the security, privacy, safety, licensing, and release
expectations for OPAP. They apply to the desktop application, Rust crates,
compatibility tools, test data, and release artifacts.

OPAP is local-first software for viewing CPAP data. CPAP records are highly
sensitive health data and may be protected health information (PHI) in a
regulated context, even though OPAP itself is not necessarily operated by a
HIPAA-covered entity. Treat them accordingly.

## Decisions and policies

- [Architecture and integration status](architecture.md)
- [OSCAR port map and compatibility boundaries](../PORTING.md)
- [ADR 0001: local-first health-data architecture](adr/0001-local-first-health-data.md)
- [Threat model](security/threat-model.md)
- [Test-fixture anonymization policy](security/fixture-anonymization.md)
- [Product safety, telemetry, backup, and export](product/safety-and-privacy.md)
- [GPL and upstream attribution](legal/gpl-and-attribution.md)
- [Development roadmap and acceptance gates](roadmap.md)

Repository-wide contribution and reporting guidance lives in
[`CONTRIBUTING.md`](../CONTRIBUTING.md) and [`SECURITY.md`](../SECURITY.md).

`MUST`, `MUST NOT`, `SHOULD`, and `MAY` describe release requirements. A change
that cannot meet a `MUST` requires a documented architecture decision and
maintainer approval before release.
