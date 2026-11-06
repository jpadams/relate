package loadtest

import (
	"fmt"
	"io"
	"math"
	"math/rand"
	"slices"

	"github.com/relatedb/relate/clients/go"
)

type FraudCustomer struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

type FraudAccount struct {
	ID      string  `json:"id"`
	Planted *string `json:"planted"`
}

type FraudTransfer struct {
	ID     string  `json:"id"`
	From   string  `json:"from"`
	To     string  `json:"to"`
	Amount float64 `json:"amount"`
	Ts     int64   `json:"ts"`
}

type FraudPlanted struct {
	FanoutSource  string   `json:"fanout_source"`
	FanoutK       int64    `json:"fanout_k"`
	CycleAccounts []string `json:"cycle_accounts"`
}

type FraudGraph struct {
	Seed      int64           `json:"seed"`
	Scale     float64         `json:"scale"`
	Customers []FraudCustomer `json:"customers"`
	Accounts  []FraudAccount  `json:"accounts"`
	Ownership [][2]string     `json:"ownership"`
	Transfers []FraudTransfer `json:"transfers"`
	Planted   FraudPlanted    `json:"planted"`
}

type FraudReportConfig struct {
	URI           string  `json:"uri"`
	Scale         float64 `json:"scale"`
	GraphSeed     int64   `json:"graph_seed"`
	WorkloadSeed  int64   `json:"workload_seed"`
	FanoutK       int     `json:"fanout_k"`
	CycleLen      int     `json:"cycle_len"`
	Ops           int     `json:"ops"`
	Warmup        int     `json:"warmup"`
	Workers       int     `json:"workers"`
	WriteFraction float64 `json:"write_fraction"`
}

type FraudSnapshot struct {
	Nodes         int64 `json:"nodes"`
	Relationships int64 `json:"relationships"`
	Accounts      int64 `json:"accounts"`
	Customers     int64 `json:"customers"`
	Transfers     int64 `json:"transfers"`
}

// fraudWork is the fraud workload state: the shared workloadState (mix RNG
// and id allocator) plus the fraud-specific pools and planted anchors.
type fraudWork struct {
	*workloadState
	ordinary  []string
	customers []string
	fanoutSrc string
	cycle     []string
	fanoutK   int64
}

// normalizeFraud moves the generator's former silent clamps to the Config
// boundary: one normalization, before any side effect, whose result is what
// the generator, the workload, and the report all see.
func normalizeFraud(cfg *Config) error {
	if err := normalizeScale(cfg); err != nil {
		return err
	}
	cfg.FanoutK = min(max(cfg.FanoutK, 3), graphSize(cfg.Scale))
	cfg.CycleLen = min(max(cfg.CycleLen, 3), 6)
	return nil
}

// fraudCLIFlags is the fraud workload's own flag table, consumed only by
// FraudCLI (the descriptor both the main and the tests drive).
func fraudCLIFlags() []CLIFlag {
	return []CLIFlag{
		intFlag("fanout-k", "Planted mule out-degree (fanout-src destinations)", func(c *Config) *int { return &c.FanoutK }),
		intFlag("cycle-len", "Planted directed cycle length (cycle-0 …)", func(c *Config) *int { return &c.CycleLen }),
	}
}

