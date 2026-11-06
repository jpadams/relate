mod common;

use relate_datasets::loadtest::{
    run_identity, write_report, Knobs, LoadtestConfig, OpKind, Phase, REPORT_SCHEMA_VERSION,
};

use common::assert_worker_report;

fn identity_cfg() -> LoadtestConfig {
    LoadtestConfig {
        knobs: Knobs::Identity {
            nest_depth: 4,
            group_k: 12,
        },
        ..LoadtestConfig::ci()
    }
}

#[test]
fn identity_loadtest_writes_report_json() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("report.json");
    let cfg = LoadtestConfig {
        ops: 24,
        warmup: 4,
        out: Some(out.clone()),
        ..identity_cfg()
    };
    let report = run_identity(&cfg).expect("loadtest");

    assert_eq!(report.schema_version, REPORT_SCHEMA_VERSION);
    assert_eq!(report.name, "identity-mixed-iam");
    assert!(!report.started_at.is_empty());
    assert!(!report.finished_at.is_empty());
    assert!(report.finished_at_unix_ms >= report.started_at_unix_ms);
    assert_eq!(report.config.scale, 0.05);
    assert_eq!(report.config.ops, 24);
    assert_eq!(report.config.warmup, 4);
    assert_eq!(report.config.workers, 1);
    assert_eq!(report.config.write_fraction, 0.5);
    assert_eq!(report.config.durability_mode, relate::DurabilityMode::Full);
    assert_eq!(report.workload.kind, "identity");
    assert_eq!(report.workload.unit, "users");
    assert_eq!(
        report.workload.config,
        serde_json::json!({"nest_depth": 4, "group_k": 12})
    );
    assert_eq!(report.workload.planted["nested_admin"], "nested-admin");
    assert_eq!(report.workload.planted["fan_group"], "nest-0");

    let start = &report.workload.dataset.start;
    assert!(start.scalar("users") >= 500);
    assert!(start.scalar("groups") > 0);
    assert!(start.scalar("roles") > 0);
    assert!(start.scalar("memberships") > 0);
    assert!(start.nodes >= start.scalar("users"));

    let measured: Vec<_> = report
        .ops
        .iter()
        .filter(|o| o.phase == Phase::Measured)
        .collect();
    let warmup: Vec<_> = report
        .ops
        .iter()
        .filter(|o| o.phase == Phase::Warmup)
        .collect();
    assert_eq!(measured.len(), 24);
    assert_eq!(warmup.len(), 4);
    assert!(
        measured.iter().all(|o| o.ok),
        "ops failed: {:?}",
        measured.iter().filter(|o| !o.ok).collect::<Vec<_>>()
    );
    assert!(measured.iter().all(|o| o.latency_ns > 0));

    let reads = measured
        .iter()
        .filter(|o| matches!(o.kind, OpKind::Read))
        .count();
    let writes = measured
        .iter()
        .filter(|o| matches!(o.kind, OpKind::Write))
        .count();
    assert!(reads > 0, "expected read usecases");
    assert!(writes > 0, "expected write usecases");
    assert_eq!(report.throughput.read_ops, reads);
    assert_eq!(report.throughput.write_ops, writes);
    assert!(report.throughput.ops_per_sec > 0.0);
    assert!(report.throughput.reads_per_sec > 0.0);
    assert!(report.throughput.writes_per_sec > 0.0);
    assert!(report.load_duration_ms > 0);
    assert!(report.worker_duration_ms > 0);
    assert!(report.duration_ms >= report.worker_duration_ms);
    assert!(report.duration_ms >= report.load_duration_ms);
    let worker_secs = (report.worker_duration_ms.max(1) as f64) / 1000.0;
    assert!((report.throughput.ops_per_sec - (reads + writes) as f64 / worker_secs).abs() < 1e-9);
    let total_secs = (report.duration_ms.max(1) as f64) / 1000.0;
    let if_total = (reads + writes) as f64 / total_secs;
    assert!(
        report.throughput.ops_per_sec + 1e-9 >= if_total,
        "worker throughput must not use load wall ({} vs {})",
        report.throughput.ops_per_sec,
        if_total
    );
    let related = measured.iter().filter(|o| o.related).count();
    assert!(related > 0, "expected some IAM-related ops");

    assert!(
        report.workload.dataset.finish.scalar("users")
            >= report.workload.dataset.start.scalar("users"),
        "provision may add users; revoke/detach do not delete User nodes"
    );
    assert!(report.workload.dataset.finish.scalar("memberships") > 0);

    assert!(
        report.memory.after_open.rss_bytes > 0,
        "process RSS after open"
    );
    assert!(
        report.memory.start.rss_bytes >= report.memory.after_open.rss_bytes,
        "loading the graph should not shrink RSS"
    );
    assert!(
        report.memory.graph.rss_bytes > 0,
        "attributed graph RSS after load"
    );
    assert!(report.memory.graph.bytes_per_entity > 0.0);
    assert!(report.memory.graph.bytes_per_unit > 0.0);
    assert!(report.memory.start.rss_per_entity > 0.0);
    assert!(report.memory.finish.rss_bytes > 0);
    assert!(report.memory.finish.peak_rss_bytes >= report.memory.start.peak_rss_bytes);

    assert!(!report.usecases.is_empty());
    for u in &report.usecases {
        assert!(!u.description.is_empty());
        if u.ops > 0 && u.errors < u.ops {
            assert!(u.latency_ns.is_some());
        }
    }

    let disk = std::fs::read_to_string(&out).unwrap();
    let parsed: relate_datasets::LoadtestReport = serde_json::from_str(&disk).unwrap();
    assert_eq!(parsed.ops.len(), report.ops.len());
    write_report(&out, &parsed).unwrap();
}

