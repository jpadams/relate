"""The comparison policy of scripts/bolt_compat_probe.py.

The transcript model (Step/StepKind/Reply/Transcript/Select), the compact
rendering and opaque-value masking the verdicts compare, and the selectors
that project a transcript onto the aspect one finding compares.
scripts/bolt_compat_wire.py produces the transcripts on the socket,
scripts/bolt_compat_findings.py names the aspects compared and
scripts/bolt_compat_probe.py evaluates them. Read the probe script's
docstring for the contract.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Callable

# Metadata values that are opaque or free-form: the key is compared, the
# value is not. The six constant-masked keys collapse to "<opaque>";
# `addresses` keeps the list length, so an address count still compares.
META_MASKS: dict[str, Callable[[Any], Any]] = {
    "server": lambda _: "<opaque>",
    "connection_id": lambda _: "<opaque>",
    "bookmark": lambda _: "<opaque>",
    "t_first": lambda _: "<opaque>",
    "t_last": lambda _: "<opaque>",
    "message": lambda _: "<opaque>",
    "addresses": lambda x: ["<address>"] * len(x),
}

# The graph-struct tags, named once: STRUCT_NAMES renders every tag and
# GRAPH_ID_FIELDS drives the Masker's id renumbering.
NODE_TAG = 0x4E
REL_TAG = 0x52
UNBOUND_REL_TAG = 0x72
# tag -> (graph-id kind, field index) per id-bearing field; a struct is
# masked only when it carries every field the layout names.
GRAPH_ID_FIELDS: dict[int, tuple[tuple[str, int], ...]] = {
    NODE_TAG: (("node", 0),),
    REL_TAG: (("rel", 0), ("node", 1), ("node", 2)),
    UNBOUND_REL_TAG: (("rel", 0),),
}
STRUCT_NAMES = {
    NODE_TAG: "Node",
    REL_TAG: "Relationship",
    UNBOUND_REL_TAG: "UnboundRelationship",
    0x50: "Path",
    0x44: "Date",
    0x54: "Time",
    0x74: "LocalTime",
    0x46: "DateTime",
    0x66: "DateTimeZoneId",
    0x49: "DateTimeUTC",
    0x69: "DateTimeZoneIdUTC",
    0x64: "LocalDateTime",
    0x45: "Duration",
    0x58: "Point2D",
    0x59: "Point3D",
}


class StepKind(Enum):
    """How the wire treats one client step: `MESSAGE` reads replies up to a
    summary, `SEND` sends bytes with no reply (a NOOP chunk, GOODBYE),
    `IDLE` waits for the server to close, `HANDSHAKE` is raw negotiation."""

    MESSAGE = "message"
    SEND = "send"
    IDLE = "idle"
    HANDSHAKE = "handshake"


@dataclass(frozen=True)
class Step:
    """One client action, sent as `data`; `kind` is how the wire treats it."""

    name: str
    label: str
    data: bytes
    kind: StepKind = StepKind.MESSAGE

    @property
    def short(self) -> str:
        return self.name if self.name in ("RUN", "HELLO", "ROUTE", "HANDSHAKE") else self.label


def step_kind(name: str) -> StepKind:
    """How the wire treats a client message named `name`: GOODBYE sends
    bytes with no reply, every other message reads replies up to a summary."""
    return StepKind.SEND if name == "GOODBYE" else StepKind.MESSAGE


@dataclass(frozen=True)
class Reply:
    """A server message (`SUCCESS`, `RECORD`, ...) or a `<marker>`; `raw` is
    the message payload as received."""

    name: str
    fields: tuple = ()
    raw: bytes = field(default=b"", compare=False)


Transcript = list[tuple[Step, list[Reply]]]

# A selector projects a transcript onto the aspect one finding compares.
Select = Callable[[Transcript], Any]


def whole(t: Transcript) -> Any:
    return masked(t)


# --------------------------------------------------------------------------
# Rendering and masking


def is_structure(value: Any) -> bool:
    """boltkit's PackStream Structure: the duck-typed `tag` + `fields` pair."""
    return hasattr(value, "tag") and hasattr(value, "fields")


def render(value: Any) -> str:
    """Compact, faithful rendering: int vs float, struct tags, bytes."""
    if value is None:
        return "null"
    if value is True or value is False:
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return repr(value)
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, (bytes, bytearray)):
        return f"bytes[{bytes(value).hex(' ')}]"
    if isinstance(value, list):
        return "[" + ", ".join(render(v) for v in value) + "]"
    if isinstance(value, dict):
        items = []
        for k, v in value.items():
            key = k if isinstance(k, str) and k.isidentifier() else render(k)
            items.append(f"{key}: {render(v)}")
        return "{" + ", ".join(items) + "}"
    if is_structure(value):
        name = STRUCT_NAMES.get(value.tag, "Struct")
        return f"{name}<0x{value.tag:02X}>(" + ", ".join(render(f) for f in value.fields) + ")"
    return repr(value)


def render_reply(reply: Reply) -> str:
    """One reply; a FAILURE message text is whitespace-collapsed and cut."""
    fields = list(reply.fields)
    if reply.name == "FAILURE" and fields and isinstance(fields[0], dict):
        meta = dict(fields[0])
        if "message" in meta:
            text = " ".join(str(meta["message"]).split())
            meta["message"] = text if len(text) <= 90 else text[:87] + "..."
        fields[0] = meta
    return " ".join([reply.name, *(render(f) for f in fields)])


