//! Extra queries (PR #235 follow-up): plain-Cypher rewrites of the
//! walkthroughs' skipped apoc statements, run differentially alongside
//! the adoc statements. Each rewrite is keyed by the adoc line of the
//! original apoc statement it replaces, so the report shows which
//! walkthrough step it stands in for.
//!
//! Coverage policy — rewrite only what plain Cypher 25 can express and
//! relate actually runs (probed against the built CLI):
//!
//! - `apoc.coll.max/min(list)` → `coll.max/min(list)` — native Cypher 25
//!   list functions (contact-tracing)
//! - `apoc.node.degree.in(n, 'T')` → `count { (n)<-[:T]-() }` (contact-tracing)
//! - `apoc.meta.stats()` / `apoc.cypher.run(dynamic)` → `db.labels()` +
//!   correlated `CALL (x) {}`, `type(r)` counts, `labels(n)` membership
//!   (fraud-detection)
//! - `apoc.agg.statistics(x)` → `min/max/avg/stDev/percentileCont/count`
//!   columns (twitch)
//! - `apoc.periodic.iterate(match, set)` → the plain `MATCH … SET` (the
//!   `-50` stores are samples; batching is irrelevant) (twitch)
//!
//! Not rewritten, by design:
//!
//! - `apoc.date.format(c.time,'ms',…)` (network-management:646) — the
//!   rewrite below would apply, but the store has zero `Connection`
//!   nodes (`MATCH (c:Connection)` is empty on both engines), so the
//!   extra would compare vacuously; the stackoverflow:198 rewrite below
//!   covers the same epoch→formatted-date path on real data
//! - `apoc.algo.dijkstra` (openstreetmap) — weighted shortest path has no
//!   plain-Cypher equivalent (QPE `SHORTEST` is hop-based)
//! - `apoc.load.json` / `apoc.util.sleep` (stackoverflow) and
//!   `apoc.import.json` (cybersecurity) — remote fetch is denied by the
//!   engine's LOAD CSV policy (ADR-0081) and a test throttle has no
//!   differential meaning
//! - `apoc.coll.intersection` (entity-resolution) — the rewrite
//!   (`[x IN l WHERE x IN r]`) works, but the statements around it call
//!   `gds.alpha.similarity.*`, so the whole block stays GDS-skipped

use crate::manifest::Dataset;

/// One extra query: the adoc line of the original apoc statement, and
/// the rewritten statement that replaces it.
pub struct Extra {
    /// 1-based adoc line of the original apoc statement.
    pub line: usize,
    /// The rewritten, apoc-free statement.
    pub statement: &'static str,
}

