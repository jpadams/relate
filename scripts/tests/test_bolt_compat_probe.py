"""Offline tests for scripts/bolt_compat_probe.py and its three modules.

No server and no network: transcripts are built by hand from `Reply`
objects, so these pin the verdict machinery itself -- framing, rendering,
the opaque-value masking, the selectors, the manifest decoding, the
latest-Neo4j agent check, `evaluate`, and the findings table's shape (unique
stable ids, valid areas and severities, every finding naming a probe that
exists, every divergent finding carrying a target).
The probe catalog is built with a stand-in for the boltkit wrapper. The
one boltkit-dependent test (the Bytes round trip through boltkit's own
PackStream) runs when BOLTKIT_DIR names a boltkit checkout and is skipped
otherwise.

Run directly, like every scripts/tests/ suite:
python3 scripts/tests/test_bolt_compat_probe.py
"""

from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import bolt_compat_probe as probe_mod  # noqa: E402
from bolt_compat_compare import (  # noqa: E402
    Reply,
    Step,
    StepKind,
    canonical,
    failure_codes,
    key_sites,
    masked,
    meta_keys,
    render,
    render_transcript,
    step_kind,
    stream,
    whole,
)
from bolt_compat_findings import FINDINGS  # noqa: E402
from bolt_compat_wire import (  # noqa: E402
    IDLE,
    NOOP,
    Boltkit,
    Probe,
    WireState,
    frame,
    hello_map,
    read_manifest,
    read_replies,
)


class Struct:
    """Stand-in for boltkit's packstream.Structure (tag + fields)."""

    def __init__(self, tag: int, *fields: object) -> None:
        self.tag = tag
        self.fields = fields


class FakeBoltkit:
    """Enough of the Boltkit wrapper to build the probe catalog offline."""

    Structure = Struct

    def hello_extra(self, **extra: object) -> dict:
        return hello_map(extra.pop("user_agent", "t"), **extra)

    def message(self, name: str, *fields: object, chunk: int = 0, label: str = "") -> Step:
        return Step(name, label or name, b"", step_kind(name))

    def tagged(self, tag: int, name: str, *fields: object, label: str = "") -> Step:
        return Step(name, label or name, b"", step_kind(name))


RUN = Step("RUN", 'RUN "RETURN 1 AS x" {} {}', b"")
PULL = Step("PULL", "PULL {n: -1}", b"")


def run_pull(run_meta: dict, pull_meta: dict, row: list) -> list:
    return [
        (RUN, [Reply("SUCCESS", (run_meta,))]),
        (PULL, [Reply("RECORD", (row,), b"\x91\x01"), Reply("SUCCESS", (pull_meta,))]),
    ]


NEO = run_pull(
    {"t_first": 3, "fields": ["x"]},
    {"bookmark": "FB:abc", "type": "r", "t_last": 0, "db": "neo4j"},
    [1],
)
CL = run_pull(
    {"fields": ["x"], "type": "r"},
    {"t_last": 7, "db": "neo4j"},
    [1],
)


class FramingAndRendering(unittest.TestCase):
    def test_frame_splits_into_chunks_and_ends_with_a_zero_chunk(self) -> None:
        self.assertEqual(frame(b"abcde", 2), b"\x00\x02ab\x00\x02cd\x00\x01e\x00\x00")
        self.assertEqual(frame(b"ab", 0xFFFF), b"\x00\x02ab\x00\x00")

    def test_render_keeps_types_struct_tags_and_bytes_apart(self) -> None:
        self.assertEqual(render([1, 1.0, True, None, "é"]), '[1, 1.0, true, null, "é"]')
        self.assertEqual(render(Struct(0x44, 19724)), "Date<0x44>(19724)")
        self.assertEqual(render(b"\x00\xff"), "bytes[00 ff]")
        self.assertEqual(render({"nodes-created": 1, "db": "x"}), '{"nodes-created": 1, db: "x"}')

    def test_render_transcript_shows_requested_steps_and_skips_send_steps(self) -> None:
        t = [*CL[:1], (NOOP, []), *CL[1:]]
        self.assertEqual(
            render_transcript(t),
            'RUN -> SUCCESS {fields: ["x"], type: "r"}; '
            'PULL {n: -1} -> RECORD [1] | SUCCESS {t_last: 7, db: "neo4j"}',
        )
        self.assertEqual(render_transcript(t, (0,)), 'RUN -> SUCCESS {fields: ["x"], type: "r"}')


