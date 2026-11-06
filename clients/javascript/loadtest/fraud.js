"use strict";

const {
  mulberry32,
  intRange,
  writeQuery,
  unwind,
  readOp,
  writeOp,
  scalarInt,
  runMixedLoadtest,
} = require("./common");

function customerSearchQ(id) {
  if (!id.startsWith("cust-")) return null;
  const rest = id.slice(5);
  if (rest.length > 0 && /^[0-9]+$/.test(rest)) return rest;
  return null;
}

// transfersOp is the multi-statement transfers op: each planned row is one
// CREATE TRANSFER statement. Four catalog rows plan one of these (their
// usecases differ in how the rows are picked, not in how they execute), so
// the transfer execution is stated once here (the JS twin of the Go
// transfersOp type).
function transfersOp(plan) {
  return {
    exec(conn) {
      for (const { from, to, id, amount, ts } of plan) {
        writeQuery(
          conn,
          "MATCH (a:Account {id: $from}) MATCH (b:Account {id: $to}) CREATE (a)-[:TRANSFER {id: $id, amount: $amount, ts: $ts}]->(b)",
          { from, to, id, amount, ts }
        );
      }
      // The op's true statement count — one per planned row, no floor:
      // every planner yields at least one row (generateFraud's ordinary
      // pool floors at 20, so an empty plan is a broken planner, and the
      // report must surface it as 0, not paper over it). Write
      // statements yield no rows (no RETURN), so the row count is the
      // constant 0.
      return { statements: plan.length, rows: 0 };
    },
  };
}

