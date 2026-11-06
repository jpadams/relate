//! The Query API v2 value encoding (ADR-0236): `CypherValue` to the
//! Neo4j Query API v2 JSON forms — the typed
//! `application/vnd.neo4j.query` envelope and the plain
//! `application/json` rendering. The server's `DefaultResponseModule` and
//! the clients' hydration codecs are the contract; this is the server's
//! side of it.

use serde_json::{json, Map, Value as Json};

use relate_model::wire;

fn tagged(tag: &str, value: Json) -> Json {
    json!({"$type": tag, "_value": value})
}

/// The Query API v2 value encoding — the Neo4j contract (ADR-0236; the
/// server's `DefaultResponseModule` and the clients' hydration codecs), NOT
/// ADR-0223's kind-tagged wire codec: ADR-0223 is the snapshot/CLI/MCP
/// codec, this surface speaks the HTTP API. The typed form (the negotiated
/// `application/vnd.neo4j.query`) wraps every value as
/// `{"$type": <Cypher type>, "_value": …}`; the plain form
/// (`application/json`) renders plain JSON with entities as plain objects.
/// A client decoder dispatches on `$type` — an ADR-0223 `kind` tag reads
/// as `undefined` to it ("Unknown type: undefined" was exactly that).
pub fn v2_value_json(value: &relate_model::CypherValue, typed: bool) -> Json {
    v2_value_json_at(value, typed, 0)
}

/// [`v2_value_json`] with the ADR-0462 `depth` guard (the number of
/// containers enclosing `value`): a container past
/// [`relate::MAX_VALUE_DEPTH`] encodes as JSON `null` instead of
/// recursing. Unreachable behind the checked constructors.
fn v2_value_json_at(value: &relate_model::CypherValue, typed: bool, depth: u32) -> Json {
    use relate_model::CypherValue as V;

    match value {
        V::Null => {
            if typed {
                tagged("Null", Json::Null)
            } else {
                Json::Null
            }
        }
        V::Bool(b) => {
            if typed {
                tagged("Boolean", json!(b))
            } else {
                json!(b)
            }
        }
        // Typed JSON carries integers and floats as strings.
        V::Int(i) => {
            if typed {
                tagged("Integer", json!(i.to_string()))
            } else {
                json!(i)
            }
        }
        V::Float(f) => {
            if typed {
                tagged("Float", json!(V::Float(*f).to_string()))
            } else {
                wire::json_f64(*f)
            }
        }
        V::Str(s) => {
            if typed {
                tagged("String", json!(s))
            } else {
                json!(s)
            }
        }
        V::List(items) => {
            let Some(next) = relate::next_depth(depth) else {
                return Json::Null;
            };
            // One mapping expression; `typed` is threaded into the closure
            // and the tag wraps the result (the Path arm's shape).
            let inner = Json::Array(
                items
                    .iter()
                    .map(|v| v2_value_json_at(v, typed, next))
                    .collect(),
            );
            if typed {
                tagged("List", inner)
            } else {
                inner
            }
        }
        V::Map(map) => {
            let Some(next) = relate::next_depth(depth) else {
                return Json::Null;
            };
            if typed {
                tagged("Map", Json::Object(v2_map_json_at(map, true, next)))
            } else {
                Json::Object(v2_map_json_at(map, false, next))
            }
        }
        V::Node {
            id,
            labels,
            properties,
        } => {
            let Some(next) = relate::next_depth(depth) else {
                return Json::Null;
            };
            if typed {
                tagged(
                    "Node",
                    json!({
                        "_element_id": id.to_string(),
                        "_labels": labels,
                        "_properties": v2_map_json_at(properties, true, next),
                    }),
                )
            } else {
                json!({
                    "elementId": id.to_string(),
                    "labels": labels,
                    "properties": v2_map_json_at(properties, false, next),
                })
            }
        }
        V::Relationship {
            id,
            start_id,
            end_id,
            rel_type,
            properties,
        } => {
            let Some(next) = relate::next_depth(depth) else {
                return Json::Null;
            };
            if typed {
                tagged(
                    "Relationship",
                    json!({
                        "_element_id": id.to_string(),
                        "_start_node_element_id": start_id.to_string(),
                        "_end_node_element_id": end_id.to_string(),
                        "_type": rel_type,
                        "_properties": v2_map_json_at(properties, true, next),
                    }),
                )
            } else {
                json!({
                    "elementId": id.to_string(),
                    "startNodeElementId": start_id.to_string(),
                    "endNodeElementId": end_id.to_string(),
                    "type": rel_type,
                    "properties": v2_map_json_at(properties, false, next),
                })
            }
        }
        V::Path {
            nodes,
            relationships,
        } => {
            // Alternating [node, rel, node, …, node], typed or plain per
            // the negotiated form.
            if relate::next_depth(depth).is_none() {
                return Json::Null;
            }
            let mut items = Vec::with_capacity(nodes.len() + relationships.len());
            for (i, n) in nodes.iter().enumerate() {
                // A path's embedded entities are not an extra container
                // level (ADR-0462 depth accounting).
                items.push(v2_value_json_at(n, typed, depth));
                if let Some(r) = relationships.get(i) {
                    items.push(v2_value_json_at(r, typed, depth));
                }
            }
            if typed {
                tagged("Path", Json::Array(items))
            } else {
                Json::Array(items)
            }
        }
        // Temporal values: canonical ISO-8601 strings (the wire codec's
        // documented Display), tagged with the Neo4j type name when typed.
        V::Date(_) => temporal(value, "Date", typed),
        V::LocalDateTime(_) => temporal(value, "LocalDateTime", typed),
        V::DateTime(_, zone) => {
            // A fixed offset is OffsetDateTime; an IANA zone name is
            // ZonedDateTime (the server's split).
            temporal(
                value,
                if zone.is_some() {
                    "ZonedDateTime"
                } else {
                    "OffsetDateTime"
                },
                typed,
            )
        }
        V::Time(_, offset) => temporal(
            value,
            if offset.is_some() {
                "Time"
            } else {
                "LocalTime"
            },
            typed,
        ),
        V::Duration(_) => temporal(value, "Duration", typed),
        V::Point(p) => {
            let mut coordinates = vec![json!(p.x), json!(p.y)];
            if let Some(z) = p.z {
                coordinates.push(json!(z));
            }
            let mut crs = json!({
                "srid": p.srid,
                "name": srid_name(p.srid),
                "type": "link",
            });
            if let Some(href) = srid_href(p.srid) {
                crs["properties"] = json!({"href": href, "type": "ogcwkt"});
            }
            if typed {
                tagged("Point", json!({"coordinates": coordinates, "crs": crs}))
            } else {
                json!({"type": "Point", "coordinates": coordinates, "crs": crs})
            }
        }
        V::Vector { coord, values, .. } => {
            let Some(next) = relate::next_depth(depth) else {
                return Json::Null;
            };
            if typed {
                tagged(
                    "Vector",
                    json!({
                        // The Neo4j coordinatesType vocabulary (INT8 …
                        // FLOAT64); coordinates are strings there.
                        "coordinatesType": vector_coord_name(coord),
                        "coordinates": values
                            .iter()
                            .map(|v| Json::String(v.to_string()))
                            .collect::<Vec<_>>(),
                    }),
                )
            } else {
                Json::Array(
                    values
                        .iter()
                        .map(|v| v2_value_json_at(v, false, next))
                        .collect(),
                )
            }
        }
        // Geometry is a Relate extension the Neo4j client codecs do not
        // know: typed reports it honestly as Unsupported, plain keeps the
        // documented ADR-0223 wire shape.
        V::Geometry(_) => {
            if typed {
                tagged("Unsupported", json!(format!("{value}")))
            } else {
                wire::value_to_wire_json(value)
            }
        }
    }
}

