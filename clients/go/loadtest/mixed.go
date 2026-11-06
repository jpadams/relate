package loadtest

import (
	"cmp"
	"fmt"
	"math/rand"
	"slices"
	"sync"
	"sync/atomic"
	"time"

	"github.com/relatedb/relate/clients/go"
)

// The shared mixed-loadtest machinery (the Go mirror of the JS
// runMixedLoadtest, common.js): the workload-state view, the op and
// catalog-row contracts, the spec each workload supplies, the runner and
// its worker pool. The report vocabulary and its renderers live in
// report.go. Fraud and identity differ only in their spec and their
// catalog; every sequence — validation, load, snapshot, the pick → plan →
// exec → record loop, the report — is stated once, across these two files.

// mixedWork is the workload-state view the shared runner drives: the RNG
// the mix draws from, held by the workload (seeded once by its newWork
// constructor, so identical seeds replay identical picks). The write
// fraction stays with the runner — it owns cfg, validated at entry — so the
// workload carries no copy of it (the JS mirror's shape: common.js keeps
// writeFraction in the runner and reads only work.rng).
type mixedWork interface {
	mixRng() *rand.Rand
}

// workloadState is the shared half of every mixed workload: the mix RNG
// (seeded from cfg.WorkloadSeed) and the shared id-allocator counter, with
// the one mixRng method satisfying mixedWork and the one alloc helper both
// workloads' planners draw through. fraudWork and identityWork embed it;
// their own fields stay workload-specific.
type workloadState struct {
	rng  *rand.Rand
	next uint64
}

func newWorkloadState(cfg Config) *workloadState {
	return &workloadState{rng: rand.New(rand.NewSource(cfg.WorkloadSeed)), next: 1}
}

func (w *workloadState) mixRng() *rand.Rand { return w.rng }

// alloc mints a fresh workload-scoped id (`lt-tx-3`, `lt-user-3`,
// `alias-3`, …): the prefix names the op, the counter keeps it unique.
func (w *workloadState) alloc(prefix string) string {
	id := fmt.Sprintf("%s-%d", prefix, w.next)
	w.next++
	return id
}

// execOp is the op side of a fused catalog row: every op runs its own exec
// step and reports per the shared op-exec policy (ops.go) — its statement
// count, its row count, and an op-level error.
type execOp interface {
	exec(conn *relate.Connection) (stmts uint32, rows int, err error)
	// grow applies the op's pool growth, run once under the worker-state
	// lock after a successful exec. Every op embeds baseOp for the no-op
	// default, so the runner calls it unconditionally — a growing op
	// overrides it rather than being discovered by a type assertion.
	grow()
}

// baseOp is the no-op growth every op embeds: an op that grows the
// workload's pools overrides grow().
type baseOp struct{}

func (baseOp) grow() {}

// opSpecRow is the catalog-row side the shared runner dispatches through:
// the row's catalogEntry metadata plus its planner returning the shared op
// contract (fraudOpSpec / identityOpSpec).
type opSpecRow[Work any] interface {
	catalogRow
	planOp(work Work) execOp
}

// mixedSpec is the workload half of a mixed loadtest: the pieces the shared
// orchestration varies per workload, mirroring the spec object the JS
// clients pass to runMixedLoadtest (common.js). Fraud and identity differ
// only in these pieces and their fused catalog. Cfg and Plant are the
// report's typed workload halves, and Snap the dataset snapshot type.
type mixedSpec[Graph, Snap, Cfg, Plant any, Work mixedWork, Spec opSpecRow[Work]] struct {
	name         string
	generate     func(Config) Graph
	load         func(*relate.Connection, Graph) error
	snapshot     func(*relate.Connection) (Snap, error)
	newWork      func(Graph, Config) Work
	normalize    func(*Config) error
	planted      func(Graph) Plant
	reportConfig func(Config) Cfg
	catalog      []Spec
}

// mixPools is the read/write split of a fused catalog, computed once per
// run: every op draws its kind pool from here instead of rescanning the
// catalog. A catalog with ops of only one kind is rejected: every consumer
// of a mixed catalog — the mix pick, summarize, throughputFrom — assumes
// both kinds are present.
type mixPools[E catalogRow] struct {
	reads, writes []E
}

func splitCatalog[E catalogRow](catalog []E) (mixPools[E], error) {
	var p mixPools[E]
	for _, c := range catalog {
		e := c.entry()
		switch e.Kind {
		case KindWrite:
			p.writes = append(p.writes, c)
		case KindRead:
			p.reads = append(p.reads, c)
		default:
			return mixPools[E]{}, fmt.Errorf("catalog row %q has unknown kind %q", e.ID, e.Kind)
		}
	}
	if len(p.reads) == 0 || len(p.writes) == 0 {
		return mixPools[E]{}, fmt.Errorf("mixed catalog must contain both read and write ops")
	}
	return p, nil
}

