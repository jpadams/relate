//! The `graph-benchmark-ldbc` workload's static inputs: scale factors, index profiles, the
//! query files, the index scripts and the expected answers, all read from
//! `workloads/graph-benchmark-ldbc/` in this crate.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use sha2::{Digest, Sha256};

use relate_test_support::fetch;
use relate_test_support::hex;

use crate::answers::Expected;
use crate::{read, split_header, Result};

/// Query ids run by the suite: upstream's `test_benchmark_query1` … `30`.
pub const QIDS: std::ops::RangeInclusive<u32> = 1..=30;

/// The workload's own name — the one spelling every site consults (the
/// workloads directory, both report structs' `workload` fields, the run
/// arm's default report path), never re-typed per site.
pub const NAME: &str = "graph-benchmark-ldbc";

/// A scale factor of the LDBC SNB data set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScaleFactor {
    /// SF0.1: 327,588 nodes and 1,477,965 relationships; the PR level.
    Sf0_1,
    /// SF1: 3,181,724 nodes and 17,256,038 relationships; local only.
    Sf1,
}

impl ScaleFactor {
    /// The scale factor as the data set's file names spell it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sf0_1 => "0.1",
            Self::Sf1 => "1",
        }
    }

    /// The data set's directory (and archive stem) name.
    pub fn dataset_name(self) -> String {
        format!(
            "social_network-sf{}-CsvBasic-StringDateFormatter",
            self.as_str()
        )
    }

    /// The published (nodes, relationships) totals with upstream's schema;
    /// a load that does not reproduce them fails.
    pub fn published_counts(self) -> (u64, u64) {
        match self {
            Self::Sf0_1 => (327_588, 1_477_965),
            Self::Sf1 => (3_181_724, 17_256_038),
        }
    }

    /// The default per-query timeout.
    pub fn default_timeout(self) -> Duration {
        match self {
            Self::Sf0_1 => Duration::from_secs(60),
            Self::Sf1 => Duration::from_secs(300),
        }
    }
}

impl std::str::FromStr for ScaleFactor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "0.1" => Ok(Self::Sf0_1),
            "1" => Ok(Self::Sf1),
            other => Err(format!("scale factor is 0.1 or 1, got {other:?}")),
        }
    }
}

impl std::fmt::Display for ScaleFactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which indexes the store gets beyond the data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexProfile {
    /// Upstream's index set: `ID` on every label plus eight lookup indexes
    /// (`indexes.cypher`).
    Upstream,
    /// The upstream set plus three TEXT indexes (`text-indexes.cypher`).
    Text,
}

impl IndexProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Text => "text",
        }
    }
}

impl std::str::FromStr for IndexProfile {
    type Err = String;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "upstream" => Ok(Self::Upstream),
            "text" => Ok(Self::Text),
            other => Err(format!("index profile is upstream or text, got {other:?}")),
        }
    }
}

/// `workloads/graph-benchmark-ldbc/` in this crate — under the
/// crate-wide [`crate::workloads_dir`] root, the one derivation of it.
fn workload_dir() -> PathBuf {
    crate::workloads_dir().join(NAME)
}

/// One query of the workload.
#[derive(Debug, Clone)]
pub struct Query {
    pub qid: u32,
    /// The file's leading `//` comment lines (upstream's docstring); never
    /// sent to the engine.
    pub header: String,
    /// The statement that runs: the file without its header, with the
    /// scale factor's literal substitutions applied (ADR-0247).
    pub text: String,
    /// sha256 of `text`, so a report names exactly what ran.
    pub sha256: String,
}

/// The query file of `qid`.
pub fn query_path(qid: u32) -> PathBuf {
    workload_dir().join(format!("queries/q{qid:02}.cypher"))
}

/// Read query `qid` as it runs at `sf`: upstream's statement, with the
/// literal substitutions of [`literals`] applied at SF0.1 (ADR-0247).
pub fn query(qid: u32, sf: ScaleFactor) -> Result<Query> {
    let path = query_path(qid);
    let (header, verbatim) =
        split_header(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))?;
    let table = literals(sf)?;
    let pairs = table.get(&qid).map_or(&[][..], Vec::as_slice);
    let text = substitute(&verbatim, pairs).map_err(|e| format!("q{qid}: {e}"))?;
    let sha256 = hex(&Sha256::digest(text.as_bytes()));
    Ok(Query {
        qid,
        header,
        text,
        sha256,
    })
}

/// A `[from, to]` literal substitution.
pub type Substitution = (String, String);

