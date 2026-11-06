use super::super::*;
use super::harness::checkpoint_at;
use super::harness::*;
use crate::memory_store;
use relate_catalog::ConstraintDef;
use relate_executor::{Graph, RedoOp};

#[test]
fn parquet_checkpoint_theater_rejects_corrupt_payload_and_accepts_legacy_absence() {
    let target = memory_store();
    let graph = Graph::default();
    graph
        .apply_redo(
            vec![
                RedoOp::create_constraint(ConstraintDef {
                    name: "rule".into(),
                    relationship: false,
                    label: "Person".into(),
                    properties: vec![],
                    kind: "nodeLabelExistence".into(),
                    property_type: None,
                    enforced_label: None,
                }),
                RedoOp::CatalogDerivedConstraint {
                    constraint: relate_model::Constraint {
                        name: "rule".into(),
                        label: "Person".into(),
                        properties: vec![],
                        constraint_type: relate_model::ConstraintType::NodeLabelExistence,
                        property_type: None,
                        entity_type: "NODE".into(),
                        enforced_label: Some("Verified".into()),
                    },
                },
                RedoOp::create_constraint(ConstraintDef {
                    name: "source_rule".into(),
                    relationship: true,
                    label: "KNOWS".into(),
                    properties: vec![],
                    kind: "sourceLabelExistence".into(),
                    property_type: None,
                    enforced_label: None,
                }),
                RedoOp::CatalogDerivedConstraint {
                    constraint: relate_model::Constraint {
                        name: "source_rule".into(),
                        label: "KNOWS".into(),
                        properties: vec![],
                        constraint_type: relate_model::ConstraintType::SourceLabelExistence,
                        property_type: None,
                        entity_type: "RELATIONSHIP".into(),
                        enforced_label: Some("Verified".into()),
                    },
                },
            ],
            1,
        )
        .unwrap();
    checkpoint_at(&target, &graph, 1, 1, DEFAULT_SHARD_TARGET_BYTES).unwrap();
    let bytes = get_object(&target, MANIFEST_JSON).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        load(&target)
            .unwrap()
            .checkpoint_theater()
            .enforced_label("rule"),
        Some("Verified")
    );
    assert_eq!(
        load(&target)
            .unwrap()
            .checkpoint_theater()
            .enforced_label("source_rule"),
        Some("Verified")
    );

    for name in ["rule", "source_rule"] {
        let mut incomplete = original.clone();
        incomplete["theater"]["derived_constraints"]
            .as_object_mut()
            .unwrap()
            .remove(name);
        put_manifest(&target, &incomplete);
        assert!(matches!(load(&target), Err(Error::Storage(_))), "{name}");
    }
    let mut wrong_schema = original.clone();
    wrong_schema["theater"]["derived_constraints"]["source_rule"]["label"] =
        serde_json::json!("OTHER");
    put_manifest(&target, &wrong_schema);
    assert!(matches!(load(&target), Err(Error::Storage(_))));

    for bad in [
        serde_json::Value::Null,
        serde_json::json!({"databases": {}}),
    ] {
        let mut manifest = original.clone();
        manifest["theater"] = bad;
        put_manifest(&target, &manifest);
        assert!(matches!(load(&target), Err(Error::Storage(_))));
    }
    let mut inconsistent = original.clone();
    inconsistent["theater"]["derived_constraints"]["rule"]["enforced_label"] =
        serde_json::Value::Null;
    put_manifest(&target, &inconsistent);
    assert!(matches!(load(&target), Err(Error::Storage(_))));

    let mut legacy = original;
    legacy.as_object_mut().unwrap().remove("theater");
    put_manifest(&target, &legacy);
    assert_eq!(load(&target).unwrap().named_constraints().len(), 2);
}

#[test]
fn oversized_checkpoint_theater_keeps_previous_manifest() {
    let target = memory_store();
    let graph = Graph::default();
    checkpoint_at(&target, &graph, 0, 0, DEFAULT_SHARD_TARGET_BYTES).unwrap();
    let previous = get_object(&target, MANIFEST_JSON).unwrap();
    // ADR-0116 §8: a `PutDatabase` redo for a non-fixed name is ignored
    // on apply, so the oversized theater entry comes from a graph type
    // instead.
    graph
        .apply_redo(
            vec![RedoOp::PutGraphType {
                name: "x".repeat(MAX_JSON_OBJECT as usize),
                body: relate_executor::GraphTypeSpec::default(),
            }],
            1,
        )
        .unwrap();
    let err = checkpoint_at(&target, &graph, 1, 1, DEFAULT_SHARD_TARGET_BYTES).unwrap_err();
    assert!(matches!(err, Error::Storage(_)), "{err}");
    assert!(err.to_string().contains("16 MiB"), "{err}");
    assert_eq!(get_object(&target, MANIFEST_JSON).unwrap(), previous);
}

/// The derived-constraint index is re-derived from the persisted graph type
/// on load, so a checkpoint written before a name/derivation change restores
/// instead of failing its identity checks — and the catalog rows move with
/// it (the stale `"<type> NOT NULL"` spelling would otherwise survive every
/// purge and turn the next non-`SET` ALTER into a conflict).
#[test]
fn load_rederives_a_pre_generated_name_theater() {
    let target = memory_store();
    let graph = Graph::default();
    graph
        .restore_graph_type_spec(Some("(:Person => {name :: STRING NOT NULL})"))
        .unwrap();
    checkpoint_at(&target, &graph, 1, 1, DEFAULT_SHARD_TARGET_BYTES).unwrap();

    // Age the image the way a pre-generated-name release wrote it: one
    // readable-named property-type row, `NOT NULL` still in the type.
    let legacy = serde_json::json!({
        "name": "constraint_propertyType_Person_name",
        "label": "Person",
        "properties": ["name"],
        "type": "propertyType",
        "property_type": "STRING NOT NULL",
        "entity_type": "NODE",
    });
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&get_object(&target, MANIFEST_JSON).unwrap()).unwrap();
    // The parquet manifest's catalog rows are `ConstraintDef`s (`kind`,
    // `relationship`); the theater record is the model shape (`type`).
    manifest["constraints"] = serde_json::json!([{
        "name": "constraint_propertyType_Person_name",
        "relationship": false,
        "label": "Person",
        "properties": ["name"],
        "kind": "propertyType",
        "property_type": "STRING NOT NULL",
    }]);
    manifest["theater"]["derived_constraints"] = serde_json::json!({
        "constraint_propertyType_Person_name": legacy,
    });
    put_manifest(&target, &manifest);

    let loaded = load(&target).expect("a pre-generated-name checkpoint re-derives and loads");
    let mut rows = loaded.named_constraints();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        rows.iter().all(|def| def.name.starts_with("constraint_")),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|def| def.property_type.as_deref() == Some("STRING")),
        "{rows:?}"
    );
    // The migrated graph checkpoints again: the re-derived index and the
    // catalog agree.
    let target = memory_store();
    checkpoint_at(&target, &loaded, 2, 2, DEFAULT_SHARD_TARGET_BYTES).unwrap();
}