// pickWeighted draws the catalog row at offset x of the catalog's weight
// accumulation (the second half of a mixed pick; mixPools.pick owns the
// first). Precondition: x < the catalog's total weight — the sole caller
// draws x = rng.Intn(catalogTotal(pool)), which panics on a zero total
// before this is reached; a violation here is a caller bug, named loudly.
func pickWeighted[E catalogRow](x uint32, catalog []E) E {
	var acc uint32
	for _, c := range catalog {
		acc += c.entry().Weight
		if x < acc {
			return c
		}
	}
	panic(fmt.Sprintf("pickWeighted: x=%d reached the catalog's total weight %d", x, acc))
}

func catalogTotal[E catalogRow](catalog []E) uint32 {
	var total uint32
	for _, c := range catalog {
		total += c.entry().Weight
	}
	return total
}

// pick draws one catalog row: the kind first (one Float64 draw), then the
// weighted row within that kind's pool (one Intn draw) — the same two
// draws, in the same order, the former per-op PickByWriteFraction made.
func (p mixPools[E]) pick(rng *rand.Rand, writeFraction float64) E {
	pool := p.reads
	if rng.Float64() < writeFraction {
		pool = p.writes
	}
	return pickWeighted(uint32(rng.Intn(int(catalogTotal(pool)))), pool)
}

// guardedWork owns the worker-shared state of one mixed run — the workload
// state (RNG and id allocator), the catalog's pools and the write fraction
// the pick consults — together with the mutex that guards it. Workers draw
// through its pick methods and an op's grow() is applied through its grow
// method under the same mutex, so no caller sees the mutex or threads its
// pieces separately.
type guardedWork[Work mixedWork, Spec opSpecRow[Work]] struct {
	mu            sync.Mutex
	work          Work
	pools         mixPools[Spec]
	writeFraction float64
}

// pickPlan draws one catalog row and plans its op in one guarded step: all
// RNG draws happen under the mutex, in the same order the former
// per-workload exec functions made them.
func (g *guardedWork[Work, Spec]) pickPlan() (Spec, execOp) {
	g.mu.Lock()
	defer g.mu.Unlock()
	uc := g.pools.pick(g.work.mixRng(), g.writeFraction)
	return uc, uc.planOp(g.work)
}

// grow applies a growing op's pool growth under the same mutex that guards
// the pools it mutates.
func (g *guardedWork[Work, Spec]) grow(op execOp) {
	g.mu.Lock()
	defer g.mu.Unlock()
	op.grow()
}

// runMixedLoadtest is the shared mixed-loadtest orchestration both Go
// workloads drive: validation, setup, the load phase, the timed worker run
// with its record builder, and the report envelope. Each workload supplies
// its spec — the generate/load/snapshot/newWork/normalize pieces and its
// fused catalog — and differs in nothing else.
func runMixedLoadtest[Graph, Snap, Cfg, Plant any, Work mixedWork, Spec opSpecRow[Work]](cfg Config, spec mixedSpec[Graph, Snap, Cfg, Plant, Work, Spec]) (*Report[Snap, Cfg, Plant], error) {
	if err := validateWriteFraction(cfg.WriteFraction); err != nil {
		return nil, err
	}
	if err := validateWorkers(cfg.Workers); err != nil {
		return nil, err
	}
	if err := validateCounts(cfg.Ops, cfg.Warmup); err != nil {
		return nil, err
	}
	if err := spec.normalize(&cfg); err != nil {
		return nil, err
	}
	restoreLogicalGraphSizeLimit, err := applyLogicalGraphSizeLimit(cfg.LogicalGraphSizeLimit)
	if err != nil {
		return nil, err
	}
	defer restoreLogicalGraphSizeLimit()
	pools, err := splitCatalog(spec.catalog)
	if err != nil {
		return nil, err
	}
	startedAt, startedMs := clock()
	loadWall := time.Now()
	graph := spec.generate(cfg)
	if cfg.Reset {
		if err := resetURI(cfg.URI, cfg.WalDir); err != nil {
			return nil, err
		}
	}
	db, err := relate.Open(cfg.URI)
	if err != nil {
		return nil, err
	}
	defer db.Close()
	start, err := withConn(db, func(conn *relate.Connection) (Snap, error) {
		if err := spec.load(conn, graph); err != nil {
			var zero Snap
			return zero, err
		}
		return spec.snapshot(conn)
	})
	if err != nil {
		return nil, err
	}
	loadMs := time.Since(loadWall).Milliseconds()

	state := &guardedWork[Work, Spec]{
		work: spec.newWork(graph, cfg), pools: pools, writeFraction: cfg.WriteFraction,
	}
	workerWall := time.Now()
	records, err := runWorkers(db, cfg.Workers, cfg.Warmup, cfg.Ops, func(conn *relate.Connection, seq uint64, phase OpPhase, worker uint32) OpRecord {
		return state.runOp(conn, seq, phase, worker)
	})
	if err != nil {
		return nil, err
	}
	// One derivation of the worker phase's elapsed time, used for both the
	// reported duration and the throughput denominator.
	workerMs := time.Since(workerWall)
	finish, err := withConn(db, spec.snapshot)
	if err != nil {
		return nil, err
	}
	finishedAt, finishedMs := clock()
	report := &Report[Snap, Cfg, Plant]{
		SchemaVersion: reportSchemaVersion, Name: spec.name,
		StartedAt: startedAt, FinishedAt: finishedAt,
		StartedAtUnixMs: startedMs, FinishedAtUnixMs: finishedMs,
		DurationMs: finishedMs - startedMs, LoadDurationMs: loadMs, WorkerDurationMs: workerMs.Milliseconds(),
		Engine: "relate", EngineVersion: engineVersion,
		Config: spec.reportConfig(cfg), Planted: spec.planted(graph),
		Dataset:    Dataset[Snap]{Start: start, Finish: finish},
		Memory:     MemoryReport{},
		Throughput: throughputFrom(records, workerMs.Seconds()),
		Usecases:   summarize(records, spec.catalog), Ops: records,
	}
	if cfg.Out != "" {
		if err := writeReport(cfg.Out, report); err != nil {
			return nil, err
		}
	}
	return report, nil
}