/// The literal substitutions at `sf`, by query id, from
/// `literals-sf<sf>.json` (ADR-0247). SF1's table lists none: it runs
/// upstream's text.
pub fn literals(sf: ScaleFactor) -> Result<BTreeMap<u32, Vec<Substitution>>> {
    let path = workload_dir().join(format!("literals-sf{}.json", sf.as_str()));
    parse_literals(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))
}

/// The per-query entries of a workload JSON document: each `q<N>` key with
/// its value, by query id. Other keys are metadata (`_source`).
fn query_entries(json: &str) -> Result<BTreeMap<u32, (String, serde_json::Value)>> {
    let doc: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut out = BTreeMap::new();
    for (key, v) in doc {
        let Some(n) = key.strip_prefix('q') else {
            continue;
        };
        let qid: u32 = n.parse().map_err(|_| format!("bad query key {key:?}"))?;
        out.insert(qid, (key, v));
    }
    Ok(out)
}

/// Parse a literals table (`{"q1": {"replace": [[from, to], ...], ...}}`).
pub fn parse_literals(json: &str) -> Result<BTreeMap<u32, Vec<Substitution>>> {
    let mut out = BTreeMap::new();
    for (qid, (key, v)) in query_entries(json)? {
        let pairs: Vec<Substitution> = serde_json::from_value(v["replace"].clone())
            .map_err(|e| format!("{key}: replace is not a list of [from, to] pairs: {e}"))?;
        if pairs.is_empty() {
            return Err(format!("{key}: no substitutions"));
        }
        out.insert(qid, pairs);
    }
    Ok(out)
}

/// Apply substitutions in order; each `from` must occur exactly once in
/// the text it is applied to.
pub fn substitute(text: &str, pairs: &[Substitution]) -> Result<String> {
    let mut out = text.to_string();
    for (from, to) in pairs {
        match out.matches(from.as_str()).count() {
            1 => out = out.replacen(from.as_str(), to, 1),
            n => return Err(format!("{from:?} occurs {n} times, not once")),
        }
    }
    Ok(out)
}

/// A named group of statements in an index script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    pub statements: Vec<String>,
}

/// The statements of an index script, grouped by `// @step <name>` lines.
/// Statements are split by the workspace's canonical splitter
/// (`relate-splitter`), which keeps each statement's leading comment
/// text: a `// @step` line there opens a step, other leading comments are
/// dropped, and a comment-only piece holds no statement.
pub fn script_steps(script: &str) -> Result<Vec<Step>> {
    if !relate_splitter::input_is_complete(script) {
        return Err("the script ends in an unterminated statement".into());
    }
    let mut steps: Vec<Step> = Vec::new();
    for piece in relate_splitter::split_statements(script) {
        let mut body: Vec<&str> = Vec::new();
        for line in piece.lines() {
            let s = line.trim();
            if body.is_empty() {
                if let Some(name) = s.strip_prefix("// @step ") {
                    steps.push(Step {
                        name: name.trim().to_string(),
                        statements: Vec::new(),
                    });
                    continue;
                }
                if s.is_empty() || s.starts_with("//") {
                    continue;
                }
            }
            body.push(line);
        }
        let stmt = body.join("\n").trim().to_string();
        if stmt.is_empty() {
            continue;
        }
        steps
            .last_mut()
            .ok_or("a statement before the first `// @step`")?
            .statements
            .push(stmt);
    }
    Ok(steps)
}

/// A profile's index statements, split the way the loader runs them.
#[derive(Debug, Clone)]
pub struct IndexScript {
    /// Runs before the data: the first step of `indexes.cypher` (the `ID`
    /// indexes, so endpoint lookups during the load are seeks).
    pub pre_load: Step,
    /// Runs after the data: the remaining steps of `indexes.cypher`, then,
    /// for the text profile, `text-indexes.cypher`.
    pub post_load: Vec<Step>,
}

/// The index script of `profile`.
pub fn index_script(profile: IndexProfile) -> Result<IndexScript> {
    let path = workload_dir().join("indexes.cypher");
    let mut steps = script_steps(&read(&path)?)?.into_iter();
    let pre_load = steps
        .next()
        .ok_or_else(|| format!("{} has no `// @step`", path.display()))?;
    let mut post_load: Vec<Step> = steps.collect();
    if profile == IndexProfile::Text {
        post_load.extend(script_steps(&read(
            &workload_dir().join("text-indexes.cypher"),
        )?)?);
    }
    Ok(IndexScript {
        pre_load,
        post_load,
    })
}

/// The expected answers at `sf`, keyed by query id (the rows shape is
/// [`crate::answers::Expected`], shared with the corpus lanes).
pub fn expected(sf: ScaleFactor) -> Result<BTreeMap<u32, Expected>> {
    let path = workload_dir().join(format!("expected-sf{}.json", sf.as_str()));
    parse_expected(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))
}

