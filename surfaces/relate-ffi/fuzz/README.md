# Fuzz harnesses for the C ABI (LABS-937, OPLANE_REQ-00340357)

Two `cargo-fuzz` targets drive the ABI exactly as a C client does — the
declarations in `src/lib.rs` mirror `include/relate.h` (keep them in
step), and every buffer the harness passes has a matching `len`/`dim`, so
the trusted-length contract holds by construction and the targets hunt
for panics, poisons and non-defined refusals instead:

- `params_builder` — an arbitrary sequence of putters (scalar, temporal,
  spatial, vector, nested containers) over one builder, freed at the end.
- `query_params` — one in-memory database, one connection, one statement
  with one string parameter; both error channels are read back and every
  handle is closed in order.

This crate is detached from the workspace on purpose (an empty
`[workspace]`): it builds only under `cargo fuzz`, never in the
workspace gates, and its dependency tree therefore is not MSRV-pinned.

Run (needs a nightly toolchain — `-Zsanitizer` and the sanitizer-coverage
flags require it):

```
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz
cd surfaces/relate-ffi/fuzz
cargo +nightly fuzz run params_builder -- -max_total_time=300
cargo +nightly fuzz run query_params   -- -max_total_time=300
```

Note: on some arm64 macOS toolchains the libFuzzer link (sancov +
`-Zsanitizer=address` against this crate's `staticlib`/`cdylib`/`rlib`
link) fails with `symbol(s) not found`; run with `--sanitizer none` or
from a Linux box if that happens — the harness itself is toolchain-
agnostic. The caps (ADR-0514) guarantee a runaway putter sequence
terminates in a defined refusal rather than unbounded growth.
