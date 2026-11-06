use crate::LoadError;
use relate_parser::{parse, Clause, Expr, ReturnItem};

/// Reject a non-graph `RETURN` before any network call (ADR-0080).
pub fn assert_graph_shape(query: &str) -> Result<(), LoadError> {
    let statement =
        parse(query).map_err(|e| LoadError::Shape(format!("shape query parse error: {e}")))?;
    let Some(return_clause) = statement.clauses.iter().find_map(|c| match c {
        Clause::Return(r) => Some(r),
        _ => None,
    }) else {
        return Err(LoadError::Shape(
            "shape query must have a RETURN clause".into(),
        ));
    };
    for item in &return_clause.items {
        if !is_graph_return_item(item) {
            let shown = item
                .source_text
                .as_deref()
                .unwrap_or("<unsupported projection>");
            return Err(LoadError::Shape(format!(
                "shape query RETURN must be graph elements (nodes/rels/paths), not projections/scalars/aggregates; rejected item: {shown}"
            )));
        }
    }
    Ok(())
}

fn is_graph_return_item(item: &ReturnItem) -> bool {
    if item.all {
        return false;
    }
    is_graph_expr(&item.expr)
}

fn is_graph_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Variable(_) => true,
        Expr::Paren(inner) => is_graph_expr(inner),
        Expr::FunctionCall(fc) => matches!(
            fc.name.with_str(str::to_ascii_lowercase).as_str(),
            "nodes" | "relationships" | "paths" | "shortestpath" | "allshortestpaths"
        ),
        Expr::ShortestPath(_) => true,
        _ => false,
    }
}

pub fn looks_like_path(arg: &str) -> bool {
    std::path::Path::new(arg).exists()
        || [".cypher", ".cql", ".txt"]
            .iter()
            .any(|ext| arg.ends_with(ext))
}

/// File path, `-` (stdin), or inline Cypher (ADR-0158). The value may carry a
/// `;`-separated statement list.
pub fn resolve_shape_query(arg: &str) -> Result<String, LoadError> {
    if arg == "-" {
        use std::io::Read;
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| LoadError::Io(format!("failed to read --query from stdin: {e}")))?;
        return Ok(text);
    }
    if looks_like_path(arg) {
        std::fs::read_to_string(arg)
            .map_err(|e| LoadError::Io(format!("failed to read shape query file {arg:?}: {e}")))
    } else {
        Ok(arg.to_string())
    }
}

/// Split a `--query` value into statements (ADR-0158): a value may carry a
/// `;`-separated statement list; every statement runs against the same
/// source, in order.
pub fn split_statements(query: &str) -> Result<Vec<String>, LoadError> {
    let statements = relate_splitter::split_executable_statements(query);
    if statements.is_empty() {
        return Err(LoadError::Shape("no statements in --query value".into()));
    }
    Ok(statements)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_graph_shape_rejects_count_aggregate() {
        assert!(matches!(
            assert_graph_shape("MATCH (n) RETURN count(n)"),
            Err(LoadError::Shape(_))
        ));
    }

    #[test]
    fn assert_graph_shape_rejects_missing_return() {
        assert!(matches!(
            assert_graph_shape("MATCH (n)"),
            Err(LoadError::Shape(_))
        ));
    }

    #[test]
    fn assert_graph_shape_rejects_star() {
        assert!(matches!(
            assert_graph_shape("MATCH (n) RETURN *"),
            Err(LoadError::Shape(_))
        ));
    }

    #[test]
    fn assert_graph_shape_accepts_graph_variables() {
        assert!(
            assert_graph_shape("MATCH (p:Person)-[r:ACTED_IN]->(m:Movie) RETURN p, m, r").is_ok()
        );
    }

    #[test]
    fn assert_graph_shape_accepts_graph_functions() {
        assert!(assert_graph_shape("MATCH p = ()-->() RETURN nodes(p)").is_ok());
        assert!(assert_graph_shape("MATCH p = ()-->() RETURN relationships(p)").is_ok());
    }

    #[test]
    fn assert_graph_shape_accepts_shortest_paths() {
        assert!(assert_graph_shape("RETURN shortestPath((a)-[*]-(b))").is_ok());
        assert!(assert_graph_shape("RETURN allShortestPaths((a)-[*]-(b))").is_ok());
    }

    #[test]
    fn assert_graph_shape_rejects_projection() {
        assert!(matches!(
            assert_graph_shape("MATCH (m:Movie) RETURN m.title"),
            Err(LoadError::Shape(_))
        ));
        assert!(matches!(
            assert_graph_shape("RETURN 123"),
            Err(LoadError::Shape(_))
        ));
    }

    #[test]
    fn resolve_shape_query_inline_and_file() {
        assert_eq!(
            resolve_shape_query("MATCH (n) RETURN n").unwrap(),
            "MATCH (n) RETURN n"
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shape.cypher");
        std::fs::write(&path, "MATCH (n) RETURN n").unwrap();
        assert_eq!(
            resolve_shape_query(path.to_str().unwrap()).unwrap(),
            "MATCH (n) RETURN n"
        );
    }

    #[test]
    fn slash_in_inline_query_is_not_a_file() {
        let q = "MATCH (n) WHERE n.url = 'a/b' RETURN n";
        assert!(!looks_like_path(q));
        assert_eq!(resolve_shape_query(q).unwrap(), q);
    }

    #[test]
    fn missing_cypher_file_is_io() {
        assert!(!std::path::Path::new("missing-shape.cypher").exists());
        assert!(looks_like_path("missing-shape.cypher"));
        assert!(matches!(
            resolve_shape_query("missing-shape.cypher"),
            Err(LoadError::Io(_))
        ));
    }

    #[test]
    fn looks_like_path_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shape.cypher");
        std::fs::write(&path, "MATCH (n) RETURN n").unwrap();
        assert!(looks_like_path(path.to_str().unwrap()));
        assert!(!looks_like_path("MATCH (n) RETURN n"));
    }
}
