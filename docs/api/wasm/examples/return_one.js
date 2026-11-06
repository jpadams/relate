// One Cypher round-trip through the wasm build. Assumes relate.js and
// relate_bg.wasm are next to this file (as scripts/build-wasm.sh emits
// them into surfaces/relate-wasm/pkg/; see docs/api/wasm/spec.md).
import init, { RelateDb } from "./relate.js";

await init();
const db = new RelateDb();

const result = JSON.parse(db.execute("RETURN 1 AS n", null));
if (result.rows[0].n !== 1) {
  throw new Error("unexpected result: " + JSON.stringify(result));
}
