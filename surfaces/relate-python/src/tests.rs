use super::*;
use crate::convert::{
    cypher_value_to_py, date_to_py, duration_to_py, py_params_to_cypher, py_to_cypher_value,
    py_type_name, PY_VALUE_DEPTH_CAP,
};
use crate::result::stats_to_map;
use indexmap::IndexMap;
use pyo3::exceptions::{PyOverflowError, PyTypeError, PyValueError};
use pyo3::types::{PyDict, PyList};
use relate::Namespace;
use relate_model::value::IsoDuration;
use relate_model::CypherValue;

/// Initialize the embedded interpreter (idempotent) and run `f` under the
/// GIL. `cargo test` links libpython but does not auto-initialize it.
fn with_py<F, R>(f: F) -> R
where
    F: for<'py> FnOnce(Python<'py>) -> R,
{
    pyo3::prepare_freethreaded_python();
    Python::with_gil(f)
}

fn new_database(path: &str) -> Database {
    new_database_ns(path, "neo4j")
}

fn new_database_ns(path: &str, namespace: &str) -> Database {
    try_new_database_ns(path, namespace).unwrap()
}

/// Fallible twin of [`new_database_ns`] for tests that assert on a failed
/// connect.
fn try_new_database_ns(path: &str, namespace: &str) -> PyResult<Database> {
    with_py(|py| Database::new(py, path.to_string(), namespace.to_string(), false))
}

/// The `match cv { Variant(x) => x, other => panic!(…) }` extraction,
/// written once per variant used in this suite (F-1-15): the panic keeps
/// the `{other:?}` payload, `#[track_caller]` keeps the call-site location.
#[track_caller]
fn expect_list(v: CypherValue) -> Vec<CypherValue> {
    match v {
        CypherValue::List(items) => items,
        other => panic!("expected List, got {other:?}"),
    }
}

/// See [`expect_list`].
#[track_caller]
fn expect_str(v: CypherValue) -> String {
    match v {
        CypherValue::Str(s) => s,
        other => panic!("expected Str, got {other:?}"),
    }
}

/// See [`expect_list`].
#[track_caller]
fn expect_map(v: CypherValue) -> IndexMap<String, CypherValue> {
    match v {
        CypherValue::Map(map) => map,
        other => panic!("expected Map, got {other:?}"),
    }
}

/// Extract the string attribute `name` (`kind`, `tck_class`, …) from the
/// value carried by a raised `PyErr`.
fn err_attr(err: &PyErr, py: Python<'_>, name: &str) -> String {
    err.value(py)
        .as_any()
        .getattr(name)
        .unwrap()
        .extract()
        .unwrap()
}

/// Run one raw engine statement through the guarded accessor — the tests'
/// direct engine access, bypassing the pymethod layer. The affinity check
/// does run and passes because `with_py` takes the GIL on the calling thread,
/// which is the thread that created the database; a foreign-thread call is
/// refused like any other engine access.
fn raw_query(db: &mut Database, query: &str) -> relate::QueryResult {
    with_py(|py| db.lock_conn(py).unwrap().query(query).unwrap())
}

/// Run `query` on `db` and return the first row's `key` value as a Python
/// object bound to `py`.
fn query_first_py<'py>(
    py: Python<'py>,
    db: &mut Database,
    query: &str,
    key: &str,
) -> Bound<'py, PyAny> {
    let result = raw_query(db, query);
    let rows = query_result_to_cypher_rows(&result);
    cypher_value_to_py(py, rows[0].get(key).unwrap(), result.header.namespace)
        .unwrap()
        .into_bound(py)
}

/// Evaluate `expr` as `RETURN <expr> AS v` and return the value together with
/// the name of the Python type it arrived as. Shared by the temporal-family
/// leaf modules (temporal_tests, duration_tests).
fn temporal_result<'py>(py: Python<'py>, expr: &str) -> (Bound<'py, PyAny>, String) {
    let mut db = new_database(":memory:");
    let value = query_first_py(py, &mut db, &format!("RETURN {expr} AS v"), "v");
    let type_name = py_type_name(&value);
    (value, type_name)
}

