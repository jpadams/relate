//! Engine-side query timing statistics for the differential run: the
//! per-dataset p50/p95/max table, the slowest statements, and the outlier
//! itemization over [`crate::record::QueryResult`] records. One concern,
//! one module (report.rs renders outcomes; this renders their timings).

use crate::record::QueryResult;

/// Render the run's engine timing view: a per-dataset p50/p95/max table
/// over every timed record, the [`SLOWEST_SHOWN`] slowest statements with
/// their stable ids, then every timed statement past [`OUTLIER_MS`] in
/// descending time order. Records without a timing (skips, setup failures,
/// reference errors) are not counted; a single timed query renders its
/// own time for p50/p95/max.
pub(crate) fn render_timings(results: &[QueryResult]) -> String {
    let timed = timed_records(results);
    let rows = timing_rows(&timed);
    if rows.is_empty() {
        return String::new();
    }
    let mut text = String::from("\nENGINE QUERY TIMINGS (ms)\n");
    for (dataset, ms) in rows {
        let stats = dataset_timing_stats(&ms);
        text.push_str(&format!(
            "{dataset}: n={} p50={:.1} p95={:.1} max={:.1}\n",
            stats.n, stats.p50, stats.p95, stats.max
        ));
    }
    let mut slowest = timed;
    slowest.sort_by(|(a_ms, _), (b_ms, _)| b_ms.total_cmp(a_ms));
    text.push_str(&format!("\nSLOWEST QUERIES (top {SLOWEST_SHOWN})\n"));
    for (ms, r) in slowest.iter().take(SLOWEST_SHOWN) {
        text.push_str(&timing_line(*ms, r));
    }
    let flagged: Vec<(f64, &QueryResult)> = slowest
        .iter()
        .filter(|(ms, _)| *ms > OUTLIER_MS)
        .copied()
        .collect();
    if !flagged.is_empty() {
        text.push_str(&format!("\nTIMED QUERIES OVER {OUTLIER_MS:.0} ms\n"));
        for (ms, r) in flagged {
            text.push_str(&timing_line(ms, r));
        }
    }
    text
}

/// The outlier threshold (ms) — the one definition the outlier section's
/// filter and its header share, so the number and the text cannot drift.
const OUTLIER_MS: f64 = 100.0;

/// How many slowest statements the top list shows — the one definition the
/// header and the `take` share.
const SLOWEST_SHOWN: usize = 10;

/// One timed statement's line in the slowest/outlier lists: the measured
/// time, the stable id, and the statement — the one rendering both lists
/// share (F-1-11).
fn timing_line(ms: f64, r: &QueryResult) -> String {
    format!("{ms:9.1}  {}  {}\n", r.id(), r.statement_text())
}

/// The run's timed records as `(ms, record)` pairs — the single definition
/// of "a timed record" (lossless over the `engine_ms` invariant: `None`
/// records are dropped exactly once, here, and never re-read through a
/// default). Every timing view — the per-dataset stats, the slowest list,
/// the outlier list — consumes this one derivation.
fn timed_records(results: &[QueryResult]) -> Vec<(f64, &QueryResult)> {
    results
        .iter()
        .filter_map(|r| r.engine_ms.map(|ms| (ms, r)))
        .collect()
}

/// Per-dataset ascending engine timings over the run's timed records —
/// derived from [`timed_records`] (F-1-9/F-1-11: the `engine_ms`-is-`None`
/// drop happens exactly once, in the one derivation), grouped through
/// [`crate::group_first_seen`] (F-2-3: the one first-seen grouping
/// derivation, shared with the record views). Datasets with no timed
/// record are absent; empty when no record is timed.
fn timing_rows<'a>(timed: &[(f64, &'a QueryResult)]) -> Vec<(&'a str, Vec<f64>)> {
    let mut groups = crate::group_first_seen(timed.iter().map(|(ms, r)| (r.dataset.as_str(), *ms)));
    for (_, times) in &mut groups {
        times.sort_by(|a, b| a.total_cmp(b));
    }
    groups
}

/// One dataset's timing stats, named (never a positional tuple of three
/// same-typed f64s).
struct DatasetStats {
    n: usize,
    p50: f64,
    p95: f64,
    max: f64,
}

/// The per-dataset stats over one dataset's ascending timings — the
/// single derivation both renderers consume, so a new stat lands in both
/// with one edit.
fn dataset_timing_stats(ascending: &[f64]) -> DatasetStats {
    DatasetStats {
        n: ascending.len(),
        p50: rank_of(ascending, 0.50),
        p95: rank_of(ascending, 0.95),
        max: ascending[ascending.len() - 1],
    }
}