def render_replies(replies: list[Reply]) -> str:
    return " | ".join(render_reply(r) for r in replies) or "(no reply)"


def render_transcript(transcript: Transcript, steps: tuple[int, ...] = ()) -> str:
    picked = [transcript[i] for i in steps] if steps else transcript
    return "; ".join(
        f"{step.short} -> {render_replies(replies)}"
        for step, replies in picked
        if step.kind != StepKind.SEND
    )


class Masker:
    """Mask opaque values and renumber graph ids by first appearance, so the
    id wiring between nodes, relationships and paths still compares."""

    def __init__(self) -> None:
        # Keyed by the kinds GRAPH_ID_FIELDS names, created on first use.
        self.ids: dict[str, dict[Any, str]] = {}

    def graph_id(self, kind: str, raw: Any) -> str:
        table = self.ids.setdefault(kind, {})
        return table.setdefault(raw, f"{kind}#{len(table)}")

    def value(self, v: Any) -> Any:
        if isinstance(v, list):
            return [self.value(x) for x in v]
        if isinstance(v, dict):
            return {k: self.value(x) for k, x in v.items()}
        if isinstance(v, (bytes, bytearray)):
            return {"$bytes": bytes(v).hex()}
        if is_structure(v):
            fields = list(v.fields)
            layout = GRAPH_ID_FIELDS.get(v.tag, ())
            if layout and len(fields) > max(index for _, index in layout):
                for kind, index in layout:
                    fields[index] = self.graph_id(kind, fields[index])
            return {"$struct": f"0x{v.tag:02X}", "fields": [self.value(f) for f in fields]}
        return v

    def meta(self, v: Any) -> Any:
        if isinstance(v, dict):
            out = {}
            for k, x in v.items():
                mask = META_MASKS.get(k)
                out[k] = mask(x) if mask is not None else self.meta(x)
            return out
        if isinstance(v, list):
            return [self.meta(x) for x in v]
        return self.value(v)

    def reply(self, r: Reply) -> list:
        convert = self.value if r.name == "RECORD" else self.meta
        return [r.name, *(convert(f) for f in r.fields)]


def masked(transcript: Transcript) -> list[list[list]]:
    """Per step, the masked replies as `[name, *fields]` lists (step labels
    are excluded: ROUTE's carry the dialed address)."""
    masker = Masker()
    return [[masker.reply(r) for r in replies] for _, replies in transcript]


def canonical(value: Any) -> str:
    """Type-aware equality key: JSON keeps 1, 1.0 and true apart."""
    return json.dumps(value, sort_keys=True, ensure_ascii=False)


# --------------------------------------------------------------------------
# Selectors: project a transcript onto the aspect a finding compares.


def at(*idx: int) -> Select:
    return lambda t: [masked(t)[i] for i in idx]


def names(t: Transcript) -> list[list[str]]:
    return [[r.name for r in replies] for _, replies in t]


def stream(t: Transcript) -> list[list]:
    """Message names, plus the masked values of every RECORD and the column
    names (`fields`) of every RUN SUCCESS."""

    def keep(r: list) -> Any:
        if r[0] == "RECORD":
            return r
        meta = meta_of(r) or {}
        return [r[0], {"fields": meta["fields"]}] if "fields" in meta else r[0]

    return [[keep(r) for r in replies] for replies in masked(t)]


def records(*idx: int) -> Select:
    return lambda t: [[r for r in masked(t)[i] if r[0] == "RECORD"] for i in idx]


def raw_records(*idx: int) -> Select:
    """The exact RECORD payload bytes: encodings (int width, ...) count."""
    return lambda t: [[r.raw.hex() for r in t[i][1] if r.name == "RECORD"] for i in idx]


def meta_of(reply: list) -> dict | None:
    return reply[1] if len(reply) > 1 and isinstance(reply[1], dict) else None


def key_sites(key: str) -> Select:
    """Per step, the replies carrying `key` in their metadata, with its value."""
    return lambda t: [
        [[r[0], meta_of(r)[key]] for r in replies if key in (meta_of(r) or {})]
        for replies in masked(t)
    ]


def meta_keys(step: int, reply: int = -1) -> Select:
    """One reply's name and metadata key set."""

    def select(t: Transcript) -> Any:
        r = masked(t)[step][reply]
        meta = meta_of(r)
        return [r[0], sorted(meta)] if meta is not None else [r[0]]

    return select


def meta_value(step: int, key: str, reply: int = -1) -> Select:
    return lambda t: (meta_of(masked(t)[step][reply]) or {}).get(key, "<absent>")


def failure_codes(t: Transcript) -> list[list[str]]:
    return [[meta_of(r)["code"] for r in replies if r[0] == "FAILURE"] for replies in masked(t)]


def route_table(project: Callable[[dict], Any]) -> Select:
    return lambda t: project(meta_of(masked(t)[0][0])["rt"])


def combine(*selects: Select) -> Select:
    return lambda t: [select(t) for select in selects]


def after(first: int, select: Select) -> Select:
    """`select` over the steps from `first` on."""
    return lambda t: select(t[first:])
