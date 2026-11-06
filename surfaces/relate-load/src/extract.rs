//! Typed Query API v2 extraction (ADR-0181): every row cell and every
//! property value must be a `$type`-tagged value, and property values decode
//! through [`crate::typed_value::decode_value`] into engine values. A value
//! the engine cannot represent fails the load with the owning element id and
//! property key attached; there is no plain-JSON element path.

use std::collections::{BTreeMap, HashMap, HashSet};

use relate::Value;
use serde_json::Value as Json;

use crate::typed_value::{decode_value, DecodeError};
use crate::{LoadError, TypedWireError};

/// Node collected from a Query API v2 response.
#[derive(Debug, Clone, PartialEq)]
pub struct SubgraphNode {
    /// Interned id for wiring start/end inside this response.
    pub id: i64,
    /// Remote Neo4j `elementId` (identity key).
    pub element_id: String,
    pub labels: Vec<String>,
    /// Engine-typed property values (ADR-0181 item 3): the extractor decoded
    /// every `_properties` entry, and `materialize` passes this map as one
    /// `Value::Map` row value.
    pub properties: BTreeMap<String, Value>,
}

/// Relationship collected from a Query API v2 response.
#[derive(Debug, Clone, PartialEq)]
pub struct SubgraphRel {
    /// Interned id for this response.
    pub id: i64,
    pub element_id: String,
    pub start: i64,
    pub end: i64,
    pub rel_type: String,
    /// Engine-typed property values (ADR-0181 item 3), like [`SubgraphNode`].
    pub properties: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Subgraph {
    pub nodes: Vec<SubgraphNode>,
    pub rels: Vec<SubgraphRel>,
}

/// Accumulator for streaming extraction (ADR-0158): rows arrive one batch
/// at a time, and the pending maps are drained per flush. The element-id
/// intern table (`ElementIds`) lives for the whole load — endpoint
/// resolution across batches needs it — so its size is O(elements), while
/// the extracted subgraphs stay batch-sized.
#[derive(Default)]
pub(crate) struct SubgraphAccumulator {
    ids: ElementIds,
    nodes: HashMap<i64, SubgraphNode>,
    rels: HashMap<i64, SubgraphRel>,
}

impl SubgraphAccumulator {
    /// Ingest one streamed `data.values` row. `ingest_v2` already walks an
    /// array cell element-by-element, so a row needs no dispatch of its own.
    /// A value the engine cannot represent surfaces as a [`TypedWireError`]
    /// naming its element and property (ADR-0181 item 7).
    pub(crate) fn push_row(&mut self, row: &Json) -> Result<(), TypedWireError> {
        ingest_v2(row, &mut self.ids, &mut self.nodes, &mut self.rels)
    }

    /// Drain the pending nodes and relationships.
    pub(crate) fn drain(&mut self) -> (Vec<SubgraphNode>, Vec<SubgraphRel>) {
        let mut nodes: Vec<SubgraphNode> = self.nodes.drain().map(|(_, n)| n).collect();
        let mut rels: Vec<SubgraphRel> = self.rels.drain().map(|(_, r)| r).collect();
        nodes.sort_by_key(|n| n.id);
        rels.sort_by_key(|r| r.id);
        (nodes, rels)
    }

    /// Pending element total (for batch-size flush decisions).
    pub(crate) fn pending(&self) -> usize {
        self.nodes.len() + self.rels.len()
    }