// The fused fraud catalog: one row per operation, stating its metadata and
// its planner together. The planner draws its picks from the workload (in
// the same order the former plan switch drew them) and returns an op object
// that carries its own plan data and its own exec step — so the operation
// vocabulary is stated once per operation (mirrors the Go client's
// fraudOpSpec and the Rust catalog's per-row plan fn; LABS-487, after
// LABS-429 did the same to identity).
const CATALOG = [
  {
    id: "p2p_transfer", kind: "write", related: false, description: "Ordinary customer payment: CREATE one TRANSFER between two accounts", weight: 28,
    plan(work) {
      const [from, to] = work.pickTwo();
      return transfersOp([{ from, to, id: work.alloc("lt-tx"), amount: work.amount(), ts: work.ts() }]);
    },
  },
  {
    id: "payroll_scatter", kind: "write", related: false, description: "Payroll-like scatter: one source pays several distinct destinations", weight: 8,
    plan(work) {
      const from = work.pickAccount();
      const rows = work
        .pickDistinct(4)
        .filter((d) => d !== from)
        .map((d) => ({ from, to: d, id: work.alloc("lt-pay"), amount: 2500.0, ts: work.ts() }));
      return transfersOp(rows);
    },
  },
  {
    id: "onboard_customer", kind: "write", related: false, description: "KYC onboard: new Customer, Account, and OWNED_BY", weight: 6,
    plan(work) {
      const cid = work.alloc("lt-cust");
      const aid = work.alloc("lt-acc");
      return writeOp(
        "CREATE (c:Customer {id: $cid, name: $name}) CREATE (a:Account {id: $aid}) CREATE (a)-[:OWNED_BY]->(c)",
        { cid, name: `Loadtest ${cid}`, aid },
        { onboard: { account: aid, customer: cid } }
      );
    },
  },
  {
    id: "mule_scatter", kind: "write", related: true, description: "Fraud continuation: extra scatter from planted fan-out mule", weight: 6,
    plan(work) {
      return transfersOp(
        work.pickDistinct(3).map((d) => ({ from: work.fanoutSrc, to: d, id: work.alloc("lt-mule"), amount: 99.0, ts: work.ts() }))
      );
    },
  },
  {
    id: "layering_cycle", kind: "write", related: true, description: "Layering: directed cycle of TRANSFERs among 3 accounts", weight: 6,
    plan(work) {
      // The short-pick fallback (fewer than three distinct picks replays
      // the planted cycle accounts) is the pre-existing client dialect,
      // carried verbatim from the former plan switch — preserved
      // bit-for-bit so the fused form replays identical reports
      // (LABS-487); tracked with the statement-count floor.
      let nodes = work.pickDistinct(3);
      if (nodes.length < 3) nodes = work.cycle.slice();
      return transfersOp(
        nodes.map((from, i) => ({
          from,
          to: nodes[(i + 1) % nodes.length],
          id: work.alloc("lt-cyc"),
          amount: 1000 + i,
          ts: work.ts(),
        }))
      );
    },
  },
  {
    id: "recent_transfers", kind: "read", related: false, description: "Account activity: last transfers out, ORDER BY ts DESC", weight: 16,
    plan(work) {
      const id = work.pickAccount();
      return readOp(
        "MATCH (a:Account {id: $id})-[t:TRANSFER]->(b) RETURN t.id AS id, t.amount AS amount, b.id AS to ORDER BY t.ts DESC LIMIT 20",
        { id }
      );
    },
  },
  {
    id: "customer_portfolio", kind: "read", related: false, description: "Customer view: accounts owned by a customer", weight: 10,
    plan(work) {
      const id = work.pickCustomer();
      return readOp("MATCH (c:Customer {id: $id})<-[:OWNED_BY]-(a:Account) RETURN a.id AS id", { id });
    },
  },
  {
    id: "outgoing_volume", kind: "read", related: false, description: "Statement: sum of outgoing TRANSFER amounts", weight: 8,
    plan(work) {
      const id = work.pickAccount();
      return readOp(
        "MATCH (a:Account {id: $id})-[t:TRANSFER]->() RETURN sum(t.amount) AS volume, count(t) AS n",
        { id }
      );
    },
  },
  {
    id: "detect_fanout", kind: "read", related: true, description: "AML: out-degree of a given account vs threshold", weight: 6,
    plan(work) {
      const id = work.fanoutSrc;
      const k = work.fanoutK;
      return readOp(
        "MATCH (a:Account {id: $id})-[:TRANSFER]->(b) WITH a, count(DISTINCT b) AS n WHERE n >= $k RETURN a.id AS id, n",
        { id, k }
      );
    },
  },
  {
    id: "detect_cycle", kind: "read", related: true, description: "AML: directed TRANSFER cycles of length 2–6", weight: 6,
    plan() {
      return readOp("MATCH (a:Account {planted: 'cycle'})-[:TRANSFER*2..6]->(a) RETURN DISTINCT a.id AS id LIMIT 20", {});
    },
  },
  {
    id: "activity_page", kind: "read", related: false, description: "Paginated activity: ORDER BY ts DESC SKIP/LIMIT", weight: 8,
    plan(work) {
      const id = work.pickAccount();
      const skip = intRange(work.rng, 3) * 5;
      return readOp(
        "MATCH (a:Account {id: $id})-[t:TRANSFER]->(b) RETURN t.id AS id, t.amount AS amount, b.id AS to ORDER BY t.ts DESC SKIP $skip LIMIT 10",
        { id, skip }
      );
    },
  },
  {
    id: "search_customer", kind: "read", related: false, description: "Directory search: Customer.name CONTAINS a customer number", weight: 6,
    plan(work) {
      const q = work.pickSearchQ();
      return readOp(
        "MATCH (c:Customer) WHERE c.name CONTAINS $q RETURN c.id AS id, c.name AS name ORDER BY c.name LIMIT 10",
        { q }
      );
    },
  },
  {
    id: "counterparties", kind: "read", related: false, description: "Undirected TRANSFER neighbors of an account", weight: 8,
    plan(work) {
      const id = work.pickAccount();
      return readOp(
        "MATCH (a:Account {id: $id})-[:TRANSFER]-(b:Account) RETURN DISTINCT b.id AS id LIMIT 20",
        { id }
      );
    },
  },
  {
    id: "payment_path", kind: "read", related: false, description: "shortestPath along outgoing TRANSFER (directed, short)", weight: 6,
    plan(work) {
      const [from, to] = work.pickTwo();
      return readOp(
        "MATCH (a:Account {id: $from}), (b:Account {id: $to}) RETURN length(shortestPath((a)-[:TRANSFER*1..3]->(b))) AS hops",
        { from, to }
      );
    },
  },
  {
    id: "amount_band", kind: "read", related: false, description: "Statement: CASE amount band plus count", weight: 6,
    plan(work) {
      const id = work.pickAccount();
      return readOp(
        "MATCH (a:Account {id: $id})-[t:TRANSFER]->() RETURN CASE WHEN t.amount >= 1000.0 THEN 'large' ELSE 'small' END AS band, count(*) AS n",
        { id }
      );
    },
  },
  {
    id: "flag_account", kind: "write", related: false, description: "Compliance hold: SET Account.frozen", weight: 5,
    plan(work) {
      const id = work.pickAccount();
      return writeOp("MATCH (a:Account {id: $id}) SET a.frozen = true", { id });
    },
  },
  {
    id: "reverse_last_out", kind: "write", related: false, description: "Reversal: DELETE the latest outgoing TRANSFER", weight: 5,
    plan(work) {
      const id = work.pickAccount();
      return writeOp("MATCH (a:Account {id: $id})-[t:TRANSFER]->() WITH t ORDER BY t.ts DESC LIMIT 1 DELETE t", { id });
    },
  },
];

