#!/usr/bin/env node
"use strict";

const { parseArgs, printMix } = require("../loadtest/common");
const { runFraud, DEFAULTS } = require("../loadtest/fraud");

const HELP = `Mixed read/write load test on the synthetic fraud graph

Usage:
  node bin/fraud-loadtest.js [flags]

Flags:
  --uri <URI>              Database URI (:memory:, path, or file://) (default :memory:)
  --scale <F64>            Graph size vs full generator (1.0 = 10,000 accounts) (default 0.05)
  --graph-seed <N>         RNG for the synthetic fraud graph (default 42)
  --workload-seed <N>      RNG for use-case mix and account picks (default 7)
  --fanout-k <N>           Planted mule out-degree (fanout-src destinations) (default 12)
  --cycle-len <N>          Planted directed cycle length (cycle-0 …) (default 4)
  --ops <N>                Measured operations after warmup (default 80)
  --warmup <N>             Unmeasured operations run first (default 8)
  --out <PATH>             Report JSON path (default target/fraud-loadtest.json)
  --wal-dir <PATH>         Accepted for flag parity; ignored (C ABI has no WAL directory)
  --reset                  Delete the existing database at --uri (and --wal-dir if set) before load
  --workers <N>            Accepted for flag parity; Node runs ops on one thread (default 1)
  --write-fraction <F64>   Fraction of ops that are writes (0.0 = reads only, 1.0 = writes only) (default 0.5)
  --logical-graph-size-limit <SIZE>  Live graph + index cap (bytes or mb/mib/gb/gib; sets RELATE_LOGICAL_GRAPH_SIZE_LIMIT)
`;

function main() {
  let cfg;
  try {
    cfg = parseArgs(process.argv.slice(2), DEFAULTS);
  } catch (e) {
    console.error(e.message || e);
    process.exit(1);
  }
  if (cfg.help) {
    process.stdout.write(HELP);
    return;
  }
  if (cfg.workers > 1) {
    console.error("warning: --workers is ignored in JavaScript (single-threaded C ABI calls)");
  }
  const r = runFraud(cfg);
  const ds = r.dataset;
  printMix(
    r,
    cfg.out,
    `graph nodes ${ds.start.nodes} → ${ds.finish.nodes}, transfers ${ds.start.transfers} → ${ds.finish.transfers}`
  );
}

main();