    /// Re-add a built rel whose endpoints have not arrived yet (a streaming
    /// flush retains it for a later batch of the same load).
    pub(crate) fn push_rel(&mut self, rel: SubgraphRel) {
        self.rels.insert(rel.id, rel);
    }
}

/// Map Query API v2 `data.values` into interned graph cells in one walk.
/// The buffered path's dangling-rel policy: a rel whose endpoints are not
/// in this subgraph is dropped (the streaming path retains them for later
/// batches instead — see `stream::StreamingLoad::flush`). Bodies are typed
/// JSON only (ADR-0181); an untagged cell is an error.
pub fn subgraph_from_v2(response: &Json) -> Result<Subgraph, LoadError> {
    let mut acc = SubgraphAccumulator::default();
    if let Some(values) = response
        .get("data")
        .and_then(|d| d.get("values"))
        .and_then(Json::as_array)
    {
        for row in values {
            acc.push_row(row)?;
        }
    }
    let (nodes, rels) = acc.drain();
    let node_ids: HashSet<i64> = nodes.iter().map(|n| n.id).collect();
    let rels: Vec<SubgraphRel> = rels
        .into_iter()
        .filter(|r| node_ids.contains(&r.start) && node_ids.contains(&r.end))
        .collect();
    Ok(Subgraph { nodes, rels })
}

#[derive(Default)]
struct ElementIds {
    ids: HashMap<String, i64>,
    next: i64,
}

impl ElementIds {
    fn intern(&mut self, eid: &str) -> i64 {
        if let Some(&id) = self.ids.get(eid) {
            return id;
        }
        if self.next == 0 {
            self.next = 1;
        }
        let id = self.next;
        self.next += 1;
        self.ids.insert(eid.to_string(), id);
        id
    }
}

/// Property-key sentinel for cell position (ADR-0181 item 7): a cell is not
/// a property, but the error contract has one shape.
const CELL: &str = "<cell>";

/// Element sentinel for a value with no owning element (ADR-0181 item 7).
const RESPONSE: &str = "<response>";

/// A malformed tagged value in cell position: the error contract's
/// `Malformed` decode error with the cell sentinels (ADR-0181 item 7).
fn malformed(tag: &str, element: &str, detail: impl Into<String>) -> TypedWireError {
    TypedWireError::with_context(
        element,
        CELL,
        DecodeError::Malformed {
            tag: tag.to_string(),
            detail: detail.into(),
        },
    )
}

/// The owning element of a cell: its `_element_id`/`elementId` when it has
/// one, else the response sentinel (ADR-0181 item 7).
fn cell_element(obj: &serde_json::Map<String, Json>) -> &str {
    obj.get("_element_id")
        .or_else(|| obj.get("elementId"))
        .and_then(Json::as_str)
        .unwrap_or(RESPONSE)
}

/// Walk one `data.values` row (or a nested `List`/`Path` payload) and intern
/// every tagged element. The typed wire tags every value (ADR-0181 item 3),
/// so anything without a `$type` tag — a bare scalar, a plain-JSON element,
/// a server that ignored the typed `Accept` — is an error, never a
/// fallback.
fn ingest_v2(
    v: &Json,
    ids: &mut ElementIds,
    nodes: &mut HashMap<i64, SubgraphNode>,
    rels: &mut HashMap<i64, SubgraphRel>,
) -> Result<(), TypedWireError> {
    match v {
        Json::Array(items) => {
            for item in items {
                ingest_v2(item, ids, nodes, rels)?;
            }
            Ok(())
        }
        Json::Object(obj) => {
            let Some(tag) = obj.get("$type").and_then(Json::as_str) else {
                return Err(TypedWireError::with_context(
                    cell_element(obj),
                    CELL,
                    DecodeError::Untagged,
                ));
            };
            ingest_typed(tag, obj, ids, nodes, rels)
        }
        _ => Err(TypedWireError::with_context(
            RESPONSE,
            CELL,
            DecodeError::Untagged,
        )),
    }
}

/// Dispatch one tagged cell: `Node`/`Relationship` intern their element,
/// `Path` walks its flat payload, and `List` walks its array payload so
/// `nodes(p)`/`paths(p)` cells contribute their elements. `Path` and `List`
/// are the only tags that carry nested elements (ADR-0181 item 3); every
/// other tagged cell is a scalar that carries no element and is ignored.
fn ingest_typed(
    tag: &str,
    envelope: &serde_json::Map<String, Json>,
    ids: &mut ElementIds,
    nodes: &mut HashMap<i64, SubgraphNode>,
    rels: &mut HashMap<i64, SubgraphRel>,
) -> Result<(), TypedWireError> {
    match tag {
        "Node" => {
            let node = typed_node(element_payload(tag, envelope)?, ids)?;
            nodes.insert(node.id, node);
            Ok(())
        }
        "Relationship" => {
            let rel = typed_rel(element_payload(tag, envelope)?, ids)?;
            rels.insert(rel.id, rel);
            Ok(())
        }
        "Path" | "List" => {
            // The v1.2 Path payload is a flat array of `Node`/`Relationship`
            // typed objects, and `nodes(p)`/`paths(p)` cells are `List`
            // payloads of tagged elements (ADR-0181 item 3); the walk
            // contributes their elements and never a property value. These
            // are the only tags whose array `_value` is walked.
            let payload = element_payload(tag, envelope)?;
            if !payload.is_array() {
                return Err(malformed(
                    tag,
                    cell_element(envelope),
                    "expected an array \"_value\"",
                ));
            }
            ingest_v2(payload, ids, nodes, rels)
        }
        // A tagged scalar carries no element and is ignored.
        _ => Ok(()),
    }
}

/// The `_value` of an element-tagged envelope; absent is malformed
/// (ADR-0181 item 3).
fn element_payload<'a>(
    tag: &str,
    envelope: &'a serde_json::Map<String, Json>,
) -> Result<&'a Json, TypedWireError> {
    envelope
        .get("_value")
        .ok_or_else(|| malformed(tag, cell_element(envelope), "missing \"_value\""))
}

