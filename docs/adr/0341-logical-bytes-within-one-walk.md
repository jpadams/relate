# ADR-0341: logical_bytes_within is the one early-stopping logical-bytes walk

Status: Proposed
Claim: d763114fcde5450741237c7c3a67b529829120dc

## Context

ADR-0042 introduced the early-stopping logical-bytes walk as
`logical_bytes_exceed(values, limit) -> bool` for the Bolt result cap.
ADR-0311 needed the summed total under a connection-wide cap and added
`logical_bytes_within(values, limit) -> Option<u64>`; `logical_bytes_exceed`
was reduced to a one-line wrapper over it (`within(..).is_none()`) and the
Bolt server migrated to `logical_bytes_within`. `logical_bytes_exceed`
now has no production caller — only its own tests and the
`cypherlite_core` re-export keep it alive — and its doc still names the
Bolt cap as its caller, which ADR-0311 made false.

## Decision

Delete `logical_bytes_exceed` and its `cypherlite-core` re-export.
`logical_bytes_within` is the one canonical walk; it carries the
early-stopping description and the shared-payload counting note. The
`value/tests.rs` cases that pinned `logical_bytes_exceed` are retargeted
to `logical_bytes_within` (`Some(total)` within, `None` over), including
the early-stop case.

## Alternatives considered

- **Keep the wrapper as a convenience alias.** Rejected: it is a second
  public name for one policy with a stale doc, and no caller uses it;
  AGENTS.md's "one canonical helper per policy" applies.
- **Deprecate instead of delete.** Rejected: the crate has no external
  published consumers of this helper (the Bolt server was the last one and
  moved in ADR-0311), so a deprecation window would keep dead API alive
  for nobody.

## Consequences

- `cypherlite_core::logical_bytes_exceed` disappears from the crate root;
  `logical_bytes_within` is the documented walk. `cypherlite` never
  re-exported it.
- Tests: `value/tests.rs` covers the same values through
  `logical_bytes_within`.
