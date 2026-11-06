//! SHOW FUNCTIONS / SHOW PROCEDURES static catalogs (CIP discoverability).
use crate::catalog_meta::ProcArgMeta;
use crate::function_registry::{function_metas, FunctionMeta, CATALOG_ONLY_FUNCTIONS};
use crate::procedure_registry::BUILTIN_PROCEDURE_SIGNATURES;
use indexmap::IndexMap;
use relate_core::PreviewFeatures;
use relate_front::KNOWN_FUNCTIONS;
use relate_model::CypherValue;

type Row = IndexMap<String, CypherValue>;

/// One entry of a function's/procedure's `argumentDescription`/
/// `returnDescription` map column (CIP-84): `name`/`description`/`type`/
/// `isDeprecated` always present, `default` only when the parameter
/// actually has a default value (CIP-84 show-functions-deprecation.feature
/// [11]/show-procedures-deprecation.feature [10]/[12]). The entries live in
/// the canonical `catalog_meta` shape both registries share.
fn arg_meta_value(a: &ProcArgMeta) -> CypherValue {
    let mut m = IndexMap::new();
    m.insert("name".to_string(), CypherValue::Str(a.name.to_string()));
    m.insert(
        "description".to_string(),
        CypherValue::Str(a.description.to_string()),
    );
    m.insert("type".to_string(), CypherValue::Str(a.type_str.to_string()));
    m.insert("isDeprecated".to_string(), CypherValue::Bool(a.deprecated));
    if let Some(d) = a.default {
        m.insert("default".to_string(), CypherValue::Str(d.to_string()));
    }
    CypherValue::Map(m)
}

