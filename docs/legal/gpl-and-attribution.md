# GPL and upstream attribution policy

OPAP is a derivative of OSCAR and SleepyHead and is distributed under GNU GPL
version 3. `COPYING` is the canonical license text. This document records project
release policy and is not legal advice.

## Required attribution

Every source distribution, packaged application, and redistributed build MUST
preserve applicable copyright notices and include this substance prominently:

> OPAP is based in part on OSCAR and on the free and open-source software
> SleepyHead, developed and copyrighted by Mark Watkins, 2011–2018. Portions of
> OSCAR are copyright the OSCAR Team.

The About screen and distribution documentation MUST identify OPAP as modified
software, link to the complete corresponding source for that exact build, offer
the GPLv3 license text, and retain OSCAR/SleepyHead credit. Installer/package
metadata and advertising materials MUST retain notices required by the upstream
project and incorporated source.

## Source and provenance requirements

- Modified or translated OSCAR logic remains GPL-covered. Rewriting C++ into
  Rust does not by itself remove GPL obligations.
- Each ported module SHOULD record the upstream project, file or subsystem,
  pinned revision from `compat/oscar-sql-revision.txt`, and material deviations.
- Release source MUST include build scripts, interface definitions, dependency
  lockfiles, and other Corresponding Source required to rebuild the shipped
  binaries, subject to GPLv3.
- Third-party dependencies and bundled assets require a license inventory.
  Dependencies with incompatible terms block release until removed or replaced.
- Contributors MUST NOT copy proprietary vendor code, documentation, keys, or
  patient data into the project.

## Release gate

Before publishing a binary, verify that `COPYING`, notices, About attribution,
source link, modified-work statement, dependency licenses, and the matching
source tag are present. A maintainer must record this check in the release notes
or release checklist.
