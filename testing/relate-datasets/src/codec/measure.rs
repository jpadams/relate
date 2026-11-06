//! Hop-stream extraction and measurement: rebuild every `TypedAdj` chunk of
//! a graph, encode and timed-decode it under both schemes, and compare
//! against the recorded benchmark-harness baseline.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use super::{cmp_ts_desc, decode_chunk, decode_chunk_simd, encode_chunk, Hop, Scheme};

/// Per-scheme codec measurement for one dataset.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SchemeReport {
    /// Total encoded bytes over every chunk.
    pub bytes: u64,
    /// raw_hop_bytes / encoded: the pure codec ratio.
    pub raw_ratio: f64,
    /// (baseline_topology_bytes - encoded) / baseline_topology_bytes × 100.
    pub percent_saved: Option<f64>,
    /// Warm best-of-N scalar decode throughput, hops/second.
    pub scalar_decode_hps: f64,
    /// Warm best-of-N SIMD decode throughput, hops/second (`None` when
    /// measured with `--simd false`).
    pub simd_decode_hps: Option<f64>,
    /// simd / scalar decode throughput ratio.
    pub simd_vs_scalar: Option<f64>,
    /// Whether SIMD closes (<0.9x), matches (0.9-1.1x), or beats (>1.1x) the
    /// scalar decode, plus the multiple over the live 1-hop baseline.
    /// `None` when SIMD was not measured.
    pub verdict: Option<String>,
}

/// Codec measurement for one dataset. The JSON form flattens the two
/// per-scheme reports into the historical `*_for128` / `*_hybrid` field
/// names (see the `Serialize` impl) so recorded reports stay comparable.
#[derive(Debug, Clone)]
pub struct CodecDatasetReport {
    pub dataset: String,
    pub chunks: u64,
    pub hops: u64,
    /// Uncompressed hop bytes: 16 B per hop (`Option<i64>` + `RelId`).
    pub raw_hop_bytes: u64,
    /// Recorded baseline topology logical bytes (None when the dataset is
    /// not in the baseline table).
    pub baseline_topology_bytes: Option<u64>,
    /// Recorded baseline 1-hop expansion throughput (hops/second).
    pub baseline_1hop_hps: Option<f64>,
    /// Per-scheme reports in [`Scheme::ALL`] order.
    schemes: [SchemeReport; Scheme::ALL.len()],
}

impl CodecDatasetReport {
    /// The report for one scheme.
    pub fn scheme(&self, scheme: Scheme) -> &SchemeReport {
        &self.schemes[scheme.index()]
    }
}

impl Serialize for CodecDatasetReport {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = serializer.serialize_map(Some(20))?;
        m.serialize_entry("dataset", &self.dataset)?;
        m.serialize_entry("chunks", &self.chunks)?;
        m.serialize_entry("hops", &self.hops)?;
        m.serialize_entry("raw_hop_bytes", &self.raw_hop_bytes)?;
        m.serialize_entry("baseline_topology_bytes", &self.baseline_topology_bytes)?;
        m.serialize_entry("baseline_1hop_hps", &self.baseline_1hop_hps)?;
        // Flat `*_for128` / `*_hybrid` field names, one loop per metric over
        // the two schemes; recorded reports and docs cite these names.
        for scheme in Scheme::ALL {
            m.serialize_entry(
                &format!("{}_bytes", scheme.as_str()),
                &self.scheme(scheme).bytes,
            )?;
        }
        for scheme in Scheme::ALL {
            m.serialize_entry(
                &format!("percent_saved_{}", scheme.as_str()),
                &self.scheme(scheme).percent_saved,
            )?;
        }
        for scheme in Scheme::ALL {
            m.serialize_entry(
                &format!("raw_ratio_{}", scheme.as_str()),
                &self.scheme(scheme).raw_ratio,
            )?;
        }
        for scheme in Scheme::ALL {
            m.serialize_entry(
                &format!("scalar_decode_hps_{}", scheme.as_str()),
                &self.scheme(scheme).scalar_decode_hps,
            )?;
        }
        for scheme in Scheme::ALL {
            m.serialize_entry(
                &format!("simd_decode_hps_{}", scheme.as_str()),
                &self.scheme(scheme).simd_decode_hps,
            )?;
        }
        for scheme in Scheme::ALL {
            m.serialize_entry(
                &format!("simd_vs_scalar_{}", scheme.as_str()),
                &self.scheme(scheme).simd_vs_scalar,
            )?;
        }
        for scheme in Scheme::ALL {
            m.serialize_entry(
                &format!("verdict_{}", scheme.as_str()),
                &self.scheme(scheme).verdict,
            )?;
        }
        m.end()
    }
}