/// Parse an expected-answers document (`{"q1": {"rows": [...],
/// "order_sensitive": bool, ...}, ...}`; keys not starting with `q` are
/// metadata).
pub fn parse_expected(json: &str) -> Result<BTreeMap<u32, Expected>> {
    let mut out = BTreeMap::new();
    for (qid, (key, v)) in query_entries(json)? {
        let rows = v["rows"]
            .as_array()
            .ok_or_else(|| format!("{key}: rows is not a list"))?
            .iter()
            .map(|r| {
                r.as_object()
                    .cloned()
                    .ok_or_else(|| format!("{key}: a row is not an object"))
            })
            .collect::<Result<Vec<_>>>()?;
        let order_sensitive = v["order_sensitive"]
            .as_bool()
            .ok_or_else(|| format!("{key}: order_sensitive is not a bool"))?;
        out.insert(
            qid,
            Expected {
                rows,
                order_sensitive,
            },
        );
    }
    Ok(out)
}

/// The pinned sha256 of `sf`'s archive, from `workloads/graph-benchmark-ldbc/data.sha256`.
pub fn pinned_sha256(sf: ScaleFactor) -> Result<String> {
    let path = workload_dir().join("data.sha256");
    let archive = format!("{}.tar.zst", sf.dataset_name());
    let pins = fetch::parse_pins(&read(&path)?, &path)?;
    Ok(fetch::pin_sha256(&pins, &archive, &path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_query_file_parses() {
        for qid in QIDS {
            let q = query(qid, ScaleFactor::Sf1).unwrap();
            assert!(q.header.starts_with(&format!("// q{qid}:")), "q{qid}");
            assert!(!q.text.contains("//"), "q{qid} body holds a comment");
            assert_eq!(q.sha256.len(), 64);
        }
    }

    #[test]
    fn sf1_runs_upstreams_text_and_sf0_1_only_swaps_literals() {
        let table = literals(ScaleFactor::Sf0_1).unwrap();
        assert_eq!(
            table.keys().copied().collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6, 9, 10, 13, 16, 20, 21, 23, 27]
        );
        for qid in QIDS {
            let (_, verbatim) = split_header(&read(&query_path(qid)).unwrap()).unwrap();
            let sf1 = query(qid, ScaleFactor::Sf1).unwrap();
            let sf01 = query(qid, ScaleFactor::Sf0_1).unwrap();
            assert_eq!(sf1.text, verbatim, "q{qid}");
            match table.get(&qid) {
                None => assert_eq!(sf01.text, verbatim, "q{qid}"),
                Some(pairs) => {
                    assert_ne!(sf01.sha256, sf1.sha256, "q{qid}");
                    // Undoing the substitutions, through the same
                    // exactly-once rule, gives upstream's text back: nothing
                    // but the literals changed.
                    let inverse: Vec<Substitution> = pairs
                        .iter()
                        .rev()
                        .map(|(from, to)| (to.clone(), from.clone()))
                        .collect();
                    assert_eq!(
                        substitute(&sf01.text, &inverse).unwrap(),
                        verbatim,
                        "q{qid}"
                    );
                }
            }
        }
    }

    #[test]
    fn order_sensitive_answers_without_order_by_have_at_most_one_row() {
        // Without ORDER BY the row order is undefined, so an order-sensitive
        // check is only sound on one row (ADR-0247).
        for sf in [ScaleFactor::Sf0_1, ScaleFactor::Sf1] {
            let exp = expected(sf).unwrap();
            for qid in QIDS {
                let q = query(qid, sf).unwrap();
                if exp[&qid].order_sensitive && !q.text.contains("ORDER BY") {
                    assert!(exp[&qid].rows.len() <= 1, "q{qid} at SF{sf}");
                }
            }
        }
    }

    #[test]
    fn expected_answers_record_the_statement_that_ran() {
        // Each expected-sf0.1.json answer names the sha256 of the statement
        // Neo4j ran, which must be the one the suite runs.
        let path = workload_dir().join("expected-sf0.1.json");
        let doc: serde_json::Value = serde_json::from_str(&read(&path).unwrap()).unwrap();
        for qid in QIDS {
            let q = query(qid, ScaleFactor::Sf0_1).unwrap();
            assert_eq!(
                doc[format!("q{qid}")]["query_sha256"],
                q.sha256.as_str(),
                "q{qid}"
            );
        }
    }

    #[test]
    fn substitute_requires_exactly_one_match() {
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            substitute("a = \"x\"", &[pair("\"x\"", "\"y\"")]).unwrap(),
            "a = \"y\""
        );
        assert!(substitute("a", &[pair("b", "c")])
            .unwrap_err()
            .contains("0 times"));
        assert!(substitute("x x", &[pair("x", "y")])
            .unwrap_err()
            .contains("2 times"));
    }

    #[test]
    fn parse_literals_rejects_bad_shapes() {
        assert!(parse_literals(r#"{"_source": {}, "q1": {"replace": [["a", "b"]]}}"#).is_ok());
        assert!(parse_literals(r#"{"q1": {"replace": []}}"#).is_err());
        assert!(parse_literals(r#"{"q1": {"replace": ["a"]}}"#).is_err());
        assert!(parse_literals(r#"{"qx": {"replace": [["a", "b"]]}}"#).is_err());
    }

    #[test]
    fn expected_answers_cover_every_query_at_both_scale_factors() {
        for sf in [ScaleFactor::Sf0_1, ScaleFactor::Sf1] {
            let exp = expected(sf).unwrap();
            assert_eq!(
                exp.keys().copied().collect::<Vec<_>>(),
                QIDS.collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn order_sensitivity_is_a_property_of_the_query_not_the_data() {
        let sf01 = expected(ScaleFactor::Sf0_1).unwrap();
        let sf1 = expected(ScaleFactor::Sf1).unwrap();
        for qid in QIDS {
            assert_eq!(
                sf01[&qid].order_sensitive, sf1[&qid].order_sensitive,
                "q{qid}"
            );
        }
    }

    #[test]
    fn index_scripts_split_into_the_steps_the_loader_runs() {
        let shape = |s: &Step| (s.name.clone(), s.statements.len());
        let upstream = index_script(IndexProfile::Upstream).unwrap();
        assert_eq!(shape(&upstream.pre_load), ("id-indexes".to_string(), 8));
        assert!(upstream
            .pre_load
            .statements
            .iter()
            .all(|s| s.contains("(n.ID)")));
        assert_eq!(
            upstream.post_load.iter().map(shape).collect::<Vec<_>>(),
            [("lookup-indexes".to_string(), 8)]
        );
        let text = index_script(IndexProfile::Text).unwrap();
        assert_eq!(
            text.post_load.iter().map(shape).collect::<Vec<_>>(),
            [
                ("lookup-indexes".to_string(), 8),
                ("text-indexes".to_string(), 3)
            ]
        );
    }

    #[test]
    fn script_steps_rejects_unterminated_and_stepless_statements() {
        assert!(script_steps("CREATE INDEX a FOR (n:A) ON (n.x);").is_err());
        assert!(script_steps("// @step s\nCREATE INDEX a FOR (n:A) ON (n.x)").is_err());
    }

    #[test]
    fn script_steps_groups_statements_under_their_step() {
        let script = "// header\n\n// @step s\n// c\nCREATE INDEX a\n  FOR (n:A) ON (n.x);\n\
                      CREATE INDEX b FOR (n:B) ON (n.y);\n\n// @step t\n\
                      CREATE TEXT INDEX c FOR (n:C) ON (n.z);\n// trailing comment\n";
        let steps = script_steps(script).unwrap();
        assert_eq!(
            steps,
            [
                Step {
                    name: "s".into(),
                    statements: vec![
                        "CREATE INDEX a\n  FOR (n:A) ON (n.x)".into(),
                        "CREATE INDEX b FOR (n:B) ON (n.y)".into()
                    ]
                },
                Step {
                    name: "t".into(),
                    statements: vec!["CREATE TEXT INDEX c FOR (n:C) ON (n.z)".into()]
                }
            ]
        );
    }

    #[test]
    fn script_steps_keeps_semicolons_inside_strings() {
        let steps = script_steps("// @step s\nRETURN 'a;b' AS x;\n").unwrap();
        assert_eq!(steps[0].statements, ["RETURN 'a;b' AS x"]);
    }

    #[test]
    fn both_archives_are_pinned() {
        for sf in [ScaleFactor::Sf0_1, ScaleFactor::Sf1] {
            assert_eq!(pinned_sha256(sf).unwrap().len(), 64);
        }
    }

    #[test]
    fn parse_expected_skips_metadata_and_rejects_bad_shapes() {
        let ok = parse_expected(
            r#"{"_source": {}, "q2": {"rows": [{"a": 1}], "order_sensitive": false}}"#,
        )
        .unwrap();
        assert_eq!(ok.len(), 1);
        assert!(!ok[&2].order_sensitive);
        assert!(parse_expected(r#"{"q1": {"rows": 1, "order_sensitive": true}}"#).is_err());
        assert!(parse_expected(r#"{"qx": {"rows": [], "order_sensitive": true}}"#).is_err());
    }
}
