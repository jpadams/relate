//! `SHOW CONSTRAINTS` row producer and its rendering helpers.

use std::collections::BTreeMap;

use relate_core::{Result, Value};
use relate_model::{ConstraintClassification, ConstraintType, EntityKind};
use relate_parser::{ConstraintEntityFilter, ConstraintKindFilter};

use crate::ctx::QueryCtx;
use crate::overlay::ExecGraph;
use crate::row::Row;

use super::entity_variable;
use crate::exec::catalog::{catalog_row, constraint_binding, optional_string_cell, string_list};

/// The `SHOW CONSTRAINTS` `type` column: Neo4j's underscore-style kind
/// name, entity-parameterized for the four property kinds. Deliberately
/// separate from `db.schema.visualization()`'s space-style kind names:
/// that mapping is entity-free (only node constraints ever render there)
/// and lives with the procedure rows, so a shared style-parameterized
/// helper would couple two surfaces that change for different reasons.
fn show_constraint_type_name(kind: ConstraintType, entity: EntityKind) -> &'static str {
    match kind {
        ConstraintType::Unique => match entity {
            EntityKind::Node => "NODE_PROPERTY_UNIQUENESS",
            EntityKind::Relationship => "RELATIONSHIP_PROPERTY_UNIQUENESS",
        },
        ConstraintType::Key => match entity {
            EntityKind::Node => "NODE_KEY",
            EntityKind::Relationship => "RELATIONSHIP_KEY",
        },
        ConstraintType::Exists => match entity {
            EntityKind::Node => "NODE_PROPERTY_EXISTENCE",
            EntityKind::Relationship => "RELATIONSHIP_PROPERTY_EXISTENCE",
        },
        ConstraintType::PropertyType => match entity {
            EntityKind::Node => "NODE_PROPERTY_TYPE",
            EntityKind::Relationship => "RELATIONSHIP_PROPERTY_TYPE",
        },
        ConstraintType::LabelExistence | ConstraintType::NodeLabelExistence => {
            "NODE_LABEL_EXISTENCE"
        }
        ConstraintType::SourceLabel => "RELATIONSHIP_SOURCE_LABEL",
        ConstraintType::SourceLabelExistence => "RELATIONSHIP_SOURCE_LABEL_EXISTENCE",
        ConstraintType::TargetLabel => "RELATIONSHIP_TARGET_LABEL",
        ConstraintType::TargetLabelExistence => "RELATIONSHIP_TARGET_LABEL_EXISTENCE",
    }
}

/// The `ConstraintType`s one `SHOW CONSTRAINTS` kind filter admits — the
/// filter's whole table, consulted by the row filter over the parser's
/// own typed [`ConstraintKindFilter`] vocabulary (CIP-187's `PROPERTY
/// EXIST[ENCE]` is its own variant there, so no side-channel narrowing
/// reaches this table): the three property kinds admit themselves,
/// `PROPERTY EXIST[ENCE]` admits property existence, and bare
/// `EXISTENCE` also admits the four label/endpoint-label kinds.
fn constraint_filter_types(wanted: ConstraintKindFilter) -> &'static [ConstraintType] {
    match wanted {
        ConstraintKindFilter::Unique => &[ConstraintType::Unique],
        ConstraintKindFilter::Key => &[ConstraintType::Key],
        ConstraintKindFilter::PropertyType => &[ConstraintType::PropertyType],
        ConstraintKindFilter::PropertyExistence => &[ConstraintType::Exists],
        ConstraintKindFilter::Existence => &[
            ConstraintType::Exists,
            ConstraintType::LabelExistence,
            ConstraintType::NodeLabelExistence,
            ConstraintType::SourceLabelExistence,
            ConstraintType::TargetLabelExistence,
        ],
    }
}

