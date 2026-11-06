"""The findings table of scripts/bolt_compat_probe.py.

One `Finding` per distinct behavior, in a stable order: F-01..F-67 grouped
by area (handshake, protocol, failures, values, cypher), later findings
appended so an id never changes meaning. Each names the probe it reads, the
aspect it compares (a selector from bolt_compat_compare), the severity if it is
divergent, and the latest-Neo4j behavior to match -- written from the
replies neo4j:latest (Neo4j/2026.09.0) sent on the wire over Bolt 4.4, and
phrased in Bolt 4.4-only terms where latest Neo4j's answer depends on
serving another version. The verdict is never stored here: the probe
computes it on every run, so re-running against a fixed server flips a
finding to compatible on its own.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable

from bolt_compat_compare import (
    Select,
    after,
    at,
    combine,
    failure_codes,
    key_sites,
    meta_keys,
    meta_value,
    names,
    raw_records,
    records,
    route_table,
    stream,
    whole,
)


@dataclass(frozen=True)
class Finding:
    """One behavior of `probe`, compared through `select`; `show` picks the
    steps rendered into the replies (default: every step)."""

    id: str
    area: str
    probe: str
    aspect: str
    select: Select
    severity: str
    target: str
    show: tuple[int, ...] = ()


def handshake(id: str, probe: str, target: str, severity: str = "high") -> Finding:
    aspect = "answer bytes and connection state"
    return Finding(id, "handshake", probe, aspect, whole, severity, target)


def violation(id: str, probe: str, message: str) -> Finding:
    """How the connection fares after one state-machine breach."""
    return Finding(
        id,
        "protocol",
        probe,
        "reply sequence and connection state after the breach",
        names,
        "low",
        f"{message} is a protocol breach: one FAILURE, then the server closes the "
        "connection (the RESET sent after it finds the connection closed).",
    )


def violation_code(id: str, probe: str, message: str, neo4j_text: str) -> Finding:
    """The FAILURE code of one state-machine breach; `neo4j_text` is the
    message latest Neo4j sent with it."""
    return Finding(
        id,
        "failures",
        probe,
        "FAILURE code",
        failure_codes,
        "low",
        f"{message} fails with code Neo.ClientError.Request.Invalid (latest Neo4j's message: "
        f"\"{neo4j_text}\").",
    )


def value(id: str, probe: str, target: str, select: Select | None = None) -> Finding:
    """A result value: the RECORDs of the probe's PULL (step 1)."""
    return Finding(id, "values", probe, "RECORD values", select or records(1), "high", target, (1,))


def route_rt(
    id: str, probe: str, aspect: str, project: Callable[[dict], Any], severity: str, target: str
) -> Finding:
    return Finding(id, "protocol", probe, aspect, route_table(project), severity, target)


