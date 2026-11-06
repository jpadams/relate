// Run a multi-statement script through the wasm build (ADR-0131).
// executeScript splits with the same scanner as the CLI, so the `;` inside
// the string literal and the comment does not split anything; it returns one
// envelope per statement and stops at the first error. Assumes relate.js
// and relate_bg.wasm are next to this file (see docs/api/wasm/spec.md).
import init, { RelateDb } from "./relate.js";

await init();
const db = new RelateDb();

const script = `
  CREATE (:Note {text: 'first; not a terminator'});  // a note; still one statement
  CREATE (:Note {text: $second});
  MATCH (n:Note) RETURN count(n) AS notes
`;

const envelopes = db.executeScript(script, JSON.stringify({ second: "second" })).map((e) => JSON.parse(e));
const last = envelopes[envelopes.length - 1];
if (envelopes.length !== 3 || last.error || last.rows[0].notes !== 2) {
  throw new Error("unexpected result: " + JSON.stringify(envelopes));
}

// A failing statement ends the script; the ones before it stay applied.
const failed = db.executeScript("CREATE (:Note); RETURN 1 +; CREATE (:Note)", null);
if (failed.length !== 2 || JSON.parse(failed[1]).error === null) {
  throw new Error("expected the script to stop at the error: " + failed);
}