/// The `createStatement` column for one constraint row: Neo4j CE's
/// rendered CREATE CONSTRAINT statement, or `None` when there is no
/// faithful rendering — a dependent constraint (graph-type-managed; the
/// column is NULL), a label/endpoint-label or unrecognized kind (no
/// standalone CREATE form), or a Cypher 5 `propertyType` constraint (the
/// `IS :: <type>` verb is hidden). The caller maps `None` to the
/// column's NULL.
fn constraint_create_statement(
    def: &relate_catalog::ConstraintDef,
    kind: Option<ConstraintType>,
    entity: EntityKind,
    classification: ConstraintClassification,
    cypher5: bool,
) -> Option<String> {
    if classification == ConstraintClassification::Dependent {
        return None;
    }
    let kind = kind?;
    // The SHOW-only guard: Cypher 5 hides the `IS :: <type>` verb form.
    if cypher5 && kind == ConstraintType::PropertyType {
        return None;
    }
    // The one verb table (`standalone_verb`): `None` for the kinds with no
    // standalone CREATE form and for an untyped `propertyType`.
    let verb = crate::graph_type::standalone_verb(kind, def.property_type.as_deref())?;
    let variable = entity_variable(entity);
    let pattern = match entity {
        EntityKind::Node => format!("({variable}:`{}`)", def.label),
        EntityKind::Relationship => format!("()-[{variable}:`{}`]-()", def.label),
    };
    let properties = def
        .properties
        .iter()
        .map(|property| format!("{variable}.`{property}`"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "CREATE CONSTRAINT `{}` FOR {pattern} REQUIRE ({properties}) {verb}",
        def.name
    ))
}

/// The classification and `createStatement` for one constraint — the one
/// binding the `SHOW CONSTRAINTS` row's columns and the ADR-0481
/// `schema.cypher` mirror share: both callers classify through the same
/// derived-record-or-parts resolution and render through the one
/// [`constraint_create_statement`] renderer (one renderer, two callers —
/// never a second copy). `detail` is the row's derived-constraint record
/// of the same name; `None` is the SHOW column's NULL.
fn constraint_statement_binding(
    def: &relate_catalog::ConstraintDef,
    kind: Option<ConstraintType>,
    entity: EntityKind,
    current: &relate_parser::GraphTypeBody,
    detail: Option<&relate_model::Constraint>,
    cypher5: bool,
) -> (ConstraintClassification, Option<String>) {
    let classification = detail
        .map(|constraint| crate::graph_type::classify_constraint(current, constraint))
        .unwrap_or_else(|| {
            crate::graph_type::classify_constraint_parts(current, kind, entity, &def.label)
        });
    let statement = constraint_create_statement(def, kind, entity, classification, cypher5);
    (classification, statement)
}

/// The constraint's CREATE statement for the checkpoint's schema mirror
/// (ADR-0481 §3), classified and rendered exactly as the SHOW row does,
/// with no `CYPHER 5` preamble — a schema mirror must recreate, not hide.
/// `None` is the SHOW column's NULL (a graph-type-managed dependent
/// constraint, a kind with no standalone CREATE form); the mirror omits
/// the entry rather than fabricate a statement — a dependent
/// constraint's schema is already carried by the graph type's canonical
/// text.
pub(super) fn constraint_ddl_statement(
    def: &relate_catalog::ConstraintDef,
    current: &relate_parser::GraphTypeBody,
    derived: &BTreeMap<String, relate_model::Constraint>,
) -> Option<String> {
    // The mirror is the SHOW binding with the dialect's full form; one
    // binding, not a second spelling of it.
    constraint_statement_for(def, current, derived, false)
}

/// The `createStatement` a constraint's backing index row shows: Neo4j
/// renders the owning constraint's CREATE CONSTRAINT there, not the
/// index's own CREATE RANGE INDEX. Resolved through the same binding as
/// the `SHOW CONSTRAINTS` row, so the two renderings cannot drift.
pub(super) fn constraint_statement_for(
    def: &relate_catalog::ConstraintDef,
    current: &relate_parser::GraphTypeBody,
    derived: &BTreeMap<String, relate_model::Constraint>,
    cypher5: bool,
) -> Option<String> {
    let binding = constraint_binding(def, derived);
    let (kind, entity, detail) = (binding.kind, binding.entity, binding.detail);
    constraint_statement_binding(def, kind, entity, current, detail, cypher5).1
}

