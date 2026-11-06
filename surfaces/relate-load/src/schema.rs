//! Pull carries the source schema (ADR-0201): after the data stream
//! completes, `load_streaming` fetches `SHOW CONSTRAINTS` / `SHOW INDEXES`
//! from the source and replays the rows as standalone
//! `CREATE … IF NOT EXISTS` DDL against the target store.
//!
//! A pull never fails on schema: a row whose kind the mapping does not
//! cover, or a statement the engine rejects, is counted as skipped and
//! reported on stderr — the data is already loaded and stays loaded.

use relate::Connection;
use serde_json::Value as Json;

use crate::creds::Credentials;
use crate::{http, LoadError};

/// The required string column of a SHOW row (F-1-13): a missing or
/// mistyped column is the row's own error, never a silent "" in the DDL.
fn required(row: &serde_json::Map<String, Json>, key: &str) -> Result<String, String> {
    match row.get(key).and_then(Json::as_str) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => Err(format!("row is missing required column {key:?}")),
    }
}

/// One `SHOW CONSTRAINTS` row (the fields the pull's YIELD list selects).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConstraintRow {
    name: String,
    kind: String,
    entity_type: String,
    labels: Vec<String>,
    properties: Vec<String>,
    property_type: Option<String>,
}

/// The five shared SHOW-row fields (F-2-5): every constraint and index
/// row parses name/type/entityType/labelsOrTypes/properties the same
/// way; the two constructors differ only in their trailing optional
/// column.
struct RowBase {
    name: String,
    kind: String,
    entity_type: String,
    labels: Vec<String>,
    properties: Vec<String>,
}

impl RowBase {
    fn from_map(row: &serde_json::Map<String, Json>) -> Result<Self, String> {
        Ok(Self {
            name: required(row, "name")?,
            kind: required(row, "type")?,
            entity_type: required(row, "entityType")?,
            labels: string_list_field(row, "labelsOrTypes"),
            properties: string_list_field(row, "properties"),
        })
    }
}

impl ConstraintRow {
    fn from_map(row: &serde_json::Map<String, Json>) -> Result<Self, String> {
        let RowBase {
            name,
            kind,
            entity_type,
            labels,
            properties,
        } = RowBase::from_map(row)?;
        Ok(Self {
            name,
            kind,
            entity_type,
            labels,
            properties,
            property_type: opt_string_field(row, "propertyType"),
        })
    }
}

/// One `SHOW INDEXES` row (the fields the pull's YIELD list selects).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexRow {
    name: String,
    kind: String,
    entity_type: String,
    labels: Vec<String>,
    properties: Vec<String>,
    owning_constraint: Option<String>,
    state: String,
}

impl IndexRow {
    fn from_map(row: &serde_json::Map<String, Json>) -> Result<Self, String> {
        let RowBase {
            name,
            kind,
            entity_type,
            labels,
            properties,
        } = RowBase::from_map(row)?;
        Ok(Self {
            name,
            kind,
            entity_type,
            labels,
            properties,
            owning_constraint: opt_string_field(row, "owningConstraint"),
            state: string_field(row, "state"),
        })
    }
}

/// Applied/skipped counts of the schema replay (ADR-0201).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SchemaStats {
    pub constraints_applied: usize,
    pub constraints_skipped: usize,
    pub indexes_applied: usize,
    pub indexes_skipped: usize,
}

const SHOW_CONSTRAINTS: &str =
    "SHOW CONSTRAINTS YIELD name, type, entityType, labelsOrTypes, properties, propertyType";
const SHOW_INDEXES: &str =
    "SHOW INDEXES YIELD name, type, entityType, labelsOrTypes, properties, owningConstraint, state";

