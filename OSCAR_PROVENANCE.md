# OSCAR provenance record

OPAP is a modified, GPLv3-covered derivative informed by OSCAR and SleepyHead.
It is a Rust and TypeScript rewrite, not an official OSCAR release and not a
claim of complete OSCAR compatibility.

## Pinned behavioral reference

- Repository: <https://gitlab.com/CrimsonNape/OSCAR-code>
- Revision: `64c5e90a26f91fb15868bcfcccde0c1e1522ac86`
- Machine-readable pin: [`compat/oscar-code-revision.txt`](compat/oscar-code-revision.txt)
- Upstream grant: GNU General Public License, version 3 or later
- Combined OPAP work: GNU General Public License, version 3 only
- Upstream notices retained for translated work:
  - `Copyright (c) 2011-2018 Mark Watkins`
  - `Copyright (c) 2019-2025 The OSCAR Team`

The reference revision was selected as the compatibility oracle for the
specific loader, EDF, channel, and analytics behavior listed in
[`PORTING.md`](PORTING.md). It is not a wholesale vendoring of the OSCAR source
tree. OPAP deliberately records safety corrections and unsupported behavior
instead of representing every difference as compatibility.

## Modification record

OPAP development and translation work began in 2026. Production code is being
rewritten in Rust, with a React and Mantine user interface, while focused tests
compare selected behavior against the pinned upstream reference. Module-level
provenance records add file and line references where a port is sufficiently
developed:

- [`crates/opap-channels/OSCAR_PROVENANCE.md`](crates/opap-channels/OSCAR_PROVENANCE.md)
- [`crates/opap-analytics/OSCAR_PROVENANCE.md`](crates/opap-analytics/OSCAR_PROVENANCE.md)
- [`crates/opap-edf/README.md`](crates/opap-edf/README.md)
- [`compat/README.md`](compat/README.md)

Missing modules, tests, or records remain porting work; this document does not
turn planned behavior into implemented behavior.

## Distribution record

The complete GPLv3 text is [`COPYING`](COPYING), and the prominent project and
upstream notices are in [`NOTICE.md`](NOTICE.md). Redistributed binaries must be
matched to the exact source revision used to build them and accompanied by the
required license and source-conveyance information. OPAP's About screen reports
when a source revision was not embedded rather than inventing provenance.

This record documents engineering provenance. It is not legal advice and is
not, by itself, evidence that a release artifact passed a complete license,
privacy, signing, or platform-hardening review.
