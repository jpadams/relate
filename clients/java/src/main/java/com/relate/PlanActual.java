package com.relate;

import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * One operator's PROFILE metrics; {@code null} on an EXPLAIN tree.
 *
 * <p>A deliberate subset of the wire object, stated so the omission is a
 * contract, not an accident: the engine also emits {@code total_time_ms},
 * {@code seek_ms}, {@code walk_ms}, {@code loops}, {@code
 * expression_built_bytes}, {@code batched}, {@code zone_skips} and the
 * {@code frontier} counters ({@code edge_visits}, {@code levels},
 * {@code endpoints}), which the lenient plan mapper drops here. Extending
 * the typed subset is follow-up work.
 */
public class PlanActual {
    public long rows;
    @JsonProperty("time_ms")
    public double timeMs;
    @JsonProperty("db_hits")
    public long dbHits;
    @JsonProperty("memory_bytes")
    public long memoryBytes;
    public long hops;
    @JsonProperty("neighbor_walks")
    public long neighborWalks;
    @JsonProperty("node_reads")
    public long nodeReads;
    public PlanStart start;
}
