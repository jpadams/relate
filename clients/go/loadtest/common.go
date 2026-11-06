// Package loadtest ports the Rust fraud and identity load-test mix over the C ABI.
package loadtest

import (
	"errors"
	"fmt"
	"math"
	"os"
	"strings"
	"time"

	"github.com/relatedb/relate/clients/go"
)

const (
	batch = 200

	// minScale is the smallest graph scale the generators accept: the Config
	// boundary normalizes into this domain (normalizeFraud/normalizeIdentity),
	// so the generators themselves carry no clamps.
	minScale = 0.05

	// writeConflictAttempts is the retry budget for an authored write-write
	// conflict. The conflict means the other writer is still committing, so
	// each retry backs off (capped) instead of spending the whole budget in
	// microseconds on a connection pool where the winner has not finished.
	writeConflictAttempts = 8
	writeConflictBackoff  = 100 * time.Microsecond
	writeConflictMaxWait  = 10 * time.Millisecond
)

type Config struct {
	URI                   string
	Scale                 float64
	GraphSeed             int64
	WorkloadSeed          int64
	FanoutK               int
	CycleLen              int
	NestDepth             int
	GroupK                int
	Ops                   int
	Warmup                int
	Out                   string
	WalDir                string
	Reset                 bool
	Workers               int
	WriteFraction         float64
	LogicalGraphSizeLimit string
}

func DefaultFraud() Config {
	return Config{
		URI: ":memory:", Scale: 0.05, GraphSeed: 42, WorkloadSeed: 7,
		FanoutK: 12, CycleLen: 4, Ops: 80, Warmup: 8,
		Out: "target/fraud-loadtest.json", Workers: 1, WriteFraction: 0.5,
	}
}

func DefaultIdentity() Config {
	return Config{
		URI: ":memory:", Scale: 0.05, GraphSeed: 42, WorkloadSeed: 7,
		NestDepth: 4, GroupK: 12, Ops: 80, Warmup: 8,
		Out: "target/identity-loadtest.json", Workers: 1, WriteFraction: 0.5,
	}
}

type catalogEntry struct {
	ID          string
	Kind        OpKind
	Related     bool
	Description string
	Weight      uint32
}

// A weighted catalog row: the metadata view the pick helpers, summarize,
// and the shared runner consume. The fused spec rows of every workload
// (fraudOpSpec / identityOpSpec: a catalogEntry plus its planner, LABS-429)
// implement it, so one set of helpers serves every catalog shape.
type catalogRow interface {
	entry() catalogEntry
}

func (c catalogEntry) entry() catalogEntry { return c }

func resetURI(uri, walDir string) error {
	if walDir != "" {
		if err := os.RemoveAll(walDir); err != nil {
			return err
		}
	}
	if uri == "" || uri == ":memory:" {
		return nil
	}
	return os.RemoveAll(strings.TrimPrefix(uri, "file://"))
}

func writeQuery(conn *relate.Connection, cypher string, params map[string]any) (*relate.Result, error) {
	var last error
	for attempt := 0; attempt < writeConflictAttempts; attempt++ {
		r, err := conn.QueryParams(cypher, params)
		if err == nil {
			return r, nil
		}
		// Only an engine error whose typed Retryable flag is set is retried;
		// every other error is final.
		if !isRetryableConflict(err) {
			return nil, err
		}
		last = err
		if attempt+1 < writeConflictAttempts {
			time.Sleep(min(writeConflictBackoff<<attempt, writeConflictMaxWait))
		}
	}
	return nil, last
}

// isRetryableConflict reports whether err is a *relate.Error the engine
// marked retryable (ADR-0502 §3: the write-write conflict is
// Neo.TransientError.Transaction.Outdated with retryable true). Any other
// error, including one that repeats the conflict's message text, is final.
func isRetryableConflict(err error) bool {
	var e *relate.Error
	return errors.As(err, &e) && e.Retryable
}

// unwind batches rows through one UNWIND statement. Row is whatever the
// caller holds — []map[string]any, or a workload's json-tagged row struct,
// which the client's parameter builder binds field-for-field under the json
// names.
func unwind[Row any](conn *relate.Connection, rows []Row, cypher string) error {
	for i := 0; i < len(rows); i += batch {
		if _, err := writeQuery(conn, cypher, map[string]any{"rows": rows[i:min(i+batch, len(rows))]}); err != nil {
			return err
		}
	}
	return nil
}

// edgeRow is the one typed shape every [2]string relation set loads
// through: From/To name the roles once, and the Cypher reads r.from/r.to —
// no second spelling of the pair's letters to drift from the query.
type edgeRow struct {
	From string `json:"from"`
	To   string `json:"to"`
}

// edgeRows converts a relation set's positional pairs to the typed edge
// rows unwind serializes.
func edgeRows(pairs [][2]string) []edgeRow {
	rows := make([]edgeRow, len(pairs))
	for i, p := range pairs {
		rows[i] = edgeRow{From: p[0], To: p[1]}
	}
	return rows
}

// loadStep is one step of a workload load: a closure over its own row set
// and statement. runSteps executes a loader's step list in order and
// returns the first failure, so a loader states its steps as data instead
// of a chain of identical branches.
type loadStep func() error

// unwindStep curries one load step over its row set and statement, so a
// loader's step list is one line of data per step, not a copy of the same
// closure per step.
func unwindStep[Row any](conn *relate.Connection, rows []Row, cypher string) loadStep {
	return func() error { return unwind(conn, rows, cypher) }
}