// runOp builds one mixed-op record: the guarded state plans the fused
// catalog row's own op (RNG draws under its mutex), the op runs its exec
// step outside it, and a successful op's pool growth is applied back under
// the mutex. An op-level failure is data, not an error: the
// record carries the error text and OK=false, counted by summarize.
func (g *guardedWork[Work, Spec]) runOp(conn *relate.Connection, seq uint64, phase OpPhase, worker uint32) OpRecord {
	uc, op := g.pickPlan()
	t0 := time.Now()
	stmts, rows, execErr := op.exec(conn)
	lat := uint64(time.Since(t0).Nanoseconds())
	e := uc.entry()
	rec := OpRecord{
		Seq: seq, Phase: phase, Usecase: e.ID, Kind: e.Kind, Related: e.Related,
		LatencyNs: lat, Worker: worker,
	}
	// The failure half of the ops.go reporting policy, enforced once
	// here (its named enforcer): a failed op reports no work — the
	// zero-valued Statements/Rows/OK, like the JS twin's catch.
	if execErr != nil {
		rec.Error = execErr.Error()
		return rec
	}
	g.grow(op)
	rec.Statements = stmts
	rec.Rows = rows
	rec.OK = true
	return rec
}

// runWorkers runs the op mix on `workers` goroutines, one connection each.
// Precondition: workers >= 1 and warmup, ops >= 0 — runMixedLoadtest's front
// door validates all three (validateWorkers / validateCounts) before any
// side effect; a direct caller must validate the same way. The exec callback
// builds one OpRecord per op: op-level failures are data (a record with
// OK=false and its error), so the only fatal error left is a failed worker
// connect — errCh's sole producer. Every worker goroutine runs its whole op
// loop inside withConn, the one statement of the connect → run → Close →
// error-check sequence.
func runWorkers(db *relate.Database, workers, warmup, ops int, exec func(conn *relate.Connection, seq uint64, phase OpPhase, worker uint32) OpRecord) ([]OpRecord, error) {
	total := uint64(warmup + ops)
	var next atomic.Uint64
	recs := make([][]OpRecord, workers)
	errCh := make(chan error, workers)
	var wg sync.WaitGroup
	for w := 0; w < workers; w++ {
		wg.Add(1)
		wid := uint32(w)
		go func() {
			defer wg.Done()
			batch, err := withConn(db, func(conn *relate.Connection) ([]OpRecord, error) {
				var batch []OpRecord
				for {
					seq := next.Add(1)
					if seq > total {
						return batch, nil
					}
					phase := PhaseMeasured
					if seq <= uint64(warmup) {
						phase = PhaseWarmup
					}
					batch = append(batch, exec(conn, seq, phase, wid))
				}
			})
			if err != nil {
				errCh <- err
				return
			}
			recs[wid] = batch
		}()
	}
	wg.Wait()
	select {
	case err := <-errCh:
		return nil, err
	default:
	}
	var all []OpRecord
	for _, b := range recs {
		all = append(all, b...)
	}
	slices.SortFunc(all, func(a, b OpRecord) int { return cmp.Compare(a.Seq, b.Seq) })
	return all, nil
}