/// A required string field of an element payload (ADR-0181 item 3): absent
/// or non-string is malformed, naming the field and the owning element.
fn required_str<'a>(
    obj: &'a serde_json::Map<String, Json>,
    field: &str,
    tag: &str,
    element: &str,
) -> Result<&'a str, TypedWireError> {
    obj.get(field)
        .and_then(Json::as_str)
        .ok_or_else(|| malformed(tag, element, format!("missing string {field:?}")))
}

/// A `Node` payload (`_element_id`, `_labels`, `_properties`) as a
/// [`SubgraphNode`] (ADR-0181 item 3). `_labels` and `_properties` absent
/// mean empty; a missing `_element_id`, a non-array `_labels`, and a
/// non-object `_properties` are malformed.
fn typed_node(value: &Json, ids: &mut ElementIds) -> Result<SubgraphNode, TypedWireError> {
    let Some(obj) = value.as_object() else {
        return Err(malformed("Node", RESPONSE, "\"_value\" is not an object"));
    };
    let eid = required_str(obj, "_element_id", "Node", cell_element(obj))?;
    Ok(SubgraphNode {
        id: ids.intern(eid),
        element_id: eid.to_string(),
        labels: labels_from(obj.get("_labels"), "Node", eid)?,
        properties: props_from(obj.get("_properties"), "Node", eid)?,
    })
}

/// A `Relationship` payload as a [`SubgraphRel`] (ADR-0181 item 3). The
/// element id, both endpoint ids, and the type are structural: a missing one
/// is malformed rather than an empty-string element. `_properties` absent
/// means empty.
fn typed_rel(value: &Json, ids: &mut ElementIds) -> Result<SubgraphRel, TypedWireError> {
    let Some(obj) = value.as_object() else {
        return Err(malformed(
            "Relationship",
            RESPONSE,
            "\"_value\" is not an object",
        ));
    };
    let eid = required_str(obj, "_element_id", "Relationship", cell_element(obj))?;
    let start = required_str(obj, "_start_node_element_id", "Relationship", eid)?;
    let end = required_str(obj, "_end_node_element_id", "Relationship", eid)?;
    let rel_type = required_str(obj, "_type", "Relationship", eid)?;
    Ok(SubgraphRel {
        id: ids.intern(eid),
        element_id: eid.to_string(),
        start: ids.intern(start),
        end: ids.intern(end),
        rel_type: rel_type.to_string(),
        properties: props_from(obj.get("_properties"), "Relationship", eid)?,
    })
}

/// `_labels` as the label list; absent means empty (ADR-0181 item 3). A
/// present non-array or a non-string entry is malformed.
fn labels_from(v: Option<&Json>, tag: &str, element: &str) -> Result<Vec<String>, TypedWireError> {
    let Some(v) = v else {
        return Ok(Vec::new());
    };
    let Some(items) = v.as_array() else {
        return Err(malformed(tag, element, "\"_labels\" is not an array"));
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| malformed(tag, element, "\"_labels\" entries must be strings"))
        })
        .collect()
}

