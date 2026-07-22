# OPAP acceptance tests

This standalone crate runs synthetic, executable Gherkin scenarios against the
public `opap-core` library and the real CLI entrypoint. It does not use or
persist patient data.

Run it from anywhere in the repository:

```sh
cargo test --manifest-path tests/acceptance/Cargo.toml
```