/// One recorded baseline row (name, topology_logical_bytes, 1-hop hps),
/// from the benchmark-harness run recorded 2026-10-11. This is the default
/// lookup; `codec-prototype --baseline FILE` loads the same lookup from a
/// benchmark-harness `--out` report instead, which is the drift-free path.
pub const BASELINE: &[(&str, u64, f64)] = &[
    ("movies", 138_018, 716_963.0),
    ("northwind", 1_729_522, 1_462_640.0),
    ("cybersecurity", 1_992_022, 1_521_403.0),
    ("entity-resolution", 1_048_139, 1_034_915.0),
    ("stackoverflow", 6_002_137, 1_372_592.0),
    ("contact-tracing", 7_017_190, 1_390_013.0),
    ("fincen", 17_639_473, 1_316_456.0),
    ("healthcare-analytics", 24_975_233, 1_514_393.0),
    ("openstreetmap", 48_768_597, 1_041_420.0),
    ("pole", 56_099_181, 1_267_648.0),
    ("recommendations", 65_967_803, 1_443_096.0),
    ("network-management", 108_515_684, 1_168_029.0),
    ("legis-graph", 179_866_528, 1_530_540.0),
    ("shape-ring", 7_362_123, 1_096_536.0),
    ("shape-fanout", 7_256_958, 1_204_832.0),
    ("shape-dense-ring", 8_277_202, 1_797_883.0),
];

/// The `(topology_logical_bytes, 1-hop hps)` lookup per dataset name,
/// resolved once at startup: either the compiled-in [`BASELINE`] table or a
/// benchmark-harness `--out` report loaded with [`BaselineTable::load`].
pub struct BaselineTable {
    entries: HashMap<String, (u64, f64)>,
}

impl BaselineTable {
    /// The compiled-in table recorded 2026-10-11.
    pub fn built_in() -> Self {
        Self {
            entries: BASELINE
                .iter()
                .map(|(name, b, h)| (name.to_string(), (*b, *h)))
                .collect(),
        }
    }

    /// Load the lookup from a benchmark-harness `--out` report JSON.
    /// Datasets absent from the report keep the no-baseline behavior.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let report: crate::bench::BenchReport =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            entries: report
                .datasets
                .into_iter()
                .map(|d| {
                    (
                        d.dataset,
                        (d.topology_logical_bytes, d.hops_per_second_1hop),
                    )
                })
                .collect(),
        })
    }

    /// `(topology_logical_bytes, 1-hop hps)` recorded for `name`.
    pub fn get(&self, name: &str) -> Option<(u64, f64)> {
        self.entries.get(name).copied()
    }
}

/// One-line verdict: how SIMD decode compares to scalar and to the recorded
/// live-expansion baseline.
fn verdict(scheme: &str, simd: f64, scalar: f64, baseline: Option<f64>) -> String {
    let ratio = simd / scalar;
    let vs_scalar = if ratio > 1.10 {
        format!("beats scalar ({ratio:.2}x)")
    } else if ratio >= 0.90 {
        format!("matches scalar ({ratio:.2}x)")
    } else {
        format!("closes on scalar ({ratio:.2}x)")
    };
    let vs_baseline = match baseline {
        Some(b) if b > 0.0 => format!("beats the live 1-hop baseline {:.1}x", simd / b),
        _ => "no baseline recorded".to_string(),
    };
    format!("{scheme}: SIMD {vs_scalar} and {vs_baseline}")
}

/// Reconstruct every `TypedAdj` chunk's hop stream of the graph behind
/// `db`: one chunk per (node, rel type, direction), in engine chunk order.
///
/// The engine's `TypedAdj` is crate-private, so the streams are rebuilt
/// from one relationship scan (`id(r)`, endpoints, type, integer `r.ts`)
/// and sorted with the same comparator the store uses. Chunk contents and
/// order are byte-identical to the live store's; the per-(node,type)
/// grouping matches `out_adj` / `in_adj`.
pub fn extract_chunks(db: &relate::Database) -> Result<Vec<Vec<Hop>>, String> {
    let mut conn = db.connect().map_err(|e| e.to_string())?;
    let r = conn
        .query(
            "MATCH (a)-[r]->(b) RETURN id(r) AS rid, id(a) AS s, id(b) AS e, \
             type(r) AS t, r.ts AS ts",
        )
        .map_err(|e| format!("relationship scan: {e}"))?;
    let mut chunks: HashMap<(i64, String, bool), Vec<Hop>> = HashMap::new();
    for row in &r.rows {
        let [relate::Value::Int(rid), relate::Value::Int(s), relate::Value::Int(e), relate::Value::String(t), ts_value] =
            &row[..]
        else {
            return Err(format!(
                "relationship scan row has unexpected shape: {row:?}"
            ));
        };
        let ts = match ts_value {
            relate::Value::Int(v) => Some(*v),
            relate::Value::Null => None,
            other => return Err(format!("r.ts is not an integer or null: {other:?}")),
        };
        let hop = Hop { ts, id: *rid };
        chunks
            .entry((*s, t.to_string(), true))
            .or_default()
            .push(hop);
        chunks
            .entry((*e, t.to_string(), false))
            .or_default()
            .push(hop);
    }
    let mut chunks: Vec<Vec<Hop>> = chunks.into_values().collect();
    for chunk in &mut chunks {
        chunk.sort_unstable_by(cmp_ts_desc);
    }
    Ok(chunks)
}

