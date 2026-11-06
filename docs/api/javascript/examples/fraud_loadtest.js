const { runFraud, DEFAULTS } = require("../../../../clients/javascript/loadtest/fraud");

const report = runFraud({ ...DEFAULTS, ops: 8, warmup: 2, out: "" });
if (report.name !== "fraud-mixed-payments") {
  throw new Error(report.name);
}
if (report.ops.filter((o) => o.phase === "measured").length !== 8) {
  throw new Error("measured count");
}