/// One attribute of a `relate.Temporal` result, extracted as `T`.
/// Shared by the temporal-family leaf modules (temporal_tests,
/// duration_tests).
fn temporal_attr<T>(value: &Bound<'_, PyAny>, name: &str) -> T
where
    T: for<'py> FromPyObject<'py>,
{
    value
        .getattr(name)
        .unwrap_or_else(|e| panic!("Temporal.{name} missing: {e}"))
        .extract()
        .unwrap_or_else(|e| panic!("Temporal.{name} has an unexpected type: {e}"))
}

#[test]
fn test_database_creation() {
    with_py(|py| {
        let db = new_database(":memory:");
        assert_eq!(db.node_count(py).unwrap(), 0);
        assert_eq!(db.relationship_count(py).unwrap(), 0);
    });
}

#[test]
fn test_database_repr() {
    with_py(|py| {
        let db = new_database(":memory:");
        assert_eq!(
            db.__repr__(py).unwrap(),
            "Database(nodes=0, relationships=0)"
        );
    });
}

#[test]
fn test_path_suffix_does_not_select_backend() {
    // A `.lance`-looking suffix is not a backend switch: the path is an
    // ordinary Relate store, so a write there round-trips.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.lance");
    let path_str = path.to_str().unwrap().to_string();
    {
        let mut db = new_database(&path_str);
        raw_query(&mut db, "CREATE (:Person {name: 'Alice'})");
    }
    with_py(|py| {
        let db = new_database(&path_str);
        assert_eq!(db.node_count(py).unwrap(), 1);
    });
}

#[test]
fn test_reset_in_memory() {
    let mut db = new_database(":memory:");
    raw_query(&mut db, "CREATE (:Person {name: 'Alice'})");
    with_py(|py| {
        assert_eq!(db.node_count(py).unwrap(), 1);
        db.reset(py).unwrap();
        assert_eq!(db.node_count(py).unwrap(), 0);
    });
}

#[test]
fn test_write_query_has_no_columns() {
    let mut db = new_database(":memory:");
    let result = raw_query(&mut db, "CREATE (:Person {name: 'Alice'})");
    assert!(result.header.columns.is_empty());
    assert_eq!(result.stats.nodes_created, 1);
}

#[test]
fn test_stats_map_has_all_eleven() {
    let mut db = new_database(":memory:");
    let result = raw_query(&mut db, "CREATE (:Person {name: 'Alice'})");
    let map = stats_to_map(&result.stats);
    assert_eq!(map.len(), 11);
    for key in [
        "nodes_created",
        "nodes_deleted",
        "relationships_created",
        "relationships_deleted",
        "properties_set",
        "labels_added",
        "labels_removed",
        "indexes_added",
        "indexes_removed",
        "constraints_added",
        "constraints_removed",
    ] {
        assert!(map.contains_key(key), "missing stat {key}");
    }
    assert_eq!(map["nodes_created"], 1);
}

#[test]
fn test_execute_exposes_engine_column_types() {
    with_py(|py| {
        let mut db = new_database(":memory:");
        raw_query(&mut db, "CREATE (:P)");
        let typed = db
            .execute(py, "MATCH (n:P) RETURN n, n.name AS name, 1 AS i", None)
            .unwrap();
        assert_eq!(typed.columns, ["n", "name", "i"]);
        assert_eq!(typed.column_types, ["NODE", "ANY", "INTEGER"]);
        let empty = db
            .execute(py, "MATCH (n:Missing) RETURN n.name AS name", None)
            .unwrap();
        assert_eq!(empty.column_types, ["ANY"]);
    });
}

#[test]
fn test_file_persistence_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("database");
    let path_str = path.to_str().unwrap().to_string();
    {
        let mut db = new_database(&path_str);
        raw_query(&mut db, "CREATE (:Person {name: 'Alice'})");
        with_py(|py| db.flush(py).unwrap());
    }
    with_py(|py| {
        let db2 = new_database(&path_str);
        assert_eq!(db2.node_count(py).unwrap(), 1);
    });
}

#[test]
fn test_node_conversion_preserves_topology() {
    with_py(|py| {
        let mut db = new_database(":memory:");
        raw_query(&mut db, "CREATE (:Person:User {name: 'Alice', age: 30})");
        let bound = query_first_py(py, &mut db, "MATCH (n) RETURN n", "n");
        let id: i64 = bound.getattr("id").unwrap().extract().unwrap();
        assert!(id >= 0);
        let labels: Vec<String> = bound.getattr("labels").unwrap().extract().unwrap();
        assert_eq!(labels, vec!["Person".to_string(), "User".to_string()]);
        let name: String = bound
            .getattr("properties")
            .unwrap()
            .get_item("name")
            .unwrap()
            .extract()
            .unwrap();
        assert_eq!(name, "Alice");
    });
}