// generateFraud builds the fraud graph from normalized Config values: scale
// is at least minScale, fanoutK in [3, ordinary accounts], cycleLen in
// [3, 6] (normalizeFraud).
func generateFraud(seed int64, scale float64, fanoutK, cycleLen int) *FraudGraph {
	rng := rand.New(rand.NewSource(seed))
	nAccounts := graphSize(scale)
	tMin := math.Max(10.0*scale, 2.0)
	tMax := math.Max(1000.0*scale, tMin+1.0)

	var customers []FraudCustomer
	var accounts []FraudAccount
	var ownership [][2]string
	i, custN := 0, 0
	for i < nAccounts {
		nOwned := 1 + rng.Intn(min(3, nAccounts-i))
		cid := fmt.Sprintf("cust-%d", custN)
		customers = append(customers, FraudCustomer{ID: cid, Name: fmt.Sprintf("Customer %d", custN)})
		for k := 0; k < nOwned; k++ {
			aid := fmt.Sprintf("acc-%d", i)
			accounts = append(accounts, FraudAccount{ID: aid})
			ownership = append(ownership, [2]string{aid, cid})
			i++
		}
		custN++
	}

	var transfers []FraudTransfer
	var tid int64
	for fromIdx := 0; fromIdx < len(accounts); fromIdx++ {
		n := logUniform(rng, tMin, tMax)
		for t := 0; t < n; t++ {
			toIdx := rng.Intn(len(accounts))
			if toIdx == fromIdx {
				toIdx = (toIdx + 1) % len(accounts)
			}
			transfers = append(transfers, FraudTransfer{
				ID: fmt.Sprintf("tx-%d", tid), From: accounts[fromIdx].ID, To: accounts[toIdx].ID,
				Amount: math.Round((10.0+rng.Float64()*(10_000.0-10.0))*100) / 100, Ts: 1_700_000_000 + tid,
			})
			tid++
		}
	}

	fanoutSrc := "fanout-src"
	pFanout := "fanout"
	accounts = append(accounts, FraudAccount{ID: fanoutSrc, Planted: &pFanout})
	cid := "cust-planted-fanout"
	customers = append(customers, FraudCustomer{ID: cid, Name: "Planted Fanout"})
	ownership = append(ownership, [2]string{fanoutSrc, cid})
	// fanoutK is normalized to [3, nAccounts]; the ordinary accounts are
	// accounts[0:nAccounts], so the distinct-destination draw has enough
	// candidates.
	dests := pickDistinct(rng, accounts[:nAccounts], fanoutK)
	for _, d := range dests {
		transfers = append(transfers, FraudTransfer{
			ID: fmt.Sprintf("tx-fanout-%d", tid), From: fanoutSrc, To: d.ID, Amount: 99.0, Ts: 1_710_000_000 + tid,
		})
		tid++
	}

	var cycle []string
	cid = "cust-planted-cycle"
	customers = append(customers, FraudCustomer{ID: cid, Name: "Planted Cycle"})
	pCycle := "cycle"
	for n := 0; n < cycleLen; n++ {
		id := fmt.Sprintf("cycle-%d", n)
		accounts = append(accounts, FraudAccount{ID: id, Planted: &pCycle})
		ownership = append(ownership, [2]string{id, cid})
		cycle = append(cycle, id)
	}
	for n := 0; n < cycleLen; n++ {
		transfers = append(transfers, FraudTransfer{
			ID: fmt.Sprintf("tx-cycle-%d", n), From: cycle[n], To: cycle[(n+1)%cycleLen],
			Amount: 1000.0 + float64(n), Ts: 1_720_000_000 + int64(n),
		})
	}

	return &FraudGraph{
		Seed: seed, Scale: scale, Customers: customers, Accounts: accounts,
		Ownership: ownership, Transfers: transfers,
		Planted: FraudPlanted{FanoutSource: fanoutSrc, FanoutK: int64(fanoutK), CycleAccounts: cycle},
	}
}

func loadFraud(conn *relate.Connection, g *FraudGraph) error {
	return runSteps(
		unwindStep(conn, g.Customers, "UNWIND $rows AS c CREATE (:Customer {id: c.id, name: c.name})"),
		unwindStep(conn, g.Accounts, "UNWIND $rows AS a CREATE (:Account {id: a.id, planted: a.planted})"),
		unwindStep(conn, edgeRows(g.Ownership), "UNWIND $rows AS r MATCH (a:Account {id: r.from}) MATCH (c:Customer {id: r.to}) CREATE (a)-[:OWNED_BY]->(c)"),
		unwindStep(conn, g.Transfers, "UNWIND $rows AS t MATCH (a:Account {id: t.from}) MATCH (b:Account {id: t.to}) CREATE (a)-[:TRANSFER {id: t.id, amount: t.amount, ts: t.ts}]->(b)"),
	)
}

