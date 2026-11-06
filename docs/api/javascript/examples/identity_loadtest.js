const { runIdentity, DEFAULTS } = require("../../../../clients/javascript/loadtest/identity");

const report = runIdentity({ ...DEFAULTS, ops: 8, warmup: 2, out: "" });
if (report.name !== "identity-mixed-iam") {
  throw new Error(report.name);
}
if (report.ops.filter((o) => o.phase === "measured").length !== 8) {
  throw new Error("measured count");
}