FINDINGS: list[Finding] = [
    # ---------------------------------------------------------------- handshake
    handshake("F-01", "handshake_44", "Answer 00 00 04 04 and keep the connection open for HELLO."),
    handshake(
        "F-02",
        "handshake_boltkit_default",
        "Latest Neo4j scans the proposal slots in client order and answers the first "
        "version it serves (it still serves 4.0-4.4): for [4.3, 4.2, 4.1, 4.0] it answers "
        "00 00 03 04, and it never answers a version the client did not propose. In 4.4-only "
        "terms: when no slot (version or range) covers 4.4, answer 00 00 00 00 and close the "
        "connection instead of 00 00 04 04 (the 00 00 03 04 answer itself needs Bolt 4.3).",
    ),
    handshake(
        "F-03",
        "handshake_range_44_42",
        "Honor the range byte: 00 02 04 04 proposes 4.4, 4.3 and 4.2; answer 00 00 04 04 and "
        "keep the connection open.",
    ),
    handshake(
        "F-04",
        "handshake_50",
        "Latest Neo4j serves Bolt 5.0 and answers 00 00 00 05, keeping the connection open. "
        "In 4.4-only terms: a proposal that does not cover 4.4 (here 5.0 only) gets 00 00 00 "
        "00 and the connection closes -- never 00 00 04 04, which the client did not propose.",
    ),
    handshake(
        "F-05",
        "handshake_3",
        "Latest Neo4j no longer serves Bolt 3: it answers 00 00 00 00 and closes the "
        "connection. Do the same for any proposal that does not cover 4.4 -- never answer "
        "00 00 04 04 to a client that did not propose it.",
        "medium",
    ),
    handshake(
        "F-06",
        "handshake_manifest_44",
        "Latest Neo4j answers a manifest-v1 slot (00 00 01 FF) ahead of the 4.4 slot with a "
        "manifest: 00 00 01 FF, a varint count, one 4-byte [00, range, minor, major] entry "
        "per served version range and a varint capability mask -- here 04 | 00 04 04 04 | "
        "00 04 04 05 | 00 02 08 05 | 00 01 01 06 | 00 (4.0-4.4, 5.0-5.4, 5.6-5.8, 6.0-6.1, "
        "no capabilities) -- then reads the client's 4-byte choice plus its capability "
        "varint and waits for HELLO. In 4.4-only terms: answer the manifest with the one "
        "entry 4.4 (00 00 01 FF 01 00 00 04 04 00), read the choice (00 00 04 04 + varint), "
        "then serve HELLO.",
        "low",
    ),
    # ----------------------------------------------------------------- protocol
    Finding(
        "F-07",
        "protocol",
        "hello_user_agent",
        "HELLO SUCCESS metadata",
        at(0),
        "low",
        'HELLO SUCCESS carries server, connection_id and hints: {"connection.recv_timeout_'
        'seconds": 120} (latest Neo4j\'s default). The hint tells drivers to treat a '
        "connection silent for that long as dead, so a server sending it must keep a "
        "long-running request's connection alive (Bolt 4.1+ NOOP chunks) within it.",
        (0,),
    ),
    Finding(
        "F-08",
        "protocol",
        "hello_user_agent",
        "session usable after HELLO",
        at(1),
        "high",
        "After a successful HELLO, RESET answers SUCCESS {}.",
        (1,),
    ),
    Finding(
        "F-09",
        "protocol",
        "hello_no_user_agent",
        "reply sequence after the rejected HELLO",
        names,
        "low",
        "A HELLO without user_agent is rejected with FAILURE and opens no session: the "
        "following RESET answers IGNORED (latest Neo4j then also fails a retried HELLO, "
        "\"cannot be handled by session in the AUTHENTICATION state\", and closes the "
        "connection).",
    ),
    Finding(
        "F-10",
        "protocol",
        "hello_patch_bolt_utc",
        "HELLO SUCCESS patch_bolt echo",
        meta_value(0, "patch_bolt"),
        "low",
        'When HELLO carries patch_bolt containing "utc", HELLO SUCCESS echoes patch_bolt: '
        '["utc"] and the connection then uses the UTC DateTime structs 0x49 / 0x69 in both '
        "directions.",
        (0,),
    ),
    Finding(
        "F-11",
        "protocol",
        "run_pull_all",
        "RUN SUCCESS t_first",
        key_sites("t_first"),
        "medium",
        "RUN SUCCESS carries t_first (int, milliseconds until the result was available to "
        "stream).",
        (0,),
    ),
    Finding(
        "F-12",
        "protocol",
        "run_pull_all",
        "RUN SUCCESS columnTypes",
        key_sites("columnTypes"),
        "low",
        "RUN SUCCESS carries no columnTypes key: latest Neo4j sends fields and t_first only "
        "(plus qid inside an explicit transaction).",
        (0,),
    ),
    Finding(
        "F-13",
        "protocol",
        "run_pull_all",
        "which SUCCESS carries type",
        key_sites("type"),
        "medium",
        'type ("r", "w", "rw" or "s") moves from RUN SUCCESS to the final SUCCESS of the '
        'result (the PULL or DISCARD that ends the stream): here PULL SUCCESS {type: "r"}.',
    ),
    Finding(
        "F-14",
        "protocol",
        "run_pull_all",
        "auto-commit PULL SUCCESS bookmark",
        key_sites("bookmark"),
        "medium",
        "The final PULL SUCCESS of every auto-commit query, read or write, carries bookmark "
        "(a non-empty string).",
        (1,),
    ),
    Finding(
        "F-15",
        "protocol",
        "run_pull_all",
        "final PULL SUCCESS t_last and db",
        combine(meta_value(1, "t_last"), meta_value(1, "db")),
        "medium",
        'The final PULL SUCCESS carries t_last (int, ms) and db (the database name, "neo4j").',
        (1,),
    ),
    Finding(
        "F-16",
        "protocol",
        "run_pull_all",
        "RECORD stream",
        stream,
        "high",
        "PULL {n: -1} streams RECORD [1] and then one SUCCESS.",
    ),
    Finding(
        "F-17",
        "protocol",
        "run_pull_paged",
        "records per PULL and has_more",
        combine(stream, key_sites("has_more")),
        "high",
        "PULL {n: 2} streams at most two RECORDs; every SUCCESS that leaves records behind "
        "carries has_more: true, the final one carries no has_more.",
    ),
    Finding(
        "F-18",
        "protocol",
        "run_pull_paged",
        "has_more SUCCESS metadata keys",
        combine(meta_keys(1), meta_keys(2)),
        "low",
        "A PULL SUCCESS with has_more: true carries only has_more; t_last, db, type and "
        "bookmark are sent only on the final SUCCESS of the stream.",
        (1, 2),
    ),
    Finding(
        "F-19",
        "protocol",
        "run_discard_all",
        "DISCARD SUCCESS metadata keys",
        meta_keys(1),
        "medium",
        "The DISCARD SUCCESS that ends an auto-commit result carries bookmark, type, t_last "
        'and db, like the final PULL SUCCESS: here {bookmark: "...", type: "r", t_last: 0, '
        'db: "neo4j"}.',
        (1,),
    ),
    Finding(
        "F-20",
        "protocol",
        "tx_commit",
        "explicit-tx PULL SUCCESS stats",
        key_sites("stats"),
        "medium",
        "Inside an explicit transaction the final PULL SUCCESS of a write carries stats: the "
        "statement's non-zero counters plus contains-updates: true (here {labels-added: 1, "
        "nodes-created: 1, properties-set: 1, contains-updates: true}).",
        (2,),
    ),
    Finding(
        "F-21",
        "protocol",
        "tx_commit",
        "which SUCCESS carries bookmark in an explicit tx",
        key_sites("bookmark"),
        "medium",
        "Inside an explicit transaction no PULL SUCCESS carries bookmark; COMMIT SUCCESS "
        "carries bookmark (a non-empty string).",
        (2, 3),
    ),
    Finding(
        "F-22",
        "protocol",
        "tx_rollback",
        "ROLLBACK discards the write",
        stream,
        "high",
        "BEGIN, RUN and ROLLBACK answer SUCCESS (the unpulled result is dropped) and the "
        "write is gone: the following count answers RECORD [0].",
    ),
    Finding(
        "F-23",
        "protocol",
        "tx_two_runs",
        "RUN SUCCESS qid",
        key_sites("qid"),
        "medium",
        "Inside an explicit transaction RUN SUCCESS carries qid (int, 0-based, incremented "
        "per RUN within the transaction): here 0 then 1. Auto-commit RUN SUCCESS carries no "
        "qid.",
        (1, 2),
    ),
    Finding(
        "F-24",
        "protocol",
        "tx_two_runs",
        "two open results, PULL by qid",
        records(3, 4),
        "high",
        "Each RUN in an explicit transaction keeps its own open result; a later RUN does not "
        "discard an earlier one. PULL {n: -1, qid: 0} streams the first result ([1], [2]); "
        "PULL without qid (or qid -1) streams the most recent one ([10], [20]).",
        (3, 4),
    ),
    Finding(
        "F-25",
        "protocol",
        "syntax_error",
        "failed state: IGNORED until RESET",
        names,
        "high",
        "After a RUN FAILURE, PULL answers IGNORED, RESET answers SUCCESS {} and the next "
        "RUN / PULL run normally.",
    ),
    Finding(
        "F-26",
        "protocol",
        "pipelined",
        "pipelined requests answered in order",
        stream,
        "high",
        "Requests pipelined in one write are answered in order, one summary each: two "
        "results, a PULL {n: 1} page with has_more, then FAILURE for the bad RUN and "
        "IGNORED for the PULL after it.",
    ),
    Finding(
        "F-27",
        "protocol",
        "reset_pipelined",
        "work queued ahead of a pipelined RESET",
        after(1, stream),
        "low",
        "RESET interrupts the work queued before it: the PULL pipelined ahead of RESET "
        "answers IGNORED (no RECORDs; latest Neo4j's reply to the RUN ahead of it races "
        "between SUCCESS and IGNORED), RESET answers SUCCESS {} and the requests after it "
        "run normally.",
        (1, 2, 3, 4),
    ),
    Finding(
        "F-28",
        "protocol",
        "split_chunks",
        "message reassembled from many chunks",
        stream,
        "high",
        'A message split over several chunks is reassembled: RECORD ["split"] then SUCCESS.',
    ),
    Finding(
        "F-29",
        "protocol",
        "noop_chunk",
        "connection after a client NOOP chunk",
        names,
        "medium",
        "A client NOOP chunk (an empty chunk, 00 00) between messages is legal since Bolt "
        "4.1 and is skipped: the connection stays open and the next message is answered "
        "normally (here PULL -> RECORD [1], SUCCESS). It must not close the connection.",
    ),
    Finding(
        "F-30",
        "protocol",
        "goodbye",
        "connection after GOODBYE",
        names,
        "low",
        "GOODBYE gets no reply and the server closes the connection.",
    ),
    Finding(
        "F-31",
        "protocol",
        "unknown_tag",
        "reply and connection state",
        names,
        "low",
        "An unknown message signature (0x0E) is a fatal decode error: no FAILURE is sent and "
        "the connection closes (the RESET after it finds the connection closed).",
    ),
    violation("F-32", "commit_without_begin", "COMMIT outside a transaction"),
    violation("F-33", "rollback_without_begin", "ROLLBACK outside a transaction"),
    violation("F-34", "nested_begin", "BEGIN inside an open transaction"),
    Finding(
        "F-35",
        "protocol",
        "pull_without_run",
        "reply and connection state",
        names,
        "low",
        "PULL with no open result is a protocol breach: one FAILURE, then the server closes "
        "the connection; it is never answered SUCCESS.",
    ),
    route_rt(
        "F-36",
        "route_default_db",
        "rt.db when ROUTE names no db",
        lambda rt: rt["db"],
        "high",
        'A ROUTE whose extra map names no db (absent or null) answers the routing table of '
        'the home database: rt.db = "neo4j" (drivers adopt rt.db as the resolved home '
        "database for later requests).",
    ),
    route_rt(
        "F-37",
        "route_default_db",
        "rt.servers role order",
        lambda rt: [s["role"] for s in rt["servers"]],
        "low",
        "rt.servers lists the roles in the order WRITE, READ, ROUTE.",
    ),
    route_rt(
        "F-38",
        "route_default_db",
        "rt keys, ttl and addresses per role",
        lambda rt: [
            sorted(rt),
            rt["ttl"],
            sorted((s["role"], len(s["addresses"])) for s in rt["servers"]),
        ],
        "high",
        "rt carries servers, ttl (300) and db; each of WRITE, READ and ROUTE lists one "
        "address, the server's advertised Bolt address.",
    ),
    route_rt(
        "F-39",
        "route_named_db",
        "rt.db when ROUTE names db",
        lambda rt: rt["db"],
        "high",
        'A ROUTE whose extra map names db "neo4j" answers rt.db = "neo4j".',
    ),
    Finding(
        "F-40",
        "protocol",
        "write_summary",
        "auto-commit write PULL SUCCESS stats",
        key_sites("stats"),
        "medium",
        "The final PULL SUCCESS of an auto-commit write carries stats: the non-zero counters "
        "plus contains-updates: true (here {nodes-created: 2, relationships-created: 1, "
        "properties-set: 2, labels-added: 2, contains-updates: true}).",
        (1,),
    ),
    # ----------------------------------------------------------------- failures
    Finding(
        "F-41",
        "failures",
        "syntax_error",
        "FAILURE code",
        failure_codes,
        "high",
        "A Cypher syntax error fails RUN with code Neo.ClientError.Statement.SyntaxError "
        "(classification ClientError: drivers raise their syntax-error type and never "
        "retry).",
        (0,),
    ),
    Finding(
        "F-42",
        "failures",
        "missing_parameter",
        "FAILURE code",
        failure_codes,
        "high",
        "A missing parameter fails RUN with code Neo.ClientError.Statement.ParameterMissing "
        '(message "Expected parameter(s): p").',
        (0,),
    ),
    violation_code(
        "F-43",
        "commit_without_begin",
        "COMMIT outside a transaction",
        "Message of type CommitMessage cannot be handled by a session in the READY state.",
    ),
    violation_code(
        "F-44",
        "rollback_without_begin",
        "ROLLBACK outside a transaction",
        "Message of type RollbackMessage cannot be handled by a session in the READY state.",
    ),
    violation_code(
        "F-45",
        "nested_begin",
        "BEGIN inside an open transaction",
        "Message of type BeginMessage cannot be handled by a session in the IN_TRANSACTION "
        "state.",
    ),
    violation_code(
        "F-46",
        "pull_without_run",
        "PULL with no open result",
        "Message of type PullMessage cannot be handled by a session in the READY state.",
    ),
    # ------------------------------------------------------------------- values
    value("F-47", "value_node", "A node is struct 0x4E [id, labels, properties]."),
    value(
        "F-48",
        "value_relationship",
        "A relationship is struct 0x52 [id, start_id, end_id, type, properties].",
    ),
    value(
        "F-49",
        "value_path",
        "A path is struct 0x50 [nodes (0x4E), unbound relationships (0x72 [id, type, "
        "properties]), sequence]; a forward hop is [1, 1].",
    ),
    value(
        "F-50",
        "value_path_reverse",
        "A hop against the relationship direction has a negative relationship index in "
        "the path sequence: [-1, 1].",
    ),
    value(
        "F-51",
        "value_date",
        "A date is struct 0x44 Date [days since 1970-01-01]: date('2024-01-02') is "
        "0x44(19724), not a string.",
    ),
    value(
        "F-52",
        "value_time",
        "A time is struct 0x54 Time [nanoseconds since midnight, offset seconds]: "
        "time('03:04:05.123456789+01:00') is 0x54(11045123456789, 3600), not a string.",
    ),
    value(
        "F-53",
        "value_localtime",
        "A localtime is struct 0x74 LocalTime [nanoseconds since midnight]: "
        "localtime('03:04:05.123456789') is 0x74(11045123456789), not a string.",
    ),
    value(
        "F-54",
        "value_datetime_offset",
        "Without the utc patch, a datetime with an offset is struct 0x46 DateTime [epoch "
        "seconds of the local date-time, nanoseconds, offset seconds]: "
        "datetime('2024-01-02T03:04:05.123456789+01:00') is 0x46(1704164645, 123456789, "
        "3600), not a string.",
    ),
    value(
        "F-55",
        "value_datetime_zone",
        "Without the utc patch, a datetime with a zone id is struct 0x66 DateTimeZoneId "
        "[epoch seconds of the local date-time, nanoseconds, zone id]: "
        "datetime('2024-01-02T03:04:05[Europe/Stockholm]') is 0x66(1704164645, 0, "
        '"Europe/Stockholm"), not a string.',
    ),
    value(
        "F-56",
        "value_localdatetime",
        "A localdatetime is struct 0x64 LocalDateTime [epoch seconds, nanoseconds]: "
        "localdatetime('2024-01-02T03:04:05.123456789') is 0x64(1704164645, 123456789), not "
        "a string.",
    ),
    Finding(
        "F-57",
        "values",
        "hello_patch_bolt_utc",
        "datetime RECORD under the utc patch",
        records(2),
        "high",
        "Under the utc patch a datetime with an offset is struct 0x49 DateTimeUTC [UTC epoch "
        "seconds, nanoseconds, offset seconds]: datetime('2024-01-02T03:04:05.123456789"
        "+01:00') is 0x49(1704161045, 123456789, 3600).",
        (2,),
    ),
    value(
        "F-58",
        "value_duration",
        "A duration is struct 0x45 Duration [months, days, seconds, nanoseconds]: "
        "duration('P1Y2M3DT4H5M6.5S') is 0x45(14, 3, 14706, 500000000), not a string.",
    ),
    value(
        "F-59",
        "value_point_2d",
        "A 2D point is struct 0x58 Point2D [srid, x, y] with float coordinates: "
        "0x58(7203, 1.5, 2.5) and 0x58(4326, 13.0, 55.6), not strings.",
    ),
    value(
        "F-60",
        "value_point_3d",
        "A 3D point is struct 0x59 Point3D [srid, x, y, z] with float coordinates: "
        "0x59(9157, 1.0, 2.0, 3.0), not a string.",
    ),
    value(
        "F-61",
        "value_scalars",
        "Scalars use the smallest PackStream encoding (TINY_INT 01, INT_16 C9 FF 7F, INT_64 "
        "for 2147483648, C1 float, UTF-8 string, C2, C0): the RECORD bytes match exactly.",
        select=raw_records(1),
    ),
    value(
        "F-62",
        "value_list",
        "A heterogeneous list keeps each element's type and encoding: the RECORD bytes "
        "match exactly.",
        select=raw_records(1),
    ),
    value("F-63", "value_map", "A nested map keeps its keys and value types (key order is free)."),
    Finding(
        "F-64",
        "values",
        "param_bytes",
        "RUN reply and echoed value",
        stream,
        "high",
        "A Bytes parameter (markers CC/CD/CE) is accepted and round-trips: RUN SUCCESS, then "
        "RECORD [bytes 00 01 ff] (CC 03 00 01 FF on the wire).",
    ),
    value(
        "F-65", "param_collections", "List and map parameters round-trip with element types intact."
    ),
    value(
        "F-66",
        "param_structs",
        "Date 0x44, legacy DateTime 0x46, Point2D 0x58 and Duration 0x45 parameters decode to "
        "their Cypher types: $d.year 2024, $dt.hour 3, $p.x 1.5, $dur.months 14.",
    ),
    # ------------------------------------------------------------------- cypher
    Finding(
        "F-67",
        "cypher",
        "missing_parameter",
        "statement with an unbound parameter",
        names,
        "high",
        "A statement that references a parameter absent from the RUN parameters fails at "
        "RUN (nothing executes, no RECORD with null): FAILURE, then PULL answers IGNORED "
        "and RESET recovers.",
    ),
    # ------------------------------------------- appended (ids keep their meaning)
    Finding(
        "F-68",
        "failures",
        "hello_no_user_agent",
        "FAILURE code of the rejected HELLO",
        failure_codes,
        "low",
        "A HELLO without user_agent fails with code Neo.ClientError.Request.Invalid (message "
        "'Illegal value for field \"user_agent\": Expected value to be non-null').",
        (0,),
    ),
]

assert len({f.id for f in FINDINGS}) == len(FINDINGS), "duplicate finding id"
