"use strict";

// Graph extraction from materialized rows (ADR-0216; ADR-0214's semantics
// on the JS side). Pure logic over the decoded value shapes (ADR-0502 §1)
// — no FFI here.

/// The unique graph entities of one result, de-duplicated by element id
/// and kept in first-seen order: `nodes` and `relationships` are arrays of
/// the decoded shapes ({kind, elementId, id, labels, properties} /
/// {kind, elementId, id, type, startElementId, endElementId, startId,
/// endId, properties}).
class ResultGraph {
  constructor(nodes, relationships) {
    this.nodes = nodes;
    this.relationships = relationships;
  }
  get size() {
    return this.nodes.length + this.relationships.length;
  }
  [Symbol.for("nodejs.util.inspect.custom")]() {
    return `ResultGraph(nodes=${this.nodes.length}, relationships=${this.relationships.length})`;
  }
}

// Decoded shapes (the value walker, ADR-0502 §1): entities are plain
// objects carrying an explicit `kind` discriminator — "node" /
// "relationship" — with the engine's elementId() spelling as the identity.
// What makes one an entity is structural, not its keys: the decoder stamps
// every NODE / RELATIONSHIP it produces with this non-enumerable marker
// (value = the entity kind), and graph() dispatches on the marker alone. A
// user MAP — even one carrying kind "node" plus every entity key — has no
// marker, so it stays a map and is walked like any other. The marker is
// module-local on purpose, never Symbol.for: the global registry would let
// any code forge or probe it, and the one invariant entity recognition
// stands on is that only a decoded entity can carry it.
const DECODED_ENTITY = Symbol("relate.decoded");

/// Build one decoded entity: the documented plain shape — the entity's
/// own fields plus `kind`, the discriminator key — stamped with the
/// non-enumerable marker (value = the kind). Non-enumerable: it never
/// shows up in Object.keys, JSON.stringify, spreads or deep-equality.
/// `kind` is written last, from the argument: this constructor is the one
/// writer of the discriminator, so a fields object cannot override it and
/// the visible kind can never disagree with the marker. Only this
/// constructor stamps: no exported function can mark an existing object,
/// so a user object can never become an entity.
function decodedEntity(kind, fields) {
  const entity = { ...fields, kind };
  Object.defineProperty(entity, DECODED_ENTITY, { value: kind });
  return entity;
}

const isPlainObject = (v) =>
  v !== null && typeof v === "object" && !Array.isArray(v);

/// The marker's value (the entity kind) on a decoded entity, or undefined
/// on anything else — the read half of the marker pair, for tests and
/// diagnostics.
function decodedEntityKind(value) {
  return isPlainObject(value) ? value[DECODED_ENTITY] : undefined;
}

function collectGraphEntities(rows) {
  const nodes = new Map();
  const relationships = new Map();
  // Which collection an entity kind lands in is data, not control flow —
  // a Map, so the lookup consults exactly the two kinds and nothing an
  // object's prototype chain might offer under another name.
  const buckets = new Map([
    ["node", nodes],
    ["relationship", relationships],
  ]);

  function walk(value) {
    const bucket = buckets.get(decodedEntityKind(value));
    if (bucket) {
      if (!bucket.has(value.elementId)) bucket.set(value.elementId, value);
    } else if (Array.isArray(value)) {
      // A path is an alternating [node, rel, node, ...] array; every array
      // is walked, so paths and user lists both contribute their entities.
      for (const item of value) walk(item);
    } else if (isPlainObject(value)) {
      for (const item of Object.values(value)) walk(item);
    }
  }

  for (const row of rows) {
    for (const value of Object.values(row)) walk(value);
  }
  // Map preserves insertion order and the keys are unique element ids, so
  // the values arrays are the de-duplicated, first-seen-order entities
  // directly. (Element ids are opaque strings — "<db>:<store>:n<n>" — and
  // lexicographic sorting would misorder them; first-seen is the honest,
  // defined order.)
  return new ResultGraph([...nodes.values()], [...relationships.values()]);
}

module.exports = { ResultGraph, collectGraphEntities, decodedEntity, decodedEntityKind };