/// Fetch the source schema and replay it against the target connection
/// (ADR-0201). Best-effort end to end: a source that cannot answer the
/// SHOW queries (older server, missing privilege) leaves the loaded data
/// untouched and returns empty stats with a stderr warning; a single DDL
/// statement that the engine rejects is one skip, never a pull failure.
pub(crate) fn pull_schema(
    creds: &Credentials,
    allow_plaintext: bool,
    conn: &mut Connection,
) -> SchemaStats {
    let mut stats = SchemaStats::default();
    // Schema replay never fails a pull (ADR-0201): both fetches degrade
    // to a warning, both apply loops only count skips. So this function
    // returns the stats directly — no Result, no dead `?` at the call
    // site (F-4).
    match fetch_constraints(creds, allow_plaintext) {
        Ok((rows, malformed)) => {
            // Malformed rows are their own counted skip category
            // (F-1-13/17), surfaced in the pull output like any skip.
            stats.constraints_skipped += malformed;
            apply_constraints(conn, &rows, &mut stats);
        }
        Err(e) => eprintln!("warning: pull could not fetch source constraints: {e}"),
    }
    match fetch_indexes(creds, allow_plaintext) {
        Ok((rows, malformed)) => {
            stats.indexes_skipped += malformed;
            apply_indexes(conn, &rows, &mut stats);
        }
        Err(e) => eprintln!("warning: pull could not fetch source indexes: {e}"),
    }
    stats
}

/// One SHOW fetch skeleton (F-1-18): run the statement, zip
/// `data.fields`/`data.values` into per-row maps, count malformed
/// (non-array) rows — they are reported, never silently dropped (F-1-17).
fn fetch_show_rows(
    creds: &Credentials,
    allow_plaintext: bool,
    query: &str,
) -> Result<(Vec<serde_json::Map<String, Json>>, usize), LoadError> {
    let json = http::run_query(creds, query, &serde_json::Map::new(), allow_plaintext)?;
    rows_as_maps(&json)
}

/// The shared fetch-parse loop (F-2-3/4): fetch the SHOW rows, parse each
/// through the row constructor, count malformed rows and rows whose
/// required columns are missing as the fetch's own skip category — never
/// a silently-empty "" threaded into DDL (F-1-13). The two fetchers pass
/// their constructor; nothing else differs.
fn fetch_parsed<T>(
    creds: &Credentials,
    allow_plaintext: bool,
    query: &str,
    label: &str,
    from_map: fn(&serde_json::Map<String, Json>) -> Result<T, String>,
) -> Result<(Vec<T>, usize), LoadError> {
    let (rows, malformed) = fetch_show_rows(creds, allow_plaintext, query)?;
    let mut out = Vec::new();
    let mut bad = 0usize;
    for row in &rows {
        match from_map(row) {
            Ok(row) => out.push(row),
            Err(e) => {
                bad += 1;
                eprintln!("warning: pull skipped a malformed {label} row: {e}");
            }
        }
    }
    Ok((out, malformed + bad))
}

fn fetch_constraints(
    creds: &Credentials,
    allow_plaintext: bool,
) -> Result<(Vec<ConstraintRow>, usize), LoadError> {
    fetch_parsed(
        creds,
        allow_plaintext,
        SHOW_CONSTRAINTS,
        "constraint",
        ConstraintRow::from_map,
    )
}

fn fetch_indexes(
    creds: &Credentials,
    allow_plaintext: bool,
) -> Result<(Vec<IndexRow>, usize), LoadError> {
    fetch_parsed(
        creds,
        allow_plaintext,
        SHOW_INDEXES,
        "index",
        IndexRow::from_map,
    )
}

/// `data.values` rows zipped with `data.fields` into per-row maps. A
/// response without the expected envelope is a fetch error, not a panic.
fn rows_as_maps(json: &Json) -> Result<(Vec<serde_json::Map<String, Json>>, usize), LoadError> {
    let fields = json["data"]["fields"]
        .as_array()
        .ok_or_else(|| LoadError::Body("schema SHOW response has no data.fields".into()))?;
    let values = json["data"]["values"]
        .as_array()
        .ok_or_else(|| LoadError::Body("schema SHOW response has no data.values".into()))?;
    let malformed = values.iter().filter(|v| !v.is_array()).count();
    Ok((
        values
            .iter()
            .filter_map(Json::as_array)
            .map(|row| {
                fields
                    .iter()
                    .filter_map(Json::as_str)
                    .zip(row.iter().cloned())
                    .map(|(f, v)| (f.to_string(), v))
                    .collect()
            })
            .collect(),
        malformed,
    ))
}

fn string_field(row: &serde_json::Map<String, Json>, key: &str) -> String {
    row.get(key)
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string()
}

/// The optional string column (`propertyType`, `owningConstraint`): the
/// fourth accessor beside string_field/string_list_field (F-5-2).
fn opt_string_field(row: &serde_json::Map<String, Json>, key: &str) -> Option<String> {
    row.get(key).and_then(Json::as_str).map(str::to_string)
}