/// The shared per-row inputs of [`constraint_row`]: the typed binding
/// (`kind`, `entity`, `detail`), the current graph type and dialect flag for
/// the classification/rendering, and the committed-ness that decides the
/// schema id — bundled so the adjacent untyped facts cannot transpose and
/// the producer stays within the argument-count budget.
struct ConstraintRowContext<'a> {
    kind: Option<ConstraintType>,
    entity: EntityKind,
    detail: Option<&'a relate_model::Constraint>,
    current: &'a relate_parser::GraphTypeBody,
    cypher5: bool,
    committed: bool,
}

/// One `SHOW CONSTRAINTS` row: the constraint classified and rendered
/// through the one [`constraint_statement_binding`] the ADR-0481
/// `schema.cypher` mirror shares, and the Neo4j CE column set assembled
/// over it — the `indexes.rs` `index_row` twin. `detail` is the
/// derived-constraint record of the same name, resolved once by the
/// caller. The 12-column vocabulary and its order are
/// `relate-front`'s `show_columns::DEFAULT_CONSTRAINT_COLUMNS` ++
/// `NON_DEFAULT_CONSTRAINT_COLUMNS` — the pinning test at the bottom of
/// this module fails the moment the pair list drifts from that table.
fn constraint_row(def: relate_catalog::ConstraintDef, ctx: ConstraintRowContext<'_>) -> Row {
    let ConstraintRowContext {
        kind,
        entity,
        detail,
        current,
        cypher5,
        committed,
    } = ctx;
    let index_backed = kind.is_some_and(|kind| kind.is_index_backed());
    // `None` (an unparseable stored tag) keeps the legacy UNKNOWN
    // display; the typed mapping itself is total.
    let display_type = kind.map_or("UNKNOWN", |kind| show_constraint_type_name(kind, entity));
    let (classification, create_statement) =
        constraint_statement_binding(&def, kind, entity, current, detail, cypher5);
    let create_statement = optional_string_cell(create_statement.as_deref());
    catalog_row([
        // An uncommitted (in-transaction) constraint has no schema id yet;
        // Neo4j reports NULL for it. Committed rows report the engine's
        // stable placeholder id (schema ids are not part of the contract).
        ("id", crate::exec::catalog::schema_id_cell(committed)),
        ("name", Value::String(def.name.as_str().into())),
        ("type", Value::String(display_type.into())),
        ("entityType", Value::String(entity.display_name().into())),
        ("labelsOrTypes", string_list([def.label])),
        (
            "properties",
            if def.properties.is_empty() {
                Value::Null
            } else {
                string_list(def.properties)
            },
        ),
        (
            "ownedIndex",
            optional_string_cell(index_backed.then_some(def.name.as_str())),
        ),
        (
            "propertyType",
            optional_string_cell(def.property_type.as_deref()),
        ),
        (
            "enforcedLabel",
            optional_string_cell(
                detail.and_then(|constraint| constraint.enforced_label.as_deref()),
            ),
        ),
        (
            "classification",
            Value::String(classification.as_str().into()),
        ),
        (
            "options",
            // An index-backed constraint owns a range index whose config is
            // always the empty (default) one — Neo4j renders
            // `{indexConfig: {}}`. Every non-index-backed kind carries no
            // options at all (`NULL`). Derived (graph-type) constraints
            // follow the same rule: only their index-backed kinds show the
            // empty config.
            if index_backed {
                crate::exec::catalog::options_cell(&BTreeMap::new())
            } else {
                Value::Null
            },
        ),
        ("createStatement", create_statement),
    ])
}

