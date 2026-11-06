#!/usr/bin/env python3
"""Probe relate-bolt against the latest Neo4j on the Bolt wire, with boltkit.

LABS-323 (gbuild feature ``bolt-compat-tests``). The reference is the latest
Neo4j (``neo4j:latest``; the probe prints the server agent its HELLO reports,
e.g. ``Neo4j/2026.09.0``), which relate-bolt aims to be compatible with
over Bolt 4.4 -- the one version relate-bolt serves and latest Neo4j
still serves. Every probe opens a fresh TCP connection to each server, sends
both the same bytes, and records the replies exactly as they arrive --
message names and decoded fields, never anything inferred from code or
docs:

* handshake probes send raw handshake bytes (the ``60 60 B0 17`` magic plus
  a 16-byte proposal) and record the 4-byte answer and whether the server
  then keeps the connection open; a manifest answer (``00 00 01 FF``) is
  read in full and answered with a choice of 4.4, as a Bolt 5.7+ driver
  does;
* message probes negotiate Bolt 4.4, send HELLO, then a scripted sequence of
  Bolt messages (one at a time, or pipelined in one write), recording every
  reply message per request. Probes that touch data first delete every
  ``:BoltProbe`` node and create their fixture on a connection of their own.

The client is the boltkit Python client (neo4j-drivers/boltkit, branch 4.3,
pinned commit ``687a47973c0cfb14f7f2789a4b9b494cbe938a46``), imported from a
checkout via ``sys.path``: only ``boltkit.client`` and
``boltkit.client.packstream`` are used, both stdlib-only, so none of
boltkit's pinned requirements need installing. boltkit stops at Bolt 4.3, so
the probe forces 4.4 (``CLIENT[(4, 4)] = CLIENT[(4, 3)]`` and
``SERVER[(4, 4)] = SERVER[(4, 3)]``: the 4.4 tags are the 4.3 ones) and adds
the PackStream Bytes type boltkit lacks. Framing and reading live in
``scripts/bolt_compat_wire.py`` (boltkit's ``Connection`` auto-RESETs after a
FAILURE and assumes one ``recv`` returns a whole chunk, which would hide what
the server sent); the transcript model, rendering, masking and the selectors
live in ``scripts/bolt_compat_compare.py``.

Verdicts
--------

The output is a list of findings, one per distinct behavior (``F-NN``, a
stable order: F-01..F-67 grouped by area -- handshake, protocol, failures,
values, cypher -- and later findings appended, so an id never changes
meaning). A finding projects one aspect of a probe's transcript (the whole
transcript, the metadata keys of one reply, the replies carrying one key,
...) and is ``compatible`` when both servers' projections are equal, else
``divergent``. Before comparing, opaque or free-form values are masked:
``server``, ``connection_id``, ``bookmark``, ``t_first``, ``t_last`` and the
FAILURE ``message`` (the key still counts, its value does not), routing-table
addresses, and node/relationship ids (renumbered by first appearance, so the
id wiring between nodes, relationships and paths still counts). Everything
else -- message order, metadata keys and values, FAILURE codes, value types
(int vs float), struct tags, and for scalar records the exact bytes -- must
match.

``severity`` is the driver impact of the behavior when divergent (``high``:
a mainstream driver misbehaves -- connection failure, wrong retry, wrong
value type; ``medium``: missing summary data; ``low``: cosmetic). A
compatible finding keeps it as the impact a regression would have.
``target_behavior`` is the latest-Neo4j behavior to match, phrased in Bolt
4.4-only terms where latest Neo4j's answer depends on serving another
version; it is emitted for divergent findings only. A probe whose transcripts still differ (in its
``focus``) while none of its findings is divergent prints an ``UNCOVERED``
warning, so a new difference is never silently compatible.

Usage
-----

Start both servers, then point the probe at them and at a boltkit checkout::

    git clone https://github.com/neo4j-drivers/boltkit /tmp/boltkit
    git -C /tmp/boltkit checkout 687a47973c0cfb14f7f2789a4b9b494cbe938a46
    docker pull neo4j:latest
    docker run -d --rm --name bolt-probe-neo4j -p 127.0.0.1:17687:7687 \\
        -e NEO4J_AUTH=none neo4j:latest
    cargo build -p relate-cli
    target/debug/relate serve --host 127.0.0.1 --bolt 17688 --database :memory: --auth none &
    python3 scripts/bolt_compat_probe.py --boltkit /tmp/boltkit \\
        --neo4j 127.0.0.1:17687 --relate 127.0.0.1:17688 --json findings.json

The probe prints both servers' agents (the reference's Neo4j version
first), every probe's transcripts side by side (``!=`` marks a request
whose replies differ after masking), then one line per finding. It warns
when the reference does not report a Neo4j 5+ agent.
``--json`` writes the findings array: objects with ``id``, ``area``,
``probe``, ``neo4j_reply``, ``relate_reply``, ``verdict``, ``severity``
and ``target_behavior``. Exit status: 0 after a run (divergences are
findings, not errors), 2 when a server is unreachable, fails a setup step,
or boltkit cannot be imported.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from bolt_compat_compare import (
    Select,
    Step,
    StepKind,
    Transcript,
    after,
    at,
    canonical,
    combine,
    failure_codes,
    key_sites,
    masked,
    render,
    render_replies,
    render_transcript,
    route_table,
    stream,
    whole,
)
from bolt_compat_findings import FINDINGS, Finding
from bolt_compat_wire import (
    IDLE,
    NOOP,
    Boltkit,
    Probe,
    ProbeError,
    Timing,
    boltkit_commit,
    run_probe,
    server_agent,
)

BOLTKIT_COMMIT = "687a47973c0cfb14f7f2789a4b9b494cbe938a46"
LABEL = "BoltProbe"
CLEANUP = f"MATCH (n:{LABEL}) DETACH DELETE n"
GRAPH = (
    CLEANUP,
    f"CREATE (a:{LABEL} {{name: 'a', k: 1}})"
    f"-[:BOLT_PROBE {{since: 2020}}]->(b:{LABEL} {{name: 'b'}})",
)

# --------------------------------------------------------------------------
# Probes


def roles_sorted(rt: dict) -> dict:
    """A routing table with its servers in role order: order-blind."""
    return {**rt, "servers": sorted(rt["servers"], key=lambda s: s["role"])}


def handshake_probes() -> list[Probe]:
    zero = "00 00 00 00"

    def hs(name: str, title: str, proposal: str) -> Probe:
        raw = bytes.fromhex(proposal)
        assert len(raw) == 16, name
        step = Step("HANDSHAKE", f"60 60 B0 17 {proposal}", b"", StepKind.HANDSHAKE)
        return Probe(name, title, (step,), handshake=raw)

    return [
        hs("handshake_44", "propose [4.4]", f"00 00 04 04 {zero} {zero} {zero}"),
        hs(
            "handshake_boltkit_default",
            "propose [4.3, 4.2, 4.1, 4.0] (boltkit's default proposal)",
            "00 00 03 04 00 00 02 04 00 00 01 04 00 00 00 04",
        ),
        hs(
            "handshake_range_44_42",
            "propose [range 4.4 down to 4.2] (00 02 04 04)",
            f"00 02 04 04 {zero} {zero} {zero}",
        ),
        hs("handshake_50", "propose [5.0] only", f"00 00 00 05 {zero} {zero} {zero}"),
        hs("handshake_3", "propose [3]", f"00 00 00 03 {zero} {zero} {zero}"),
        hs(
            "handshake_manifest_44",
            "propose [manifest 00 00 01 FF, 4.4]; a manifest answer is read and 4.4 chosen",
            f"00 00 01 FF 00 00 04 04 {zero} {zero}",
        ),
    ]


def message_probes(bk: Boltkit, dialed: str) -> list[Probe]:
    """The message probes; `dialed` is the server address as dialed, sent
    in ROUTE's routing context like a driver does."""
    m = bk.message
    hello = bk.hello_extra()
    reset = m("RESET")
    S = bk.Structure

    def run(cypher: str, params: dict | None = None) -> Step:
        return m("RUN", cypher, params or {}, {})

    def pull(n: int = -1, qid: int | None = None) -> Step:
        return m("PULL", {"n": n} if qid is None else {"n": n, "qid": qid})

    def probe(name: str, title: str, *steps: Step, **kw: Any) -> Probe:
        kw.setdefault("hello", hello)
        return Probe(name, title, steps, **kw)

    def value(name: str, title: str, cypher: str, params: dict | None = None, **kw: Any) -> Probe:
        return probe(name, title, run(cypher, params), pull(), focus=stream, **kw)

    def route(name: str, extra: dict, **kw: Any) -> Probe:
        label = f'ROUTE {{address: "<dialed>"}} [] {render(extra)}'
        return probe(name, label, m("ROUTE", {"address": dialed}, [], extra, label=label), **kw)

    return [
        probe(
            "hello_user_agent",
            "HELLO with user_agent (boltkit's HELLO map), then RESET",
            m("HELLO", hello),
            reset,
            hello=None,
        ),
        probe(
            "hello_no_user_agent",
            "HELLO without user_agent, then RESET",
            m("HELLO", bk.hello_extra(user_agent=None)),
            reset,
            hello=None,
        ),
        probe(
            "hello_patch_bolt_utc",
            'HELLO with patch_bolt ["utc"], then a datetime result',
            m("HELLO", bk.hello_extra(patch_bolt=["utc"])),
            run("RETURN datetime('2024-01-02T03:04:05.123456789+01:00') AS dt"),
            pull(),
            hello=None,
            focus=combine(at(0), stream),
        ),
        probe("run_pull_all", "RUN + PULL n=-1", run("RETURN 1 AS x"), pull()),
        probe(
            "run_pull_paged",
            "RUN + PULL n=2 paging (has_more)",
            run("UNWIND range(1, 5) AS i RETURN i"),
            pull(2),
            pull(2),
            pull(2),
            focus=combine(stream, at(1, 2)),
        ),
        probe(
            "run_discard_all",
            "RUN + DISCARD n=-1",
            run("UNWIND range(1, 3) AS i RETURN i"),
            m("DISCARD", {"n": -1}),
            focus=at(1),
        ),
        probe(
            "tx_commit",
            "BEGIN / RUN (write) / PULL / COMMIT",
            m("BEGIN", {}),
            run(f"CREATE (n:{LABEL} {{k: 1}}) RETURN n.k AS k"),
            pull(),
            m("COMMIT"),
            setup=(CLEANUP,),
            focus=combine(stream, key_sites("stats"), key_sites("bookmark")),
        ),
        probe(
            "tx_rollback",
            "BEGIN / RUN (write) / ROLLBACK, then count",
            m("BEGIN", {}),
            run(f"CREATE (n:{LABEL} {{k: 2}}) RETURN n.k AS k"),
            m("ROLLBACK"),
            run(f"MATCH (n:{LABEL}) RETURN count(n) AS c"),
            pull(),
            setup=(CLEANUP,),
            focus=stream,
        ),
        probe(
            "tx_two_runs",
            "two RUNs in one explicit tx before any PULL (qid)",
            m("BEGIN", {}),
            run("UNWIND [1, 2] AS a RETURN a"),
            run("UNWIND [10, 20] AS b RETURN b"),
            pull(-1, 0),
            pull(),
            m("COMMIT"),
            focus=combine(stream, key_sites("qid")),
        ),
        probe(
            "syntax_error",
            "syntax error, then PULL, RESET, and a good query",
            run("RETURN 1 +"),
            pull(),
            reset,
            run("RETURN 2 AS y"),
            pull(),
            focus=combine(stream, failure_codes),
        ),
        probe(
            "missing_parameter",
            "RUN with a missing parameter, then PULL and RESET",
            run("RETURN $p AS p"),
            pull(),
            reset,
            focus=combine(stream, failure_codes),
        ),
        probe("commit_without_begin", "COMMIT without BEGIN, then RESET", m("COMMIT"), reset),
        probe("rollback_without_begin", "ROLLBACK without BEGIN, then RESET", m("ROLLBACK"), reset),
        probe(
            "nested_begin",
            "BEGIN inside an open BEGIN, then RESET",
            m("BEGIN", {}),
            m("BEGIN", {}),
            reset,
        ),
        probe("pull_without_run", "PULL with no open result, then RESET", pull(), reset),
        probe(
            "unknown_tag",
            "unknown message tag 0x0E (a v1/v2 ACK_FAILURE), then RESET",
            bk.tagged(0x0E, "TAG<0x0E>"),
            reset,
        ),
        probe(
            "noop_chunk",
            "client NOOP chunk (00 00) between RUN and PULL",
            run("RETURN 1 AS x"),
            NOOP,
            pull(),
            focus=stream,
        ),
        probe(
            "pipelined",
            "seven requests pipelined in one write, a failure last",
            run("RETURN 1 AS x"),
            pull(),
            run("UNWIND [1, 2] AS j RETURN j"),
            pull(1),
            pull(),
            run("RETURN 1 +"),
            pull(),
            pipelined=True,
            focus=stream,
        ),
        probe(
            "reset_pipelined",
            "RUN, PULL, RESET, RUN, PULL pipelined in one write (Neo4j races the first RUN)",
            run("UNWIND range(1, 3) AS i RETURN i"),
            pull(),
            reset,
            run("RETURN 2 AS y"),
            pull(),
            pipelined=True,
            focus=after(1, stream),
        ),
        probe(
            "split_chunks",
            "RUN split into 4-byte chunks, PULL into 1-byte chunks",
            m("RUN", "RETURN 'split' AS s", {}, {}, chunk=4),
            m("PULL", {"n": -1}, chunk=1),
            focus=stream,
        ),
        probe("goodbye", "GOODBYE, then wait for close", m("GOODBYE"), IDLE),
        route("route_default_db", {}),
        # F-37 pins the role order on route_default_db.
        route("route_named_db", {"db": "neo4j"}, focus=route_table(roles_sorted)),
        probe(
            "write_summary",
            "auto-commit write: summary stats, bookmark, type",
            run(f"CREATE (a:{LABEL} {{k: 1}})-[:BOLT_PROBE]->(b:{LABEL}) SET b.k = 2"),
            pull(),
            setup=(CLEANUP,),
            focus=combine(stream, key_sites("stats")),
        ),
        value(
            "value_node", "node result", f"MATCH (n:{LABEL} {{name: 'a'}}) RETURN n", setup=GRAPH
        ),
        value(
            "value_relationship",
            "relationship result",
            "MATCH ()-[r:BOLT_PROBE]->() RETURN r",
            setup=GRAPH,
        ),
        value(
            "value_path",
            "path result (forward hop)",
            f"MATCH p = (:{LABEL} {{name: 'a'}})-[:BOLT_PROBE]->() RETURN p",
            setup=GRAPH,
        ),
        value(
            "value_path_reverse",
            "path result (hop against the relationship direction)",
            f"MATCH p = (:{LABEL} {{name: 'b'}})<-[:BOLT_PROBE]-() RETURN p",
            setup=GRAPH,
        ),
        value("value_date", "date result", "RETURN date('2024-01-02') AS d"),
        value("value_time", "time result", "RETURN time('03:04:05.123456789+01:00') AS t"),
        value("value_localtime", "localtime result", "RETURN localtime('03:04:05.123456789') AS t"),
        value(
            "value_datetime_offset",
            "datetime result with an offset",
            "RETURN datetime('2024-01-02T03:04:05.123456789+01:00') AS dt",
        ),
        value(
            "value_datetime_zone",
            "datetime result with a zone id",
            "RETURN datetime('2024-01-02T03:04:05[Europe/Stockholm]') AS dt",
        ),
        value(
            "value_localdatetime",
            "localdatetime result",
            "RETURN localdatetime('2024-01-02T03:04:05.123456789') AS dt",
        ),
        value("value_duration", "duration result", "RETURN duration('P1Y2M3DT4H5M6.5S') AS d"),
        value(
            "value_point_2d",
            "2D point results (cartesian, WGS-84)",
            "RETURN point({x: 1.5, y: 2.5}) AS cartesian, "
            "point({longitude: 13.0, latitude: 55.6}) AS wgs84",
        ),
        value("value_point_3d", "3D point result", "RETURN point({x: 1.0, y: 2.0, z: 3.0}) AS p"),
        value(
            "value_scalars",
            "scalar results (int widths, float, string, bool, null)",
            "RETURN 1 AS tiny, -129 AS i16, 2147483648 AS i64, 1.5 AS f, "
            "'é' AS s, false AS b, null AS n",
        ),
        value("value_list", "list result", "RETURN [1, 2.5, 'three', true, null, [4]] AS l"),
        value("value_map", "map result", "RETURN {a: 1, b: 'two', c: [3], d: {e: null}} AS m"),
        value("param_bytes", "bytes parameter echoed", "RETURN $b AS b", {"b": b"\x00\x01\xff"}),
        value(
            "param_collections",
            "list and map parameters echoed",
            "RETURN $l AS l, $m AS m",
            {"l": [1, "a", None], "m": {"k": [True]}},
        ),
        value(
            "param_structs",
            "temporal/point/duration struct parameters, read back as scalars",
            "RETURN $d.year AS year, $dt.hour AS hour, $p.x AS x, $dur.months AS months",
            {
                "d": S(0x44, 19724),
                "dt": S(0x46, 1704164645, 0, 3600),
                "p": S(0x58, 7203, 1.5, 2.5),
                "dur": S(0x45, 14, 3, 14706, 500000000),
            },
        ),
    ]