#[test]
fn test_relationship_conversion_preserves_endpoints() {
    with_py(|py| {
        let mut db = new_database(":memory:");
        raw_query(&mut db, "CREATE (:A)-[:KNOWS {since: 2020}]->(:B)");
        let bound = query_first_py(py, &mut db, "MATCH ()-[r]->() RETURN r", "r");
        let rtype: String = bound.getattr("type").unwrap().extract().unwrap();
        assert_eq!(rtype, "KNOWS");
        let start: i64 = bound.getattr("start_node").unwrap().extract().unwrap();
        let end: i64 = bound.getattr("end_node").unwrap().extract().unwrap();
        assert_ne!(start, end);
    });
}

#[test]
fn test_datetime_param_naive_is_localdatetime() {
    with_py(|py| {
        let dtmod = py.import("datetime").unwrap();
        let dt = dtmod
            .call_method1("datetime", (2024, 3, 14, 12, 30, 45))
            .unwrap();
        let cv = py_to_cypher_value(&dt).unwrap();
        assert!(matches!(cv, CypherValue::LocalDateTime(_)), "{cv:?}");
    });
}

#[test]
fn test_date_param_roundtrips() {
    with_py(|py| {
        let dtmod = py.import("datetime").unwrap();
        let d = dtmod.call_method1("date", (2024, 3, 14)).unwrap();
        let cv = py_to_cypher_value(&d).unwrap();
        match cv {
            CypherValue::Date(nd) => {
                let back = date_to_py(py, &nd).unwrap();
                assert!(back.bind(py).eq(&d).unwrap());
            }
            other => panic!("expected Date, got {other:?}"),
        }
    });
}

#[test]
fn test_point_param_roundtrips() {
    with_py(|py| {
        let pt = Py::new(
            py,
            Point {
                srid: 4326,
                x: -73.98,
                y: 40.75,
                z: None,
                crs: "wgs-84".to_string(),
            },
        )
        .unwrap();
        let bound = pt.bind(py).clone().into_any();
        let cv = py_to_cypher_value(&bound).unwrap();
        match cv {
            CypherValue::Point(p) => {
                assert_eq!(p.srid, 4326);
                assert_eq!(p.x, -73.98);
                assert_eq!(p.y, 40.75);
            }
            other => panic!("expected Point, got {other:?}"),
        }
    });
}

#[test]
fn test_unknown_param_type_raises() {
    with_py(|py| {
        // A set is not convertible.
        let obj = py.eval(c"{1, 2, 3}", None, None).unwrap();
        let err = py_to_cypher_value(&obj).unwrap_err();
        assert!(err.is_instance_of::<PyTypeError>(py));
    });
}

#[test]
fn test_cyclic_list_param_raises() {
    with_py(|py| {
        // a = []; a.append(a)
        let list = PyList::empty(py);
        list.append(list.clone()).unwrap();
        let obj = list.into_any();
        let err = py_to_cypher_value(&obj).unwrap_err();
        assert!(err.is_instance_of::<PyValueError>(py));
        assert!(
            err.to_string().to_lowercase().contains("cycle"),
            "expected cycle in message, got: {err}"
        );
    });
}

#[test]
fn test_cyclic_dict_param_raises() {
    with_py(|py| {
        // d = {}; d["self"] = d
        let dict = PyDict::new(py);
        dict.set_item("self", dict.clone()).unwrap();
        let obj = dict.into_any();
        let err = py_to_cypher_value(&obj).unwrap_err();
        assert!(err.is_instance_of::<PyValueError>(py));
        assert!(
            err.to_string().to_lowercase().contains("cycle"),
            "expected cycle in message, got: {err}"
        );
    });
}

#[test]
fn test_over_deep_nesting_raises() {
    with_py(|py| {
        // Build far more than `PY_VALUE_DEPTH_CAP` nested single-item lists.
        let mut current = PyList::empty(py).into_any();
        for _ in 0..(PY_VALUE_DEPTH_CAP + 64) {
            let outer = PyList::empty(py);
            outer.append(current.clone()).unwrap();
            current = outer.into_any();
        }
        let err = py_to_cypher_value(&current).unwrap_err();
        assert!(err.is_instance_of::<PyValueError>(py));
        assert!(
            err.to_string().contains("depth"),
            "expected depth in message, got: {err}"
        );
    });
}

