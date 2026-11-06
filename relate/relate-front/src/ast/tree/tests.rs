use super::*;
use relate_error::ErrorKind;
use relate_names::SmallName;
use relate_parser::Clause;

pub(crate) fn return_expr_of(query: &str) -> Expr {
    let stmt = relate_parser::parse(query).unwrap();
    match &stmt.clauses[0] {
        Clause::Return(rc) => rc.items[0].expr.clone(),
        _ => panic!("expected a RETURN clause"),
    }
}

#[test]
fn walk_expr_visits_every_subexpression() {
    // A variable buried inside a CASE, a list-comprehension map, a reduce and
    // a nested function call must all be visited — proving the walker's
    // completeness (the property the ported find-and-check validators rely
    // on to catch a violation wherever it appears).
    let e = return_expr_of(
        "RETURN CASE WHEN aa > 0 THEN [x IN bb | reduce(s = 0, y IN cc | s + dd)] \
                ELSE toInteger(ee) END",
    );
    let mut seen = std::collections::HashSet::new();
    walk_expr(&e, &mut |node| {
        if let Expr::Variable(v) = node {
            seen.insert(v.clone());
        }
        Ok::<(), EngineError>(())
    })
    .unwrap();
    for v in ["aa", "bb", "cc", "dd", "ee"] {
        assert!(
            seen.contains(&SmallName::from_name(v)),
            "walker did not visit variable `{v}`"
        );
    }
}

#[test]
fn walk_expr_early_exits_on_err() {
    // The walk stops and propagates the first Err (early exit).
    let e = return_expr_of("RETURN a + b + c");
    let mut visited = 0usize;
    let r = walk_expr(&e, &mut |_node| {
        visited += 1;
        Err::<(), _>(EngineError::semantic(
            ErrorKind::SyntaxError,
            "stop".to_string(),
        ))
    });
    assert!(r.is_err());
    assert_eq!(visited, 1, "walk must stop at the first Err");
}

#[test]
fn walk_expr_does_not_enter_subquery_statements() {
    // Subquery expressions are opaque leaves: a variable used only inside an
    // EXISTS { ... } body is not visited by the expression walker (matching
    // the validators built on it, which treat subqueries as leaves).
    let e = return_expr_of("RETURN exists { MATCH (n) WHERE n.p = insideOnly RETURN n }");
    let mut seen = std::collections::HashSet::new();
    walk_expr(&e, &mut |node| {
        if let Expr::Variable(v) = node {
            seen.insert(v.clone());
        }
        Ok::<(), EngineError>(())
    })
    .unwrap();
    assert!(!seen.contains(&SmallName::from_name("insideOnly")));
}

#[test]
fn statement_tree_walk_enters_pattern_embedded_subqueries() {
    // Pattern comprehensions, pattern predicates and shortestPath()
    // patterns are walked too: a COUNT { } subquery nested in a node
    // property map inside any of them reaches the statement visitor
    // (pre-fix these patterns were skipped/opaque, so embedded
    // subqueries escaped the nested-statement validation battery).
    for query in [
        "RETURN [(n {p: count { MATCH (m) }})-->(m) | m] AS x",
        "MATCH (n) WHERE (n {p: count { MATCH (m) }})-->() RETURN n",
        "RETURN shortestPath((n {p: count { MATCH (m) }})-->(m)) AS p",
    ] {
        let stmt = relate_parser::parse(query).unwrap();
        let mut visited = 0usize;
        walk_statement_tree(
            &stmt,
            &mut |_| {
                visited += 1;
                Ok::<(), EngineError>(())
            },
            &mut |_| Ok(()),
            &mut |_| Ok(()),
        )
        .unwrap();
        // Top statement + the COUNT { } body.
        assert_eq!(visited, 2, "for {query}");
    }
}

#[test]
fn statement_tree_walk_enters_every_nested_statement() {
    // The complement of `walk_expr_does_not_enter_subquery_statements`:
    // the whole-tree walk DOES reach UNION arms, NEXT stages, CALL { } /
    // WHEN / ELSE bodies, FOREACH bodies, and EXISTS/COUNT/COLLECT
    // bodies — the property the nested-statement validation battery and
    // the ADR-0257 depth guard build on.
    let stmt = relate_parser::parse(
        "MATCH (n) WHERE exists { MATCH (n)-->(m) } \
         CALL { FOREACH (x IN [1] | SET n.p = x) } \
         RETURN n UNION RETURN n NEXT RETURN n",
    )
    .unwrap();
    let mut visited = 0usize;
    walk_statement_tree(
        &stmt,
        &mut |_| {
            visited += 1;
            Ok::<(), EngineError>(())
        },
        &mut |_| Ok(()),
        &mut |_| Ok(()),
    )
    .unwrap();
    // Top statement, EXISTS body, CALL body, FOREACH body, UNION arm,
    // NEXT stage.
    assert_eq!(visited, 6);
}

#[test]
fn statement_tree_walk_visits_the_disjoint_by_keys() {
    // The `IN TRANSACTIONS` slot enumeration is total: a CIP-260
    // `DISJOINT BY (...)` key expression is walked like the concurrency
    // value, the batch size and the retry duration. The two key variables
    // appear nowhere else in the statement, so only that slot can surface
    // them.
    let stmt = relate_parser::parse(
        "CALL { CREATE (:A) } IN CONCURRENT TRANSACTIONS OF 5 ROWS \
         DISJOINT BY (disjointKey, otherKey)",
    )
    .unwrap();
    let mut seen = std::collections::HashSet::new();
    walk_statement_tree(&stmt, &mut |_| Ok(()), &mut |_| Ok(()), &mut |expr| {
        if let Expr::Variable(v) = expr {
            seen.insert(v.clone());
        }
        Ok::<(), EngineError>(())
    })
    .unwrap();
    for v in ["disjointKey", "otherKey"] {
        assert!(
            seen.contains(&SmallName::from_name(v)),
            "walker did not visit `{v}`"
        );
    }
}