/// A property/map value under the v2 encoding.
pub fn v2_map_json(
    map: &indexmap::IndexMap<String, relate_model::CypherValue>,
    typed: bool,
) -> Map<String, Json> {
    v2_map_json_at(map, typed, 0)
}

/// [`v2_map_json`] with the ADR-0462 `depth` guard.
fn v2_map_json_at(
    map: &indexmap::IndexMap<String, relate_model::CypherValue>,
    typed: bool,
    depth: u32,
) -> Map<String, Json> {
    map.iter()
        .map(|(k, v)| (k.clone(), v2_value_json_at(v, typed, depth)))
        .collect()
}

/// A temporal value: the canonical ISO-8601 string, tagged when typed.
fn temporal(value: &relate_model::CypherValue, tag: &str, typed: bool) -> Json {
    if typed {
        tagged(tag, Json::String(value.to_string()))
    } else {
        Json::String(value.to_string())
    }
}

/// The CRS name for a point SRID (the server's mapping).
fn srid_name(srid: i32) -> &'static str {
    match srid {
        7203 => "cartesian",
        9157 => "cartesian-3d",
        4326 => "wgs-84",
        4979 => "wgs-84-3d",
        _ => "n/a",
    }
}

/// The OGC WKT href for a known SRID (the server's mapping).
fn srid_href(srid: i32) -> Option<String> {
    match srid {
        7203 | 9157 => Some(format!(
            "http://spatialreference.org/ref/sr-org/{srid}/ogcwkt/"
        )),
        4326 | 4979 => Some(format!(
            "http://spatialreference.org/ref/epsg/{srid}/ogcwkt/"
        )),
        _ => None,
    }
}

/// The Neo4j `coordinatesType` vocabulary for a vector's coordinate kind.
fn vector_coord_name(coord: &relate_model::VectorCoordKind) -> &'static str {
    use relate_model::VectorCoordKind as K;
    match coord {
        K::Int8 => "INT8",
        K::Int16 => "INT16",
        K::Int32 => "INT32",
        K::Int64 => "INT64",
        K::Float16 => "FLOAT16",
        K::BFloat16 => "BFLOAT16",
        K::Float32 => "FLOAT32",
        K::Float64 => "FLOAT64",
    }
}