#[test]
fn test_shared_substructure_is_not_a_cycle() {
    with_py(|py| {
        // The same list referenced twice is a DAG, not a cycle: it must
        // still convert, since the shared node is only on the path once.
        let shared = PyList::new(py, [1, 2, 3]).unwrap();
        let outer = PyList::empty(py);
        outer.append(shared.clone()).unwrap();
        outer.append(shared.clone()).unwrap();
        let obj = outer.into_any();
        assert_eq!(expect_list(py_to_cypher_value(&obj).unwrap()).len(), 2);
    });
}

#[test]
fn test_geometry_is_structured_dict() {
    with_py(|py| {
        let mut db = new_database(":memory:");
        let bound = query_first_py(py, &mut db, "RETURN point({x: 1.0, y: 2.0}) AS p", "p");
        // A point becomes a Point instance, not a dict.
        assert!(bound.is_instance_of::<Point>());
    });
}

#[test]
fn database_execute_and_introspection() {
    with_py(|py| {
        let db = new_database(":memory:");
        let created = db
            .execute(py, "CREATE (:Person)-[:KNOWS]->(:Person)", None)
            .unwrap();
        assert_eq!(created.stats["nodes_created"], 2);
        assert_eq!(created.stats.len(), 11);
        assert_eq!(db.node_count(py).unwrap(), 2);
        assert_eq!(db.relationship_count(py).unwrap(), 1);
        assert_eq!(db.labels(py).unwrap(), vec!["Person"]);
        assert_eq!(db.relationship_types(py).unwrap(), vec!["KNOWS"]);
    });
}

#[test]
fn reset_reopens_persistent_directory() {
    with_py(|py| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("database");
        let mut db = new_database(&path.to_string_lossy());
        db.execute(py, "CREATE (:Person {name: 'Alice'})", None)
            .unwrap();
        db.reset(py).unwrap();
        assert_eq!(db.node_count(py).unwrap(), 1);
        assert!(path.is_dir());
        assert!(path.join("wal").is_dir());
    });
}

#[test]
fn rollback_discards_transaction() {
    with_py(|py| {
        let db = new_database(":memory:");
        db.begin_transaction(py).unwrap();
        db.execute(py, "CREATE (:Transient)", None).unwrap();
        assert!(db.transaction_active(py).unwrap());
        db.rollback_transaction(py).unwrap();
        assert_eq!(db.node_count(py).unwrap(), 0);
    });
}

#[test]
fn relate_error_carries_binding_metadata() {
    with_py(|py| {
        let db = new_database(":memory:");
        let err = db.execute(py, "INVALID SYNTAX", None).unwrap_err();
        let kind = err_attr(&err, py, "kind");
        let tck_class = err_attr(&err, py, "tck_class");
        assert_eq!(kind, "SyntaxError");
        assert_eq!(tck_class, "SyntaxError");
    });
}

#[test]
fn foreign_thread_engine_use_raises_typed_error_not_panic() {
    // `db` moves to the foreign thread; `db_keep` proves the typed refusal
    // leaves the engine untouched for the owning thread.
    let (db, db_keep): (Py<Database>, Py<Database>) = with_py(|py| {
        let db = new_database(":memory:");
        let db = Py::new(py, db).unwrap();
        let keep = db.clone_ref(py);
        (db, keep)
    });
    let refusal = std::thread::spawn(move || {
        Python::with_gil(|py| {
            match db.call_method(py, "execute", ("MATCH (n) RETURN n",), None) {
                // A panic here (pyo3's unsendable assertion) is exactly the
                // regression this test guards against (GH #352 / LABS-700).
                Ok(_) => panic!("foreign-thread execute unexpectedly succeeded"),
                Err(err) => err,
            }
        })
    })
    .join()
    .unwrap();
    with_py(|py| {
        assert_eq!(err_attr(&refusal, py, "kind"), "WrongThread");
        let type_name = refusal.get_type(py).name().unwrap().to_string();
        assert_eq!(type_name, "RelateNativeError");
        let bound = db_keep.borrow_mut(py);
        bound.execute(py, "CREATE (:T {id: 1})", None).unwrap();
        assert_eq!(bound.node_count(py).unwrap(), 1);
    });
}