/// The extras for one dataset, in adoc order; empty for datasets with no
/// rewritable apoc statements.
pub fn for_dataset(dataset: &Dataset) -> Vec<Extra> {
    match dataset.name {
        "contact-tracing" => vec![
            // adoc:75 — apoc.coll.max / apoc.coll.min (the adoc's
            // LIMIT 10 is dropped: no ORDER BY means an unspecified
            // subset, and the count must be deterministic)
            Extra {
                line: 75,
                statement: r#"MATCH (p:Person {healthstatus:"Sick"})-->(v1:Visit)-->(pl:Place)
WITH p,v1,pl
MATCH path = (p)-->(v1)-->(pl)<--(v2:Visit)<--(p2:Person {healthstatus:"Healthy"})
WITH path, coll.max([v1.starttime.epochMillis, v2.starttime.epochMillis]) as maxStart,
     coll.min([v1.endtime.epochMillis, v2.endtime.epochMillis]) as minEnd
WHERE maxStart <= minEnd
RETURN count(path) AS overlaps"#,
            },
            // adoc:93 — same rewrite over the VISITS relationship
            // (LIMIT 10 dropped for determinism, like adoc:75)
            Extra {
                line: 93,
                statement: r#"MATCH (p:Person {healthstatus:"Sick"})-[v1:VISITS]->(pl:Place)
WITH p,v1,pl
MATCH path = (p)-[v1]->(pl)<-[v2:VISITS]-(p2:Person {healthstatus:"Healthy"})
WITH path, coll.max([v1.starttime.epochMillis, v2.starttime.epochMillis]) as maxStart,
     coll.min([v1.endtime.epochMillis, v2.endtime.epochMillis]) as minEnd
WHERE maxStart <= minEnd
RETURN count(path) AS overlaps"#,
            },
            // adoc:112 — same rewrite over the simplified relationship
            // (LIMIT 10 dropped for determinism, like adoc:75)
            Extra {
                line: 112,
                statement: r#"MATCH (p:Person {healthstatus:"Sick"})-->(s1:Visit)-->(pl:Place)
WITH p,s1,pl
MATCH path = (p)-->(s1)-->(pl)<--(s2:Visit)<--(p2:Person {healthstatus:"Healthy"})
WITH path, coll.max([s1.starttime.epochMillis, s2.starttime.epochMillis]) as maxStart,
     coll.min([s1.endtime.epochMillis, s2.endtime.epochMillis]) as minEnd
WHERE maxStart <= minEnd
RETURN count(path) AS overlaps"#,
            },
            // adoc:151 — overlap time per healthy/sick visitor pair
            Extra {
                line: 151,
                statement: r#"MATCH (hp:Person {healthstatus:"Healthy"})-[v1:VISITS]->(pl:Place)<-[v2:VISITS]-(sp:Person {healthstatus:"Sick"})
WITH hp, coll.max([v1.starttime.epochMillis, v2.starttime.epochMillis]) AS maxStart,
     coll.min([v1.endtime.epochMillis, v2.endtime.epochMillis]) AS minEnd
WHERE maxStart <= minEnd
RETURN hp.name, hp.healthstatus, sum(minEnd-maxStart) AS overlaptime
ORDER BY overlaptime DESC"#,
            },
            // adoc:184 — apoc.node.degree.in
            Extra {
                line: 184,
                statement: r#"MATCH (p:Person {healthstatus:"Sick"})-[v:VISITS]->(pl:Place)
WITH DISTINCT pl.name as placename, count(v) AS nrofsickvisits,
     count { (pl)<-[:VISITS]-() } AS totalnrofvisits
ORDER BY nrofsickvisits DESC LIMIT 10
RETURN placename, nrofsickvisits, totalnrofvisits,
       round(toFloat(nrofsickvisits)/toFloat(totalnrofvisits)*10000)/100 AS percentageofsickvisits"#,
            },
        ],
        "fraud-detection" => vec![
            // adoc:69 — apoc.meta.stats: label counts (plus the node total);
            // dynamic labels (`:$(label)`) replace the apoc.meta counters
            Extra {
                line: 69,
                statement: r#"MATCH (n) WITH count(n) AS nodeCount
CALL db.labels() YIELD label
CALL (label) {
  MATCH (n:$(label))
  RETURN count(n) AS count
}
RETURN label, count, nodeCount"#,
            },
            // adoc:78 — apoc.cypher.run per-label counts via dynamic labels
            Extra {
                line: 78,
                statement: r#"CALL db.labels() YIELD label
CALL (label) {
  MATCH (n:$(label))
  RETURN count(*) AS count
}
RETURN label as Label, count AS Count
ORDER BY Count DESC"#,
            },
            // adoc:88 — apoc.cypher.run per-rel-type counts via dynamic
            // relationship types (`[:$(type)]`)
            Extra {
                line: 88,
                statement: r#"CALL db.relationshipTypes() YIELD relationshipType AS type
CALL (type) {
  MATCH ()-[r:$(type)]->()
  RETURN count(*) AS count
}
RETURN type AS Relationship, count AS Count
ORDER BY Count DESC"#,
            },
            // adoc:102 — apoc.cypher.run per-transaction-type metrics:
            // dynamic labels replace the dynamic-label query string
            Extra {
                line: 102,
                statement: r#"MATCH (t:Transaction)
WITH count(t) AS globalCnt
UNWIND ['CashIn', 'CashOut', 'Payment', 'Debit', 'Transfer'] AS txType
CALL (txType) {
  MATCH (t:$(txType))
  RETURN count(t) AS txCnt
}
RETURN txType, txCnt AS NumberOfTransactions,
       round(toFloat(txCnt)/toFloat(globalCnt), 2) AS pct
ORDER BY pct DESC"#,
            },
        ],
        "twitch" => vec![
            // adoc:168 — apoc.agg.statistics over out-degree
            Extra {
                line: 168,
                statement: r#"MATCH (u:User)
WITH u, count{ (u)-[:CHATTER|VIP|MODERATOR]->() } as node_outdegree
RETURN min(node_outdegree) AS min, max(node_outdegree) AS max,
       avg(node_outdegree) AS avg, stDev(node_outdegree) AS stdDev,
       percentileCont(node_outdegree, 0.5) AS p50,
       percentileCont(node_outdegree, 0.95) AS p95,
       count(node_outdegree) AS count"#,
            },
            // adoc:178 — apoc.agg.statistics over in-degree (streamers)
            Extra {
                line: 178,
                statement: r#"MATCH (u:Stream)
WITH u, count{ (u)<-[:CHATTER|VIP|MODERATOR]-() } as node_indegree
RETURN min(node_indegree) AS min, max(node_indegree) AS max,
       avg(node_indegree) AS avg, stDev(node_indegree) AS stdDev,
       percentileCont(node_indegree, 0.5) AS p50,
       percentileCont(node_indegree, 0.95) AS p95,
       count(node_indegree) AS count"#,
            },
            // adoc:289 — apoc.periodic.iterate: the plain write (the -50
            // store is a sample; batching is irrelevant)
            Extra {
                line: 289,
                statement: r#"MATCH (u:User)
WHERE NOT u:Stream AND COUNT {(u)-->(:Stream)} > 1
SET u:Audience
RETURN count(u) AS audience"#,
            },
        ],
        "cybersecurity" => vec![
            // adoc:289 — apoc.coll.pairsMin: consecutive path-node pairs as
            // a list comprehension (ORDER BY added for determinism)
            Extra {
                line: 289,
                statement: r#"MATCH (crownJewel:Group {objectid:'S-1-5-21-883232822-274137685-4173207997-512'})
MATCH (source) WHERE NOT source:HighValue AND source <> crownJewel
MATCH path = shortestPath((source)-[*..100]->(crownJewel))
UNWIND [i IN range(0, size(nodes(path))-2) | [nodes(path)[i], nodes(path)[i+1]]] AS pair
WITH pair[0] AS a, pair[1] AS b
RETURN a.name AS from, b.name AS to
ORDER BY from, to LIMIT 10"#,
            },
            // adoc:307 — same rewrite materialized as ATTACK_PATH writes
            Extra {
                line: 307,
                statement: r#"MATCH (crownJewel:Group {objectid:'S-1-5-21-883232822-274137685-4173207997-512'})
MATCH (source) WHERE NOT source:HighValue AND source <> crownJewel
MATCH path = shortestPath((source)-[*..100]->(crownJewel))
UNWIND [i IN range(0, size(nodes(path))-2) | [nodes(path)[i], nodes(path)[i+1]]] AS pair
WITH pair[0] AS a, pair[1] AS b
MERGE (a)-[r:ATTACK_PATH]->(b)
RETURN count(r) AS attackPaths"#,
            },
        ],
        "stackoverflow" => vec![
            // adoc:237 — apoc.create.vRelationship: the virtual edge as a
            // plain map (no virtual elements in Cypher 25; ORDER BY added
            // for determinism)
            Extra {
                line: 237,
                statement: r#"MATCH (t1:Tag)<-[:TAGGED]-()-[:TAGGED]->(t2:Tag)
WHERE id(t1) < id(t2) and t1.name <> 'neo4j' and t2.name <> 'neo4j'
WITH t1, t2, count(*) as freq WHERE freq > 3
RETURN t1.name AS tag1, t2.name AS tag2, {type: 'OCCURRED', freq: freq} AS rel
ORDER BY freq DESC, tag1, tag2"#,
            },
            // adoc:262 — same rewrite over user co-commenting
            Extra {
                line: 262,
                statement: r#"MATCH p1=(u1:User)-[:COMMENTED]->(c1:Comment)-[:COMMENTED_ON]-(q:Question)
MATCH p2=(u2:User)-[:COMMENTED]->(c2:Comment)-[:COMMENTED_ON]-(q)
WHERE id(u1) < id(u2)
WITH u1, u2, count(distinct q) as freq
WHERE freq > 2
RETURN u1.display_name AS user1, u2.display_name AS user2,
       {type: 'OCCURRED', freq: freq} AS rel
ORDER BY freq DESC, user1, user2"#,
            },
            // adoc:198 — apoc.date.format(epoch seconds, 'yyyy-MM') →
            // datetime({epochSeconds}) + ISO prefix slice (unblocked by
            // the epochMillis/epochSeconds constructor fix, LABS-261)
            Extra {
                line: 198,
                statement: r#"MATCH (u:User)-[:PROVIDED]->()-[:ANSWERED]->
      (q:Question)-[:TAGGED]->(t:Tag)
WHERE u.display_name = "InverseFalcon"
RETURN substring(toString(datetime({epochSeconds: q.creation_date})), 0, 7) as month,
       count(distinct q) as count, collect(distinct t.name) as tags
ORDER BY month asc"#,
            },
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every dataset's extras carry their original adoc line and a
    /// non-empty statement — the report keys on both.
    #[test]
    fn extras_carry_a_line_and_a_statement() {
        for dataset in crate::manifest::DATASETS {
            for extra in for_dataset(dataset) {
                assert!(extra.line > 0, "{}: line", dataset.name);
                assert!(
                    !extra.statement.trim().is_empty(),
                    "{}: statement",
                    dataset.name
                );
            }
        }
    }

    /// Only known datasets carry extras; a typo'd name in the match would
    /// silently drop them.
    #[test]
    fn extras_only_for_datasets_with_rewrites() {
        let with_extras: Vec<&str> = crate::manifest::DATASETS
            .iter()
            .filter(|d| !for_dataset(d).is_empty())
            .map(|d| d.name)
            .collect();
        assert_eq!(
            with_extras,
            [
                "fraud-detection",
                "stackoverflow",
                "cybersecurity",
                "contact-tracing",
                "twitch"
            ]
        );
    }

    /// The rewritten statements contain no apoc or gds references — a
    /// leftover call would defeat the point of the rewrite.
    #[test]
    fn extras_are_apoc_and_gds_free() {
        for dataset in crate::manifest::DATASETS {
            for extra in for_dataset(dataset) {
                let lowered = extra.statement.to_ascii_lowercase();
                assert!(
                    !lowered.contains("apoc."),
                    "{}:{}",
                    dataset.name,
                    extra.line
                );
                assert!(!lowered.contains("gds."), "{}:{}", dataset.name, extra.line);
            }
        }
    }
}