// The fraud workload's spec for the shared mixed runner (runMixedLoadtest
// in mixed.go): the pieces the orchestration varies per workload, mirroring
// the spec object JS's runFraud passes to its runMixedLoadtest. Everything
// else — validation, the load and snapshot sequences, the pick → plan →
// exec loop, the report envelope — lives once in the runner.
var fraudSpec = mixedSpec[*FraudGraph, FraudSnapshot, FraudReportConfig, FraudPlanted, *fraudWork, fraudOpSpec]{
	name: "fraud-mixed-payments",
	generate: func(cfg Config) *FraudGraph {
		return generateFraud(cfg.GraphSeed, cfg.Scale, cfg.FanoutK, cfg.CycleLen)
	},
	load:      loadFraud,
	snapshot:  fraudSnapshot,
	newWork:   newFraudWork,
	normalize: normalizeFraud,
	planted:   func(g *FraudGraph) FraudPlanted { return g.Planted },
	reportConfig: func(cfg Config) FraudReportConfig {
		return FraudReportConfig{
			URI: cfg.URI, Scale: cfg.Scale, GraphSeed: cfg.GraphSeed, WorkloadSeed: cfg.WorkloadSeed,
			FanoutK: cfg.FanoutK, CycleLen: cfg.CycleLen, Ops: cfg.Ops, Warmup: cfg.Warmup, Workers: cfg.Workers,
			WriteFraction: cfg.WriteFraction,
		}
	},
	catalog: fraudOpSpecs,
}

// FraudCLI states the fraud binary's command line once: the descriptor
// (name, summary, defaults, vocabulary, workload flag table) with the
// caller's run/print pair — the cmd main and the CLI tests both build
// their CLI through it, so the tests drive the descriptor that ships.
func FraudCLI[R any](
	run func(Config) (R, error),
	print func(R, string, io.Writer),
) CLI[R] {
	return CLI[R]{
		Name:          "fraud-loadtest",
		Summary:       "Mixed read/write load test on the synthetic fraud graph",
		Defaults:      DefaultFraud(),
		Vocabulary:    CLIVocabulary{Graph: "fraud", ScaleUnit: "accounts", PickNoun: "account"},
		WorkloadFlags: fraudCLIFlags(),
		Run:           run,
		Print:         print,
	}
}

func RunFraud(cfg Config) (*Report[FraudSnapshot, FraudReportConfig, FraudPlanted], error) {
	return runMixedLoadtest(cfg, fraudSpec)
}

func PrintFraud(r *Report[FraudSnapshot, FraudReportConfig, FraudPlanted], out string, w io.Writer) {
	printReport(w, r, r.Config.Ops, r.Config.Workers, out,
		fmt.Sprintf("graph nodes %d → %d, transfers %d → %d\n",
			r.Dataset.Start.Nodes, r.Dataset.Finish.Nodes, r.Dataset.Start.Transfers, r.Dataset.Finish.Transfers))
}

// The workload construction, extracted from RunFraud's orchestration
// (mirrors makeWork on the JS side and newIdentityWork here): the ordinary
// pools read the typed planted markers; the shared workloadState is seeded
// once from cfg.WorkloadSeed (the write fraction stays with the runner).
func newFraudWork(graph *FraudGraph, cfg Config) *fraudWork {
	work := &fraudWork{
		workloadState: newWorkloadState(cfg),
		fanoutSrc:     graph.Planted.FanoutSource,
		cycle:         graph.Planted.CycleAccounts,
		fanoutK:       graph.Planted.FanoutK,
	}
	for _, a := range graph.Accounts {
		if a.Planted == nil {
			work.ordinary = append(work.ordinary, a.ID)
		}
	}
	for _, c := range graph.Customers {
		work.customers = append(work.customers, c.ID)
	}
	return work
}

