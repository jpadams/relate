use super::*;

fn one_col(col: &str, vals: Vec<CypherValue>) -> (Vec<String>, Vec<IndexMap<String, CypherValue>>) {
    let rows = vals
        .into_iter()
        .map(|v| {
            let mut m = IndexMap::new();
            m.insert(col.to_string(), v);
            m
        })
        .collect();
    (vec![col.to_string()], rows)
}

fn make_rows() -> Vec<IndexMap<String, CypherValue>> {
    let mut row1 = IndexMap::new();
    row1.insert("name".to_string(), CypherValue::Str("Alice".into()));
    row1.insert("age".to_string(), CypherValue::Int(30));
    row1.insert("active".to_string(), CypherValue::Bool(true));

    let mut row2 = IndexMap::new();
    row2.insert("name".to_string(), CypherValue::Str("Bob".into()));
    row2.insert("age".to_string(), CypherValue::Int(25));
    row2.insert("active".to_string(), CypherValue::Bool(false));

    vec![row1, row2]
}

#[path = "tests/composite_graph.rs"]
mod composite_graph;
#[path = "tests/scalar_temporal.rs"]
mod scalar_temporal;
#[path = "tests/spatial.rs"]
mod spatial;
#[path = "tests/vector.rs"]
mod vector;