function logUniform(rng, min, max) {
  const v = Math.exp(rng() * (Math.log(max) - Math.log(min)) + Math.log(min));
  return Math.max(min, Math.min(max, Math.round(v)));
}

function generateFraud({ graphSeed, scale, fanoutK, cycleLen }) {
  const rng = mulberry32(graphSeed >>> 0);
  let nAccounts = Math.max(20, Math.round(10000 * scale));
  const scaleT = Math.max(scale, 0.05);
  const tMin = Math.max(10 * scaleT, 2);
  const tMax = Math.max(1000 * scaleT, tMin + 1);

  const customers = [];
  const accounts = [];
  const ownership = [];
  let i = 0;
  let custN = 0;
  while (i < nAccounts) {
    const nOwned = 1 + intRange(rng, Math.min(3, nAccounts - i));
    const cid = `cust-${custN}`;
    customers.push({ id: cid, name: `Customer ${custN}` });
    for (let k = 0; k < nOwned; k++) {
      const aid = `acc-${i}`;
      accounts.push({ id: aid, planted: null });
      ownership.push([aid, cid]);
      i++;
    }
    custN++;
  }

  const transfers = [];
  let tid = 0;
  for (let fromIdx = 0; fromIdx < accounts.length; fromIdx++) {
    const n = logUniform(rng, tMin, tMax);
    for (let t = 0; t < n; t++) {
      let toIdx = intRange(rng, accounts.length);
      if (toIdx === fromIdx) toIdx = (toIdx + 1) % accounts.length;
      transfers.push({
        id: `tx-${tid}`,
        from: accounts[fromIdx].id,
        to: accounts[toIdx].id,
        amount: Math.round((10 + rng() * (10000 - 10)) * 100) / 100,
        ts: 1700000000 + tid,
      });
      tid++;
    }
  }

  const fanoutSrc = "fanout-src";
  accounts.push({ id: fanoutSrc, planted: "fanout" });
  customers.push({ id: "cust-planted-fanout", name: "Planted Fanout" });
  ownership.push([fanoutSrc, "cust-planted-fanout"]);
  let k = Math.max(3, Math.min(fanoutK, accounts.length - 1));
  const dests = [];
  while (dests.length < k) {
    const id = accounts[intRange(rng, nAccounts)].id;
    if (!dests.includes(id)) dests.push(id);
  }
  for (const d of dests) {
    transfers.push({ id: `tx-fanout-${tid}`, from: fanoutSrc, to: d, amount: 99.0, ts: 1710000000 + tid });
    tid++;
  }

  cycleLen = Math.max(3, Math.min(6, cycleLen));
  const cycle = [];
  customers.push({ id: "cust-planted-cycle", name: "Planted Cycle" });
  for (let n = 0; n < cycleLen; n++) {
    const id = `cycle-${n}`;
    accounts.push({ id, planted: "cycle" });
    ownership.push([id, "cust-planted-cycle"]);
    cycle.push(id);
  }
  for (let n = 0; n < cycleLen; n++) {
    transfers.push({
      id: `tx-cycle-${n}`,
      from: cycle[n],
      to: cycle[(n + 1) % cycleLen],
      amount: 1000 + n,
      ts: 1720000000 + n,
    });
  }

  return {
    seed: graphSeed,
    scale,
    customers,
    accounts,
    ownership,
    transfers,
    planted: { fanout_source: fanoutSrc, fanout_k: k, cycle_accounts: cycle },
  };
}

