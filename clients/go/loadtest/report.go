package loadtest

// The mixed-loadtest report: the wire envelope and its row/summary
// vocabulary, the aggregation over recorded ops, and the one renderer and
// JSON writer. The runner (mixed.go) produces these values; nothing in this
// file knows how a workload executes.

import (
	"encoding/json"
	"fmt"
	"io"
	"math"
	"os"
	"path/filepath"
	"slices"
)

const (
	reportSchemaVersion = 5
	engineVersion       = "0.1.0"
)

// The report's phase and kind vocabularies — the wire words on
// OpRecord.Phase, OpRecord.Kind, and UsecaseSummary.Kind — stated once as
// named types with their values: every producer and consumer (runWorkers,
// summarize, throughputFrom, mixPools.pick) consults these constants, not
// re-spelled literals, and a rogue value fails the type system or a
// classification arm instead of comparing equal to nothing.
type OpPhase string

type OpKind string

const (
	PhaseWarmup   OpPhase = "warmup"
	PhaseMeasured OpPhase = "measured"
	KindRead      OpKind  = "read"
	KindWrite     OpKind  = "write"
)

type OpRecord struct {
	Seq        uint64  `json:"seq"`
	Phase      OpPhase `json:"phase"`
	Usecase    string  `json:"usecase"`
	Kind       OpKind  `json:"kind"`
	Related    bool    `json:"related,omitempty"`
	Statements uint32  `json:"statements"`
	OK         bool    `json:"ok"`
	Error      string  `json:"error,omitempty"`
	LatencyNs  uint64  `json:"latency_ns"`
	Rows       int     `json:"rows"`
	Worker     uint32  `json:"worker"`
}

// LatencyPercentiles is the latency summary of one usecase's measured ops
// (named for what it holds — OpRecord.LatencyNs is the per-op duration; this
// type is the distribution over those durations; both serialize under the
// `latency_ns` wire key).
type LatencyPercentiles struct {
	Min  uint64  `json:"min"`
	Max  uint64  `json:"max"`
	Mean float64 `json:"mean"`
	P50  uint64  `json:"p50"`
	P95  uint64  `json:"p95"`
	P99  uint64  `json:"p99"`
}

type UsecaseSummary struct {
	ID          string              `json:"id"`
	Kind        OpKind              `json:"kind"`
	Related     bool                `json:"related,omitempty"`
	Description string              `json:"description"`
	Ops         int                 `json:"ops"`
	Errors      int                 `json:"errors"`
	Statements  int                 `json:"statements"`
	LatencyNs   *LatencyPercentiles `json:"latency_ns"`
}

type Throughput struct {
	OpsPerSec    float64 `json:"ops_per_sec"`
	ReadsPerSec  float64 `json:"reads_per_sec"`
	WritesPerSec float64 `json:"writes_per_sec"`
	ReadOps      int     `json:"read_ops"`
	WriteOps     int     `json:"write_ops"`
}

type MemoryPoint struct {
	RSSBytes           uint64  `json:"rss_bytes"`
	PeakRSSBytes       uint64  `json:"peak_rss_bytes"`
	RSSPerNode         float64 `json:"rss_per_node"`
	RSSPerRelationship float64 `json:"rss_per_relationship"`
	RSSPerEntity       float64 `json:"rss_per_entity"`
	RSSPerAccount      float64 `json:"rss_per_account,omitempty"`
	RSSPerUser         float64 `json:"rss_per_user,omitempty"`
}

type GraphMemory struct {
	RSSBytes             uint64  `json:"rss_bytes"`
	BytesPerNode         float64 `json:"bytes_per_node"`
	BytesPerRelationship float64 `json:"bytes_per_relationship"`
	BytesPerEntity       float64 `json:"bytes_per_entity"`
	BytesPerAccount      float64 `json:"bytes_per_account,omitempty"`
	BytesPerUser         float64 `json:"bytes_per_user,omitempty"`
}

type MemoryWorkloadDelta struct {
	RSSBytes             int64   `json:"rss_bytes"`
	Nodes                int64   `json:"nodes"`
	Relationships        int64   `json:"relationships"`
	RSSBytesPerNewEntity float64 `json:"rss_bytes_per_new_entity"`
}

type MemoryReport struct {
	AfterOpen             MemoryPoint         `json:"after_open"`
	Start                 MemoryPoint         `json:"start"`
	Finish                MemoryPoint         `json:"finish"`
	Graph                 GraphMemory         `json:"graph"`
	WorkloadDelta         MemoryWorkloadDelta `json:"workload_delta"`
	LogicalUsedStartBytes uint64              `json:"logical_used_start_bytes"`
	LogicalUsedBytes      uint64              `json:"logical_used_bytes"`
	LogicalLimitBytes     *uint64             `json:"logical_limit_bytes"`
}

// isMeasured classifies an op into the report's measured window, failing
// loudly on a phase outside the two-value vocabulary — the same policy the
// Kind arms apply, so neither half of the vocabulary is silently bucketed.
func isMeasured(o *OpRecord) bool {
	switch o.Phase {
	case PhaseWarmup:
		return false
	case PhaseMeasured:
		return true
	default:
		panic(fmt.Sprintf("op has a rogue phase %q", o.Phase))
	}
}

func summarize[E catalogRow](ops []OpRecord, catalog []E) []UsecaseSummary {
	var out []UsecaseSummary
	for _, c := range catalog {
		e := c.entry()
		n, errs, stmts := 0, 0, 0
		var lats []uint64
		for i := range ops {
			o := &ops[i]
			if !isMeasured(o) || o.Usecase != e.ID {
				continue
			}
			n++
			stmts += int(o.Statements)
			if !o.OK {
				errs++
			} else {
				lats = append(lats, o.LatencyNs)
			}
		}
		if n == 0 {
			continue
		}
		out = append(out, UsecaseSummary{
			ID: e.ID, Kind: e.Kind, Related: e.Related, Description: e.Description,
			Ops: n, Errors: errs, Statements: stmts, LatencyNs: percentiles(lats),
		})
	}
	return out
}

