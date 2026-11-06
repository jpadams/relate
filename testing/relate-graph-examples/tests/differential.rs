//! The differential run (ADR-0144). Opt-in via `RELATE_GRAPH_EXAMPLES=1`
//! (the nightly workflow sets it); without it the test self-skips so
//! regular `cargo test` never boots docker or downloads dumps.
//!
//! Requires: docker (reference container), `RELATE_BIN` (the built CLI),
//! network (dumps/adocs on first run, docker image). Dumps are cached and
//! sha-pinned in `target/graph-examples/` (override with
//! `RELATE_GRAPH_EXAMPLES_CACHE`). `RELATE_GRAPH_EXAMPLES_DATASETS`
//! (comma-separated manifest names) runs a subset, in manifest order; an
//! unknown name fails before any docker command. Opt-in datasets (twitch —
//! 465 MB dump) are excluded from the default run and run only when their
//! flag is `1` (`RELATE_GRAPH_EXAMPLES_TWITCH=1`) or when named in the
//! filter: naming them is an explicit opt-in.
//!
//! The run is report-only (ADR-0176): DIFF, order-only, skip and
//! inconclusive reference-timebox outcomes are reported, never gated on.
//! Only infrastructure failures fail the run — a dataset setup failure
//! (kind 1, and kind 4's no-runnable-statements error), a pull-verification
//! count mismatch (kind 3), and a reference-boot failure (kind 2, the panic
//! in [`boot_reference`]). Kinds 1, 3 and 4 all surface as the
//! [`record::Outcome::SetupFailed`] variant, which [`verdict`] gates on.

use std::path::{Path, PathBuf};

use relate_graph_examples::{cache, config, docker, manifest, record, report, suite};

fn main_gate() -> bool {
    config::env_flag("RELATE_GRAPH_EXAMPLES")
}

fn apoc_enabled() -> bool {
    config::env_flag("RELATE_GRAPH_EXAMPLES_APOC")
}

/// Preflight gates. The suite self-skips unless `RELATE_GRAPH_EXAMPLES=1`;
/// a gated run without docker or the CLI binary fails loudly. Returns the
/// selected datasets: `RELATE_GRAPH_EXAMPLES_DATASETS` (comma-separated
/// manifest names) restricts the run to a subset, resolved before any
/// docker command — an unknown name panics naming the value and listing
/// the valid names.
fn preflight() -> Option<Vec<&'static manifest::Dataset>> {
    if !main_gate() {
        eprintln!(
            "skipping: set RELATE_GRAPH_EXAMPLES=1, RELATE_BIN=<built cli>, \
             and ensure docker to run the graph-examples differential suite"
        );
        return None;
    }
    let datasets =
        config::dataset_filter("RELATE_GRAPH_EXAMPLES_DATASETS").unwrap_or_else(|e| panic!("{e}"));
    for &dataset in &datasets {
        if let Some(flag) = dataset.opt_in_env {
            eprintln!(
                "{}: opted in ({flag}=1 or named in the dataset filter)",
                dataset.name
            );
        }
    }
    for dataset in manifest::DATASETS
        .iter()
        .filter(|d| d.opt_in_env.is_some() && !datasets.iter().any(|&s| s.name == d.name))
    {
        eprintln!(
            "{}: skipped by default; set {}=1 to include it",
            dataset.name,
            dataset.opt_in_env.unwrap_or_default()
        );
    }
    if !docker::docker_available() {
        panic!("RELATE_GRAPH_EXAMPLES=1 but docker is not available");
    }
    if relate_test_support::relate_bin().is_none() {
        panic!("{}", relate_test_support::relate_bin_missing_message());
    }
    Some(datasets)
}

/// Fetch (or verify from cache) every selected dump, then boot the reference
/// container with all of them pre-loaded: the reference volume is loaded
/// before the server boots (loading under a running server quarantines the
/// store).
///
/// A boot failure panics: with no reference, no result is interpretable —
/// ADR-0176 infrastructure failure kind 2.
fn boot_reference(datasets: &[&'static manifest::Dataset]) -> docker::DatasetTest {
    let mut dumps: Vec<(String, PathBuf)> = Vec::new();
    for &dataset in datasets {
        let path = cache::ensure_dump(dataset, true)
            .unwrap_or_else(|e| panic!("{} dump: {e}", dataset.name));
        dumps.push((dataset.name.to_string(), path));
    }
    docker::start_with_dumps(&dumps).unwrap_or_else(|e| {
        // ADR-0204: capture whatever the half-booted reference logged
        // before the panic's unwind removes the container (the armed
        // CleanupGuard's Drop), or the workflow's capture step finds
        // nothing.
        docker::capture_reference_logs(&cache::cache_root());
        panic!("reference container: {e}")
    })
}

/// Shared-index work queue (F-1-3): `width` scoped workers pull the next
/// item index from one atomic counter, so a worker that finishes picks up
/// the next item immediately — one straggler never idles the other slots
/// behind a wave barrier the way `chunks(width)` did. One output entry
/// per item; order is per-worker completion, not input order.
fn work_queue<T: Sync, R: Send>(
    items: &[T],
    width: usize,
    work: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..width.min(items.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    while let Some(item) =
                        items.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                    {
                        mine.push(work(item));
                    }
                    mine
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|h| h.join().expect("worker panicked"))
            .collect()
    })
}