fn string_list_field(row: &serde_json::Map<String, Json>, key: &str) -> Vec<String> {
    row.get(key)
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Schema tokens quote through the canonical formatter helper (F-1-2):
/// one definition of the bare-identifier rule and the backtick doubling,
/// not a bespoke copy. `relate-load` already depends on
/// `relate-formatter`.
use relate_formatter::cypher_identifier as quote;

/// Validate an `IS TYPED` property-type spelling from the source before it
/// lands in a DDL statement (F-1: every other source token is
/// backtick-quoted; this one interpolates raw, so it gets a whitelist
/// shape check instead). Cypher property types are a closed grammar:
/// `INTEGER`/`FLOAT`/`STRING`/`BOOLEAN`/`POINT`/`DATE`/`LOCAL TIME`/
/// `ZONED TIME`/`LOCAL DATETIME`/`ZONED DATETIME`/`DURATION`, optional
/// `NOT NULL`, optional `LIST<…>`. Anything else (a backtick, a
/// statement terminator, …) makes the row unmappable — it is skipped by
/// the caller like any unmappable shape, never interpolated.
fn safe_property_type(pt: &str) -> Option<&str> {
    let upper = pt.trim();
    if upper.len() <= 64
        && upper
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == ' ' || c == '<' || c == '>' || c == ',')
        && upper.chars().next().is_some_and(|c| c.is_ascii_uppercase())
    {
        Some(upper)
    } else {
        None
    }
}

/// The `FOR` target of a DDL statement: node or relationship pattern with
/// the canonical variable (`n` / `r`).
fn for_target(entity_type: &str, labels: &[String]) -> Option<(&'static str, String)> {
    let label = labels.first()?;
    match entity_type {
        "NODE" => Some(("n", format!("(n:{})", quote(label)))),
        "RELATIONSHIP" => Some(("r", format!("()-[r:{}]-()", quote(label)))),
        _ => None,
    }
}

/// The constraint shapes the mapper knows (F-3) — the single shape
/// vocabulary. Membership gating (`parse_constraint_kind`) and the DDL
/// match both speak in these variants, so a shape the mapper does not
/// handle fails to compile at the match instead of falling through to a
/// runtime skip.
pub(crate) enum ConstraintShape {
    PropertyUniqueness,
    PropertyExistence,
    Key,
    PropertyType,
}

/// Parse a `SHOW CONSTRAINTS` `type` string — the
/// `{NODE|RELATIONSHIP}_{shape}` cross-product — into its halves; `None`
/// is an unsupported kind. The returned entity is the canonical
/// `'static` spelling, so it can flow straight into the KEY rendering.
/// Shared by the pull's DDL replay and the push's merge-key derivation
/// (ADR-0401): the one parser of the SHOW-row kind vocabulary.
pub(crate) fn parse_constraint_kind(kind: &str) -> Option<(&'static str, ConstraintShape)> {
    let (entity, shape) = kind.split_once('_')?;
    let entity = match entity {
        "NODE" => "NODE",
        "RELATIONSHIP" => "RELATIONSHIP",
        _ => return None,
    };
    let shape = match shape {
        "PROPERTY_UNIQUENESS" => ConstraintShape::PropertyUniqueness,
        "PROPERTY_EXISTENCE" => ConstraintShape::PropertyExistence,
        "KEY" => ConstraintShape::Key,
        "PROPERTY_TYPE" => ConstraintShape::PropertyType,
        _ => return None,
    };
    Some((entity, shape))
}