/// Nearest-rank value over a non-empty **already-ascending** slice —
/// no sort, no copy, just the index — via the workspace's one
/// nearest-rank definition (relate-test-support; the sorting
/// `percentile` beside it is the general form).
fn rank_of(ascending: &[f64], p: f64) -> f64 {
    ascending[relate_test_support::nearest_rank(ascending.len(), p)]
}

/// The step summary's engine-timing markdown section: a per-dataset
/// `queries / p50 / p95 / max` table over the same [`timed_records`]
/// derivation the run report's text table renders, so the two views cannot
/// disagree. Empty when no record is timed.
pub(crate) fn summary_markdown_section(results: &[QueryResult]) -> String {
    let timings = timing_rows(&timed_records(results));
    if timings.is_empty() {
        return String::new();
    }
    let mut md = String::from("\n## Engine query timings (ms)\n\n");
    md.push_str("| dataset | queries | p50 | p95 | max |\n| --- | ---: | ---: | ---: | ---: |\n");
    for (dataset, ms) in timings {
        let stats = dataset_timing_stats(&ms);
        md.push_str(&format!(
            "| {dataset} | {} | {:.1} | {:.1} | {:.1} |\n",
            stats.n, stats.p50, stats.p95, stats.max
        ));
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::fixtures::record;
    use crate::record::Outcome;
    use crate::record::SkipReason;

    /// A timed record: [`record`]'s shape with the engine measurement.
    fn timed_record(outcome: Outcome, engine_ms: f64) -> QueryResult {
        QueryResult {
            engine_ms: Some(engine_ms),
            ..record(outcome)
        }
    }

    /// The timing renderers cover every timed record — a dataset's
    /// p50/p95/max over its ascending timings, the slowest statements
    /// first (a failed run's timing included), the outlier section past
    /// the threshold — and a run with no timed records renders no timing
    /// section at all.
    #[test]
    fn timing_renders_per_dataset_stats_and_slowest_queries() {
        let results = vec![
            timed_record(Outcome::Pass, 1.0),
            timed_record(Outcome::Pass, 5.0),
            timed_record(Outcome::PassWithOrderNote("list order".into()), 10.0),
            record(Outcome::Skipped(SkipReason::Gds)),
            QueryResult {
                dataset: "other".to_string(),
                line: None,
                statement: Some("pull verification: MATCH (n) RETURN count(n) AS c".to_string()),
                outcome: Outcome::Pass,
                engine_ms: Some(2.0),
                apoc: false,
            },
        ];
        let text = render_timings(&results);
        assert!(text.contains("ENGINE QUERY TIMINGS (ms)"));
        // ds: 1.0, 5.0, 10.0 → p50 5.0, p95 10.0, max 10.0
        assert!(text.contains("ds: n=3 p50=5.0 p95=10.0 max=10.0"));
        // other: a single timing renders its own time for all three stats.
        assert!(text.contains("other: n=1 p50=2.0 p95=2.0 max=2.0"));
        // Slowest first: 10.0 before 5.0, and the skipped record is absent.
        let slowest_marker = format!("\nSLOWEST QUERIES (top {SLOWEST_SHOWN})\n");
        let slowest = text.split(slowest_marker.as_str()).nth(1).unwrap();
        assert!(slowest.starts_with("     10.0  ds:7:"));
        assert!(!slowest.contains("SKIP"));
        // The outlier section lists every timed statement past the
        // threshold, in the same descending order; none of the 1–10 ms
        // records above crosses it, so it renders only when one does.
        let outlier_marker = format!("TIMED QUERIES OVER {OUTLIER_MS:.0} ms\n");
        assert!(!text.contains(&outlier_marker));
        let flagged = render_timings(&[
            timed_record(Outcome::Pass, 250.0),
            timed_record(Outcome::Pass, 5.0),
            // A failed engine run carries its measured time too.
            timed_record(Outcome::Fail("relate error".into()), 300.0),
        ]);
        let over = flagged.split(outlier_marker.as_str()).nth(1).unwrap();
        assert!(over.starts_with("    300.0  ds:7:"));
        assert!(!over.contains("    5.0  "));
        // A run with no timed records renders no timing section at all.
        assert_eq!(render_timings(&[record(Outcome::Pass)]), "");
    }

    /// The step-summary markdown section renders the same per-dataset
    /// stats the run report's text table renders — one derivation, two
    /// views — and nothing when no record is timed.
    #[test]
    fn summary_markdown_section_matches_the_text_stats() {
        let results = vec![
            timed_record(Outcome::Pass, 4.0),
            timed_record(Outcome::Pass, 2.0),
        ];
        let md = summary_markdown_section(&results);
        assert!(md.contains("## Engine query timings (ms)"));
        assert!(md.contains("| ds | 2 | 2.0 | 4.0 | 4.0 |"));
        assert_eq!(summary_markdown_section(&[record(Outcome::Pass)]), "");
    }
}
