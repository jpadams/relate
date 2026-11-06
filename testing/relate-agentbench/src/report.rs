//! The report: the honest-shape table DuckDB published (the token win
//! and the non-wins together).

use std::fmt::Display;

use relate_test_support::percentile;

use crate::arms::Arm;
use crate::harness::RunStats;

/// Per arm, the distribution of every metric over all question × rep
/// runs — the p25/median/p75 through the canonical rank rule, not the
/// median alone — plus the correctness count. The run-to-run variance
/// (prompt-cache effects swing the token counts several-fold between
/// reps of the same arm) is larger than the arms' differences at small
/// result sizes, so a bare median reads as flat where the distribution
/// shows the spread; the whole table is the honest shape.
pub(crate) fn report(arms: &[Arm], stats: &[RunStats]) {
    println!("\n| arm | tool-output bytes p25/med/p75 | input tokens p25/med/p75 | turns p25/med/p75 | seconds p25/med/p75 | correct |");
    println!("| --- | --- | --- | --- | --- | --- |");
    for arm in arms {
        let label = arm.label();
        let runs: Vec<&RunStats> = stats.iter().filter(|s| &s.arm == arm).collect();
        assert!(
            !runs.is_empty(),
            "arm {label} has no runs — a requested A/B arm cannot vanish from the report"
        );
        let bytes = render(metric(&runs, |s| s.tool_output_bytes));
        let input = render(metric(&runs, |s| s.input_tokens));
        let turn_dist = render(metric(&runs, |s| s.turns));
        // The one float keeps its one-decimal shape; the counts print
        // as themselves.
        let secs = {
            let t = metric(&runs, |s| s.seconds);
            format!("{:.1}/{:.1}/{:.1}", t.0, t.1, t.2)
        };
        let correct = runs.iter().filter(|s| s.correct).count();
        println!(
            "| {label} | {bytes} | {input} | {turn_dist} | {secs} | {correct}/{} |",
            runs.len()
        );
    }
    println!(
        "\n(complete per-run rows: {})",
        serde_json::to_string(stats).expect("the per-run rows serialize")
    );
}

/// One metric's distribution over the arm's runs, as the
/// `(p25, median, p75)` triple — the sort-and-rank scaffolding lives
/// once, in the workspace's canonical [`percentile`].
fn metric<T, F>(runs: &[&RunStats], get: F) -> (T, T, T)
where
    T: PartialOrd + Clone,
    F: Fn(&RunStats) -> T,
{
    let values: Vec<T> = runs.iter().map(|s| get(s)).collect();
    (
        percentile(&values, 0.25),
        percentile(&values, 0.50),
        percentile(&values, 0.75),
    )
}

/// A count metric's triple, joined with the distribution's `/`.
fn render<T: Display>(triple: (T, T, T)) -> String {
    format!("{}/{}/{}", triple.0, triple.1, triple.2)
}