/// Run every selected dataset with bounded concurrency. The datasets are
/// independent (each has its own reference database, pulled store, and
/// destination), but every one drives the same reference container —
/// running all of them at once starves it (and its heap) on the larger
/// sandbox sets. One dataset's infrastructure error becomes that dataset's
/// `Err` entry; the others' results are kept.
fn run_datasets_bounded(
    workdir: &Path,
    reference: &docker::DatasetTest,
    datasets: &[&'static manifest::Dataset],
    apoc_enabled: bool,
) -> Vec<(String, Result<Vec<record::QueryResult>, String>)> {
    // Default 4 assumes a release build (the nightly workflow builds
    // --release): a debug build's slower ingest under 4-way dataset
    // concurrency can stall a pull's read loop past the server's
    // read-stall window (~400-500 s, measured). Docs: docs/datasets.md
    // env table.
    let width = config::env_or("RELATE_GRAPH_EXAMPLES_CONCURRENCY", 4) as usize;
    work_queue(datasets, width, |&dataset| {
        eprintln!("{}: running", dataset.name);
        let out = suite::run_dataset(
            reference,
            dataset,
            workdir,
            // Named fields (F-1-13): the two flags cannot be swapped
            // silently the way adjacent bare bools can.
            suite::RunOptions {
                fetch: true,
                apoc_enabled,
            },
        );
        eprintln!(
            "{}: {}",
            dataset.name,
            match &out {
                Ok(r) => format!("{} statements done", r.len()),
                Err(e) => format!("setup failed: {e}"),
            }
        );
        (dataset.name.to_string(), out)
    })
}

/// Fold per-dataset outcomes into the run's record list: a dataset-level
/// infrastructure error (ADR-0176 kinds 1 and 4) becomes one setup-failed
/// record, so one bad dataset loses none of the others' results (and the
/// report still gets written). The setup-failed record carries only real
/// data (F-1-4): no adoc line and no statement exist for it, so both are
/// `None` — the error text is the outcome's.
fn fold_outcomes(
    outcomes: Vec<(String, Result<Vec<record::QueryResult>, String>)>,
) -> Vec<record::QueryResult> {
    let mut all = Vec::new();
    for (name, result) in outcomes {
        match result {
            Ok(results) => all.extend(results),
            // A dataset-level failure is one record built through the
            // record model's statement-less constructor: no line, no
            // statement, no engine run (F-2-2).
            Err(e) => all.push(record::QueryResult::dataset_level(
                &name,
                record::Outcome::SetupFailed(e),
            )),
        }
    }
    all
}

/// Write the run's report artifacts for the nightly upload — the full
/// report and, when the apoc capture renderer has apoc-classified records
/// to render (F-1-11: the renderer owns that decision), the apoc capture —
/// and return the full report's path.
fn write_reports(
    workdir: &Path,
    all_results: &[record::QueryResult],
    summary: &record::Summary,
    apoc_enabled: bool,
) -> PathBuf {
    let report_path = workdir.join("report.txt");
    std::fs::write(&report_path, report::render_run(all_results, summary)).expect("writing report");
    eprintln!("{} (report: {})", summary.line(), report_path.display());
    append_step_summary(all_results, summary);

    if let Some(capture) = report::render_apoc_capture(all_results, apoc_enabled) {
        let apoc_path = workdir.join("apoc-report.txt");
        std::fs::write(&apoc_path, capture).expect("writing apoc report");
        eprintln!("apoc capture written (report: {})", apoc_path.display());
    }
    report_path
}

/// Append the markdown run summary (summary line + per-dataset rollup) to
/// GitHub's step summary file. Silent no-op when `GITHUB_STEP_SUMMARY` is
/// unset (local runs); an unwritable file warns, never fails the run.
fn append_step_summary(all_results: &[record::QueryResult], summary: &record::Summary) {
    let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY") else {
        return;
    };
    let markdown = report::render_summary_markdown(all_results, summary);
    let result = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, markdown.as_bytes()));
    if let Err(e) = result {
        eprintln!("warning: could not append to GITHUB_STEP_SUMMARY {path}: {e}");
    }
}