class TypedModel(unittest.TestCase):
    def test_step_kind_is_the_typed_enum(self) -> None:
        self.assertIs(Step("RUN", "RUN", b"").kind, StepKind.MESSAGE)
        self.assertIs(NOOP.kind, StepKind.SEND)
        self.assertIs(IDLE.kind, StepKind.IDLE)
        self.assertIs(FakeBoltkit().message("GOODBYE").kind, StepKind.SEND)
        self.assertIs(probe_mod.handshake_probes()[0].steps[0].kind, StepKind.HANDSHAKE)

    def test_probe_focus_defaults_to_the_whole_selector(self) -> None:
        self.assertIs(Probe("p", "t", ()).focus, whole)

    def test_wire_state_is_the_typed_enum(self) -> None:
        self.assertEqual(
            [(state.name, state.value) for state in WireState],
            [("OPEN", "open"), ("CLOSED", "closed"), ("DESYNC", "desync")],
        )

    def test_read_replies_reports_a_wire_that_is_no_longer_open(self) -> None:
        class FakeWire:
            def __init__(self, state: WireState) -> None:
                self.state = state

        step = Step("RUN", "RUN", b"")
        # The state short-circuits before the boltkit wrapper is consulted.
        closed = read_replies(FakeBoltkit(), FakeWire(WireState.CLOSED), step)  # type: ignore[arg-type]
        desync = read_replies(FakeBoltkit(), FakeWire(WireState.DESYNC), step)  # type: ignore[arg-type]
        self.assertEqual(closed, [Reply("<closed>")])
        self.assertEqual(desync, [Reply("<skipped>")])

    def test_hello_map_keeps_the_shared_policy(self) -> None:
        base = {"scheme": "basic", "principal": "neo4j", "credentials": ""}
        self.assertEqual(hello_map("agent"), {**base, "user_agent": "agent"})
        self.assertEqual(hello_map(None), base)
        self.assertEqual(
            hello_map("agent", patch_bolt=["utc"]),
            {**base, "user_agent": "agent", "patch_bolt": ["utc"]},
        )


class Masking(unittest.TestCase):
    def test_opaque_values_are_masked_but_their_keys_count(self) -> None:
        a = [(RUN, [Reply("SUCCESS", ({"t_first": 1, "server": "Neo4j/2026.09.0"},))])]
        b = [(RUN, [Reply("SUCCESS", ({"t_first": 99, "server": "Relate/0"},))])]
        c = [(RUN, [Reply("SUCCESS", ({"server": "x"},))])]
        self.assertEqual(canonical(masked(a)), canonical(masked(b)))
        self.assertNotEqual(canonical(masked(a)), canonical(masked(c)))

    def test_failure_message_is_opaque_and_the_code_is_not(self) -> None:
        def fail(code: str, message: str) -> list:
            return [(RUN, [Reply("FAILURE", ({"code": code, "message": message},))])]

        same = fail("Neo.X", "one"), fail("Neo.X", "two")
        self.assertEqual(canonical(masked(same[0])), canonical(masked(same[1])))
        self.assertNotEqual(
            canonical(masked(fail("Neo.X", "m"))), canonical(masked(fail("Relate.Error", "m")))
        )

    def test_int_float_and_bool_values_stay_distinct(self) -> None:
        def rec(v: object) -> list:
            return [(PULL, [Reply("RECORD", ([v],))])]

        keys = {canonical(masked(rec(v))) for v in (1, 1.0, True)}
        self.assertEqual(len(keys), 3)

    def test_graph_ids_are_renumbered_by_first_appearance_keeping_the_wiring(self) -> None:
        def graph(a: int, b: int, r: int, start: int) -> list:
            node_a = Struct(0x4E, a, ["L"], {})
            node_b = Struct(0x4E, b, ["L"], {})
            rel = Struct(0x52, r, start, b, "T", {})
            return [(PULL, [Reply("RECORD", ([node_a, node_b, rel],))])]

        base = canonical(masked(graph(3, 4, 0, 3)))
        self.assertEqual(base, canonical(masked(graph(51, 52, 9, 51))))
        # The relationship starts at the second node instead: the wiring differs.
        self.assertNotEqual(base, canonical(masked(graph(3, 4, 0, 4))))

    def test_route_addresses_are_masked_to_a_count(self) -> None:
        def rt(addresses: list) -> list:
            table = {"servers": [{"role": "WRITE", "addresses": addresses}], "ttl": 300}
            return [(RUN, [Reply("SUCCESS", ({"rt": table},))])]

        self.assertEqual(canonical(masked(rt(["a:1"]))), canonical(masked(rt(["b:2"]))))
        self.assertNotEqual(canonical(masked(rt(["a:1"]))), canonical(masked(rt(["a:1", "b:2"]))))