func fraudSnapshot(conn *relate.Connection) (FraudSnapshot, error) {
	var s FraudSnapshot
	var err error
	if s.Nodes, err = scalarInt(conn, "MATCH (n) RETURN count(n) AS n"); err != nil {
		return s, err
	}
	if s.Relationships, err = scalarInt(conn, "MATCH ()-[r]->() RETURN count(r) AS n"); err != nil {
		return s, err
	}
	if s.Accounts, err = scalarInt(conn, "MATCH (a:Account) RETURN count(a) AS n"); err != nil {
		return s, err
	}
	if s.Customers, err = scalarInt(conn, "MATCH (c:Customer) RETURN count(c) AS n"); err != nil {
		return s, err
	}
	s.Transfers, err = scalarInt(conn, "MATCH ()-[t:TRANSFER]->() RETURN count(t) AS n")
	return s, err
}

func (w *fraudWork) pickAccount() string {
	return w.ordinary[w.rng.Intn(len(w.ordinary))]
}

func (w *fraudWork) pickTwo() (string, string) {
	a := w.pickAccount()
	b := w.pickAccount()
	if a == b && len(w.ordinary) > 1 {
		b = w.ordinary[(w.rng.Intn(len(w.ordinary)-1)+1)%len(w.ordinary)]
		if b == a {
			for _, x := range w.ordinary {
				if x != a {
					return a, x
				}
			}
		}
	}
	return a, b
}

func (w *fraudWork) pickCustomer() string {
	return w.customers[w.rng.Intn(len(w.customers))]
}

// customerSearchQ maps cust-42 → 42. Planted / onboard ids are not numeric names.
func customerSearchQ(id string) (string, bool) {
	const prefix = "cust-"
	if len(id) <= len(prefix) || id[:len(prefix)] != prefix {
		return "", false
	}
	rest := id[len(prefix):]
	for _, c := range rest {
		if c < '0' || c > '9' {
			return "", false
		}
	}
	return rest, true
}

func (w *fraudWork) pickSearchQ() string {
	for i := 0; i < 16; i++ {
		if n, ok := customerSearchQ(w.pickCustomer()); ok {
			return n
		}
	}
	for _, id := range w.customers {
		if n, ok := customerSearchQ(id); ok {
			return n
		}
	}
	return "0"
}

// pickDistinct draws n distinct candidates: the one distinct-sampling
// policy the generator's planted fan-out and the workload's scatter ops
// share. The draw is a rejection loop rather than rng.Perm because the
// seeded replay contract pins the RNG stream: a permutation would consume
// different draws and change every report for an identical seed. Both
// callers feed duplicate-free pools (account and customer ids are unique by
// construction), so the loop terminates; that precondition is stated rather
// than bounded by a magic attempt count.
func pickDistinct[E comparable](rng *rand.Rand, candidates []E, n int) []E {
	if n > len(candidates) {
		panic(fmt.Sprintf("pickDistinct: cannot draw %d distinct values from %d candidates", n, len(candidates)))
	}
	out := make([]E, 0, n)
	for len(out) < n {
		candidate := candidates[rng.Intn(len(candidates))]
		if !slices.Contains(out, candidate) {
			out = append(out, candidate)
		}
	}
	return out
}

func (w *fraudWork) pickDistinct(n int) []string {
	return pickDistinct(w.rng, w.ordinary, n)
}

func (w *fraudWork) amount() float64 {
	return math.Round((5.0+w.rng.Float64()*(2500.0-5.0))*100) / 100
}

func (w *fraudWork) ts() int64 {
	return 1_730_000_000 + int64(w.next)
}

func logUniform(rng *rand.Rand, min, max float64) int {
	v := math.Exp(rng.Float64()*(math.Log(max)-math.Log(min)) + math.Log(min))
	n := int(math.Round(v))
	if float64(n) < min {
		n = int(min)
	}
	if float64(n) > max {
		n = int(max)
	}
	return n
}
