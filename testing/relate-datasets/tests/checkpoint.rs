use relate_datasets::checkpoint::{run, CheckpointConfig};

#[test]
fn fraud_checkpoint_reports_logical_rss_and_files() {
    let dir = tempfile::tempdir().unwrap();
    let uri = dir.path().join("db");
    let out = dir.path().join("report.json");
    let report = run(&CheckpointConfig {
        uri: uri.to_str().unwrap().to_string(),
        reset: true,
        out: Some(out.clone()),
        ..CheckpointConfig::ci()
    })
    .expect("checkpoint");

    assert_eq!(report.name, "fraud-checkpoint");
    assert_eq!(report.scale, 0.05);
    assert!(report.accounts >= 500, "accounts={}", report.accounts);
    assert!(report.transfers > 0);
    assert!(report.nodes > report.accounts);
    assert!(
        report.logical_used_bytes > 0,
        "logical={}",
        report.logical_used_bytes
    );
    assert!(
        report.data_file_bytes > 0,
        "data files={}",
        report.data_file_bytes
    );
    assert!(
        std::fs::metadata(uri.join("manifest.json")).is_ok(),
        "checkpoint must write manifest.json"
    );
    let disk = std::fs::read_to_string(&out).unwrap();
    let parsed: relate_datasets::CheckpointReport = serde_json::from_str(&disk).unwrap();
    assert_eq!(parsed.logical_used_bytes, report.logical_used_bytes);
    assert_eq!(parsed.data_file_bytes, report.data_file_bytes);
}

#[test]
fn fraud_checkpoint_memory_uri_has_in_process_files() {
    let report = run(&CheckpointConfig {
        uri: ":memory:".into(),
        reset: true,
        out: None,
        ..CheckpointConfig::ci()
    })
    .expect("memory checkpoint");
    assert!(report.logical_used_bytes > 0);
    assert!(
        report.data_file_bytes > 0,
        "in-process store still has snapshot objects"
    );
}
