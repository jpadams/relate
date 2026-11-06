//! The boolean-requiring sites carry the error *class* Neo4j surfaces
//! (ADR-0511, LABS-400): a statically non-boolean predicate (`WHERE 1`),
//! an entity variable in a predicate position (`WHERE n`), and the
//! list-quantifier's non-boolean `WHERE` predicate are the argument type
//! validator's family — `Neo.ClientError.Statement.TypeError`, GQLSTATUS
//! `22G03` — superseding the SyntaxError class ADR-0003 pinned for them.
//! The class flows from the one ABI v2 classification table
//! (ADR-0502 §2) the typed `InvalidArgumentType` variant rides.
//!
//! Every expectation here was verified against
//! `neo4j:2026.09-enterprise` (Cypher 25) through `cypher-shell`
//! (2026-11-04; ADR-0511's probe table): Neo4j reports every one of these
//! statements as a type error, not a syntax error — the `22NB1` type
//! mismatch, the `22N27` entity-in-predicate, and the `22N37` list
//! coercion alike.

mod common;

use common::{connection, error_class_literal};

/// The boolean family's class pins, one row per site: the planner's
/// statically non-boolean predicate, the planner's entity-variable
/// predicate, and the executor's `bool_or_null` behind the
/// list-quantifier `WHERE`. Each row carries the exact message fragment
/// its own site produces, so the statements are not interchangeable — a
/// rejection from the wrong site fails its row.
#[test]
fn boolean_requiring_sites_classify_as_type_error() {
    let mut conn = connection();
    for (query, fragment) in [
        ("MATCH (n) WHERE 1 RETURN n", "predicate requires a boolean"),
        ("MATCH (n) WHERE n RETURN n", "not a boolean predicate"),
        (
            "WITH [1, 2] AS xs RETURN all(x IN [1, 2] WHERE xs) AS r",
            "quantifier WHERE requires a boolean",
        ),
    ] {
        assert_eq!(
            error_class_literal(&mut conn, query),
            "TypeError",
            "{query}"
        );
        let error = conn.query(query).unwrap_err();
        assert!(error.to_string().contains(fragment), "{query}: {error}");
        let info = relate::error_info(&error);
        assert_eq!(
            info.neo4j_code, "Neo.ClientError.Statement.TypeError",
            "{query}: {error}"
        );
        assert_eq!(info.gql_status, "22G03", "{query}: {error}");
    }
}
