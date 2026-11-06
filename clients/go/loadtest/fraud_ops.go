package loadtest

import (
	"github.com/relatedb/relate/clients/go"
)

// One type per fraud operation, carrying its own plan data (its fields)
// and its own exec step (its exec method): the operation vocabulary is
// stated once per operation — a type plus a planner plus a catalog row in
// fraudOpSpecs — mirroring identityOpSpecs (LABS-429), which mirrors the
// Rust catalog's per-row plan fn. A new operation threads through one row,
// not through a plan switch and an exec switch.
//
// fraudOp is the fraud instantiation of the shared op contract (execOp in
// mixed.go): exec reports per the shared op-exec policy (ops.go), and
// onboard_customer alone overrides grow() to record its new ids
// into the account and customer pools.
type fraudOp = execOp

// One planned transfer row: the fields of one CREATE TRANSFER statement.
type transferRow struct {
	from, to, id string
	amount       float64
	ts           int64
}

// transfersOp is the multi-statement transfers op: each planned row is one
// CREATE TRANSFER statement. Four catalog rows plan one of these (their
// usecases differ in how the rows are picked, not in how they execute), so
// the transfer execution is stated once here.
type transfersOp struct {
	baseOp
	rows []transferRow
}

func (op transfersOp) exec(conn *relate.Connection) (uint32, int, error) {
	// The op's true statement count — one per planned row, no floor:
	// every planner yields at least one row (generateFraud's ordinary
	// pool floors at minScale's 500 accounts, so an empty plan is a
	// broken planner, and the report must surface it as 0, not paper
	// over it). Write statements yield no rows (no RETURN), so the row
	// count is the constant 0. The counts ride the error in the
	// writeExec/readExec shape — contract documentation, since runOp
	// (the failure half's one enforcer) discards them on failure.
	n := uint32(len(op.rows))
	for _, t := range op.rows {
		if _, err := writeQuery(conn,
			"MATCH (a:Account {id: $from}) MATCH (b:Account {id: $to}) CREATE (a)-[:TRANSFER {id: $id, amount: $amount, ts: $ts}]->(b)",
			map[string]any{"from": t.from, "to": t.to, "id": t.id, "amount": t.amount, "ts": t.ts},
		); err != nil {
			return n, 0, err
		}
	}
	return n, 0, nil
}

type onboardCustomerOp struct {
	baseOp
	w              *fraudWork
	cid, aid, name string
}

func (op onboardCustomerOp) exec(conn *relate.Connection) (uint32, int, error) {
	return writeExec(conn,
		"CREATE (c:Customer {id: $cid, name: $name}) CREATE (a:Account {id: $aid}) CREATE (a)-[:OWNED_BY]->(c)",
		map[string]any{"cid": op.cid, "name": op.name, "aid": op.aid},
	)
}

// grow applies onboard_customer's pool growth after a successful exec.
func (op onboardCustomerOp) grow() {
	op.w.ordinary = append(op.w.ordinary, op.aid)
	op.w.customers = append(op.w.customers, op.cid)
}

type recentTransfersOp struct {
	baseOp
	id string
}

func (op recentTransfersOp) exec(conn *relate.Connection) (uint32, int, error) {
	return readExec(conn,
		"MATCH (a:Account {id: $id})-[t:TRANSFER]->(b) RETURN t.id AS id, t.amount AS amount, b.id AS to ORDER BY t.ts DESC LIMIT 20",
		map[string]any{"id": op.id})
}

type customerPortfolioOp struct {
	baseOp
	id string
}

func (op customerPortfolioOp) exec(conn *relate.Connection) (uint32, int, error) {
	return readExec(conn,
		"MATCH (c:Customer {id: $id})<-[:OWNED_BY]-(a:Account) RETURN a.id AS id",
		map[string]any{"id": op.id})
}

type outgoingVolumeOp struct {
	baseOp
	id string
}

func (op outgoingVolumeOp) exec(conn *relate.Connection) (uint32, int, error) {
	return readExec(conn,
		"MATCH (a:Account {id: $id})-[t:TRANSFER]->() RETURN sum(t.amount) AS volume, count(t) AS n",
		map[string]any{"id": op.id})
}

type detectFanoutOp struct {
	baseOp
	id string
	k  int64
}

func (op detectFanoutOp) exec(conn *relate.Connection) (uint32, int, error) {
	return readExec(conn,
		"MATCH (a:Account {id: $id})-[:TRANSFER]->(b) WITH a, count(DISTINCT b) AS n WHERE n >= $k RETURN a.id AS id, n",
		map[string]any{"id": op.id, "k": op.k})
}

type detectCycleOp struct{ baseOp }

func (detectCycleOp) exec(conn *relate.Connection) (uint32, int, error) {
	return readExec(conn,
		"MATCH (a:Account {planted: 'cycle'})-[:TRANSFER*2..6]->(a) RETURN DISTINCT a.id AS id LIMIT 20",
		nil)
}

type activityPageOp struct {
	baseOp
	id   string
	skip int64
}

