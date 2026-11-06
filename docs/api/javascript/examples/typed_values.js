// Typed values, errors and async queries (ADR-0502, the C ABI v2): the
// value walker's JS shapes, the params builder's typed classes, the one
// error shape, bookmarks, and the Promise-returning query form. Run from a
// checkout with the FFI library built (or RELATE_LIB set):
// node docs/api/javascript/examples/typed_values.js

const {
  Database, RelateError, CypherDate, float,
} = require("@relate/relate");

const db = Database.open(":memory:");
const conn = db.connect();

// int64 is lossless (LABS-901): inside ±2^53-1 a Cypher INTEGER is a JS
// number; beyond it a BigInt — `RETURN 9007199254740993` round-trips
// exactly, and a BigInt parameter crosses untouched.
const big = conn.query("RETURN 9007199254740993 AS lit, $bound AS bound", {
  bound: 9223372036854775807n,
}).single();
console.log(`${big.lit === 9007199254740993n ? "ok" : "BROKEN"}: ${big.lit}, ${big.bound}`);

// Strings keep interior NULs; the kinds JSON could not spell decode to
// typed classes with lossless components.
const values = conn.query(
  "RETURN 'a\\u0000b' AS s, date('2026-11-04') AS d, duration('P1Y2M') AS dur, " +
    "point({x: 1, y: 2, z: 3}) AS p, vector([1, 2], 2, INTEGER8) AS v"
).single();
console.log(values.s.length === 3 ? "ok: interior NUL" : "BROKEN: string truncated");
console.log(`${values.d.year}-${values.d.month}-${values.d.day}`, // a CypherDate
  values.dur.months, // a CypherDuration: 12 months, lossless
  values.p.srid, values.p.z, // a CypherPoint
  values.v.dtype, values.v.values.join(",")); // a CypherVector

// The same classes bind back as parameters — decode and encode share them,
// so a decoded value (or a hand-constructed one) round-trips unchanged.
const echoed = conn.query(
  "RETURN $d = date('2026-11-04') AS dOk, $dur AS dur, $p AS p, $v AS v",
  { d: new CypherDate(2026, 11, 4), dur: values.dur, p: values.p, v: values.v }
).single();
console.log(`round-trip ${echoed.dOk}, ${echoed.dur.months} months, srid ${echoed.p.srid}`);

// A float-typed 1: the JSON era could not send it. float(1) binds FLOAT
// 1.0 — observable through the rendering (`=` compares numerically across
// INT and FLOAT, so toString is the discriminator).
const f = conn.query("RETURN $f AS f, toString($f) AS s", { f: float(1) }).single();
console.log(`float(1) renders as ${f.s}`); // "1.0", like the literal 1.0

// Errors are one typed shape (ADR-0502 §2) with the engine's own
// classification — code, GQLSTATUS, stage and the retryable flag.
try {
  conn.query("RETURN 1/0");
  throw new Error("unreachable");
} catch (e) {
  if (!(e instanceof RelateError)) throw e;
  console.log(`${e.code} (${e.gqlStatus}, ${e.stage}, retryable=${e.retryable}): ${e.message}`);
}

// Bookmarks (ADR-0502 §7): an auto-commit write's bookmark rides the
// result; an explicit transaction's is commit()'s return value.
const auto = conn.query("CREATE (:Example)").bookmark;
conn.begin();
conn.query("CREATE (:Example)");
const committed = conn.commit();
console.log(`bookmarks: ${auto}, ${committed}`);

// The async form: the call runs on koffi's worker thread, and a failing
// statement's error rides the result handle back — the rejected promise
// carries the Neo4j code. Overlapping calls on one connection are
// serialized (one call at a time is the connection contract).
(async () => {
  const row = await conn.queryAsync("RETURN 1 AS n");
  console.log(`async: ${row.single().n}`);
  try {
    await conn.queryAsync("RETURN 1/0");
    throw new Error("unreachable");
  } catch (e) {
    console.log(`async error recovered: ${e.code}`);
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
