# Private OSCAR oracle workflow

This workflow keeps real CPAP cards, patient-derived manifests, external
OSCAR build products, and private adapter work outside the OPAP repository.
The pinned oracle is `CrimsonNape/OSCAR-code` at
`64c5e90a26f91fb15868bcfcccde0c1e1522ac86`. Do not replace it with the
similarly named OSCAR-SQL repository.

## Prepare reproducible trees

1. Put private cards in an encrypted, access-controlled directory outside this
   checkout. Mount or open each card read-only. Use a random case identifier,
   never a patient name, device serial, source directory name, or care-provider
   identifier.
2. Clone OSCAR-code into a separate external working directory and detach the
   exact commit:

   ```sh
   git clone https://gitlab.com/CrimsonNape/OSCAR-code.git /tmp/opap-oscar-oracle
   git -C /tmp/opap-oscar-oracle checkout --detach \
     64c5e90a26f91fb15868bcfcccde0c1e1522ac86
   ```

3. Keep the OSCAR exporter and OPAP exporter in separate, reviewed Git
   repositories. Only non-PHI adapter code and synthetic conformance vectors
   belong there. Commit every change before a real-card comparison.
4. At the exact OPAP and adapter commits to test, verify all four trees and
   capture the output locally:

   ```sh
   compat/scripts/verify-compat-trees.sh \
     /tmp/opap-oscar-oracle \
     /path/to/oracle-export-adapter \
     /path/to/clean-opap-checkout \
     /path/to/subject-export-adapter
   ```

The verifier rejects dirty or untracked trees, enforces the OSCAR pin, prints
all four revisions, hashes each clean Git archive, and prints the digest of the
checked-in source, aggregate, waveform, and synthetic manifest conformance
vectors. Record the appropriate adapter revision/archive digest and the common
vector-set digest in each manifest.

`adapter_attestation: synthetic_fixture_only` is for generated fixtures. A
manifest with `fixture.synthetic: false` is invalid unless its adapter uses
`verified_clean_tree`; `adapter_clean` must always be `true`. This is necessary
but not sufficient evidence: all attestation fields are self-asserted JSON.
Before trusting them, independently reproduce the verifier output, inspect the
adapter commit, and observe each adapter passing the public digest vectors. Do
not mark a tree verified because someone copied a digest into a manifest.

## Export and compare

1. Build OSCAR in its external checkout using its documented build process and
   run the relevant upstream C++ tests. Load the private card without modifying
   it.
2. Have the reviewed oracle adapter export OSCAR's loaded machine, sessions,
   slices, per-session summaries, settings, events, timing metadata, and
   provenance to `opap-oscar-compat/v1`. It must feed OSCAR's complete decoded
   digital and final physical waveform streams into the semantic digest without
   rereading the card to manufacture the result. The manifest stores that
   full-stream digest and only the prescribed head/tail waveform previews; all
   non-waveform ordered records remain complete.
3. Run the pinned OPAP checkout against the same read-only card and export the
   same contract with the subject adapter. Keep all output under the encrypted
   private corpus, for example:

   ```text
   PRIVATE_CORPUS/random-case-id/card/       # read-only; never committed
   PRIVATE_CORPUS/random-case-id/oscar.json  # never committed
   PRIVATE_CORPUS/random-case-id/opap.json   # never committed
   PRIVATE_CORPUS/random-case-id/provenance/ # verifier output; never committed
   ```

4. Validate both documents, then compare OSCAR on the expected/left and OPAP on
   the actual/right:

   ```sh
   cargo run --manifest-path compat/Cargo.toml -- \
     validate "$PRIVATE_CORPUS/random-case-id/oscar.json"
   cargo run --manifest-path compat/Cargo.toml -- \
     validate "$PRIVATE_CORPUS/random-case-id/opap.json"
   cargo run --manifest-path compat/Cargo.toml -- \
     compare \
     "$PRIVATE_CORPUS/random-case-id/oscar.json" \
     "$PRIVATE_CORPUS/random-case-id/opap.json"
   ```

Required collection objects may explicitly contain zero summary metrics,
settings, event channels/events, or waveform channels when that is the true
source result. Empty does not mean unknown or ignored: if OSCAR contains a
member that OPAP omits, comparison fails. Empty sessions, empty slice lists,
missing required members, and missing digests fail closed.

## Privacy gate

Treat raw cards and both manifests as sensitive even when the CLI redacts
values and paths in ordinary diagnostics. Keep commands and logs local; never
paste a private manifest, digest preimage containing source metadata, absolute
private path, or failure log into an issue, chat, CI artifact, or pull request.
Do not run real-card comparisons in public CI.

Commit a fixture only when it is wholly generated, or after publication
consent and an independent re-identification review. Before any review, remove
or replace names, patient and clinician IDs, serial numbers, free text, exact
device timestamps, filesystem metadata, and unused EDF header fields. Deleting
obvious names is not sufficient anonymization. Prefer generated cards and
synthetic source identities.

Before every commit, inspect staged paths and diffs and confirm that
`testdata/private`, generated manifests, external checkouts, and build products
are absent. If any private material was staged, unstage it and follow the
project's incident process rather than trying to sanitize Git history ad hoc.

## Interpretation boundary

A passing comparison demonstrates agreement only for the pinned program and
adapter revisions, the reviewed fixtures, and the v1 fields. It is not a claim
of medical accuracy, support for every device/firmware, or full OSCAR feature
parity. V1 also excludes noon-boundary/day assignment, daily aggregation, and
cross-session analytics such as daily AHI/RDI and percentiles; test those only
with a separate pinned oracle contract.
