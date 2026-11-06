"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const { percentiles } = require("./common");
const { runFraud, customerSearchQ } = require("./fraud");
const { runIdentity } = require("./identity");

function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}

function assertMix(r, ops, warmup) {
  const measured = r.ops.filter((o) => o.phase === "measured");
  const warm = r.ops.filter((o) => o.phase === "warmup");
  assert(measured.length === ops, `measured ${measured.length}`);
  assert(warm.length === warmup, `warmup ${warm.length}`);
  const failed = measured.filter((o) => !o.ok);
  assert(failed.length === 0, `ops failed: ${JSON.stringify(failed)}`);
  assert(measured.every((o) => o.latency_ns > 0), "zero latency");
  const reads = measured.filter((o) => o.kind === "read").length;
  const writes = measured.filter((o) => o.kind === "write").length;
  assert(reads > 0 && writes > 0, `mixed kinds r=${reads} w=${writes}`);
  assert(measured.some((o) => o.related), "expected related ops");
  assert(r.throughput.read_ops === reads && r.throughput.write_ops === writes, "throughput counts");
  assert(r.throughput.ops_per_sec > 0 && r.load_duration_ms > 0 && r.worker_duration_ms > 0, "clocks");
}

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "mdb-lt-"));
const fraudOut = path.join(dir, "fraud.json");
const fraud = runFraud({
  uri: ":memory:",
  scale: 0.05,
  graphSeed: 42,
  workloadSeed: 7,
  fanoutK: 12,
  cycleLen: 4,
  ops: 32,
  warmup: 4,
  out: fraudOut,
  walDir: "",
  reset: false,
  workers: 1,
  writeFraction: 0.5,
});
assert(fraud.name === "fraud-mixed-payments" && fraud.schema_version === 5, "fraud meta");
assert(fraud.config.write_fraction === 0.5, "fraud write_fraction");
assert(fraud.planted.fanout_source === "fanout-src", "fanout planted");
assert(fraud.dataset.start.accounts >= 500 && fraud.dataset.start.transfers > 0, "fraud size");
assertMix(fraud, 32, 4);
assert(fs.existsSync(fraudOut), "fraud report");

const identityOut = path.join(dir, "identity.json");
const identity = runIdentity({
  uri: ":memory:",
  scale: 0.05,
  graphSeed: 42,
  workloadSeed: 7,
  nestDepth: 4,
  groupK: 12,
  ops: 32,
  warmup: 4,
  out: identityOut,
  walDir: "",
  reset: false,
  workers: 1,
  writeFraction: 0.5,
});
assert(identity.name === "identity-mixed-iam" && identity.schema_version === 5, "identity meta");
assert(identity.config.write_fraction === 0.5, "identity write_fraction");
assert(identity.planted.nested_admin === "nested-admin", "nested admin");
assert(identity.dataset.start.users >= 500 && identity.dataset.start.memberships > 0, "identity size");
assertMix(identity, 32, 4);
assert(fs.existsSync(identityOut), "identity report");

const readsOnly = runFraud({
  uri: ":memory:",
  scale: 0.05,
  graphSeed: 42,
  workloadSeed: 7,
  fanoutK: 12,
  cycleLen: 4,
  ops: 12,
  warmup: 0,
  out: "",
  walDir: "",
  reset: false,
  workers: 1,
  writeFraction: 0,
});
assert(readsOnly.ops.every((o) => o.kind === "read"), "write-fraction 0 reads");
assert(readsOnly.throughput.write_ops === 0, "write-fraction 0 writes");

const writesOnly = runIdentity({
  uri: ":memory:",
  scale: 0.05,
  graphSeed: 42,
  workloadSeed: 7,
  nestDepth: 4,
  groupK: 12,
  ops: 12,
  warmup: 0,
  out: "",
  walDir: "",
  reset: false,
  workers: 1,
  writeFraction: 1,
});
assert(writesOnly.ops.every((o) => o.kind === "write"), "write-fraction 1 writes");
assert(writesOnly.throughput.read_ops === 0, "write-fraction 1 reads");

assert(customerSearchQ("cust-42") === "42", "search q 42");
assert(customerSearchQ("cust-0") === "0", "search q 0");
assert(customerSearchQ("cust-planted-fanout") == null, "planted not numeric");
assert(customerSearchQ("lt-cust-3") == null, "onboard not numeric");

const lat = percentiles([10, 20, 30, 40, 50]);
assert(lat.min === 10 && lat.max === 50 && lat.p50 === 30, "percentiles");

console.log("loadtest ok");