pub(crate) fn show_constraints_rows(
    graph: &ExecGraph<'_>,
    c: &relate_parser::ShowConstraintsClause,
    qctx: QueryCtx<'_>,
) -> Result<Vec<Row>> {
    let show = graph.show_schema()?;
    let derived = &show.data.derived_constraints;
    let current = show.data.current_graph_type.to_body();
    // The entity filter, resolved once to the row producer's own typed
    // vocabulary before any row is walked: the clause's typed
    // `ConstraintEntityFilter` mapped onto `EntityKind` (never a
    // display-name string round-trip per row). The kind filter is the
    // parser's own typed vocabulary already — read as-is.
    let wanted_entity = match c.type_filter.entity {
        ConstraintEntityFilter::Any => None,
        ConstraintEntityFilter::Node => Some(EntityKind::Node),
        ConstraintEntityFilter::Relationship => Some(EntityKind::Relationship),
    };
    let wanted_kind = c.type_filter.kind;
    Ok(graph
        .named_constraints()
        .into_iter()
        .filter_map(|def| {
            // The one def→typed-binding resolution; the filters, the
            // `type` column, and the createStatement all read from
            // these typed bindings.
            let binding = constraint_binding(&def, derived);
            let (kind, entity, detail) = (binding.kind, binding.entity, binding.detail);
            let entity_matches = wanted_entity.is_none_or(|wanted| wanted == entity);
            let kind_matches = wanted_kind.is_none_or(|wanted| {
                kind.is_some_and(|k| constraint_filter_types(wanted).contains(&k))
            });
            (entity_matches && kind_matches).then_some((def, kind, entity, detail))
        })
        .map(|(def, kind, entity, detail)| {
            let committed = graph.base().has_constraint(&def.name);
            constraint_row(
                def,
                ConstraintRowContext {
                    kind,
                    entity,
                    detail,
                    current: &current,
                    cypher5: qctx.is_cypher5(),
                    committed,
                },
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::exec::test_support::{assert_row_keys_match, column, exec_autocommit, exec_cypher};
    use crate::graph::Graph;

    /// Pins the load-bearing YIELD-* invariant: `constraint_row` inserts
    /// keys in exactly
    /// `relate_front::show_columns::full_constraint_columns()` order
    /// — the front crate's own composition of its DEFAULT ++
    /// NON_DEFAULT tables; the shared key-order pin
    /// (`assert_row_keys_match`) carries the invariant.
    #[test]
    fn constraint_row_keys_match_front_column_tables() {
        assert_row_keys_match(
            Some("CREATE CONSTRAINT person_email FOR (n:Person) REQUIRE n.email IS NOT NULL"),
            "SHOW CONSTRAINTS YIELD *",
            &relate_front::show_columns::full_constraint_columns(),
        );
    }

    /// The `options` column follows the index-backed rule (live Neo4j
    /// 2026.09): unique/key constraints own a range index and render the
    /// empty `{indexConfig: {}}`; existence/property-type constraints
    /// render `NULL`.
    #[test]
    fn constraint_options_follow_the_index_backed_rule() {
        let g = Graph::default();
        exec_autocommit(
            &g,
            "CREATE CONSTRAINT k1 FOR (n:P) REQUIRE (n.id) IS UNIQUE",
        )
        .unwrap();
        exec_autocommit(
            &g,
            "CREATE CONSTRAINT e1 FOR (n:P) REQUIRE (n.name) IS NOT NULL",
        )
        .unwrap();
        let options = column(
            &exec_cypher(&g, "SHOW CONSTRAINTS YIELD options"),
            "options",
            |value| Some(value.clone()),
        );
        let empty_config = relate_core::Value::Map(
            [(
                crate::exec::ddl_options::KEY_INDEX_CONFIG.to_string(),
                relate_core::Value::Map(Default::default()),
            )]
            .into_iter()
            .collect::<BTreeMap<_, _>>()
            .into(),
        );
        // Catalog order is the name-keyed map's order: e1 before k1.
        assert_eq!(options, vec![relate_core::Value::Null, empty_config]);
    }
}