# --------------------------------------------------------------------------
# Driver


def is_latest_neo4j(agent: str) -> bool:
    """A Neo4j 5+ agent (`Neo4j/5.26.0`, `Neo4j/2026.09.0`), not 4.x or another server."""
    name, _, version = agent.partition("/")
    major = version.split(".")[0]
    return name == "Neo4j" and major.isdigit() and int(major) >= 5


def parse_address(text: str) -> tuple[str, int]:
    host, _, port = text.rpartition(":")
    if not host or not port.isdigit():
        raise argparse.ArgumentTypeError(f"expected HOST:PORT, got {text!r}")
    return host, int(port)


def same(select: Select, results: dict[str, Transcript]) -> bool:
    """Whether both servers' transcripts project equal. A transcript the
    selector cannot project (a reply missing) only equals the same whole
    transcript."""

    def project(t: Transcript) -> str:
        try:
            return canonical(select(t))
        except (IndexError, KeyError, TypeError):
            return "<not projectable> " + canonical(masked(t))

    return project(results["neo4j"]) == project(results["relate"])


def print_probe(probe: Probe, results: dict[str, Transcript]) -> None:
    neo, cl = results["neo4j"], results["relate"]
    m_neo, m_cl = masked(neo), masked(cl)
    print(f"== {probe.name}: {probe.title}  [{'same' if same(whole, results) else 'differs'}]")
    for i, (step, replies) in enumerate(neo):
        if step.kind == StepKind.SEND:
            print(f"   C: {step.label}")
            continue
        mark = "  " if canonical(m_neo[i]) == canonical(m_cl[i]) else "!="
        print(f"{mark} C: {step.label}")
        print(f"      neo4j      {render_replies(replies)}")
        print(f"      relate     {render_replies(cl[i][1])}")


