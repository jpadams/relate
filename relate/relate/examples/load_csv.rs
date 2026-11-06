//! Opt in to bounded, rooted local LOAD CSV on an in-memory database.
//! Run: cargo run -p relate --example load_csv

use relate::{Database, OpenOptions, Value};
use relate_executor::{CsvPolicy, Graph};

fn main() -> relate::Result<()> {
    let root = std::env::temp_dir().join(format!("relate-load-csv-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("create CSV directory");
    std::fs::write(root.join("people.csv"), "Ada\nLin\n").expect("write CSV");

    // Direct executor users install only policies accepted by the validator.
    let mut graph = Graph::default();
    graph.configure_load_csv(CsvPolicy::new(Some(root.clone()), Some(1024))?);
    assert!(CsvPolicy::new(Some(root.join("missing")), None).is_err());

    let db = Database::open_with(
        ":memory:",
        OpenOptions {
            load_csv_dir: Some(root.clone()),
            load_csv_max_bytes: Some(1024),
            mode: relate::OpenMode::Writable,
            ..OpenOptions::default()
        },
    )?;
    let mut conn = db.connect()?;
    let result = conn.query("LOAD CSV FROM 'people.csv' AS row RETURN row[0] AS name")?;
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0][0], Value::String("Ada".into()));

    std::fs::write(root.join("too_large.csv"), vec![b'x'; 1025]).expect("write oversized CSV");
    let error = conn
        .query("LOAD CSV FROM 'too_large.csv' AS row RETURN row")
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("LOAD CSV file exceeds byte limit"));

    drop(conn);
    drop(db);
    std::fs::remove_dir_all(root).expect("remove CSV directory");
    Ok(())
}
