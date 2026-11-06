# 0049. Gate the merged product in-tree

- **Status**: Accepted
- **Date**: 2026-09-16
- **Deciders**: CypherLite / MemoryDB merge

## Context

The merge harness originally executed tests in the read-only
`../cyperlite-original` and `../memorydb` source trees while the in-tree
runtime was incomplete. The merged runtime now exceeds the original CIP
scenario set and passes all 3897 openCypher TCK scenarios.

Continuing to execute sibling source trees makes the gate depend on checkouts
that are not part of the product. It also makes the gate permanently fail on
the known source-MemoryDB self-relationship scenario even though that defect
is fixed in-tree.

## Decision

`make gate` tests only the merged `cypherlite/` workspace:

1. legacy CypherLite surface tests and its CIP reference;
2. MemoryDB runtime, front-end, and 3897/3897 TCK;
3. strict MemoryDB-to-original scenario parity;
4. formatting and Clippy;
5. frozen load-test comparisons.

Sibling trees remain read-only historical baselines and are not runtime gate
dependencies.

## Alternatives considered

- Keep running both sibling trees — rejected because their known failures and
  environment drift do not describe the merged product.
- Patch the source MemoryDB checkout — rejected because source trees are
  immutable merge inputs.
- Allow the source TCK failure in the gate — rejected because the in-tree gate
  has the stronger zero-failure requirement.

## Consequences

- The gate can run from a standalone merged checkout.
- Original behavior remains protected by the in-tree GraphStore reference and
  the scenario-identity parity test established by ADR-0047.
- Source baselines are re-recorded only through an explicit later decision.