func (op activityPageOp) exec(conn *relate.Connection) (uint32, int, error) {
	return readExec(conn,
		"MATCH (a:Account {id: $id})-[t:TRANSFER]->(b) RETURN t.id AS id, t.amount AS amount, b.id AS to ORDER BY t.ts DESC SKIP $skip LIMIT 10",
		map[string]any{"id": op.id, "skip": op.skip})
}

type searchCustomerOp struct {
	baseOp
	q string
}

func (op searchCustomerOp) exec(conn *relate.Connection) (uint32, int, error) {
	return readExec(conn,
		"MATCH (c:Customer) WHERE c.name CONTAINS $q RETURN c.id AS id, c.name AS name ORDER BY c.name LIMIT 10",
		map[string]any{"q": op.q})
}

type counterpartiesOp struct {
	baseOp
	id string
}

func (op counterpartiesOp) exec(conn *relate.Connection) (uint32, int, error) {
	return readExec(conn,
		"MATCH (a:Account {id: $id})-[:TRANSFER]-(b:Account) RETURN DISTINCT b.id AS id LIMIT 20",
		map[string]any{"id": op.id})
}

type paymentPathOp struct {
	baseOp
	from, to string
}

func (op paymentPathOp) exec(conn *relate.Connection) (uint32, int, error) {
	return readExec(conn,
		"MATCH (a:Account {id: $from}), (b:Account {id: $to}) RETURN length(shortestPath((a)-[:TRANSFER*1..3]->(b))) AS hops",
		map[string]any{"from": op.from, "to": op.to})
}

type amountBandOp struct {
	baseOp
	id string
}

func (op amountBandOp) exec(conn *relate.Connection) (uint32, int, error) {
	return readExec(conn,
		"MATCH (a:Account {id: $id})-[t:TRANSFER]->() RETURN CASE WHEN t.amount >= 1000.0 THEN 'large' ELSE 'small' END AS band, count(*) AS n",
		map[string]any{"id": op.id})
}

type flagAccountOp struct {
	baseOp
	id string
}

func (op flagAccountOp) exec(conn *relate.Connection) (uint32, int, error) {
	return writeExec(conn, "MATCH (a:Account {id: $id}) SET a.frozen = true", map[string]any{"id": op.id})
}

type reverseLastOutOp struct {
	baseOp
	id string
}

func (op reverseLastOutOp) exec(conn *relate.Connection) (uint32, int, error) {
	return writeExec(conn, "MATCH (a:Account {id: $id})-[t:TRANSFER]->() WITH t ORDER BY t.ts DESC LIMIT 1 DELETE t", map[string]any{"id": op.id})
}

// One planner per op, mirroring Rust's plan_* functions: each draws its
// picks from the workload (under the caller's mutex) and builds its op.
// The planner is the only place an op's picks are made; exec never draws.
func planP2PTransfer(w *fraudWork) fraudOp {
	from, to := w.pickTwo()
	return transfersOp{rows: []transferRow{{from: from, to: to, id: w.alloc("lt-tx"), amount: w.amount(), ts: w.ts()}}}
}

func planPayrollScatter(w *fraudWork) fraudOp {
	from := w.pickAccount()
	var rows []transferRow
	for _, d := range w.pickDistinct(4) {
		if d == from {
			continue
		}
		rows = append(rows, transferRow{from: from, to: d, id: w.alloc("lt-pay"), amount: 2500.0, ts: w.ts()})
	}
	return transfersOp{rows: rows}
}

func planOnboardCustomer(w *fraudWork) fraudOp {
	cid := w.alloc("lt-cust")
	aid := w.alloc("lt-acc")
	return onboardCustomerOp{w: w, cid: cid, aid: aid, name: "Loadtest " + cid}
}

func planMuleScatter(w *fraudWork) fraudOp {
	var rows []transferRow
	for _, d := range w.pickDistinct(3) {
		rows = append(rows, transferRow{from: w.fanoutSrc, to: d, id: w.alloc("lt-mule"), amount: 99.0, ts: w.ts()})
	}
	return transfersOp{rows: rows}
}

func planLayeringCycle(w *fraudWork) fraudOp {
	// The short-pick fallback (fewer than three distinct picks replays the
	// planted cycle accounts) is the pre-existing client dialect, carried
	// verbatim from the former plan switch — preserved bit-for-bit so the
	// fused form replays identical reports (LABS-487); tracked with the
	// statement-count floor for its own change.
	nodes := w.pickDistinct(3)
	if len(nodes) < 3 {
		nodes = append([]string(nil), w.cycle...)
	}
	var rows []transferRow
	for i, from := range nodes {
		rows = append(rows, transferRow{from: from, to: nodes[(i+1)%len(nodes)], id: w.alloc("lt-cyc"), amount: 1000.0 + float64(i), ts: w.ts()})
	}
	return transfersOp{rows: rows}
}

func planRecentTransfers(w *fraudWork) fraudOp {
	return recentTransfersOp{id: w.pickAccount()}
}

func planCustomerPortfolio(w *fraudWork) fraudOp {
	return customerPortfolioOp{id: w.pickCustomer()}
}

func planOutgoingVolume(w *fraudWork) fraudOp {
	return outgoingVolumeOp{id: w.pickAccount()}
}