function loadFraud(conn, g) {
  unwind(
    conn,
    g.customers.map((c) => ({ id: c.id, name: c.name })),
    "UNWIND $rows AS c CREATE (:Customer {id: c.id, name: c.name})"
  );
  unwind(
    conn,
    g.accounts.map((a) => ({ id: a.id, planted: a.planted })),
    "UNWIND $rows AS a CREATE (:Account {id: a.id, planted: a.planted})"
  );
  unwind(
    conn,
    g.ownership.map(([a, c]) => ({ a, c })),
    "UNWIND $rows AS r MATCH (a:Account {id: r.a}) MATCH (c:Customer {id: r.c}) CREATE (a)-[:OWNED_BY]->(c)"
  );
  unwind(
    conn,
    g.transfers.map((t) => ({ id: t.id, from: t.from, to: t.to, amount: t.amount, ts: t.ts })),
    "UNWIND $rows AS t MATCH (a:Account {id: t.from}) MATCH (b:Account {id: t.to}) CREATE (a)-[:TRANSFER {id: t.id, amount: t.amount, ts: t.ts}]->(b)"
  );
}

function snapshot(conn) {
  return {
    nodes: scalarInt(conn, "MATCH (n) RETURN count(n) AS n"),
    relationships: scalarInt(conn, "MATCH ()-[r]->() RETURN count(r) AS n"),
    accounts: scalarInt(conn, "MATCH (a:Account) RETURN count(a) AS n"),
    customers: scalarInt(conn, "MATCH (c:Customer) RETURN count(c) AS n"),
    transfers: scalarInt(conn, "MATCH ()-[t:TRANSFER]->() RETURN count(t) AS n"),
  };
}

function makeWork(graph, seed) {
  return {
    rng: mulberry32(seed >>> 0),
    ordinary: graph.accounts.filter((a) => !a.planted).map((a) => a.id),
    customers: graph.customers.map((c) => c.id),
    fanoutSrc: graph.planted.fanout_source,
    cycle: graph.planted.cycle_accounts.slice(),
    next: 1,
    fanoutK: graph.planted.fanout_k,
    pickAccount() {
      return this.ordinary[intRange(this.rng, this.ordinary.length)];
    },
    pickTwo() {
      const a = this.pickAccount();
      let b = this.pickAccount();
      if (a === b && this.ordinary.length > 1) {
        b = this.ordinary[(intRange(this.rng, this.ordinary.length - 1) + 1) % this.ordinary.length];
        if (b === a) b = this.ordinary.find((x) => x !== a) || a;
      }
      return [a, b];
    },
    pickCustomer() {
      return this.customers[intRange(this.rng, this.customers.length)];
    },
    pickSearchQ() {
      for (let i = 0; i < 16; i++) {
        const n = customerSearchQ(this.pickCustomer());
        if (n != null) return n;
      }
      for (const id of this.customers) {
        const n = customerSearchQ(id);
        if (n != null) return n;
      }
      return "0";
    },
    pickDistinct(n) {
      const out = [];
      let guard = 0;
      while (out.length < n && guard < n * 20) {
        guard++;
        const a = this.pickAccount();
        if (!out.includes(a)) out.push(a);
      }
      return out;
    },
    alloc(prefix) {
      const id = `${prefix}-${this.next}`;
      this.next++;
      return id;
    },
    amount() {
      return Math.round((5 + this.rng() * (2500 - 5)) * 100) / 100;
    },
    ts() {
      return 1730000000 + this.next;
    },
  };
}

function runFraud(cfg) {
  return runMixedLoadtest(cfg, {
    name: "fraud-mixed-payments",
    configExtras: (c) => ({ fanout_k: c.fanoutK, cycle_len: c.cycleLen }),
    generate: generateFraud,
    load: loadFraud,
    snapshot,
    makeWork,
    catalog: CATALOG,
    applyResult: (work, out) => {
      if (out.onboard) {
        work.ordinary.push(out.onboard.account);
        work.customers.push(out.onboard.customer);
      }
    },
  });
}

const DEFAULTS = {
  uri: ":memory:",
  scale: 0.05,
  graphSeed: 42,
  workloadSeed: 7,
  fanoutK: 12,
  cycleLen: 4,
  ops: 80,
  warmup: 8,
  out: "target/fraud-loadtest.json",
  walDir: "",
  reset: false,
  workers: 1,
  writeFraction: 0.5,
  logicalGraphSizeLimit: "",
};

module.exports = { CATALOG, generateFraud, loadFraud, runFraud, DEFAULTS, customerSearchQ };
