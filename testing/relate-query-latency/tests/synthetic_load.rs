//! The loader on a tiny data set written from the schema table: every node
//! file gets two rows (ids 1 and 2), every relationship file one row (1 →
//! 2). No network and no LDBC data needed, so this always runs.

use std::fmt::Write as _;
use std::path::Path;

use relate::{Database, Value};
use relate_query_latency::load::load;
use relate_query_latency::schema::{csv_path, PropType, NODE_FILES, REL_FILES};
use relate_query_latency::workload::IndexProfile;

fn cell(ty: PropType, row: i64) -> String {
    match (ty, row) {
        // Row 2 leaves every property empty: the loader must omit it.
        (_, 2) => String::new(),
        (PropType::Int, _) => "7".into(),
        (PropType::Str, _) => "x".into(),
    }
}

fn write_dataset(root: &Path) {
    for nf in NODE_FILES {
        let mut s = String::from("id");
        for (name, _) in nf.props {
            write!(s, "|{name}").unwrap();
        }
        s.push('\n');
        for id in [1, 2] {
            write!(s, "{id}").unwrap();
            for (_, ty) in nf.props {
                write!(s, "|{}", cell(*ty, id)).unwrap();
            }
            s.push('\n');
        }
        let p = csv_path(root, nf.file);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }
    for rf in REL_FILES {
        // Real relationship files repeat their id column's name, which is
        // why the loader reads them by position.
        let mut s = format!("{}.id|{}.id", rf.from, rf.to);
        let mut row = "1|2".to_string();
        if let Some((name, ty)) = rf.prop {
            write!(s, "|{name}").unwrap();
            write!(row, "|{}", cell(ty, 1)).unwrap();
        }
        let p = csv_path(root, rf.file);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, format!("{s}\n{row}\n")).unwrap();
    }
}

fn single(conn: &mut relate::Connection, q: &str) -> Value {
    let r = conn.query(q).unwrap();
    assert_eq!(r.rows.len(), 1, "{q}");
    r.rows[0][0].clone()
}

#[test]
fn loads_every_file_with_upstreams_types() {
    let dir = tempfile::tempdir().unwrap();
    write_dataset(dir.path());
    let db = Database::open(":memory:").unwrap();
    let mut conn = db.connect().unwrap();
    let want = (2 * NODE_FILES.len() as u64, REL_FILES.len() as u64);
    let report = load(&mut conn, dir.path(), IndexProfile::Text, Some(want)).unwrap();
    assert_eq!((report.nodes, report.relationships), want);
    assert!(report.labels.values().all(|&c| c == 2));
    assert!(report.types.values().all(|&c| c == 1));

    // `id` → INTEGER `ID`; Int columns are INTEGER, others STRING.
    assert_eq!(
        single(&mut conn, "MATCH (c:Comment {ID: 1}) RETURN c.length"),
        Value::Int(7)
    );
    assert_eq!(
        single(&mut conn, "MATCH (p:Person {ID: 1}) RETURN p.birthday"),
        Value::String("x".into())
    );
    // Empty cells are left out, not stored as empty strings.
    assert_eq!(
        single(&mut conn, "MATCH (p:Person {ID: 2}) RETURN size(keys(p))"),
        Value::Int(1)
    );
    // Relationship properties, typed, on the right endpoints.
    assert_eq!(
        single(
            &mut conn,
            "MATCH (:Person {ID: 1})-[r:studyAt]->(:Organisation {ID: 2}) RETURN r.classYear"
        ),
        Value::Int(7)
    );
    assert_eq!(
        single(
            &mut conn,
            "MATCH (:Person {ID: 1})-[r:knows]->(:Person {ID: 2}) RETURN r.creationDate"
        ),
        Value::String("x".into())
    );
    // The text profile's TEXT indexes were created.
    let idx = conn.query("SHOW INDEXES YIELD name RETURN name").unwrap();
    let names: Vec<_> = idx.rows.iter().map(|r| r[0].clone()).collect();
    for n in ["comment_ID", "tagclass_name", "tag_name_text"] {
        assert!(
            names.contains(&Value::String(n.into())),
            "{n} missing from {names:?}"
        );
    }
}

#[test]
fn a_dangling_endpoint_fails_the_load() {
    let dir = tempfile::tempdir().unwrap();
    write_dataset(dir.path());
    let knows = csv_path(dir.path(), "dynamic/person_knows_person");
    std::fs::write(knows, "Person.id|Person.id|creationDate\n1|99|x\n").unwrap();
    let db = Database::open(":memory:").unwrap();
    let mut conn = db.connect().unwrap();
    let err = load(&mut conn, dir.path(), IndexProfile::Upstream, None).unwrap_err();
    assert!(err.contains("no Person with id 99"), "{err}");
}

#[test]
fn totals_must_match_the_published_ones() {
    let dir = tempfile::tempdir().unwrap();
    write_dataset(dir.path());
    let db = Database::open(":memory:").unwrap();
    let mut conn = db.connect().unwrap();
    let err = load(&mut conn, dir.path(), IndexProfile::Upstream, Some((1, 1))).unwrap_err();
    assert!(err.contains("published totals"), "{err}");
}

#[test]
fn a_non_integer_id_fails_the_load() {
    let dir = tempfile::tempdir().unwrap();
    write_dataset(dir.path());
    std::fs::write(
        csv_path(dir.path(), "static/tag"),
        "id|name|url\nnope|a|b\n",
    )
    .unwrap();
    let db = Database::open(":memory:").unwrap();
    let mut conn = db.connect().unwrap();
    let err = load(&mut conn, dir.path(), IndexProfile::Upstream, None).unwrap_err();
    assert!(err.contains("is not an integer"), "{err}");
}
