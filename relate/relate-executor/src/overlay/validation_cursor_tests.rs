//! The redo log's catalog-op positions and constraint-validation cursor
//! as the overlay drives them.

use std::collections::BTreeMap;

use super::ExecGraph;
use crate::exec::test_support::write_scratch;
use crate::graph::Graph;
use crate::redo::RedoOp;

/// ADR-0172 §1/§8: the schema overlay replays only the catalog ops, the
/// validation cursor drains each op once, and `take_redo` resets both.
#[test]
fn catalog_positions_and_validation_cursor_follow_the_redo_log() {
    let g = Graph::default();
    let scratch = write_scratch(&g);
    let view = ExecGraph::write(&g, &scratch);
    let constraint = |name: &str| {
        view.create_named_constraint(relate_catalog::ConstraintDef {
            name: name.into(),
            relationship: false,
            label: "P".into(),
            properties: vec!["id".into()],
            kind: "unique".into(),
            property_type: None,
            enforced_label: None,
        })
        .unwrap()
    };
    view.create_node(vec!["P".into()], BTreeMap::new()).unwrap();
    constraint("c");
    view.create_node(vec!["P".into()], BTreeMap::new()).unwrap();
    view.create_named_index("i".into(), "P".into(), "id".into())
        .unwrap();
    view.drop_named_constraint("c".into()).unwrap();
    constraint("d");
    let names: Vec<String> = view
        .named_constraints()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(names, ["d"]);
    assert_eq!(view.named_indexes().len(), 1);
    let drain = || {
        let (len, end) = view.unvalidated_redo(<[RedoOp]>::len);
        view.mark_redo_validated(end);
        len
    };
    assert_eq!(drain(), 6);
    assert_eq!(drain(), 0);
    view.create_node(vec!["P".into()], BTreeMap::new()).unwrap();
    assert_eq!(drain(), 1);
    assert_eq!(scratch.borrow_mut().take_redo().len(), 7);
    assert!(view.named_constraints().is_empty());
    view.create_node(vec!["P".into()], BTreeMap::new()).unwrap();
    assert_eq!(drain(), 1);
}
