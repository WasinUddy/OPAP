# OPAP acceptance tests

This standalone crate runs synthetic, executable Gherkin scenarios against the
public `opap-core` library, the real CLI entrypoint, and the framework-neutral
`opap-service` boundary. It does not use patient data. Temporary fixtures cover
card detection, machine identification, serial-verified STR intervals,
STR-only and STR-plus-BRP core imports, detail fallback, privacy-safe source
inspection, and durable blocked/cancelled job states. The service/native
scenarios explicitly prove that application-level durable session import is
still unavailable; the direct core scenarios do not claim full OSCAR parity.

Run it from anywhere in the repository:

```sh
cargo test --manifest-path tests/acceptance/Cargo.toml --locked
```