/// Renders a comma-separated `reqParamList, optParamList` (CIP-113's
/// abstract syntax: every parameter as `name :: type` or, when it carries a
/// default, `name = default :: type`). Required parameters are expected to
/// be listed before optional ones in `args` itself -- callers build `args`
/// in that order, so this function doesn't need to reorder anything (CIP-113
/// procedure-signatures.feature [15]).
fn render_param_list(args: &[ProcArgMeta]) -> String {
    args.iter()
        .map(|a| match a.default {
            Some(d) => format!("{} = {} :: {}", a.name, d, a.type_str),
            None => format!("{} :: {}", a.name, a.type_str),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders a CIP-113 function signature: `name(paramList) :: returnType`,
/// with no parentheses around the (possibly closed-union) return type and
/// an empty `()` when `args` is empty.
fn render_function_signature(name: &str, args: &[ProcArgMeta], return_type: &str) -> String {
    format!("{}({}) :: {}", name, render_param_list(args), return_type)
}

/// Renders a CIP-113 procedure signature: `name(paramList) :: (ret :: type,
/// ...)`, with the return-type suffix dropped entirely (not `::VOID`,
/// `::()`, or `::NOTHING` -- all three were considered and rejected per the
/// CIP's Revision History) when the procedure yields no columns.
fn render_procedure_signature(name: &str, args: &[ProcArgMeta], returns: &[ProcArgMeta]) -> String {
    let params = render_param_list(args);
    if returns.is_empty() {
        format!("{name}({params})")
    } else {
        let rets = returns
            .iter()
            .map(|r| format!("{} :: {}", r.name, r.type_str))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{name}({params}) :: ({rets})")
    }
}

/// Every advertised function name, in catalog order, filtered by the preview
/// set: the one derivation `function_catalog_rows` renders and the
/// registry-coverage test compares its tables against, so the name sources
/// and the listing predicate are spelled once.
pub(crate) fn advertised_function_names(
    preview: PreviewFeatures,
) -> impl Iterator<Item = &'static str> {
    KNOWN_FUNCTIONS
        .iter()
        .copied()
        .chain(relate_core::temporal::clock_spellings())
        .chain(CATALOG_ONLY_FUNCTIONS.iter().copied())
        .filter(move |&raw| relate_front::validate::function_passes_name_gate(raw, preview))
}

/// Build the function catalog as virtual rows, one per registry row in the
/// known-function registry (`validate::KNOWN_FUNCTIONS`) that also backs
/// function-call validation — so every callable built-in is discoverable
/// and the catalog can't drift out of sync with what actually works. The
/// clock-access spellings are generated from their own vocabulary (they are
/// not registry entries) and the catalog-only `graph.*` names
/// (`CATALOG_ONLY_FUNCTIONS`, CIP-230) ride beside them through the same
/// renderer. A name with arity overloads (`range`, `round`, `coll.flatten`,
/// …) contributes one row per signature (CIP-113).
///
/// `preview` is the store-level preview-feature set (ADR-0371, ADR-0497): a
/// gated name is omitted unless its feature is enabled, so discovery never
/// advertises a name the admission gate would refuse. The predicate is the
/// front's [`function_passes_name_gate`], the listing side of the same table
/// `validate_function_admission` reads.
pub fn function_catalog_rows(preview: PreviewFeatures) -> Vec<Row> {
    advertised_function_names(preview)
        .flat_map(|raw| {
            let metas: Vec<&'static FunctionMeta> = function_metas(raw).collect();
            // Every advertised name owns at least one registry row; the
            // pairing is pinned by
            // `the_metadata_tables_cover_exactly_the_advertised_registry`.
            assert!(
                !metas.is_empty(),
                "no function_registry row for advertised `{raw}`"
            );
            metas
        })
        .map(function_row)
        .collect()
}

/// One function's catalog row, rendered from its [`FunctionMeta`]: the
/// display-spelling name (ADR-0497 §11), the category, the `aggregating`
/// cell, description, CIP-113 signature, argument descriptions and
/// deprecation cells all come from the registry entry, so the renderer never
/// re-derives a name or a classification.
fn function_row(meta: &FunctionMeta) -> Row {
    let mut r = Row::new();
    r.insert("name".to_string(), CypherValue::Str(meta.name.to_string()));
    r.insert(
        "category".to_string(),
        CypherValue::Str(meta.category.as_str().to_string()),
    );
    r.insert("isBuiltIn".to_string(), CypherValue::Bool(true));
    r.insert(
        "aggregating".to_string(),
        CypherValue::Bool(meta.is_aggregating()),
    );
    r.insert(
        "description".to_string(),
        CypherValue::Str(meta.description.to_string()),
    );
    let signature = meta
        .special_signature
        .map(|tail| format!("{}{}", meta.name, tail))
        .unwrap_or_else(|| render_function_signature(meta.name, meta.args, meta.return_type));
    r.insert("signature".to_string(), CypherValue::Str(signature));
    r.insert(
        "argumentDescription".to_string(),
        CypherValue::List(meta.args.iter().map(arg_meta_value).collect()),
    );
    r.insert(
        "returnDescription".to_string(),
        CypherValue::Str(meta.return_type.to_string()),
    );
    r.insert(
        "isDeprecated".to_string(),
        CypherValue::Bool(meta.deprecated_by.is_some()),
    );
    r.insert(
        "deprecatedBy".to_string(),
        meta.deprecated_by
            .map(|deprecated| CypherValue::Str(deprecated.display().to_string()))
            .unwrap_or(CypherValue::Null),
    );
    r
}

/// `SHOW PROCEDURES` catalog rows, rendered from the canonical
/// `procedure_registry` table (the same table the executor's `CALL`
/// `SHOW PROCEDURES` catalog rows, rendered from the canonical
/// `procedure_registry` table (the same table the executor's `CALL`
/// dispatch and `Database::validate_calls` bind against), in name order
/// (live Neo4j's own ordering, sorted over the registry). The
/// `catalog-only` entries (`executable: false`) are inventory rows the
/// corpus lists but never calls (`db.resampleIndex`, `dbms.setConfigValue`,
/// the `cdc.*` family, the `dbms.cluster.*` and `fleetManagement.*`
/// families, …): `SHOW PROCEDURES` reports them and a `CALL` refuses them.
///
/// `preview` is the store-level preview-feature set (ADR-0371, ADR-0378):
/// a signature whose gating feature is disabled is omitted, so discovery
/// never advertises a procedure the `CALL` gate would refuse. The mapping
/// is the executor's one [`procedure_preview_feature`](crate::procedure_preview_feature),
/// and with the gate enabled every row is identical to the ungated table.
pub fn procedure_catalog_rows(preview: PreviewFeatures) -> Vec<Row> {
    // Neo4j reports the catalog in name order (`db.cdc.current` before
    // `db.cdc.earliest` …, and `tx.getMetaData` before `tx.setMetaData`),
    // not in registration order.
    let mut listed: Vec<&'static crate::procedure_registry::ProcSignature> =
        BUILTIN_PROCEDURE_SIGNATURES
            .iter()
            .filter(|signature| signature.listed)
            .filter(|signature| {
                crate::procedure_gate::procedure_preview_feature(signature.name)
                    .is_none_or(|feature| preview.enabled(feature))
            })
            .collect();
    listed.sort_unstable_by(|a, b| a.name.cmp(b.name));
    listed
        .into_iter()
        .map(|signature| {
            let mut r = Row::new();
            r.insert(
                "name".to_string(),
                CypherValue::Str(signature.name.to_string()),
            );
            r.insert(
                "description".into(),
                CypherValue::Str(signature.description.to_string()),
            );
            r.insert("mode".into(), CypherValue::Str(signature.mode.to_string()));
            r.insert(
                "worksOnSystem".into(),
                CypherValue::Bool(signature.works_on_system),
            );
            r.insert("admin".into(), CypherValue::Bool(signature.admin));
            r.insert(
                "signature".into(),
                CypherValue::Str(render_procedure_signature(
                    signature.name,
                    signature.args,
                    signature.returns,
                )),
            );
            r.insert(
                "argumentDescription".into(),
                CypherValue::List(signature.args.iter().map(arg_meta_value).collect()),
            );
            r.insert(
                "returnDescription".into(),
                CypherValue::List(signature.returns.iter().map(arg_meta_value).collect()),
            );
            let deprecated = signature.deprecated_by.is_some();
            r.insert("isDeprecated".into(), CypherValue::Bool(deprecated));
            let mut option = IndexMap::new();
            option.insert("deprecated".into(), CypherValue::Bool(deprecated));
            r.insert("option".into(), CypherValue::Map(option));
            r.insert(
                "deprecatedBy".into(),
                signature
                    .deprecated_by
                    .map(|deprecated| CypherValue::Str(deprecated.display().to_string()))
                    .unwrap_or(CypherValue::Null),
            );
            r
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::function_registry::FunctionCategory;
    use crate::procedure_registry::is_ontology_procedure;
    use crate::Graph;

    /// The one required-cell reader: a missing or non-STRING cell panics
    /// naming the row and column.
    fn text<'a>(row: &'a Row, column: &str, name: &str) -> &'a str {
        row[column]
            .as_str()
            .unwrap_or_else(|| panic!("{name} has no {column} cell"))
    }

    fn name_of(row: &Row) -> &str {
        row["name"]
            .as_str()
            .expect("every catalog row carries a string name")
    }

    fn is_ontology_row(row: &Row) -> bool {
        is_ontology_procedure(name_of(row))
    }

    /// A row's `signature` cell starts with its own `name` cell: the renderer
    /// names a row from its registry name for both cells — including the
    /// hand-written `special_signature` tails — so a disagreement means the
    /// row carries two names.
    fn assert_signature_names_row(row: &Row) {
        let name = name_of(row);
        let signature = text(row, "signature", name);
        assert!(
            signature.starts_with(&format!("{name}(")),
            "{name} renders `{signature}`"
        );
    }

    /// The catalog rows whose rendered name is spelled in mixed case, one
    /// entry per rendered row (no dedup: a name that lost its spelling on an
    /// arity sibling row must fail the count, not hide behind it), sorted:
    /// the display-spelling surface of one preview set.
    fn mixed_case_names(rows: &[Row]) -> Vec<&str> {
        let mut names: Vec<&str> = rows
            .iter()
            .map(name_of)
            .filter(|name| name.chars().any(|c| c.is_ascii_uppercase()))
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn show_functions_hides_the_preview_gated_names_while_their_flags_are_disabled() {
        // ADR-0497 §4: the catalogue and the admission gate read the one
        // `function_preview_feature` table, so a disabled name is listed
        // nowhere.
        let disabled: Vec<String> = function_catalog_rows(Graph::default().preview())
            .iter()
            .map(|row| name_of(row).to_ascii_lowercase())
            .collect();
        // Derived from the one table, never hand-copied: `SHOW FUNCTIONS`
        // renders display spellings (`coll.hasAnyOf`), so the lowercased
        // row names are compared against the accessor's lowercase entries.
        let gated: Vec<&str> = relate_front::validate::gated_function_names().collect();
        for name in &gated {
            assert!(
                !disabled.contains(&name.to_string()),
                "{name} must not be listed without its opt-in"
            );
        }

        let mut enabled = Graph::default();
        enabled.configure_preview(relate_front::validate::every_name_gate());
        let listed: Vec<String> = function_catalog_rows(enabled.preview())
            .iter()
            .map(|row| name_of(row).to_ascii_lowercase())
            .collect();
        for name in &gated {
            assert!(
                listed.contains(&name.to_string()),
                "{name} must be listed with its opt-in"
            );
        }
        // Enabling the gates adds exactly the gated names and leaves every
        // other row identical and in order.
        assert_eq!(listed.len(), disabled.len() + gated.len());
        let other_rows: Vec<String> = listed
            .iter()
            .filter(|name| !gated.contains(&name.as_str()))
            .cloned()
            .collect();
        assert_eq!(other_rows, disabled);
    }

    #[test]
    fn show_procedures_hides_the_ontology_family_while_the_preview_is_disabled() {
        // ADR-0378: a default executor graph carries the empty preview set,
        // so no ontology-family row is discoverable.
        let disabled = procedure_catalog_rows(Graph::default().preview());
        assert!(
            disabled.iter().all(|row| !is_ontology_row(row)),
            "a disabled store lists no db.ontology.* row"
        );

        let mut enabled = Graph::default();
        enabled.configure_preview(PreviewFeatures::parse("ontology").unwrap());
        let listed = procedure_catalog_rows(enabled.preview());
        // Enabling the gate adds exactly the registry's ontology-family rows
        // that the catalog lists — the count applies the catalog's own
        // `listed` filter as well as the family predicate — and leaves every
        // other row identical and in order.
        let catalog_family: Vec<_> = BUILTIN_PROCEDURE_SIGNATURES
            .iter()
            .filter(|signature| signature.listed)
            .filter(|signature| is_ontology_procedure(signature.name))
            .collect();
        assert_eq!(listed.len(), disabled.len() + catalog_family.len());
        let other_rows: Vec<Row> = listed
            .iter()
            .filter(|row| !is_ontology_row(row))
            .cloned()
            .collect();
        assert_eq!(other_rows, disabled);

        // Each listed row carries the registry's own name, mode,
        // description and rendered signature: the gate changes availability
        // only.
        for signature in catalog_family {
            let row = listed
                .iter()
                .find(|row| name_of(row) == signature.name)
                .unwrap_or_else(|| panic!("{} must be listed while enabled", signature.name));
            assert_eq!(row["mode"], CypherValue::Str(signature.mode.to_string()));
            assert_eq!(
                row["description"],
                CypherValue::Str(signature.description.to_string())
            );
            assert_eq!(
                row["signature"],
                CypherValue::Str(render_procedure_signature(
                    signature.name,
                    signature.args,
                    signature.returns,
                ))
            );
        }
    }

    /// ADR-0497 §6: every `SHOW PROCEDURES` row the catalog lists renders a
    /// non-empty description and the CIP-113 signature of its own name —
    /// the catalog is the whole inventory, so a row with a placeholder or a
    /// blank signature is a catalogue gap rather than a display detail.
    #[test]
    fn every_listed_row_renders_a_description_and_a_cip113_signature() {
        let rows = procedure_catalog_rows(Graph::default().preview());
        assert!(!rows.is_empty());
        for row in &rows {
            let name = name_of(row);
            let description = text(row, "description", name);
            assert!(!description.is_empty(), "{name} has an empty description");
            assert_signature_names_row(row);
        }
    }

    /// ADR-0497 §8: every `SHOW FUNCTIONS` row of a default store carries the
    /// brief columns — a non-empty description and a CIP-113 signature
    /// rendered from typed parameters (never the old `(...) :: ANY`
    /// placeholder). The category cell is the registry row's own
    /// `FunctionCategory` (rendered verbatim by `function_row`), and the
    /// row-per-advertised-name pairing is pinned by
    /// `function_registry::the_metadata_tables_cover_exactly_the_advertised_registry`.
    #[test]
    fn every_function_catalog_row_carries_full_metadata() {
        let rows = function_catalog_rows(Graph::default().preview());
        assert!(!rows.is_empty());
        for row in &rows {
            let name = name_of(row);
            let description = text(row, "description", name);
            assert!(!description.is_empty(), "{name} has an empty description");
            let signature = text(row, "signature", name);
            assert!(
                !signature.contains("(...) :: ANY"),
                "{name} keeps the placeholder signature `{signature}`"
            );
            assert_signature_names_row(row);
        }
    }

    /// The `aggregating` cell is the row's own `Aggregating` category, never a
    /// second name-based lookup: every listed row's cell agrees with its
    /// category and with the engine's aggregate vocabulary, so a registry row
    /// cannot claim a classification the aggregation machinery does not fold
    /// (or miss one it does). The aggregate rows are the aggregate catalogue,
    /// gated aliases included.
    #[test]
    fn the_aggregating_cell_is_the_rows_own_aggregate_category() {
        let mut every_gate = Graph::default();
        every_gate.configure_preview(relate_front::validate::every_name_gate());
        let rows = function_catalog_rows(every_gate.preview());
        for row in &rows {
            let name = name_of(row);
            let category = text(row, "category", name);
            let aggregating = row["aggregating"]
                .as_bool()
                .unwrap_or_else(|| panic!("{name} has no aggregating cell"));
            assert_eq!(
                aggregating,
                category == FunctionCategory::Aggregating.as_str(),
                "{name}'s aggregating cell disagrees with its `{category}` category"
            );
            assert_eq!(
                aggregating,
                relate_front::agg::is_aggregate_name(name),
                "{name}'s aggregating cell disagrees with the aggregate vocabulary"
            );
        }
        let mut aggregates: Vec<&str> = rows
            .iter()
            .filter(|row| row["aggregating"].as_bool() == Some(true))
            .map(name_of)
            .collect();
        aggregates.sort_unstable();
        assert_eq!(
            aggregates,
            [
                "avg",
                "collect",
                "collect_list",
                "count",
                "max",
                "min",
                "percentileCont",
                "percentileDisc",
                "percentile_cont",
                "percentile_disc",
                "stDev",
                "stDevP",
                "stddev_pop",
                "stddev_samp",
                "stdev_pop",
                "stdev_samp",
                "sum",
            ]
        );
    }

    /// The `show_expectations` corpus rows this metadata is filled from
    /// (`features/correctness/ShowFunctions.feature`): the spot rows assert
    /// name, category and description verbatim, and the `Numeric` filter
    /// lists the rounding family once per `round` arity (lines 2660-2670).
    #[test]
    fn function_metadata_matches_the_recorded_corpus_rows() {
        let rows = function_catalog_rows(Graph::default().preview());
        let named =
            |name: &str| -> Vec<&Row> { rows.iter().filter(|row| name_of(row) == name).collect() };
        let one = |name: &str| -> &Row {
            let mut found = named(name);
            assert_eq!(found.len(), 1, "{name} must be listed once");
            found.pop().expect("checked len")
        };

        let abs = one("abs");
        assert_eq!(text(abs, "category", name_of(abs)), "Numeric");
        assert_eq!(
            text(abs, "description", name_of(abs)),
            "Returns the absolute value of an `INTEGER` or `FLOAT`."
        );

        let from_element_id = one("db.nameFromElementId");
        assert_eq!(
            text(from_element_id, "category", name_of(from_element_id)),
            "Database"
        );
        assert_eq!(
            text(from_element_id, "description", name_of(from_element_id)),
            "Resolves the database name for the given element id"
        );

        let tail = one("tail");
        assert_eq!(text(tail, "category", name_of(tail)), "List");
        assert_eq!(
            text(tail, "description", name_of(tail)),
            "Returns all but the first element in a `LIST<ANY>`."
        );

        // Lines 82-83: the arity overloads render as two rows with their own
        // descriptions, transcribed verbatim from the corpus (the second
        // cell really does end in `....`).
        let flatten = named("coll.flatten");
        let mut descriptions: Vec<&str> = flatten
            .iter()
            .map(|row| text(row, "description", name_of(row)))
            .collect();
        descriptions.sort_unstable();
        assert_eq!(
            descriptions,
            [
                "Returns a list flattened to ....",
                "Returns a list flattened to the given nesting depth.",
            ]
        );

        // Lines 2660-2670: the `Numeric` category is exactly the rounding
        // family, with `round` repeated per arity row.
        let mut numeric: Vec<&str> = rows
            .iter()
            .filter(|row| text(row, "category", name_of(row)) == "Numeric")
            .map(name_of)
            .collect();
        numeric.sort_unstable();
        assert_eq!(
            numeric,
            [
                "abs", "ceil", "ceiling", "floor", "isNaN", "rand", "round", "round", "round",
                "sign",
            ]
        );
    }

    /// The display-spelling contract (ADR-0497 §11): the catalog's mixed-case
    /// names are exactly the reference catalogue's. The reference is the
    /// Cypher-25 corpus's generated `ShowFunctions*` tables, live-verified
    /// against Neo4j 2026.10.1 (`SHOW FUNCTIONS YIELD name`): 42 default
    /// rows, plus the three preview-gated `coll.has*` rows, which render
    /// their CIP camelCase. Every other row renders its lowercase spelling,
    /// so a name that loses or gains a spelling — or gains a typo'd one —
    /// fails here instead of silently rendering lowercase. That every
    /// rendered spelling is still callable in its own casing is the
    /// case-insensitive matching fact pinned end-to-end by
    /// `canonical_registries::catalog_display_spellings_are_callable_in_their_own_casing`.
    #[test]
    fn the_catalog_renders_the_reference_display_spellings() {
        /// The 42 default-surface names the reference catalogue spells in
        /// mixed case.
        const DEFAULT: &[&str] = &[
            "allReduce",
            "allShortestPaths",
            "coll.indexOf",
            "db.nameFromElementId",
            "duration.inDays",
            "duration.inMonths",
            "duration.inSeconds",
            "elementId",
            "endNode",
            "graph.byElementId",
            "graph.byName",
            "graph.propertiesByName",
            "isEmpty",
            "isNaN",
            "nullIf",
            "percentileCont",
            "percentileDisc",
            "point.withinBBox",
            "randomUUID",
            "shortestPath",
            "startNode",
            "stDev",
            "stDevP",
            "string.indexOf",
            "string.regexReplace",
            "toBoolean",
            "toBooleanList",
            "toBooleanOrNull",
            "toFloat",
            "toFloatList",
            "toFloatOrNull",
            "toInteger",
            "toIntegerList",
            "toIntegerOrNull",
            "toLower",
            "toString",
            "toStringList",
            "toStringOrNull",
            "toUpper",
            "uuid.leastSignificantBits",
            "uuid.mostSignificantBits",
            "valueType",
        ];
        /// The preview-gated names, rendered in their CIP camelCase: they
        /// join the default set exactly while their flags are on.
        const GATED: &[&str] = &["coll.hasAllOf", "coll.hasAnyOf", "coll.hasNoneOf"];

        let mut default_expected = DEFAULT.to_vec();
        default_expected.sort_unstable();
        let default_rows = function_catalog_rows(Graph::default().preview());
        assert_eq!(mixed_case_names(&default_rows), default_expected);

        let mut every_expected: Vec<&str> = DEFAULT.iter().chain(GATED).copied().collect();
        every_expected.sort_unstable();
        let mut every_gate = Graph::default();
        every_gate.configure_preview(relate_front::validate::every_name_gate());
        let gated_rows = function_catalog_rows(every_gate.preview());
        assert_eq!(mixed_case_names(&gated_rows), every_expected);

        // A row's `signature` cell starts with its own `name` cell: the only
        // hand-written signature text (`special_signature` tails, e.g.
        // `allReduce`) must agree with the row's own name spelling, or the row
        // would carry two names. Covers every rendered row, gated rows
        // included.
        for row in &gated_rows {
            assert_signature_names_row(row);
        }
    }
}