#[test]
fn foreign_thread_drop_runs_deferred_cleanup_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("thread-drop");
    let path_str = path.to_str().unwrap().to_string();
    let db: Py<Database> = with_py(|py| {
        let mut db = new_database(&path_str);
        raw_query(&mut db, "CREATE (:Person {name: 'Alice'})");
        Py::new(py, db).unwrap()
    });
    std::thread::spawn(move || {
        // Releasing the last reference here runs the pyclass `Drop` (the
        // deferred checkpoint) on this thread — the unsendable design turned
        // this into an unraisable RuntimeError (GH #352 / LABS-700).
        Python::with_gil(|_py| drop(db));
    })
    .join()
    .unwrap();
    with_py(|py| {
        let db2 = new_database(&path_str);
        assert_eq!(db2.node_count(py).unwrap(), 1);
    });
}

#[test]
fn namespace_defaults_to_neo4j() {
    with_py(|py| {
        let db = new_database(":memory:");
        assert_eq!(db.namespace(py).unwrap(), "neo4j");
    });
}

#[test]
fn connect_namespace_is_case_insensitive() {
    with_py(|py| {
        let db = new_database_ns(":memory:", "SYSTEM");
        assert_eq!(db.namespace(py).unwrap(), "system");
    });
}

#[test]
fn connect_unknown_namespace_is_database_not_found() {
    with_py(|py| {
        let err = match try_new_database_ns(":memory:", "nosuch") {
            Ok(_) => panic!("expected connecting with an unknown namespace to fail"),
            Err(e) => e,
        };
        let kind = err_attr(&err, py, "kind");
        assert_eq!(kind, "DatabaseNotFound");
        assert!(
            err.to_string().ends_with("Graph not found: nosuch"),
            "unexpected message: {err}"
        );
    });
}

#[test]
fn set_namespace_switches_between_transactions_only() {
    with_py(|py| {
        let db = new_database(":memory:");
        db.set_namespace(py, "SYSTEM").unwrap();
        assert_eq!(db.namespace(py).unwrap(), "system");
        db.set_namespace(py, "neo4j").unwrap();
        assert_eq!(db.namespace(py).unwrap(), "neo4j");

        db.begin_transaction(py).unwrap();
        let err = db.set_namespace(py, "system").unwrap_err();
        assert!(
            err.to_string()
                .contains("Cannot change the namespace while a transaction is open."),
            "unexpected message: {err}"
        );
        db.rollback_transaction(py).unwrap();
        db.set_namespace(py, "system").unwrap();
        assert_eq!(db.namespace(py).unwrap(), "system");

        let err = db.set_namespace(py, "nosuch").unwrap_err();
        let kind = err_attr(&err, py, "kind");
        assert_eq!(kind, "DatabaseNotFound");
        // A failed switch leaves the namespace unchanged.
        assert_eq!(db.namespace(py).unwrap(), "system");
    });
}

#[test]
fn reset_keeps_namespace() {
    with_py(|py| {
        let mut db = new_database(":memory:");
        db.set_namespace(py, "system").unwrap();
        db.reset(py).unwrap();
        assert_eq!(db.namespace(py).unwrap(), "system");
    });
}

#[test]
fn explain_create_reports_write_type_without_executing() {
    with_py(|py| {
        let db = new_database(":memory:");
        // EXPLAIN reports the wrapped statement's plan-time query type
        // (ADR-0162) without running it.
        let explained = db.execute(py, "EXPLAIN CREATE (n)", None).unwrap();
        assert_eq!(explained.query_type, "w");
        assert_eq!(explained.namespace, "neo4j");
        // The CREATE never executed: the graph is still empty.
        let after = db.execute(py, "MATCH (n) RETURN n", None).unwrap();
        assert!(after.data.is_empty());
        assert_eq!(after.query_type, "r");
        assert_eq!(db.node_count(py).unwrap(), 0);
    });
}

