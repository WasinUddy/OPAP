# OPAP acceptance tests

This standalone crate runs synthetic, executable Gherkin scenarios against the
public `opap-core` library, the real CLI entrypoint, and the framework-neutral
`opap-service` boundary. It does not use patient data. Temporary fixtures cover
card detection, machine identification, privacy-safe source inspection, and
durable blocked/cancelled job states. The scenarios explicitly prove that
session import is unavailable; they do not claim session import or OSCAR
session parity.

Run it from anywhere in the repository:

```sh
cargo test --manifest-path tests/acceptance/Cargo.toml --locked
```
