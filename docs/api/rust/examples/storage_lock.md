# Exclusive persistent open (ADR-0097)

Run `cargo run -p relate --example storage_lock --locked`. The
[compiled example](../../../../relate/relate/examples/storage_lock.rs)
opens a temporary Native database, checks that a second independent open
returns `Error::Locked` (`storage locked: …`), retains ownership through a
`Connection` after dropping the `Database`, then reopens successfully once the
connection drops. The temporary directory is removed when the example exits.