func percentiles(xs []uint64) *LatencyPercentiles {
	if len(xs) == 0 {
		return nil
	}
	v := slices.Clone(xs)
	slices.Sort(v)
	var sum float64
	for _, x := range v {
		sum += float64(x)
	}
	// p is always one of 0.50/0.95/0.99 and len(v) >= 1 here, so the index
	// is in range by construction — no clamp.
	pct := func(p float64) uint64 {
		return v[int(math.Round(float64(len(v)-1)*p))]
	}
	return &LatencyPercentiles{Min: v[0], Max: v[len(v)-1], Mean: sum / float64(len(v)), P50: pct(0.50), P95: pct(0.95), P99: pct(0.99)}
}

// throughputFrom divides the measured ops by the worker phase's elapsed
// seconds — a measured denominator, not one invented for a sub-millisecond
// phase (runMixedLoadtest passes the same workerMs it reports).
func throughputFrom(ops []OpRecord, secs float64) Throughput {
	var r, w int
	for i := range ops {
		if !isMeasured(&ops[i]) {
			continue
		}
		switch ops[i].Kind {
		case KindRead:
			r++
		case KindWrite:
			w++
		default:
			panic(fmt.Sprintf("throughputFrom: op has a rogue kind %q", ops[i].Kind))
		}
	}
	n := r + w
	return Throughput{
		OpsPerSec: float64(n) / secs, ReadsPerSec: float64(r) / secs, WritesPerSec: float64(w) / secs,
		ReadOps: r, WriteOps: w,
	}
}

// Dataset is the typed two-point dataset record the JS mirror writes as
// `dataset: { start, finish }`: one compile-checked shape instead of a
// stringly-keyed map, with the same wire keys.
type Dataset[Snap any] struct {
	Start  Snap `json:"start"`
	Finish Snap `json:"finish"`
}

// Report is the mixed-loadtest report envelope, typed over the workload's
// snapshot, config, and planted values: every consumer reads the concrete
// halves directly (no `any` assertions), and the wire shape is unchanged.
type Report[Snap, Cfg, Plant any] struct {
	SchemaVersion    uint32        `json:"schema_version"`
	Name             string        `json:"name"`
	StartedAt        string        `json:"started_at"`
	FinishedAt       string        `json:"finished_at"`
	StartedAtUnixMs  int64         `json:"started_at_unix_ms"`
	FinishedAtUnixMs int64         `json:"finished_at_unix_ms"`
	DurationMs       int64         `json:"duration_ms"`
	LoadDurationMs   int64         `json:"load_duration_ms"`
	WorkerDurationMs int64         `json:"worker_duration_ms"`
	Engine           string        `json:"engine"`
	EngineVersion    string        `json:"engine_version"`
	Config           Cfg           `json:"config"`
	Planted          Plant         `json:"planted"`
	Dataset          Dataset[Snap] `json:"dataset"`
	Throughput       Throughput    `json:"throughput"`
	// Memory is the unmeasured memory block: this reporter does not sample
	// memory, so it serializes the JS twin's emptyMemory() shape — every
	// counter zero and logical_limit_bytes null (these fields carry no
	// omitempty, so the shape does not depend on the zero value's tags).
	Memory   MemoryReport     `json:"memory"`
	Usecases []UsecaseSummary `json:"usecases"`
	Ops      []OpRecord       `json:"ops"`
}

func writeReport[Snap, Cfg, Plant any](path string, report *Report[Snap, Cfg, Plant]) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	raw, err := json.MarshalIndent(report, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, raw, 0o644)
}

// usecaseColumn is the one usecase-id column width both report printers use.
const usecaseColumn = 22

// printReport is the one report printer both workloads share: the header,
// throughput block, and usecase loop are identical; the workload supplies
// only its dataset line.
func printReport[Snap, Cfg, Plant any](w io.Writer, r *Report[Snap, Cfg, Plant], ops, workers int, out, datasetLine string) {
	// Empty --out means the runner wrote nothing; the printer states the
	// same policy rather than announcing a write that never happened.
	if out == "" {
		fmt.Fprintln(w, "no report written (--out is empty)")
	} else {
		fmt.Fprintf(w, "wrote %s (%d measured ops, %d workers, %d usecases)\n", out, ops, workers, len(r.Usecases))
	}
	fmt.Fprintf(w, "load %d ms  workers %d ms  total %d ms\n", r.LoadDurationMs, r.WorkerDurationMs, r.DurationMs)
	fmt.Fprintf(w, "throughput %.1f ops/s  writes %.1f/s (%d ops)  reads %.1f/s (%d ops)\n",
		r.Throughput.OpsPerSec, r.Throughput.WritesPerSec, r.Throughput.WriteOps,
		r.Throughput.ReadsPerSec, r.Throughput.ReadOps)
	fmt.Fprint(w, datasetLine)
	for _, u := range r.Usecases {
		// Every measured op of this usecase failing leaves no latency
		// distribution — but the error count is exactly what the operator
		// needs to see, so the row is not dropped.
		latency := "p50=- p95=-"
		if u.LatencyNs != nil {
			latency = fmt.Sprintf("p50=%.3fms p95=%.3fms", float64(u.LatencyNs.P50)/1e6, float64(u.LatencyNs.P95)/1e6)
		}
		fmt.Fprintf(w, "  %-*s %4d ops  %s errors=%d\n", usecaseColumn, u.ID, u.Ops, latency, u.Errors)
	}
}