/// Loop-invariant per-dataset context for the per-scheme measurement pass,
/// built once by [`measure_dataset`] and shared by both scheme passes.
struct MeasureJob<'a> {
    name: &'a str,
    chunks: &'a [Vec<Hop>],
    rounds: u32,
    simd: bool,
    /// Total hops over every chunk, folded once by the caller.
    hops: u64,
    /// Recorded baseline `(topology_logical_bytes, 1-hop hps)` for `name`.
    baseline: Option<(u64, f64)>,
}

/// Encode and timed-decode every chunk of one dataset under both schemes.
/// Roundtrip equality is asserted for every chunk in both schemes, through
/// the scalar decoder and (when `simd`) the SIMD decoder. `baseline` is the
/// `(bytes, hps)` lookup resolved once by the caller.
pub fn measure_dataset(
    name: &str,
    chunks: &[Vec<Hop>],
    rounds: u32,
    simd: bool,
    baseline: &BaselineTable,
) -> Result<CodecDatasetReport, String> {
    if rounds == 0 {
        return Err("--rounds must be at least 1".into());
    }
    let hops: u64 = chunks.iter().map(|c| c.len() as u64).sum();
    let job = MeasureJob {
        name,
        chunks,
        rounds,
        simd,
        hops,
        baseline: baseline.get(name),
    };
    let (baseline_bytes, baseline_hps) = job.baseline.unzip();
    Ok(CodecDatasetReport {
        dataset: name.to_string(),
        chunks: chunks.len() as u64,
        hops,
        raw_hop_bytes: hops * 16,
        baseline_topology_bytes: baseline_bytes,
        baseline_1hop_hps: baseline_hps,
        schemes: Scheme::ALL.map(|scheme| measure_scheme(scheme, &job)),
    })
}

/// One scheme pass over a dataset: encode every chunk, verify the
/// roundtrip(s), time the decode, and assemble the report row.
fn measure_scheme(scheme: Scheme, job: &MeasureJob) -> SchemeReport {
    let bufs = encode_and_verify(job.name, scheme, job.chunks, job.simd);
    let bytes: u64 = bufs.iter().map(|b| b.len() as u64).sum();
    let raw_hop_bytes = job.hops * 16;
    let best = time_decode(decode_chunk, scheme, &bufs, job.hops, job.rounds);
    let best_simd = job
        .simd
        .then(|| time_decode(decode_chunk_simd, scheme, &bufs, job.hops, job.rounds));
    let (baseline_bytes, baseline_hps) = job.baseline.unzip();
    SchemeReport {
        bytes,
        raw_ratio: raw_hop_bytes as f64 / bytes.max(1) as f64,
        percent_saved: baseline_bytes.map(|b| (b as f64 - bytes as f64) / b as f64 * 100.0),
        scalar_decode_hps: best,
        simd_decode_hps: best_simd,
        simd_vs_scalar: best_simd.map(|s| s / best),
        verdict: best_simd.map(|s| verdict(scheme.as_str(), s, best, baseline_hps)),
    }
}

/// Encode every chunk under `scheme` and assert the scalar (and, when
/// `simd`, the SIMD) roundtrip on each one.
fn encode_and_verify(name: &str, scheme: Scheme, chunks: &[Vec<Hop>], simd: bool) -> Vec<Vec<u8>> {
    let bufs: Vec<Vec<u8>> = chunks.iter().map(|c| encode_chunk(scheme, c)).collect();
    for (buf, chunk) in bufs.iter().zip(chunks) {
        assert_eq!(
            &decode_chunk(scheme, buf),
            chunk,
            "{name}/{}: scalar roundtrip mismatch",
            scheme.as_str()
        );
        if simd {
            assert_eq!(
                &decode_chunk_simd(scheme, buf),
                chunk,
                "{name}/{}: SIMD roundtrip mismatch",
                scheme.as_str()
            );
        }
    }
    bufs
}

/// Best hops/second over `rounds` timed decodes of every buffer, after one
/// warm-up. `decode` is chosen once by the caller (scalar or SIMD).
fn time_decode(
    decode: fn(Scheme, &[u8]) -> Vec<Hop>,
    scheme: Scheme,
    bufs: &[Vec<u8>],
    hops: u64,
    rounds: u32,
) -> f64 {
    let decode_once = |sink: &mut u64| {
        for buf in bufs {
            let decoded = decode(scheme, buf);
            *sink = sink.wrapping_add(decoded.len() as u64);
            std::hint::black_box(&decoded);
        }
    };
    let mut sink = 0u64;
    decode_once(&mut sink); // warm-up
    let mut best = 0.0f64;
    for _ in 0..rounds {
        let started = Instant::now();
        decode_once(&mut sink);
        let secs = started.elapsed().as_secs_f64();
        if secs > 0.0 {
            best = best.max(hops as f64 / secs);
        }
    }
    std::hint::black_box(sink);
    best
}