#[test]
fn use_routed_statement_reports_resolved_namespace() {
    with_py(|py| {
        let db = new_database(":memory:");
        // A USE-carrying statement resolves to its USE target (ADR-0117
        // §7)…
        let routed = db
            .execute(py, "USE system SHOW DATABASES YIELD name RETURN name", None)
            .unwrap();
        assert_eq!(routed.namespace, "system");
        // …while the session namespace is unchanged: a following unrouted
        // statement still resolves to neo4j.
        let unrouted = db.execute(py, "RETURN 1 AS n", None).unwrap();
        assert_eq!(unrouted.namespace, "neo4j");
        assert_eq!(db.namespace(py).unwrap(), "neo4j");
    });
}

#[test]
fn temporal_and_point_parameters_remain_rich() {
    with_py(|py| {
        let dtmod = py.import("datetime").unwrap();
        let date = dtmod.call_method1("date", (2024, 3, 14)).unwrap();
        assert!(matches!(
            py_to_cypher_value(&date).unwrap(),
            CypherValue::Date(_)
        ));

        let point = Py::new(py, Point::new(4326, -73.98, 40.75, None)).unwrap();
        assert!(matches!(
            py_to_cypher_value(&point.into_bound(py).into_any()).unwrap(),
            CypherValue::Point(_)
        ));
    });
}

#[test]
fn read_only_rejects_writes_and_skips_checkpoint() {
    with_py(|py| {
        let mut db = Database::new(py, ":memory:".to_string(), "neo4j".to_string(), true).unwrap();
        db.execute(py, "RETURN 1 AS n", None).unwrap();
        let err = db.execute(py, "CREATE ()", None).unwrap_err();
        assert!(err.to_string().contains("read-only"), "{err}");
        db.flush(py).unwrap();
        db.reset(py).unwrap();
        let err = db.execute(py, "CREATE ()", None).unwrap_err();
        assert!(err.to_string().contains("read-only"), "{err}");
        db.close(py).unwrap();

        let writable =
            Database::new(py, ":memory:".to_string(), "neo4j".to_string(), false).unwrap();
        writable.execute(py, "CREATE ()", None).unwrap();
    });
}

// ============================================================
// ADR-0219: UUID / bytes / tuple params, explicit-only points,
// ordered maps, DB-API-worded parameter errors
// ============================================================

#[test]
fn test_uuid_param_becomes_canonical_string() {
    with_py(|py| {
        let uuid_mod = py.import("uuid").unwrap();
        let u = uuid_mod
            .call_method1("UUID", ("12345678-1234-5678-9abc-def012345678",))
            .unwrap();
        let cv = py_to_cypher_value(&u).unwrap();
        assert_eq!(expect_str(cv), "12345678-1234-5678-9abc-def012345678");
        // Uppercase input normalizes to the canonical lowercase form.
        let u2 = uuid_mod
            .call_method1("UUID", ("12345678-1234-5678-9ABC-DEF012345678",))
            .unwrap();
        assert_eq!(
            expect_str(py_to_cypher_value(&u2).unwrap()),
            "12345678-1234-5678-9abc-def012345678"
        );
    });
}

#[test]
fn test_bytes_param_becomes_int_list() {
    with_py(|py| {
        let obj = PyBytes::new(py, b"\x00\x01\xff").as_any().clone();
        assert_eq!(
            expect_list(py_to_cypher_value(&obj).unwrap()),
            vec![
                CypherValue::Int(0),
                CypherValue::Int(1),
                CypherValue::Int(255)
            ]
        );
        // An empty payload converts too.
        let empty = PyBytes::new(py, b"").as_any().clone();
        assert!(expect_list(py_to_cypher_value(&empty).unwrap()).is_empty());
    });
}

#[test]
fn test_bytearray_param_becomes_int_list() {
    with_py(|py| {
        let obj = py.eval(c"bytearray(b'AB')", None, None).unwrap();
        assert_eq!(
            expect_list(py_to_cypher_value(&obj).unwrap()),
            vec![CypherValue::Int(65), CypherValue::Int(66)]
        );
    });
}

#[test]
fn test_tuple_param_becomes_list() {
    with_py(|py| {
        let obj = py.eval(c"(1, 'two', 3.5, (4, 5))", None, None).unwrap();
        let mut items = expect_list(py_to_cypher_value(&obj).unwrap());
        assert_eq!(items[0], CypherValue::Int(1));
        assert_eq!(items[1], CypherValue::Str("two".to_string()));
        assert_eq!(items[2], CypherValue::Float(3.5));
        let inner = expect_list(items.swap_remove(3));
        assert_eq!(inner, vec![CypherValue::Int(4), CypherValue::Int(5)]);
    });
}

