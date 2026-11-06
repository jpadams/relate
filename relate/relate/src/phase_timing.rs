//! Always-on per-phase statement timing (Neo4j timing alignment, LABS-832):
//! the PARSE / PLAN / EXECUTE split of the ADR-0221 wall-clock `elapsed`.
//!
//! The phases partition the same measured statement body
//! [`crate::ResultHeader::elapsed`] spans: PARSE (the front-end parse and
//! lowering), PLAN (binding, logical planning, admission classification),
//! EXECUTE (the dispatch — or, for `EXPLAIN`, the plan render). The stages
//! between them (the post-EXPLAIN gates, token reservation) stay in the
//! total alone, so the three durations are disjoint sub-windows and their
//! sum never exceeds `elapsed`.
//!
//! The struct carries no `merge` arm: no production site merges phases —
//! `elapsed` and `stats` are per-statement at every multi-statement site
//! in the tree, and the one `QueryStats::merge` call is the per-batch
//! merge inside a single ADR-0209 batched bulk write, one statement whose
//! phases are stamped once. A merged-result surface re-adds the arm in
//! its own change, together with its production caller.

use std::time::Duration;
use web_time::Instant;

/// A statement's per-phase timings (LABS-832): how the ADR-0221
/// wall-clock total splits across the pipeline's three stages,
/// measured always-on — no PROFILE opt-in, no feature flag.
///
/// Home: this crate (the facade), because the parse/plan/execute
/// boundary is the facade's statement pipeline —
/// `Connection::admit_statement` and `Connection::dispatch` in
/// `connection/dispatch.rs`, the same sites that measure the
/// ADR-0221 `elapsed`. The executor receives a finished logical
/// plan, so no lower crate can host the split. `pub(crate)`
/// permanently: the public exposure (ADR-0491) is the accessor
/// trio `QueryResult::parse_ms` / `plan_ms` / `execute_ms` over
/// the header's copy — one public path, never a `pub` type beside
/// them. The fields are private to this module: outside it, the
/// split is only read (through the phase accessors below) or
/// stamped (through [`TimingStamp::stamped`]), never constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhaseTimings {
    /// The PARSE stage: the front-end parse and lowering of the statement
    /// text (`parse_and_lower_statement_with_policy_and_params`).
    parse: Duration,
    /// The PLAN stage: binding and logical planning with the admission
    /// classification (`plan_gate_classify`).
    plan: Duration,
    /// The EXECUTE stage: the dispatch — a transaction's overlay run, an
    /// auto-commit write, or a snapshot read. For `EXPLAIN` this is the
    /// plan render: the wrapped statement never executes, so the render
    /// is that path's execute-equivalent stage.
    execute: Duration,
}

impl PhaseTimings {
    /// The one construction — structurally: the fields are private to this
    /// module, so no code outside it can build a split. Called only by
    /// [`TimingStamp::stamped`], at the statement's stamping tail: PARSE
    /// and PLAN as admission measured them, EXECUTE as the statement's
    /// third stage (the dispatch, or the EXPLAIN render) measured it. A
    /// split is never built half-measured, so no stage ever carries a
    /// zero placeholder for work still ahead of it.
    fn complete(parse: Duration, plan: Duration, execute: Duration) -> Self {
        Self {
            parse,
            plan,
            execute,
        }
    }

    /// The PARSE stage's measured duration.
    pub(crate) fn parse(&self) -> Duration {
        self.parse
    }

    /// The PLAN stage's measured duration.
    pub(crate) fn plan(&self) -> Duration {
        self.plan
    }

    /// The EXECUTE stage's measured duration.
    pub(crate) fn execute(&self) -> Duration {
        self.execute
    }
}

/// The statement's measured timing as one stamp: the ADR-0221 wall-clock
/// total (`elapsed`) together with its always-on PARSE / PLAN / EXECUTE
/// split (`phases`, LABS-832). Bundled like `ResultStamp` — so a
/// result-metadata construction site cannot stamp the total without its
/// split, and the header constructions stay under
/// `clippy::too_many_arguments`.
pub(crate) struct TimingStamp {
    /// The ADR-0221 wall-clock total — what [`crate::ResultHeader::elapsed`]
    /// announces.
    pub(crate) elapsed: Duration,
    /// The total's PARSE / PLAN / EXECUTE split.
    pub(crate) phases: PhaseTimings,
}

impl TimingStamp {
    /// The one spelling of the LABS-832 stamping policy (ADR-0491): the
    /// total is measured from the statement's start instant
    /// (`started.elapsed()`), PARSE and PLAN are the durations admission
    /// measured, and EXECUTE is this stage's own window
    /// (`execute_started.elapsed()` — the dispatch, or the EXPLAIN plan
    /// render). Called at both stamping tails (the dispatch tail and the
    /// lazy closure inside the EXPLAIN stage), so the policy lives here
    /// once instead of being re-spelled per tail. The windows close in a
    /// fixed order — EXECUTE first, the total last — so the total's
    /// window structurally contains the disjoint sub-windows and their
    /// sum never exceeds `elapsed`.
    pub(crate) fn stamped(
        started: Instant,
        execute_started: Instant,
        parse: Duration,
        plan: Duration,
    ) -> Self {
        let execute = execute_started.elapsed();
        Self {
            elapsed: started.elapsed(),
            phases: PhaseTimings::complete(parse, plan, execute),
        }
    }
}

/// The sample statements every phase-timing test runs — one per
/// phase-carrying path (a plain read, a property read, an aggregate, an
/// `EXPLAIN` whose render is the EXECUTE stage, a `PROFILE` with its
/// epilogue) over the one-node fixture each test sets up. Consulted by
/// this module's Duration-level test and `result`'s accessor-level test,
/// so the corpus has one spelling.
#[cfg(test)]
pub(crate) const SAMPLE_STATEMENTS: [&str; 5] = [
    "RETURN 1 AS one",
    "MATCH (n:Person) WHERE n.name = 'a' RETURN n.name",
    "MATCH (n) RETURN count(n) AS people",
    "EXPLAIN MATCH (n:Person) RETURN n",
    "PROFILE MATCH (n:Person) RETURN n.name",
];

#[cfg(test)]
mod tests {
    use super::SAMPLE_STATEMENTS;
    use crate::Database;

    /// Invariant: the phases partition the measured window — PARSE, PLAN
    /// and EXECUTE are disjoint sub-windows of the statement body the
    /// ADR-0221 `elapsed` spans (the between-stages work stays in the
    /// total alone), so their sum never exceeds `elapsed`. Pinned over
    /// sample statements covering every phase-carrying path: a plain
    /// read, a write, an aggregate, an `EXPLAIN` (render as the EXECUTE
    /// stage) and a `PROFILE` (normal dispatch plus the epilogue).
    #[test]
    fn phase_sum_never_exceeds_elapsed_for_sample_statements() {
        let db = Database::open(":memory:").unwrap();
        let mut conn = db.connect().unwrap();
        conn.query("CREATE (:Person {name: 'a'})").unwrap();
        for statement in SAMPLE_STATEMENTS {
            let result = conn.query(statement).unwrap();
            let phases = result.header.phases;
            let sum = phases.parse + phases.plan + phases.execute;
            assert!(
                sum <= result.header.elapsed,
                "{statement}: parse {phases:?} sum {sum:?} exceeds elapsed {:?}",
                result.header.elapsed
            );
        }
    }
}