/// `_properties` as engine-typed values (ADR-0181 items 3 and 7): every
/// value must be `$type`-tagged and decode through the shared decoder, with
/// the owning element id and property key attached on failure. Absent means
/// empty; a present non-object is malformed.
fn props_from(
    v: Option<&Json>,
    tag: &str,
    element: &str,
) -> Result<BTreeMap<String, Value>, TypedWireError> {
    let Some(v) = v else {
        return Ok(BTreeMap::new());
    };
    let Some(obj) = v.as_object() else {
        return Err(malformed(tag, element, "\"_properties\" is not an object"));
    };
    let mut props = BTreeMap::new();
    for (key, value) in obj {
        let decoded = decode_value(value)
            .map_err(|error| TypedWireError::with_context(element, key, error))?;
        props.insert(key.clone(), decoded);
    }
    Ok(props)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A typed `Node` cell (ADR-0181 item 3).
    fn node_cell(eid: &str, labels: &[&str], properties: Json) -> Json {
        json!({
            "$type": "Node",
            "_value": {
                "_element_id": eid,
                "_labels": labels,
                "_properties": properties,
            }
        })
    }

    /// A typed `Relationship` cell (ADR-0181 item 3).
    fn rel_cell(eid: &str, start: &str, end: &str, rel_type: &str, properties: Json) -> Json {
        json!({
            "$type": "Relationship",
            "_value": {
                "_element_id": eid,
                "_start_node_element_id": start,
                "_end_node_element_id": end,
                "_type": rel_type,
                "_properties": properties,
            }
        })
    }

    fn body(values: Json) -> Json {
        json!({"data": {"fields": ["n"], "values": values}})
    }

    /// A plain-JSON node cell, the shape a server that ignored the typed
    /// `Accept` answers with. Built as a map (not a literal) so the loader's
    /// fixture files stay typed-JSON-only; the extractor must fail it as
    /// untagged and name the cell's own element id.
    fn plain_node_cell(element_id: &str) -> Json {
        let mut cell = serde_json::Map::new();
        cell.insert("elementId".into(), Json::String(element_id.into()));
        cell.insert("labels".into(), json!(["Person"]));
        cell.insert("properties".into(), json!({"name": "Ada"}));
        Json::Object(cell)
    }

    /// A plain-JSON relationship cell, like [`plain_node_cell`].
    fn plain_rel_cell(element_id: &str) -> Json {
        let mut cell = serde_json::Map::new();
        cell.insert("elementId".into(), Json::String(element_id.into()));
        cell.insert("startNodeElementId".into(), json!("4:aaa:0"));
        cell.insert("endNodeElementId".into(), json!("4:aaa:1"));
        cell.insert("type".into(), json!("KNOWS"));
        cell.insert("properties".into(), json!({}));
        Json::Object(cell)
    }

    #[test]
    fn intern_distinct_element_ids() {
        let parsed = body(json!([[
            node_cell(
                "4:aaa:0",
                &["Account"],
                json!({"id": {"$type": "String", "_value": "acc-746"}})
            ),
            rel_cell("5:aaa:1", "4:aaa:0", "4:aaa:2", "TRANSFER", json!({})),
            node_cell(
                "4:aaa:2",
                &["Account"],
                json!({"id": {"$type": "String", "_value": "acc-1"}})
            ),
        ]]));
        let g = subgraph_from_v2(&parsed).unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.rels.len(), 1);
        assert_eq!(g.rels[0].rel_type, "TRANSFER");
        assert_ne!(g.nodes[0].id, g.nodes[1].id);
        assert!(g.nodes.iter().any(|n| n.element_id == "4:aaa:0"));
        assert!(g.nodes.iter().any(|n| n.element_id == "4:aaa:2"));
        assert_eq!(g.rels[0].element_id, "5:aaa:1");
        // The properties are engine values, decoded by the typed decoder.
        assert_eq!(g.nodes[0].properties["id"], Value::String("acc-746".into()));
    }

    #[test]
    fn dangling_rel_dropped() {
        let parsed = body(json!([[
            node_cell("4:aaa:1", &["A"], json!({})),
            rel_cell("5:aaa:1", "4:aaa:1", "4:aaa:99", "R", json!({})),
        ]]));
        let g = subgraph_from_v2(&parsed).unwrap();
        assert_eq!(g.nodes.len(), 1);
        assert!(g.rels.is_empty());
    }

    #[test]
    fn scalar_cells_ignored() {
        // A tagged scalar cell carries no element: extraction ignores it
        // (ADR-0181 item 7).
        let parsed = body(json!([[
            node_cell(
                "4:aaa:1",
                &["Person"],
                json!({"name": {"$type": "String", "_value": "Ada"}})
            ),
            {"$type": "Integer", "_value": "42"},
        ]]));
        let g = subgraph_from_v2(&parsed).unwrap();
        assert_eq!(g.nodes.len(), 1);
        assert!(g.rels.is_empty());
        assert_eq!(g.nodes[0].properties["name"], Value::String("Ada".into()));
    }

    #[test]
    fn only_list_cells_walk_their_array_payload() {
        // Only the `List` tag carries nested elements (ADR-0181 item 3): an
        // array `_value` under any other tag contributes nothing, so an
        // unknown or scalar tag cannot smuggle elements into the subgraph.
        let parsed = body(json!([[
            {
                "$type": "Bogus",
                "_value": [node_cell("4:aaa:0", &["A"], json!({}))],
            },
            {
                "$type": "Vector",
                "_value": [node_cell("4:aaa:1", &["B"], json!({}))],
            },
        ]]));
        let g = subgraph_from_v2(&parsed).unwrap();
        assert!(g.nodes.is_empty(), "{:?}", g.nodes);
        assert!(g.rels.is_empty());
    }

    #[test]
    fn non_array_list_cell_is_malformed() {
        // `List._value` is an array of tagged values (ADR-0181 item 3); any
        // other shape is malformed, never silently dropped.
        let parsed = body(json!([[{"$type": "List", "_value": {"not": "an array"}}]]));
        let err = subgraph_from_v2(&parsed).unwrap_err();
        assert!(
            matches!(
                err,
                LoadError::TypedWire(TypedWireError {
                    error: DecodeError::Malformed { .. },
                    ..
                })
            ),
            "{err:?}"
        );
        assert!(err.to_string().contains("List"), "{err}");
    }

    #[test]
    fn typed_path_walks_nested_nodes_and_rels() {
        // The v1.2 Path payload is a flat array of Node/Relationship typed
        // objects (ADR-0181 item 3).
        let parsed = body(json!([[
            {
                "$type": "Path",
                "_value": [
                    node_cell(
                        "4:aaa:0",
                        &["A"],
                        json!({"k": {"$type": "Integer", "_value": "1"}})
                    ),
                    rel_cell("5:aaa:1", "4:aaa:0", "4:aaa:1", "R", json!({})),
                    node_cell("4:aaa:1", &["B"], json!({})),
                ]
            }
        ]]));
        let g = subgraph_from_v2(&parsed).unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.rels.len(), 1);
        assert_eq!(g.rels[0].rel_type, "R");
        assert!(g.nodes.iter().any(|n| n.element_id == "4:aaa:0"));
        assert!(g.nodes.iter().any(|n| n.element_id == "4:aaa:1"));
    }

    #[test]
    fn list_cells_contribute_their_elements() {
        // `RETURN nodes(p)` / `relationships(p)` / `paths(p)` cells are
        // tagged lists of elements.
        let parsed = body(json!([[
            {
                "$type": "List",
                "_value": [
                    node_cell("4:aaa:0", &["A"], json!({})),
                    node_cell("4:aaa:1", &["B"], json!({})),
                ]
            },
            {
                "$type": "List",
                "_value": [
                    {
                        "$type": "Path",
                        "_value": [
                            node_cell("4:aaa:0", &["A"], json!({})),
                            rel_cell("5:aaa:0", "4:aaa:0", "4:aaa:1", "R", json!({})),
                            node_cell("4:aaa:1", &["B"], json!({})),
                        ]
                    }
                ]
            },
        ]]));
        let g = subgraph_from_v2(&parsed).unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.rels.len(), 1);
    }

    #[test]
    fn untagged_cells_error() {
        // A server (or proxy) that ignored the typed `Accept` answers plain
        // JSON: every untagged cell fails the load instead of silently
        // loading untyped strings (ADR-0181 item 7).
        for cell in [
            json!(42),
            json!("Ada"),
            json!(null),
            json!([1, 2]),
            json!({"labels": ["Person"], "properties": {"name": "Ada"}}),
            plain_node_cell("4:aaa:0"),
            plain_rel_cell("5:aaa:0"),
        ] {
            let parsed = body(json!([[cell.clone()]]));
            let err = subgraph_from_v2(&parsed).unwrap_err();
            assert!(
                matches!(
                    err,
                    LoadError::TypedWire(TypedWireError {
                        error: DecodeError::Untagged,
                        ..
                    })
                ),
                "{cell}: {err:?}"
            );
            assert!(err.to_string().contains("untagged"), "{cell}: {err}");
        }
    }

    #[test]
    fn untagged_cell_reports_its_element_id() {
        let parsed = body(json!([[plain_node_cell("4:aaa:0")]]));
        let err = subgraph_from_v2(&parsed).unwrap_err();
        assert!(err.to_string().contains("4:aaa:0"), "{err}");
    }

    #[test]
    fn untagged_property_value_errors_with_key_and_element() {
        let parsed = body(json!([[node_cell(
            "4:aaa:7",
            &["Person"],
            json!({"name": "Ada"})
        )]]));
        let err = subgraph_from_v2(&parsed).unwrap_err();
        assert!(
            matches!(&err, LoadError::TypedWire(TypedWireError {
                element, property, error: DecodeError::Untagged,
            }) if element == "4:aaa:7" && property == "name"),
            "{err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains("4:aaa:7"), "{msg}");
        assert!(msg.contains("\"name\""), "{msg}");
    }

    #[test]
    fn unsupported_property_errors_with_element_and_key() {
        let parsed = body(json!([[node_cell(
            "4:x:0",
            &["TypedUnmappable"],
            json!({
                "name": {"$type": "String", "_value": "unmappable"},
                "blob": {"$type": "Base64", "_value": "A2+/"},
            })
        )]]));
        let err = subgraph_from_v2(&parsed).unwrap_err();
        assert!(
            matches!(&err, LoadError::TypedWire(TypedWireError {
                error: DecodeError::Unsupported { tag }, ..
            }) if tag == "Base64"),
            "{err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains("4:x:0"), "{msg}");
        assert!(msg.contains("\"blob\""), "{msg}");
        assert!(msg.contains("Base64"), "{msg}");
    }

    #[test]
    fn malformed_element_payloads_error() {
        for (payload, needle) in [
            (
                json!({"$type": "Node", "_value": {"_labels": []}}),
                "_element_id",
            ),
            (
                json!({"$type": "Node", "_value": {"_element_id": "4:x:0", "_properties": []}}),
                "_properties",
            ),
            (
                json!({"$type": "Relationship", "_value": {
                    "_element_id": "5:x:0",
                    "_end_node_element_id": "4:x:1",
                    "_type": "R"
                }}),
                "_start_node_element_id",
            ),
            (json!({"$type": "Path", "_value": {"nodes": []}}), "Path"),
            (json!({"$type": "Node"}), "_value"),
        ] {
            let parsed = body(json!([[payload.clone()]]));
            let err = subgraph_from_v2(&parsed).unwrap_err();
            assert!(
                matches!(
                    err,
                    LoadError::TypedWire(TypedWireError {
                        error: DecodeError::Malformed { .. },
                        ..
                    })
                ),
                "{payload}: {err:?}"
            );
            assert!(err.to_string().contains(needle), "{payload}: {err}");
        }
    }
}