class Selectors(unittest.TestCase):
    def test_key_sites_report_where_a_key_appears(self) -> None:
        self.assertEqual(key_sites("type")(NEO), [[], [["SUCCESS", "r"]]])
        self.assertEqual(key_sites("type")(CL), [[["SUCCESS", "r"]], []])
        self.assertEqual(key_sites("t_first")(NEO), [[["SUCCESS", "<opaque>"]], []])

    def test_meta_keys_and_stream(self) -> None:
        self.assertEqual(meta_keys(1)(CL), ["SUCCESS", ["db", "t_last"]])
        self.assertEqual(stream(NEO), stream(CL))
        self.assertEqual(stream(NEO)[0], [["SUCCESS", {"fields": ["x"]}]])

    def test_failure_codes(self) -> None:
        failure = Reply("FAILURE", ({"code": "Neo.A", "message": "m"},))
        t = [(RUN, [failure]), (PULL, [Reply("IGNORED")])]
        self.assertEqual(failure_codes(t), [["Neo.A"], []])

    def test_unprojectable_transcripts_compare_whole(self) -> None:
        closed_a = [(RUN, [Reply("<closed>")])]
        closed_b = [(RUN, [Reply("FAILURE", ("not a map",))])]
        select = meta_keys(1)
        self.assertTrue(probe_mod.same(select, {"neo4j": closed_a, "relate": closed_a}))
        self.assertFalse(probe_mod.same(select, {"neo4j": closed_a, "relate": closed_b}))


class FakeWire:
    """Serves `read_manifest` bytes from a buffer, like Wire.recv_upto."""

    def __init__(self, data: bytes) -> None:
        self.data = data

    def recv_upto(self, n: int) -> bytes:
        out, self.data = self.data[:n], self.data[n:]
        return out