def evaluate(finding: Finding, probe: Probe, results: dict[str, Transcript]) -> dict[str, str]:
    verdict = "compatible" if same(finding.select, results) else "divergent"
    return {
        "id": finding.id,
        "area": finding.area,
        "probe": f"{probe.name}: {probe.title} -- {finding.aspect}",
        "neo4j_reply": render_transcript(results["neo4j"], finding.show),
        "relate_reply": render_transcript(results["relate"], finding.show),
        "verdict": verdict,
        "severity": finding.severity,
        "target_behavior": finding.target if verdict == "divergent" else "",
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--relate",
        type=parse_address,
        default="127.0.0.1:17688",
        metavar="HOST:PORT",
        help="relate serve Bolt address, plaintext, --auth none (default 127.0.0.1:17688)",
    )
    parser.add_argument(
        "--neo4j",
        type=parse_address,
        default="127.0.0.1:17687",
        metavar="HOST:PORT",
        help="latest Neo4j (the reference) address, plaintext, NEO4J_AUTH=none "
        "(default 127.0.0.1:17687)",
    )
    parser.add_argument(
        "--boltkit",
        type=Path,
        required=True,
        metavar="PATH",
        help=f"boltkit checkout (neo4j-drivers/boltkit at {BOLTKIT_COMMIT})",
    )
    parser.add_argument(
        "--json", type=Path, metavar="PATH", help="write the findings array to PATH"
    )
    parser.add_argument(
        "--probe",
        action="append",
        default=[],
        metavar="NAME",
        help="run only this probe and its findings (repeatable; default: every probe)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=5.0,
        metavar="SECONDS",
        help="socket timeout per read; a read past it is recorded as <timeout> (default 5)",
    )
    parser.add_argument(
        "--idle-wait",
        type=float,
        default=1.0,
        metavar="SECONDS",
        help="how long to wait for a server to close the connection (default 1)",
    )
    args = parser.parse_args()

    try:
        bk = Boltkit(args.boltkit.resolve())
    except (ProbeError, ImportError) as e:
        print(f"bolt_compat_probe: cannot load boltkit: {e}", file=sys.stderr)
        return 2
    commit = boltkit_commit(args.boltkit)
    if commit != BOLTKIT_COMMIT:
        print(
            f"bolt_compat_probe: warning: boltkit checkout is at {commit or 'unknown'}, "
            f"not the pinned {BOLTKIT_COMMIT}",
            file=sys.stderr,
        )
    servers = {
        "neo4j": ("reference", args.neo4j),
        "relate": ("under test", args.relate),
    }
    catalog = {
        name: {
            p.name: p
            for p in handshake_probes() + message_probes(bk, f"{address[0]}:{address[1]}")
        }
        for name, (_, address) in servers.items()
    }
    order = list(catalog["neo4j"])
    unknown = [name for name in args.probe if name not in catalog["neo4j"]]
    if unknown:
        parser.error(f"unknown probe(s) {', '.join(unknown)}; known: {', '.join(order)}")
    selected = [name for name in order if not args.probe or name in args.probe]
    timing = Timing(args.timeout, args.idle_wait)

    results: dict[str, dict[str, Transcript]] = {}
    try:
        for role, address in servers.values():
            agent = server_agent(bk, address, timing)
            print(f"== {role}: {agent} at {address[0]}:{address[1]}")
            if role == "reference" and not is_latest_neo4j(agent):
                print(
                    f"bolt_compat_probe: warning: the reference reports {agent!r}; "
                    "the reference is the latest Neo4j (neo4j:latest)",
                    file=sys.stderr,
                )
        for name in selected:
            results[name] = {
                server: run_probe(bk, catalog[server][name], address, timing)
                for server, (_, address) in servers.items()
            }
            print_probe(catalog["neo4j"][name], results[name])
    except ProbeError as e:
        print(f"bolt_compat_probe: {e}", file=sys.stderr)
        return 2

    evaluated = [
        (f.probe, evaluate(f, catalog["neo4j"][f.probe], results[f.probe]))
        for f in FINDINGS
        if f.probe in results
    ]
    findings = [row for _, row in evaluated]
    print("\n== findings")
    for f in findings:
        print(f"{f['id']}  {f['verdict']:<10}  {f['severity']:<6}  {f['area']:<9}  {f['probe']}")
    explained = {name for name, row in evaluated if row["verdict"] == "divergent"}
    for name in selected:
        if name not in explained and not same(catalog["neo4j"][name].focus, results[name]):
            print(f"UNCOVERED: probe {name} differs but none of its findings is divergent")
    tally: dict[tuple[str, str], int] = {}
    for f in findings:
        tally[(f["area"], f["verdict"])] = tally.get((f["area"], f["verdict"]), 0) + 1
    print(f"{len(findings)} findings: " + ", ".join(f"{a} {v} {n}" for (a, v), n in tally.items()))
    if args.json:
        args.json.write_text(json.dumps(findings, indent=2, ensure_ascii=False) + "\n")
        print(f"wrote {args.json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
