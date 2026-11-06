//! Open an empty directory with the JSON snapshot codec (ADR-0050).
//! WAL is unchanged; checkpoints rewrite `graph.json`.

use relate::{Database, OpenOptions, SnapshotCodecKind};

fn main() -> relate::Result<()> {
    let dir = std::env::temp_dir().join(format!("relate-snapshot-json-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let db = Database::open_with(
        dir.to_str().expect("temp dir is valid UTF-8"),
        OpenOptions {
            snapshot_codec: Some(SnapshotCodecKind::Json),
            ..OpenOptions::default()
        },
    )?;
    let mut conn = db.connect()?;
    conn.query("CREATE (n:Person {name: 'Ada'})")?;
    db.checkpoint()?;
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
