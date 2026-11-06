mod common;

use relate::{Database, Value};
use relate_datasets::fraud::{generate, FraudConfig};
use relate_datasets::load_fraud;

use common::params;

#[test]
fn fraud_ci_scale_detects_planted_patterns() {
    let cfg = FraudConfig::ci();
    let graph = generate(&cfg);
    let ordinary = graph
        .accounts
        .iter()
        .filter(|a| a.planted.is_none())
        .count();
    assert_eq!(ordinary, 500);
    assert!(graph.planted.fanout_k <= ordinary);
    assert!(graph.accounts.len() > ordinary);
    assert_eq!(graph.planted.fanout_source, "fanout-src");
    assert_eq!(graph.planted.cycle_accounts.len(), 4);

    let db = Database::open(":memory:").unwrap();
    let mut c = db.connect().unwrap();
    load_fraud(&mut c, &graph).expect("load fraud");

    let fan = c
        .query_with_params(
            "MATCH (a:Account {id: $id})-[:TRANSFER]->(b) WITH a, count(DISTINCT b) AS n WHERE n >= $k RETURN a.id AS id, n",
            params([
                (
                    "id",
                    Value::String(graph.planted.fanout_source.as_str().into()),
                ),
                ("k", Value::Int(graph.planted.fanout_k as i64)),
            ]),
        )
        .expect("fan-out query");
    let ids: Vec<_> = fan
        .rows
        .iter()
        .filter_map(|r| r[0].as_string().map(|s| s.to_string()))
        .collect();
    assert!(
        ids.iter().any(|id| id == "fanout-src"),
        "planted fan-out source must be detected, got {ids:?}"
    );

    let cyc = c
        .query(
            "MATCH (a:Account {planted: 'cycle'})-[:TRANSFER*2..6]->(a) RETURN DISTINCT a.id AS id",
        )
        .expect("circular query");
    let cids: Vec<_> = cyc
        .rows
        .iter()
        .filter_map(|r| r[0].as_string().map(|s| s.to_string()))
        .collect();
    for id in &graph.planted.cycle_accounts {
        assert!(
            cids.contains(id),
            "planted cycle account {id} missing from {cids:?}"
        );
    }

    let aid = "acc-0";
    c.query_with_params(
        "MATCH (a:Account {id: $id})-[t:TRANSFER]->(b) \
         RETURN t.id AS id ORDER BY t.ts DESC SKIP $skip LIMIT 5",
        params([("id", Value::String(aid.into())), ("skip", Value::Int(0))]),
    )
    .expect("activity page");
    c.query_with_params(
        "MATCH (c:Customer) WHERE c.name CONTAINS $q RETURN c.id AS id ORDER BY c.name LIMIT 5",
        params([("q", Value::String("0".into()))]),
    )
    .expect("search customer");
    c.query_with_params(
        "MATCH (a:Account {id: $id})-[:TRANSFER]-(b:Account) RETURN DISTINCT b.id AS id LIMIT 5",
        params([("id", Value::String(aid.into()))]),
    )
    .expect("undirected counterparties");
    c.query_with_params(
        "MATCH (a:Account {id: $from}), (b:Account {id: $to}) \
         RETURN length(shortestPath((a)-[:TRANSFER*1..3]->(b))) AS hops",
        params([
            ("from", Value::String(aid.into())),
            ("to", Value::String("acc-1".into())),
        ]),
    )
    .expect("payment path");
    c.query_with_params(
        "MATCH (a:Account {id: $id})-[t:TRANSFER]->() \
         RETURN CASE WHEN t.amount >= 1000.0 THEN 'large' ELSE 'small' END AS band, count(*) AS n",
        params([("id", Value::String(aid.into()))]),
    )
    .expect("amount band");
    c.query_with_params(
        "MATCH (a:Account {id: $id}) SET a.frozen = true",
        params([("id", Value::String(aid.into()))]),
    )
    .expect("flag account");
    c.query_with_params(
        "MATCH (a:Account {id: $id})-[t:TRANSFER]->() WITH t ORDER BY t.ts DESC LIMIT 1 DELETE t",
        params([("id", Value::String(aid.into()))]),
    )
    .expect("reverse last out");
}

#[test]
#[ignore]
fn fraud_full_scale_generates() {
    let g = generate(&FraudConfig::default());
    assert_eq!(g.accounts.len(), 10_000 + 1 + 4);
    assert!(g.transfers.len() > 10_000);
}