/// The bare comma-separated `var.prop` list (`n.a, n.b`).
fn props_inner(var: &str, properties: &[String]) -> String {
    properties
        .iter()
        .map(|p| format!("{var}.{}", quote(p)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The always-parenthesized shape (`(n.a)` even single-property): the KEY
/// constraint forms and an index's `ON` clause use it.
fn props_paren(var: &str, properties: &[String]) -> String {
    format!("({})", props_inner(var, properties))
}

/// Neo4j's composite spelling: parenthesized when more than one property,
/// bare for the single-property form (`REQUIRE (n.a, n.b) IS UNIQUE` vs
/// `REQUIRE n.a IS NOT NULL`). No mode flag — the shape computes it.
fn props_list(var: &str, properties: &[String]) -> String {
    let inner = props_inner(var, properties);
    if properties.len() > 1 {
        format!("({inner})")
    } else {
        inner
    }
}

/// What the mapper made of one schema row (constraint or index) — the one
/// decision enum both mappers return (F-1-22): the standalone DDL to
/// replay, a counted skip with its reason fully formatted at the mapper
/// (where the row's kind/entity context is in scope), or a filtered row
/// dropped uncounted.
enum DdlDecision {
    /// The standalone DDL to replay.
    Ddl(String),
    /// A counted skip; the string is the complete parenthesized reason.
    Skip(String),
    /// Filtered (constraint-backing, offline, LOOKUP index): expected,
    /// dropped uncounted.
    Filtered,
}

/// Map one `SHOW CONSTRAINTS` row to its standalone DDL (F-1-5: the kind
/// is the {NODE|RELATIONSHIP}_{shape} cross-product — parse the prefix
/// once, then the shape table; no 8-arm match of full strings).
fn constraint_ddl(row: &ConstraintRow) -> DdlDecision {
    // One lookup: the kind parses into its (entity, shape) halves — an
    // unknown kind skips here, a known kind needs no re-split.
    let (kind_entity, shape) = match parse_constraint_kind(&row.kind) {
        Some(pair) => pair,
        None => {
            return DdlDecision::Skip(format!("unsupported kind {} {}", row.kind, row.entity_type));
        }
    };
    let Some((var, target)) = for_target(&row.entity_type, &row.labels) else {
        return DdlDecision::Skip("unmappable shape: unknown entity type or no label".into());
    };
    // The kind's prefix IS the entity (NODE_… / RELATIONSHIP_…) — one
    // exact, symmetric comparison, no derived boolean (F-4-1/2).
    if kind_entity != row.entity_type {
        return DdlDecision::Skip(
            "unmappable shape: kind's entity does not match entityType".into(),
        );
    }
    let props = &row.properties;
    if props.is_empty() {
        return DdlDecision::Skip("unmappable shape: no properties to require".into());
    }
    // Total match over the shape vocabulary — no catch-all, so a new
    // ConstraintShape variant fails to compile until its rule is written.
    // Existence and type constraints are single-property-only; both arms
    // say so the same way (an early skip with its own precise reason).
    let require = match shape {
        ConstraintShape::PropertyUniqueness => {
            format!("REQUIRE {} IS UNIQUE", props_list(var, props))
        }
        ConstraintShape::PropertyExistence => {
            if props.len() != 1 {
                return DdlDecision::Skip(
                    "unmappable shape: multi-property existence constraint".into(),
                );
            }
            format!("REQUIRE {} IS NOT NULL", props_list(var, props))
        }
        ConstraintShape::Key => {
            format!("REQUIRE {} IS {kind_entity} KEY", props_paren(var, props))
        }
        ConstraintShape::PropertyType => {
            if props.len() != 1 {
                return DdlDecision::Skip(
                    "unmappable shape: multi-property type constraint".into(),
                );
            }
            // Optionality stays typed — None never enters the validator
            // as a sentinel (F-4-5).
            let Some(pt) = row.property_type.as_deref().and_then(safe_property_type) else {
                return DdlDecision::Skip(
                    "unmappable shape: missing or unmappable property type".into(),
                );
            };
            format!("REQUIRE {} IS TYPED {pt}", props_list(var, props))
        }
    };
    DdlDecision::Ddl(format!(
        "CREATE CONSTRAINT {} IF NOT EXISTS FOR {target} {require}",
        quote(&row.name)
    ))
}

/// The one index-filter definition (F-2: `apply_indexes` and `index_ddl`
/// both ask this — never two copies that can drift): a row is FILTERED
/// (dropped uncounted) when the index backs a constraint (replayed with
/// its constraint), is not ONLINE, or is an engine-internal LOOKUP. Rows
/// that pass the filter but whose kind is outside the mapping are the
/// counted skips.
fn index_is_filtered(row: &IndexRow) -> bool {
    row.owning_constraint.is_some() || row.state != "ONLINE" || row.kind == "LOOKUP"
}

/// Map one `SHOW INDEXES` row to its standalone DDL (ADR-0201), or the
/// skip/filter decision with its reason.
fn index_ddl(row: &IndexRow) -> DdlDecision {
    if index_is_filtered(row) {
        return DdlDecision::Filtered;
    }
    let keyword = match row.kind.as_str() {
        "RANGE" => "",
        "TEXT" => "TEXT ",
        "POINT" => "POINT ",
        // LOOKUP indexes are engine-internal (filtered above);
        // FULLTEXT/VECTOR stay a recorded gap (ADR-0201).
        _ => return DdlDecision::Skip("unsupported kind".into()),
    };
    let Some((var, target)) = for_target(&row.entity_type, &row.labels) else {
        return DdlDecision::Skip("unknown entity type or no label".into());
    };
    if row.properties.is_empty() {
        return DdlDecision::Skip("no properties to index".into());
    }
    // The ON clause is the always-parenthesized shape (one rendering of
    // var.prop lists, shared with the constraint arms).
    DdlDecision::Ddl(format!(
        "CREATE {keyword}INDEX {} IF NOT EXISTS FOR {target} ON {}",
        quote(&row.name),
        props_paren(var, &row.properties)
    ))
}

/// Run one standalone DDL statement inside the loader's transaction policy
/// (`with_txn`); an engine rejection is the caller's skip, not a failure.
fn apply_one(conn: &mut Connection, ddl: &str) -> Result<(), LoadError> {
    crate::materialize::with_txn(conn, |conn| {
        conn.query(ddl)
            .map_err(|e| LoadError::Engine(e.to_string()))?;
        Ok(())
    })
}

/// The one apply loop for schema rows (constraints and indexes alike):
/// replay each mappable DDL inside the transaction policy, count and warn
/// on an engine rejection or a mapper skip, drop filtered rows uncounted.
/// Returns the (applied, skipped) counts; the caller adds them to the
/// matching `SchemaStats` pair, so the pairing is the caller's type.
fn apply_schema_rows<'a>(
    conn: &mut Connection,
    label: &str,
    decisions: impl IntoIterator<Item = (&'a str, DdlDecision)>,
) -> (usize, usize) {
    let mut applied = 0;
    let mut skipped = 0;
    for (name, decision) in decisions {
        match decision {
            DdlDecision::Ddl(ddl) => match apply_one(conn, &ddl) {
                Ok(()) => applied += 1,
                Err(e) => {
                    skipped += 1;
                    eprintln!("warning: pull skipped {label} {name:?}: {e}");
                }
            },
            // The mapper's own classification carries the reason (F-1-22).
            DdlDecision::Skip(reason) => {
                skipped += 1;
                eprintln!("warning: pull skipped {label} {name:?} ({reason})");
            }
            DdlDecision::Filtered => {}
        }
    }
    (applied, skipped)
}

fn apply_constraints(conn: &mut Connection, rows: &[ConstraintRow], stats: &mut SchemaStats) {
    let (applied, skipped) = apply_schema_rows(
        conn,
        "constraint",
        rows.iter()
            .map(|row| (row.name.as_str(), constraint_ddl(row))),
    );
    stats.constraints_applied += applied;
    stats.constraints_skipped += skipped;
}

fn apply_indexes(conn: &mut Connection, rows: &[IndexRow], stats: &mut SchemaStats) {
    let (applied, skipped) = apply_schema_rows(
        conn,
        "index",
        rows.iter().map(|row| (row.name.as_str(), index_ddl(row))),
    );
    stats.indexes_applied += applied;
    stats.indexes_skipped += skipped;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-side view of a mapper's outcome (F-1-7: production carries
    /// the reason; tests only need the DDL or the failure).
    fn ddl_of(d: DdlDecision) -> Option<String> {
        match d {
            DdlDecision::Ddl(s) => Some(s),
            _ => None,
        }
    }

    fn ddl(row: &ConstraintRow) -> Option<String> {
        ddl_of(constraint_ddl(row))
    }

    fn index_ddl_str(row: &IndexRow) -> Option<String> {
        ddl_of(index_ddl(row))
    }
    use serde_json::json;

    fn constraint_row(
        name: &str,
        kind: &str,
        entity: &str,
        labels: &[&str],
        props: &[&str],
    ) -> ConstraintRow {
        ConstraintRow {
            name: name.into(),
            kind: kind.into(),
            entity_type: entity.into(),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            properties: props.iter().map(|s| s.to_string()).collect(),
            property_type: None,
        }
    }

    fn index_row(
        name: &str,
        kind: &str,
        entity: &str,
        labels: &[&str],
        props: &[&str],
        owning: Option<&str>,
        state: &str,
    ) -> IndexRow {
        IndexRow {
            name: name.into(),
            kind: kind.into(),
            entity_type: entity.into(),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            properties: props.iter().map(|s| s.to_string()).collect(),
            owning_constraint: owning.map(str::to_string),
            state: state.into(),
        }
    }

    #[test]
    fn uniqueness_constraint_maps_single_and_composite() {
        assert_eq!(
            ddl(&constraint_row(
                "c1",
                "NODE_PROPERTY_UNIQUENESS",
                "NODE",
                &["Person"],
                &["email"]
            ))
            .as_deref(),
            Some("CREATE CONSTRAINT c1 IF NOT EXISTS FOR (n:Person) REQUIRE n.email IS UNIQUE")
        );
        assert_eq!(
            ddl(&constraint_row(
                "c2",
                "RELATIONSHIP_PROPERTY_UNIQUENESS",
                "RELATIONSHIP",
                &["KNOWS"],
                &["a", "b"]
            ))
            .as_deref(),
            Some(
                "CREATE CONSTRAINT c2 IF NOT EXISTS FOR ()-[r:KNOWS]-() REQUIRE (r.a, r.b) IS UNIQUE"
            )
        );
    }

    #[test]
    fn existence_and_key_constraints_map() {
        assert_eq!(
            ddl(&constraint_row(
                "c3",
                "NODE_PROPERTY_EXISTENCE",
                "NODE",
                &["Person"],
                &["name"]
            ))
            .as_deref(),
            Some("CREATE CONSTRAINT c3 IF NOT EXISTS FOR (n:Person) REQUIRE n.name IS NOT NULL")
        );
        assert_eq!(
            ddl(&constraint_row(
                "c4",
                "RELATIONSHIP_PROPERTY_EXISTENCE",
                "RELATIONSHIP",
                &["KNOWS"],
                &["since"]
            ))
            .as_deref(),
            Some(
                "CREATE CONSTRAINT c4 IF NOT EXISTS FOR ()-[r:KNOWS]-() REQUIRE r.since IS NOT NULL"
            )
        );
        assert_eq!(
            ddl(&constraint_row(
                "c5",
                "NODE_KEY",
                "NODE",
                &["Person"],
                &["a", "b"]
            ))
            .as_deref(),
            Some(
                "CREATE CONSTRAINT c5 IF NOT EXISTS FOR (n:Person) REQUIRE (n.a, n.b) IS NODE KEY"
            )
        );
        assert_eq!(
            ddl(&constraint_row(
                "c6",
                "RELATIONSHIP_KEY",
                "RELATIONSHIP",
                &["KNOWS"],
                &["id"]
            ))
            .as_deref(),
            Some(
                "CREATE CONSTRAINT c6 IF NOT EXISTS FOR ()-[r:KNOWS]-() REQUIRE (r.id) IS RELATIONSHIP KEY"
            )
        );
    }

    #[test]
    fn property_type_constraint_needs_the_property_type_column() {
        let mut row = constraint_row("c7", "NODE_PROPERTY_TYPE", "NODE", &["Person"], &["age"]);
        assert!(ddl(&row).is_none());
        row.property_type = Some("INTEGER".into());
        assert_eq!(
            ddl(&row).as_deref(),
            Some(
                "CREATE CONSTRAINT c7 IF NOT EXISTS FOR (n:Person) REQUIRE n.age IS TYPED INTEGER"
            )
        );
    }

    #[test]
    fn unknown_constraint_kinds_and_bad_shapes_do_not_map() {
        for row in [
            constraint_row("x", "NODE_LABEL_EXISTENCE", "NODE", &["Person"], &["name"]),
            // Existence is single-property; a multi-property row cannot map.
            constraint_row(
                "x",
                "NODE_PROPERTY_EXISTENCE",
                "NODE",
                &["Person"],
                &["a", "b"],
            ),
            // A node kind on a relationship entity (or vice versa).
            constraint_row("x", "NODE_KEY", "RELATIONSHIP", &["KNOWS"], &["a"]),
            // No label/type: no target.
            constraint_row("x", "NODE_PROPERTY_UNIQUENESS", "NODE", &[], &["a"]),
            // No properties: nothing to require.
            constraint_row("x", "NODE_PROPERTY_UNIQUENESS", "NODE", &["Person"], &[]),
        ] {
            assert!(ddl(&row).is_none(), "{row:?}");
        }
    }

    #[test]
    fn names_labels_and_properties_are_quoted_when_needed() {
        assert_eq!(
            ddl(&constraint_row(
                "my constraint",
                "NODE_PROPERTY_UNIQUENESS",
                "NODE",
                &["Label With Space"],
                &["weird`prop"]
            ))
            .as_deref(),
            Some(
                "CREATE CONSTRAINT `my constraint` IF NOT EXISTS FOR (n:`Label With Space`) REQUIRE n.`weird``prop` IS UNIQUE"
            )
        );
        assert_eq!(quote("plain_1"), "plain_1");
        assert_eq!(quote("1st"), "`1st`");
        assert_eq!(quote(""), "``");
    }

    #[test]
    fn index_kinds_map_and_filters_hold() {
        assert_eq!(
            index_ddl_str(&index_row(
                "i1",
                "RANGE",
                "NODE",
                &["Person"],
                &["name"],
                None,
                "ONLINE"
            ))
            .as_deref(),
            Some("CREATE INDEX i1 IF NOT EXISTS FOR (n:Person) ON (n.name)")
        );
        assert_eq!(
            index_ddl_str(&index_row(
                "i2",
                "TEXT",
                "RELATIONSHIP",
                &["KNOWS"],
                &["since"],
                None,
                "ONLINE"
            ))
            .as_deref(),
            Some("CREATE TEXT INDEX i2 IF NOT EXISTS FOR ()-[r:KNOWS]-() ON (r.since)")
        );
        assert_eq!(
            index_ddl_str(&index_row(
                "i3",
                "POINT",
                "NODE",
                &["Person"],
                &["loc"],
                None,
                "ONLINE"
            ))
            .as_deref(),
            Some("CREATE POINT INDEX i3 IF NOT EXISTS FOR (n:Person) ON (n.loc)")
        );
        // A composite range index keeps the property list order.
        assert_eq!(
            index_ddl_str(&index_row(
                "i4",
                "RANGE",
                "NODE",
                &["Person"],
                &["a", "b"],
                None,
                "ONLINE"
            ))
            .as_deref(),
            Some("CREATE INDEX i4 IF NOT EXISTS FOR (n:Person) ON (n.a, n.b)")
        );
        // Constraint-backing, offline, LOOKUP, FULLTEXT and VECTOR rows do
        // not map.
        for row in [
            index_row("b", "RANGE", "NODE", &["P"], &["p"], Some("c1"), "ONLINE"),
            index_row("b", "RANGE", "NODE", &["P"], &["p"], None, "POPULATING"),
            index_row("b", "LOOKUP", "NODE", &[], &[], None, "ONLINE"),
            index_row("b", "FULLTEXT", "NODE", &["P"], &["p"], None, "ONLINE"),
            index_row("b", "VECTOR", "NODE", &["P"], &["p"], None, "ONLINE"),
        ] {
            assert!(index_ddl_str(&row).is_none(), "{row:?}");
        }
    }

    #[test]
    fn show_response_rows_parse_into_field_maps() {
        let body = json!({
            "data": {
                "fields": ["name", "type", "labelsOrTypes", "properties"],
                "values": [["c1", "NODE_PROPERTY_UNIQUENESS", ["Person"], ["email"]]],
            },
            "errors": [],
        });
        let (rows, malformed) = rows_as_maps(&body).unwrap();
        assert_eq!(malformed, 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(string_field(&rows[0], "name"), "c1");
        assert_eq!(string_list_field(&rows[0], "labelsOrTypes"), ["Person"]);
        // A non-array row is counted as malformed (F-1-17), never silently
        // dropped.
        let mixed = json!({
            "data": {"fields": ["name"], "values": [["ok"], {"not": "an array"}]},
            "errors": [],
        });
        let (rows, malformed) = rows_as_maps(&mixed).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(malformed, 1);
        // A response without the envelope is an error, not a panic.
        assert!(rows_as_maps(&json!({"unexpected": true})).is_err());
    }
}