/// The verdict (ADR-0176): the run is report-only, so it fails if and only
/// if a record carries an infrastructure failure — the setup-failed variant
/// (kinds 1, 3 and 4; kind 2 is the boot panic above). DIFF, order-only,
/// skip and inconclusive records are reported, never fatal. The panic names
/// each failed dataset and its error.
fn verdict(all_results: &[record::QueryResult], report_path: &Path) {
    let infra_failures: Vec<(&str, &str)> = all_results
        .iter()
        .filter_map(|r| match &r.outcome {
            record::Outcome::SetupFailed(why) => Some((r.dataset.as_str(), why.as_str())),
            _ => None,
        })
        .collect();
    if infra_failures.is_empty() {
        return;
    }
    // ADR-0204: capture the reference container's logs BEFORE the panic —
    // the unwind drops the DatasetTest, which removes the container, and
    // the workflow's own capture step then finds nothing. The files land
    // next to report.txt, inside the workflow's existing `reference-*.log`
    // upload glob.
    docker::capture_reference_logs(
        report_path
            .parent()
            .expect("the report always lives in the workdir"),
    );
    panic!(
        "{} infrastructure failures (ADR-0176):\n  {}\nreport: {}",
        infra_failures.len(),
        infra_failures
            .iter()
            .map(|(dataset, why)| format!("{dataset}: {why}"))
            .collect::<Vec<_>>()
            .join("\n  "),
        report_path.display()
    );
}

#[test]
fn graph_examples_differential_suite() {
    let Some(datasets) = preflight() else {
        return;
    };
    let workdir = cache::cache_root();
    std::fs::create_dir_all(&workdir).expect("workdir");
    let reference = boot_reference(&datasets);
    let apoc_enabled = apoc_enabled();

    let outcomes = run_datasets_bounded(&workdir, &reference, &datasets, apoc_enabled);
    let all_results = fold_outcomes(outcomes);
    let summary = record::Summary::of(&all_results);
    let report_path = write_reports(&workdir, &all_results, &summary, apoc_enabled);

    verdict(&all_results, &report_path);
}

/// The queue runs every item exactly once, however the workers interleave.
#[test]
fn work_queue_processes_every_item_exactly_once() {
    let items: Vec<usize> = (0..100).collect();
    let mut out = work_queue(&items, 4, |&i| i * 2);
    out.sort_unstable();
    let expected: Vec<usize> = items.iter().map(|i| i * 2).collect();
    assert_eq!(out, expected);
}

/// Degenerate shapes: more workers than items, and no items at all.
#[test]
fn work_queue_handles_degenerate_shapes() {
    let mut out = work_queue(&[1, 2], 8, |&i| i);
    out.sort_unstable();
    assert_eq!(out, [1, 2]);
    let empty: Vec<usize> = Vec::new();
    assert!(work_queue(&empty, 4, |&i| i).is_empty());
}

/// F-1-3: no wave barrier. Item 0 is a straggler that completes only once
/// every other item has been processed by the other worker; under
/// `chunks(width)` waves those items would sit behind the barrier and this
/// test would hit its timeout instead.
#[test]
fn work_queue_has_no_wave_barrier() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let others_done = AtomicUsize::new(0);
    let items: Vec<usize> = (0..5).collect();
    let out = work_queue(&items, 2, |&i| {
        if i == 0 {
            let start = Instant::now();
            while others_done.load(Ordering::SeqCst) < 4 {
                assert!(
                    start.elapsed() < Duration::from_secs(30),
                    "wave barrier: the straggler's wave never releases the other items"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        } else {
            others_done.fetch_add(1, Ordering::SeqCst);
        }
        i
    });
    assert_eq!(out.len(), 5);
}

/// F-1-4: a dataset setup failure becomes one setup-failed record carrying
/// only real data — no fabricated adoc line, no fabricated statement; the
/// error text is the outcome's. Successful datasets keep their records.
#[test]
fn fold_outcomes_records_setup_failures_without_fake_fields() {
    let ok = record::QueryResult {
        dataset: "movies".to_string(),
        line: Some(3),
        statement: Some("MATCH (n) RETURN n".to_string()),
        outcome: record::Outcome::Pass,
        engine_ms: None,
        apoc: false,
    };
    let all = fold_outcomes(vec![
        ("movies".to_string(), Ok(vec![ok])),
        ("pole".to_string(), Err("pull failed".to_string())),
    ]);
    assert_eq!(all.len(), 2);
    assert!(matches!(all[0].outcome, record::Outcome::Pass));
    let setup = &all[1];
    assert_eq!(setup.dataset, "pole");
    assert_eq!(setup.line, None);
    assert_eq!(setup.statement, None);
    assert!(
        matches!(&setup.outcome, record::Outcome::SetupFailed(e) if e == "pull failed"),
        "{:?}",
        setup.outcome
    );
}