class Handshake(unittest.TestCase):
    def test_manifest_is_decoded_into_version_ranges_and_capabilities(self) -> None:
        # The manifest latest Neo4j (2026.09.0) sent after 00 00 01 FF.
        wire = FakeWire(bytes.fromhex("04 00040404 00040405 00020805 00010106 00"))
        self.assertEqual(
            read_manifest(wire).name,  # type: ignore[arg-type]
            "MANIFEST 04 00 04 04 04 00 04 04 05 00 02 08 05 00 01 01 06 00 = "
            "[4.0-4.4, 5.0-5.4, 5.6-5.8, 6.0-6.1] capabilities 0",
        )

    def test_manifest_with_one_entry_and_a_multi_byte_capability_varint(self) -> None:
        wire = FakeWire(bytes.fromhex("01 00000404 8101"))
        self.assertEqual(
            read_manifest(wire).name,  # type: ignore[arg-type]
            "MANIFEST 01 00 00 04 04 81 01 = [4.4] capabilities 129",
        )

    def test_a_short_manifest_is_marked(self) -> None:
        wire = FakeWire(bytes.fromhex("02 00000404"))
        self.assertEqual(read_manifest(wire).name, "<short manifest>")  # type: ignore[arg-type]

    def test_only_a_neo4j_5_plus_agent_counts_as_latest(self) -> None:
        self.assertTrue(probe_mod.is_latest_neo4j("Neo4j/2026.09.0"))
        self.assertTrue(probe_mod.is_latest_neo4j("Neo4j/5.26.0"))
        self.assertFalse(probe_mod.is_latest_neo4j("Neo4j/4.4.48"))
        self.assertFalse(probe_mod.is_latest_neo4j("Relate/0.12.3"))


class Findings(unittest.TestCase):
    def catalog(self) -> dict:
        probes = probe_mod.handshake_probes() + probe_mod.message_probes(FakeBoltkit(), "h:1")
        return {p.name: p for p in probes}

    def test_ids_are_sequential_and_the_first_67_grouped_by_area(self) -> None:
        expected = [f"F-{i:02d}" for i in range(1, len(FINDINGS) + 1)]
        self.assertEqual([f.id for f in FINDINGS], expected)
        order = ["handshake", "protocol", "failures", "values", "cypher"]
        # Findings after F-67 are appended, so earlier ids keep their meaning.
        ranks = [order.index(f.area) for f in FINDINGS[:67]]
        self.assertEqual(ranks, sorted(ranks))
        self.assertTrue(all(f.area in order for f in FINDINGS))

    def test_every_finding_is_well_formed_and_names_a_probe(self) -> None:
        catalog = self.catalog()
        for f in FINDINGS:
            with self.subTest(f.id):
                self.assertIn(f.probe, catalog)
                self.assertIn(f.severity, ("high", "medium", "low"))
                self.assertTrue(f.target.strip())

    def test_every_probe_has_a_finding(self) -> None:
        self.assertEqual(set(self.catalog()), {f.probe for f in FINDINGS})

    def test_evaluate_emits_the_target_only_when_divergent(self) -> None:
        finding = next(f for f in FINDINGS if f.aspect == "which SUCCESS carries type")
        probe = self.catalog()[finding.probe]
        row = probe_mod.evaluate(finding, probe, {"neo4j": NEO, "relate": CL})
        self.assertEqual(row["verdict"], "divergent")
        self.assertEqual(row["target_behavior"], finding.target)
        self.assertEqual(
            list(row),
            [
                "id",
                "area",
                "probe",
                "neo4j_reply",
                "relate_reply",
                "verdict",
                "severity",
                "target_behavior",
            ],
        )
        row = probe_mod.evaluate(finding, probe, {"neo4j": NEO, "relate": NEO})
        self.assertEqual((row["verdict"], row["target_behavior"]), ("compatible", ""))


@unittest.skipUnless(os.environ.get("BOLTKIT_DIR"), "BOLTKIT_DIR names no boltkit checkout")
class BoltkitPatches(unittest.TestCase):
    def test_bolt_44_tables_and_bytes_round_trip(self) -> None:
        bk = Boltkit(Path(os.environ["BOLTKIT_DIR"]).resolve())
        self.assertEqual(bk.proposal_44, bytes.fromhex("00000404" + "00" * 12))
        self.assertEqual(bk.tags["ROUTE"], 0x66)
        packed = bk.pack(bk.Structure(0x71, [b"\x00\x01\xff", {"k": b""}]))
        self.assertEqual(packed, bytes.fromhex("b17192cc030001ffa1816bcc00"))
        reply = bk.decode(packed)
        self.assertEqual((reply.name, reply.fields), ("RECORD", ([b"\x00\x01\xff", {"k": b""}],)))


if __name__ == "__main__":
    unittest.main()
