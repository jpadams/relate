//! Always-on per-phase statement timing (ADR-0221 total, ADR-0491 split):
//! every `QueryResult` reports its statement's elapsed split across PARSE
//! (front-end parse and lowering), PLAN (binding, logical planning,
//! admission classification) and EXECUTE (the dispatch — or, for EXPLAIN,
//! the plan render), measured once around `Connection::run_query`.
//!
//! Run with `cargo run -p relate --example phase_timing`.

use relate::{Database, Result};

fn main() -> Result<()> {
    let db = Database::open(":memory:")?;
    let mut conn = db.connect()?;

    // Plain runs, EXPLAIN and PROFILE all carry the split, always on.
    for stmt in [
        "CREATE (n:Person {name: 'Alice'})",
        "MATCH (n:Person) RETURN n.name AS name",
        "EXPLAIN MATCH (n:Person) RETURN n",
        "PROFILE MATCH (n:Person) RETURN n.name AS name",
    ] {
        let result = conn.query(stmt)?;
        let (parse, plan, execute, elapsed) = (
            result.parse_ms(),
            result.plan_ms(),
            result.execute_ms(),
            result.elapsed_ms(),
        );
        // The phases partition the measured window: the sum never exceeds
        // the total (the inter-stage gaps stay in the total alone).
        assert!(
            parse + plan + execute <= elapsed,
            "phases must partition the elapsed window, got parse {parse} + \
             plan {plan} + execute {execute} > elapsed {elapsed} for {stmt}",
        );
        println!(
            "{stmt} -> elapsed {elapsed:.3}ms (parse {parse:.3} / plan {plan:.3} / execute {execute:.3})"
        );

        // The wire envelope's timing block: the one projection bundling the
        // total and its split into the stamp the canonical envelope builder
        // takes (`wire::TimingStampMs`, ADR-0491) — the same four numbers,
        // never a second conversion.
        let stamp = relate::timing_stamp_ms(&result);
        assert_eq!(stamp.parse_ms, parse);
        assert_eq!(stamp.plan_ms, plan);
        assert_eq!(stamp.execute_ms, execute);
        assert_eq!(stamp.elapsed_ms, elapsed);
    }
    Ok(())
}