func runSteps(steps ...loadStep) error {
	for _, step := range steps {
		if err := step(); err != nil {
			return err
		}
	}
	return nil
}

// withConn is the shared connect → run → Close → error-check sequence the
// mixed runner drives its load and snapshot phases through (and every
// runWorkers goroutine its op loop): Close is deferred, so the connection
// is released on every path — error and panic alike — and the fn error is
// still checked after it.
func withConn[R any](db *relate.Database, fn func(*relate.Connection) (R, error)) (R, error) {
	conn, err := db.Connect()
	if err != nil {
		var zero R
		return zero, err
	}
	defer conn.Close()
	return fn(conn)
}

func validateWriteFraction(writeFraction float64) error {
	if writeFraction < 0 || writeFraction > 1 || math.IsNaN(writeFraction) {
		return fmt.Errorf("--write-fraction must be between 0.0 and 1.0 (got %v)", writeFraction)
	}
	return nil
}

// validateWorkers rejects a non-positive worker count: `--workers 0` is a
// nonsense input, not one to silently coerce to 1 (the former behavior), so
// it errors like validateWriteFraction — before any side effect, before the
// database is even opened.
func validateWorkers(workers int) error {
	if workers < 1 {
		return fmt.Errorf("--workers must be at least 1 (got %d)", workers)
	}
	return nil
}

// validateCounts rejects a negative op or warmup count: runWorkers casts
// warmup+ops to uint64, so a negative count would wrap to ~2^64 and spin
// the worker loop essentially forever — the same nonsense-input class as
// validateWorkers, failed loudly before any side effect. Zero is legal on
// both (warmup 0 is a normal config; ops 0 runs the load and nothing else).
func validateCounts(ops, warmup int) error {
	if warmup < 0 || ops < 0 {
		return fmt.Errorf("--ops and --warmup must be at least 0 (got ops %d, warmup %d)", ops, warmup)
	}
	return nil
}

// entitiesPerScale is the size model's factor: a scale of 1.0 generates
// 10,000 graph entities (accounts or users). graphSize is its one use in
// code; the CLI help renders the same number through groupThousands.
const entitiesPerScale = 10_000

// maxScaleEntities bounds the normalized scale so graphSize's float→int
// conversion is always representable: the generators index and loop over
// that count, so an absurd scale is rejected at the boundary instead of
// hitting an implementation-defined conversion.
const maxScaleEntities = 10_000_000

// graphSize is the graph's entity count at a normalized scale (10,000 at
// scale 1.0): the one statement of the size model both generators and the
// fraud fan-out clamp consult.
func graphSize(scale float64) int {
	return int(math.Round(float64(entitiesPerScale) * scale))
}

// normalizeScale is the shared scale policy at the Config boundary: a
// non-finite scale is rejected, an entity count above maxScaleEntities is
// rejected, and any remaining scale is floored at minScale.
func normalizeScale(cfg *Config) error {
	if math.IsNaN(cfg.Scale) || math.IsInf(cfg.Scale, 0) {
		return fmt.Errorf("--scale must be finite (got %v)", cfg.Scale)
	}
	if maxScale := float64(maxScaleEntities) / entitiesPerScale; cfg.Scale > maxScale {
		return fmt.Errorf("--scale must generate at most %d graph entities (got %v, max scale %v)", maxScaleEntities, cfg.Scale, maxScale)
	}
	cfg.Scale = max(cfg.Scale, minScale)
	return nil
}

func clock() (string, int64) {
	t := time.Now().UTC()
	return t.Format("2006-01-02T15:04:05.000Z"), t.UnixMilli()
}

// scalarInt reads one count() aggregate cell. The walker decodes an INT cell
// to int64; any other shape is a contract violation worth failing loudly,
// not a fabricated zero.
func scalarInt(conn *relate.Connection, cypher string) (int64, error) {
	r, err := conn.Query(cypher)
	if err != nil {
		return 0, err
	}
	if len(r.Rows) == 0 || len(r.Rows[0]) == 0 || r.Rows[0][0] == nil {
		return 0, fmt.Errorf("scalarInt: expected one non-null cell from %q", cypher)
	}
	v, ok := r.Rows[0][0].(int64)
	if !ok {
		return 0, fmt.Errorf("scalarInt: expected a numeric cell, got %T from %q", r.Rows[0][0], cypher)
	}
	return v, nil
}

// logicalGraphSizeLimitEnv is the engine's live graph+index cap, read at open.
const logicalGraphSizeLimitEnv = "RELATE_LOGICAL_GRAPH_SIZE_LIMIT"

// applyLogicalGraphSizeLimit sets the engine's logical-graph-size-limit env
// var for the duration of one run and returns the restore that puts the prior
// value back, so no state outlives the run (a later in-process run with an
// empty cap must not inherit an earlier one). A failed Setenv propagates —
// the run must not silently proceed uncapped.
func applyLogicalGraphSizeLimit(size string) (restore func(), err error) {
	if size == "" {
		return func() {}, nil
	}
	prior, had := os.LookupEnv(logicalGraphSizeLimitEnv)
	if err := os.Setenv(logicalGraphSizeLimitEnv, size); err != nil {
		return nil, err
	}
	return func() {
		// The key is a fixed valid name, so restore cannot fail; the errors
		// are structural only.
		if had {
			_ = os.Setenv(logicalGraphSizeLimitEnv, prior)
		} else {
			_ = os.Unsetenv(logicalGraphSizeLimitEnv)
		}
	}, nil
}