#[test]
fn reset_wipes_existing_uri_before_load() {
    let dir = tempfile::tempdir().unwrap();
    let uri = dir.path().to_str().unwrap().to_string();
    {
        let db = relate::Database::open(&uri).unwrap();
        let mut c = db.connect().unwrap();
        c.query("CREATE (n:Leftover {id: 'stale'})").unwrap();
    }
    assert!(dir.path().join("manifest.json").exists() || dir.path().join("wal").exists());

    let report = run_identity(&LoadtestConfig {
        uri: uri.clone(),
        reset: true,
        ops: 2,
        warmup: 0,
        out: None,
        ..identity_cfg()
    })
    .expect("loadtest with --reset");

    assert!(
        report.workload.dataset.start.scalar("users") >= 500,
        "identity users must be present after --reset"
    );
    let r = {
        let db = relate::Database::open(&uri).unwrap();
        let mut c = db.connect().unwrap();
        c.query("MATCH (n:Leftover) RETURN count(n) AS n").unwrap()
    };
    assert_eq!(r.rows[0][0], relate::Value::Int(0));
}

#[test]
fn workers_issue_ops_from_multiple_connections() {
    let report = run_identity(&LoadtestConfig {
        workers: 4,
        ops: 32,
        warmup: 4,
        out: None,
        ..identity_cfg()
    })
    .expect("loadtest workers");

    assert_worker_report(&report, 4, 32, 4);
}

#[test]
fn write_fraction_zero_is_reads_only() {
    let report = run_identity(&LoadtestConfig {
        write_fraction: 0.0,
        ops: 20,
        warmup: 0,
        out: None,
        ..identity_cfg()
    })
    .expect("write_fraction 0");
    assert_eq!(report.config.write_fraction, 0.0);
    assert!(report.ops.iter().all(|o| matches!(o.kind, OpKind::Read)));
    assert_eq!(report.throughput.write_ops, 0);
}

#[test]
fn write_fraction_one_is_writes_only() {
    let report = run_identity(&LoadtestConfig {
        write_fraction: 1.0,
        ops: 20,
        warmup: 0,
        out: None,
        ..identity_cfg()
    })
    .expect("write_fraction 1");
    assert_eq!(report.config.write_fraction, 1.0);
    assert!(report.ops.iter().all(|o| matches!(o.kind, OpKind::Write)));
    assert_eq!(report.throughput.read_ops, 0);
}

#[test]
fn mismatched_knobs_are_an_error() {
    // Fraud knobs on the identity dataset must not silently generate a fraud graph.
    assert!(run_identity(&LoadtestConfig {
        ops: 1,
        warmup: 0,
        out: None,
        ..LoadtestConfig::ci()
    })
    .is_err());
}
