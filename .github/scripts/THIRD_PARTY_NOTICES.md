# Third-party notice generation

`generate-third-party-licenses.mjs` creates the checked-in
`THIRD_PARTY_LICENSES.txt` used by OPAP's downloadable artifacts and Tauri
resource bundle.

The exact tool policy and distribution tuples live in
`third-party-targets.json`. The current inventory covers:

- the root Cargo workspace for `x86_64-unknown-linux-gnu`;
- the Tauri Cargo workspace for `aarch64-apple-darwin` and
  `x86_64-apple-darwin`; and
- independent production-only pnpm runtime graphs for `darwin-arm64`,
  `darwin-x64`, and `linux-x64-glibc`.

The normal desktop workspace remains host-native. Notice generation creates
isolated, ignored installs below `target/third-party-node/`; each generated
workspace selects one target tuple. A platform package is labeled only with
the exact tuple allowed by its own published `os`, `cpu`, and `libc`
constraints.

Dev-only build and test packages are intentionally not represented as shipped
runtime components. For example, `stackback` is reachable only through
Vitest's dev dependency chain and `pnpm why --prod stackback` returns no
production path. The security workflow still audits the full locked frontend
graph; this distribution notice inventories code included in or required by
the shipped runtime.

## Generate and verify

Dependency fetching happens before the offline Cargo metadata pass. From the
repository root:

```sh
cargo fetch --locked --target x86_64-unknown-linux-gnu
cargo fetch --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --target aarch64-apple-darwin
cargo fetch --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --target x86_64-apple-darwin
node .github/scripts/prepare-third-party-node-dependencies.mjs
node .github/scripts/generate-third-party-licenses.mjs
node .github/scripts/generate-third-party-licenses.mjs --check
```

Run the focused safety and resource tests with:

```sh
node --test .github/scripts/tests/generate-third-party-licenses.test.mjs
node .github/scripts/test-tauri-legal-resources.mjs
node .github/scripts/verify-license-supplement-sources.mjs
```

The output has no timestamp or local absolute path. It uses code-point sorting,
normalizes text, stores identical texts once, and references them by SHA-256.
It records package name/version, SPDX expression, exact target membership,
available upstream links, release evidence, and every generation input hash.

## Pinned supplemental sources

Published packages occasionally omit terms that their metadata or source
headers reference. OPAP does not redirect those packages to an installed
"companion" package and does not silently treat a license template as
package-specific terms.

Instead, `license-supplements/manifest.json` maps an exact ecosystem/name/version
key to checked-in documents. Each document records:

- its upstream repository, immutable revision, path, and retrieval URL;
- the SHA-256 of the checked-in source bytes;
- whether it is complete license terms or an additional notice; and
- any checked installed README, package metadata, or source-header evidence.

The generator verifies every expected source hash, exact package version, and
declared SPDX expression. Unused manifest entries fail generation so stale
evidence cannot silently accumulate. For `react-remove-scroll-bar@2.3.8`, the
pinned npm tarball and integrity independently identify the MIT declaration
and Anton Korzunov; the complete author-issued terms come from a later
immutable upstream commit and are labeled as a later license clarification.
For `siphasher@1.0.3`, the exact release's installed `COPYING` file is retained
as evidence of its MIT-or-Apache-2.0 choice, but it only points to absent files
and URLs. The complete standard terms are therefore supplied from immutable
SPDX License List sources. Pointer-only files are labeled explicitly and never
count as complete terms; generation fails closed unless complete terms are
available from another verified source.

All installed license files, README sections, evidence files, manifests, and
supplements go through the same containment, realpath, 2 MiB, NUL-byte, and
strict UTF-8 checks. When exact complete terms cannot be independently
verified, normal generation records a version-pinned unresolved exception and
preserves all available package notices. `--check` verifies freshness and then
fails closed while any such exception remains, blocking release CI.

The generated inventory is a release-review input, not proof of legal
compliance. The macOS CI build creates an unsigned `.app` and byte-compares
every required legal resource in `Contents/Resources` with the checked-in
source file.
