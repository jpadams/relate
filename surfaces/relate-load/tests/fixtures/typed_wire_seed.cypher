// typed_wire_seed.cypher
//
// Ground-truth sample graph for
// surfaces/relate-load/tests/fixtures/typed_wire.json.
// Run against an EMPTY Neo4j database: the fixture's `mappable` query expects
// exactly one :TypedSample node and one :TYPED_REL relationship.
//
// Server compatibility:
//   * Neo4j 2026.09-enterprise: run this file as-is.
//   * Neo4j 5.26-enterprise: drop the `uuid`, `list_uuid` and `vector`
//     properties (the uuid() and vector() functions do not exist there).
//
// The BYTE ARRAY property has no Cypher literal (Neo4j: "byte arrays are not
// considered a first class data type by Cypher, so they do not have a literal
// representation"). The :TypedUnmappable node's `blob` property is therefore
// added by the typed-JSON parameter step documented at the bottom; running
// this file with cypher-shell recreates everything else.
//
// Usage (2026.09):
//   cypher-shell -u neo4j -p <password> -d neo4j -f typed_wire_seed.cypher
//   then run the curl step at the bottom.

// --- mappable sample: one property per Cypher type, and one LIST property
// --- per listable type ------------------------------------------------------
CREATE (n:TypedSample {
  name: 'mappable',
  integer: 42,
  float: 3.14159,
  string: 'hello',
  boolean: true,
  date: date('2015-03-26'),
  time: time('12:50:35.556+01:00'),
  localtime: localtime('12:50:35.556'),
  datetime_named_zone: datetime('2015-11-21T21:40:32.142[Antarctica/Troll]'),
  datetime_offset: datetime('2015-11-21T21:40:32.142-01:00'),
  localdatetime: localdatetime('2015-07-04T19:32:24'),
  duration: duration('P14DT16H12M'),
  point_cartesian: point({srid: 7203, x: 1.2, y: 3.4}),
  point_cartesian_3d: point({srid: 9157, x: 1.0, y: 2.0, z: 3.0}),
  point_wgs84: point({srid: 4326, longitude: -0.1278, latitude: 51.5074}),
  point_wgs84_3d: point({srid: 4979, longitude: -0.1278, latitude: 51.5074, height: 11.0}),
  uuid: uuid('550e8400-e29b-41d4-a716-446655440000'),
  vector: vector([1.0, 2.0], 2, FLOAT64),
  list_uuid: [uuid('550e8400-e29b-41d4-a716-446655440000'), uuid('00000000-0000-0000-0000-000000000000')],
  list_integer: [1, 2, 3],
  list_float: [1.5, 2.5],
  list_string: ['a', 'b'],
  list_boolean: [true, false],
  list_date: [date('2020-01-01'), date('2020-01-02')],
  list_time: [time('01:02:03Z'), time('04:05:06+02:00')],
  list_localtime: [localtime('01:02:03'), localtime('04:05:06')],
  list_datetime: [datetime('2020-01-01T10:00:00[Europe/Stockholm]'), datetime('2020-06-01T10:00:00+02:00')],
  list_localdatetime: [localdatetime('2020-01-01T10:00:00'), localdatetime('2020-06-01T10:00:00')],
  list_duration: [duration('P1D'), duration('PT2H30M')],
  list_point: [point({srid: 4326, longitude: 0.0, latitude: 0.0}), point({srid: 4326, longitude: 1.0, latitude: 2.0})]
});

// --- relationship target and relationship ----------------------------------
CREATE (p:TypedPeer {name: 'peer'});

MATCH (n:TypedSample {name: 'mappable'}), (p:TypedPeer {name: 'peer'})
CREATE (n)-[:TYPED_REL {
  since: date('2020-01-01'),
  at: point({srid: 4326, longitude: 10.0, latitude: 20.0})
}]->(p);

// --- unmappable sample: BYTE ARRAY property --------------------------------
// `blob` is added by the typed-JSON parameter step below.
CREATE (u:TypedUnmappable {name: 'unmappable'});

// --- typed-JSON parameter step (not expressible in pure Cypher) ------------
// Cypher has no byte-array literal, so the BYTE ARRAY property is sent as a
// typed JSON parameter. Content-Type application/vnd.neo4j.query.v1.2 on
// Neo4j 2026.09 (application/vnd.neo4j.query on 5.26).
//
//   curl -sS -u neo4j:<password> \
//     -H 'Content-Type: application/vnd.neo4j.query.v1.2' \
//     -H 'Accept: application/vnd.neo4j.query.v1.2' \
//     -d '{"statement":"MATCH (u:TypedUnmappable) SET u.blob = $blob",
//          "parameters":{"blob":{"$type":"Base64","_value":"A2+/"}}}' \
//     http://127.0.0.1:<port>/db/neo4j/query/v2
//
// The three bytes decode to 0x03 0x6f 0xbf.