#[test]
fn test_cyclic_tuple_param_raises() {
    with_py(|py| {
        // t = ([],); t[0].append(t)
        let obj = py
            .eval(c"__import__('builtins').tuple([[]])", None, None)
            .unwrap();
        obj.get_item(0)
            .unwrap()
            .call_method1("append", (&obj,))
            .unwrap();
        let err = py_to_cypher_value(&obj).unwrap_err();
        assert!(err.is_instance_of::<PyValueError>(py));
    });
}

#[test]
fn test_srid_dict_stays_a_map() {
    with_py(|py| {
        for text in [
            c"{'srid': 4326, 'x': 1.0, 'y': 2.0}",
            c"{'srid': 4326, 'longitude': -73.98, 'latitude': 40.75}",
            c"{'srid': 9157, 'x': 1.0, 'y': 2.0, 'z': 3.0}",
        ] {
            let obj = py.eval(text, None, None).unwrap();
            let map = expect_map(py_to_cypher_value(&obj).unwrap());
            assert!(map.contains_key("srid"), "map lost its srid key");
        }
    });
}

#[test]
fn test_returned_map_preserves_engine_key_order() {
    with_py(|py| {
        // Keys deliberately not alphabetical: the Python dict must keep
        // the engine IndexMap's insertion order (ADR-0219).
        let mut map = IndexMap::new();
        map.insert("zeta".to_string(), CypherValue::Int(1));
        map.insert("alpha".to_string(), CypherValue::Int(2));
        map.insert(
            "mid".to_string(),
            CypherValue::Map(IndexMap::from_iter([
                ("y".to_string(), CypherValue::Int(3)),
                ("b".to_string(), CypherValue::Int(4)),
            ])),
        );
        let obj = cypher_value_to_py(py, &CypherValue::Map(map), Namespace::Neo4j).unwrap();
        let dict = obj.into_bound(py);
        let keys: Vec<String> = dict
            .downcast::<PyDict>()
            .unwrap()
            .keys()
            .iter()
            .map(|k| k.extract().unwrap())
            .collect();
        assert_eq!(keys, vec!["zeta", "alpha", "mid"]);
        let nested = dict.get_item("mid").unwrap();
        let nested_keys: Vec<String> = nested
            .downcast::<PyDict>()
            .unwrap()
            .keys()
            .iter()
            .map(|k| k.extract().unwrap())
            .collect();
        assert_eq!(nested_keys, vec!["y", "b"]);
    });
}

#[test]
fn test_non_dict_params_error_is_dbapi_worded() {
    with_py(|py| {
        let list = PyList::empty(py).into_any();
        let err = py_params_to_cypher(&list).unwrap_err();
        assert!(err.is_instance_of::<PyTypeError>(py));
        let msg = err.to_string();
        assert!(
            msg.contains("parameters must be a dict"),
            "unexpected message: {msg}"
        );
        assert!(msg.contains("'list'"), "message must name the type: {msg}");
    });
}

#[test]
fn test_non_string_param_key_error_is_dbapi_worded() {
    with_py(|py| {
        let dict = PyDict::new(py);
        dict.set_item(42, 1).unwrap();
        let err = py_params_to_cypher(dict.as_any()).unwrap_err();
        assert!(err.is_instance_of::<PyTypeError>(py));
        let msg = err.to_string();
        assert!(
            msg.contains("keys must be strings"),
            "unexpected message: {msg}"
        );
        assert!(msg.contains("'int'"), "message must name the type: {msg}");

        // Same wording inside a nested map parameter value.
        let inner = PyDict::new(py);
        inner.set_item(1.5, "v").unwrap();
        let err = py_to_cypher_value(inner.as_any()).unwrap_err();
        assert!(err.is_instance_of::<PyTypeError>(py));
        assert!(
            err.to_string().contains("keys must be strings"),
            "unexpected message: {err}"
        );
    });
}

// LABS-680 (ADR-0477) safe mode is an engine-level opt-in; the Python binding
// never sets it, so there is nothing binding-specific to assert here (the
// engine default is covered by `labs680_safe_mode` in the engine crate).

mod duration_tests;
mod temporal_tests;