func planDetectFanout(w *fraudWork) fraudOp {
	return detectFanoutOp{id: w.fanoutSrc, k: w.fanoutK}
}

func planDetectCycle(*fraudWork) fraudOp {
	return detectCycleOp{}
}

func planActivityPage(w *fraudWork) fraudOp {
	return activityPageOp{id: w.pickAccount(), skip: int64(w.rng.Intn(3) * 5)}
}

func planSearchCustomer(w *fraudWork) fraudOp {
	return searchCustomerOp{q: w.pickSearchQ()}
}

func planCounterparties(w *fraudWork) fraudOp {
	return counterpartiesOp{id: w.pickAccount()}
}

func planPaymentPath(w *fraudWork) fraudOp {
	from, to := w.pickTwo()
	return paymentPathOp{from: from, to: to}
}

func planAmountBand(w *fraudWork) fraudOp {
	return amountBandOp{id: w.pickAccount()}
}

func planFlagAccount(w *fraudWork) fraudOp {
	return flagAccountOp{id: w.pickAccount()}
}

func planReverseLastOut(w *fraudWork) fraudOp {
	return reverseLastOutOp{id: w.pickAccount()}
}

// fraudOpSpec: one catalog row per operation, stating its metadata and its
// planner together. This is the fraud vocabulary's only table — the pick
// helpers, summarize, and the shared runner consume it directly through
// their catalogRow / opSpecRow views — replacing the former pair of a bare
// metadata table plus a string-keyed plan switch and a string-keyed exec
// switch (the same shape LABS-429 deleted one layer down, in identity).
type fraudOpSpec struct {
	catalogEntry
	plan func(*fraudWork) fraudOp
}

// planOp adapts the plan field to the shared catalog-row contract (opSpecRow
// in mixed.go): the shared runner dispatches through it.
func (s fraudOpSpec) planOp(w *fraudWork) fraudOp { return s.plan(w) }

var fraudOpSpecs = []fraudOpSpec{
	{catalogEntry{ID: "p2p_transfer", Kind: KindWrite, Description: "Ordinary customer payment: CREATE one TRANSFER between two accounts", Weight: 28}, planP2PTransfer},
	{catalogEntry{ID: "payroll_scatter", Kind: KindWrite, Description: "Payroll-like scatter: one source pays several distinct destinations", Weight: 8}, planPayrollScatter},
	{catalogEntry{ID: "onboard_customer", Kind: KindWrite, Description: "KYC onboard: new Customer, Account, and OWNED_BY", Weight: 6}, planOnboardCustomer},
	{catalogEntry{ID: "mule_scatter", Kind: KindWrite, Related: true, Description: "Fraud continuation: extra scatter from planted fan-out mule", Weight: 6}, planMuleScatter},
	{catalogEntry{ID: "layering_cycle", Kind: KindWrite, Related: true, Description: "Layering: directed cycle of TRANSFERs among 3 accounts", Weight: 6}, planLayeringCycle},
	{catalogEntry{ID: "recent_transfers", Kind: KindRead, Description: "Account activity: last transfers out, ORDER BY ts DESC", Weight: 16}, planRecentTransfers},
	{catalogEntry{ID: "customer_portfolio", Kind: KindRead, Description: "Customer view: accounts owned by a customer", Weight: 10}, planCustomerPortfolio},
	{catalogEntry{ID: "outgoing_volume", Kind: KindRead, Description: "Statement: sum of outgoing TRANSFER amounts", Weight: 8}, planOutgoingVolume},
	{catalogEntry{ID: "detect_fanout", Kind: KindRead, Related: true, Description: "AML: out-degree of a given account vs threshold", Weight: 6}, planDetectFanout},
	{catalogEntry{ID: "detect_cycle", Kind: KindRead, Related: true, Description: "AML: directed TRANSFER cycles of length 2–6", Weight: 6}, planDetectCycle},
	{catalogEntry{ID: "activity_page", Kind: KindRead, Description: "Paginated activity: ORDER BY ts DESC SKIP/LIMIT", Weight: 8}, planActivityPage},
	{catalogEntry{ID: "search_customer", Kind: KindRead, Description: "Directory search: Customer.name CONTAINS a customer number", Weight: 6}, planSearchCustomer},
	{catalogEntry{ID: "counterparties", Kind: KindRead, Description: "Undirected TRANSFER neighbors of an account", Weight: 8}, planCounterparties},
	{catalogEntry{ID: "payment_path", Kind: KindRead, Description: "shortestPath along outgoing TRANSFER (directed, short)", Weight: 6}, planPaymentPath},
	{catalogEntry{ID: "amount_band", Kind: KindRead, Description: "Statement: CASE amount band plus count", Weight: 6}, planAmountBand},
	{catalogEntry{ID: "flag_account", Kind: KindWrite, Description: "Compliance hold: SET Account.frozen", Weight: 5}, planFlagAccount},
	{catalogEntry{ID: "reverse_last_out", Kind: KindWrite, Description: "Reversal: DELETE the latest outgoing TRANSFER", Weight: 5}, planReverseLastOut},
}
