# OSCAR compatibility harness

OSCAR-SQL is the behavioral oracle during the Rust rewrite. The pinned baseline
revision is recorded in `oscar-sql-revision.txt`.

## Test strategy

1. Run the OSCAR C++ tests against an anonymized ResMed test-card corpus.
2. Preserve OSCAR's generated session YAML as immutable golden output.
3. Run the Rust importer against the same card directories.
4. Normalize both outputs to the canonical OPAP compatibility schema.
5. Compare exact identifiers, settings, event types, sample counts, and integer
   values. Compare floating-point values using an explicitly documented
   per-field tolerance. Never update a golden file merely to make a failure pass.

The upstream `ResmedTests::testSessionsToYaml` test discovers card directories
under `testdata/resmed/input` and writes session YAML under
`testdata/resmed/output`. The public source checkout does not include those
patient-derived fixtures, so OPAP keeps its corpus outside Git by default.

## Private fixture layout

```text
testdata/private/resmed/<case>/
  card/
    DATALOG/
    STR.edf
    Identification.tgt or Identification.json
  expected/
    machine-info.json
    sessions/*.json      # added when session parsing lands
```

To run the current machine-identification conformance test:

```sh
OPAP_RESMED_FIXTURES="$PWD/testdata/private/resmed" \
  cargo test -p opap-core --test resmed_conformance -- --ignored
```

Raw real-world CPAP cards must not be committed. Only minimal synthetic or
explicitly anonymized fixtures may enter the repository.

