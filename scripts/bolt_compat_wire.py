"""The Bolt wire layer of scripts/bolt_compat_probe.py.

boltkit (neo4j-drivers/boltkit, branch 4.3) forced to Bolt 4.4 and taught
the PackStream Bytes type; message framing and chunking; one probe's run
against one server as a transcript of (client step, server replies). The
transcript model, rendering, masking and selectors live in
scripts/bolt_compat_compare.py. Read the probe script's docstring for the
contract.
"""

from __future__ import annotations

import socket
import struct
import subprocess
import sys
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any

from bolt_compat_compare import (
    Reply,
    Select,
    Step,
    StepKind,
    Transcript,
    render,
    render_replies,
    step_kind,
    whole,
)

BOLT_44 = (4, 4)
MAX_CHUNK = 0xFFFF
SUMMARIES = ("SUCCESS", "FAILURE", "IGNORED")
NEGOTIATED_44 = b"\x00\x00\x04\x04"
# A Bolt 5.7+ handshake slot asking for manifest negotiation (version 1).
MANIFEST_V1 = b"\x00\x00\x01\xff"
BYTES_MARKERS = {0xCC: ">B", 0xCD: ">H", 0xCE: ">I"}


class ProbeError(Exception):
    """A server is unreachable or misbehaves outside a probe, or boltkit
    cannot be loaded (the probe exits 2)."""


@dataclass(frozen=True)
class Timing:
    timeout: float  # socket timeout per read
    idle_wait: float  # how long to wait for the server to close


@dataclass(frozen=True)
class Probe:
    """`handshake` (raw 16-byte proposal) makes a handshake probe. Else the
    probe negotiates 4.4, sends `hello` (None: the probe sends its own) and
    its steps; `setup` statements run first on a connection of their own.
    `focus` projects what the UNCOVERED check compares (default: all)."""

    name: str
    title: str
    steps: tuple[Step, ...]
    handshake: bytes = b""
    hello: dict | None = None
    setup: tuple[str, ...] = ()
    pipelined: bool = False
    focus: Select = whole


NOOP = Step("NOOP", "NOOP chunk 00 00", b"\x00\x00", StepKind.SEND)
IDLE = Step("IDLE", "(wait for close)", b"", StepKind.IDLE)


# --------------------------------------------------------------------------
# boltkit, forced to Bolt 4.4


def hello_map(user_agent: str | None, **extra: Any) -> dict[str, Any]:
    """boltkit's own HELLO map (Connection.__init__), updated by `extra`;
    a None value drops the key."""
    hello = {"scheme": "basic", "principal": "neo4j", "credentials": ""}
    hello["user_agent"] = user_agent
    hello.update(extra)
    return {k: v for k, v in hello.items() if v is not None}


class Boltkit:
    """The boltkit client modules, patched for Bolt 4.4 and PackStream Bytes."""

    def __init__(self, checkout: Path) -> None:
        if not (checkout / "boltkit" / "client" / "__init__.py").is_file():
            raise ProbeError(f"{checkout} is not a boltkit checkout")
        sys.path.insert(0, str(checkout))
        import boltkit.client as client
        import boltkit.client.packstream as packstream

        # boltkit stops at 4.3; the 4.4 message tags are the 4.3 ones.
        client.CLIENT[BOLT_44] = client.CLIENT[(4, 3)]
        client.SERVER[BOLT_44] = client.SERVER[(4, 3)]
        add_bytes_type(packstream)
        self.magic: bytes = client.BOLT
        self.tags: dict[str, int] = client.CLIENT[BOLT_44]
        self.names = {tag: name for name, tag in client.SERVER[BOLT_44].items()}
        self.Structure = packstream.Structure
        self.pack = packstream.pack
        self.unpacker = unpacker_class(packstream)
        self.user_agent: str = client.Connection.default_user_agent()
        # boltkit's own handshake layout (Connection._open_to), 4.4 only.
        self.proposal_44 = b"".join(
            bytes([0, 0, minor, major])
            for major, minor in client.Connection.fix_bolt_versions([BOLT_44])
        )

    def hello_extra(self, **extra: Any) -> dict[str, Any]:
        """`hello_map` with boltkit's own user agent; an `extra` user_agent
        overrides it and a None value drops the key."""
        return hello_map(extra.pop("user_agent", self.user_agent), **extra)

    def message(self, name: str, *fields: Any, chunk: int = MAX_CHUNK, label: str = "") -> Step:
        return self.tagged(self.tags[name], name, *fields, chunk=chunk, label=label)

    def tagged(
        self, tag: int, name: str, *fields: Any, chunk: int = MAX_CHUNK, label: str = ""
    ) -> Step:
        payload = self.pack(self.Structure(tag, *fields))
        label = label or " ".join([name, *(render(f) for f in fields)])
        if chunk < MAX_CHUNK:
            label += f"  [{-(-len(payload) // chunk)} chunks of <= {chunk} bytes]"
        kind = step_kind(name)
        return Step(name, label, frame(payload, chunk), kind)

    def decode(self, payload: bytes) -> Reply:
        message = next(self.unpacker(payload).unpack())
        name = self.names.get(message.tag, f"MESSAGE<0x{message.tag:02X}>")
        return Reply(name, tuple(message.fields), payload)


def add_bytes_type(packstream: Any) -> None:
    """Teach boltkit's `pack` the Bytes type. Its recursion goes through the
    module global, so bytes nested in lists and maps are covered too.
    Idempotent: a second Boltkit in one process does not wrap it twice."""
    original = packstream.pack
    if getattr(original, "with_bytes", False):
        return

    def pack(*values: Any) -> bytes:
        out = []
        for value in values:
            if isinstance(value, (bytes, bytearray)):
                size = len(value)
                marker = 0xCC if size < 0x100 else 0xCD if size < 0x10000 else 0xCE
                out.append(bytes([marker]) + struct.pack(BYTES_MARKERS[marker], size))
                out.append(bytes(value))
            else:
                out.append(original(value))
        return b"".join(out)

    pack.with_bytes = True  # type: ignore[attr-defined]
    packstream.pack = pack


def unpacker_class(packstream: Any) -> type:
    """boltkit's `Unpackable`, extended with the Bytes markers."""

    class Unpacker(packstream.Unpackable):  # type: ignore[misc, name-defined]
        def unpack(self, count: int = 1):  # type: ignore[no-untyped-def]
            for _ in range(count):
                marker = self.data[self.offset]
                if marker in BYTES_MARKERS:
                    self.offset += 1
                    size = self.raw_unpack(BYTES_MARKERS[marker])
                    self.offset += size
                    yield bytes(self.data[self.offset - size : self.offset])
                else:
                    yield next(super().unpack(1))

    return Unpacker


def frame(payload: bytes, chunk: int) -> bytes:
    parts = []
    for offset in range(0, len(payload), chunk):
        piece = payload[offset : offset + chunk]
        parts.append(struct.pack(">H", len(piece)) + piece)
    return b"".join(parts) + b"\x00\x00"


def boltkit_commit(checkout: Path) -> str | None:
    result = subprocess.run(
        ["git", "-C", str(checkout), "rev-parse", "HEAD"],
        capture_output=True,
        text=True,
        check=False,
    )
    return result.stdout.strip() if result.returncode == 0 else None


# --------------------------------------------------------------------------
# Running a probe


class WireState(Enum):
    """How a wire can still be read: `OPEN` replies are attributed, `CLOSED`
    the server's EOF or reset was seen, `DESYNC` a read timed out so later
    replies cannot be attributed."""

    OPEN = "open"
    CLOSED = "closed"
    DESYNC = "desync"


class Wire:
    """One TCP connection. `state` is `WireState.OPEN`, `WireState.CLOSED`
    (EOF or reset seen) or `WireState.DESYNC` (a read timed out, so later
    replies cannot be attributed)."""

    def __init__(self, address: tuple[str, int], timing: Timing) -> None:
        self.timing = timing
        self.where = f"{address[0]}:{address[1]}"
        try:
            self.sock = socket.create_connection(address, timeout=timing.timeout)
        except OSError as e:
            raise ProbeError(f"cannot connect to {self.where}: {e}") from e
        self.state = WireState.OPEN
        # The HELLO SUCCESS metadata, once open_session has sent HELLO.
        self.hello: dict = {}

    def close(self) -> None:
        self.sock.close()

    def send(self, data: bytes) -> None:
        try:
            self.sock.sendall(data)
        except OSError:
            self.state = WireState.CLOSED

    def recv_upto(self, n: int) -> bytes:
        """Up to `n` bytes: fewer when the server closes or stays silent."""
        data = b""
        try:
            while len(data) < n:
                chunk = self.sock.recv(n - len(data))
                if not chunk:
                    self.state = WireState.CLOSED
                    break
                data += chunk
        except socket.timeout:
            pass
        except OSError:
            self.state = WireState.CLOSED
        return data

    def recv_exact(self, n: int) -> bytes:
        data = b""
        while len(data) < n:
            chunk = self.sock.recv(n - len(data))
            if not chunk:
                raise EOFError
            data += chunk
        return data

    def read_payload(self) -> bytes:
        """One message payload; server NOOP chunks between messages are skipped."""
        payload = b""
        while True:
            (size,) = struct.unpack(">H", self.recv_exact(2))
            if size == 0:
                if payload:
                    return payload
                continue
            payload += self.recv_exact(size)

    def wait_close(self) -> Reply:
        self.sock.settimeout(self.timing.idle_wait)
        try:
            data = self.sock.recv(1)
        except socket.timeout:
            return Reply("<open>")
        except OSError:
            data = b""
        finally:
            self.sock.settimeout(self.timing.timeout)
        if data:
            self.state = WireState.DESYNC
            return Reply("<unexpected bytes>")
        self.state = WireState.CLOSED
        return Reply("<closed>")

    def negotiate_44(self, magic: bytes, proposal: bytes) -> None:
        self.send(magic + proposal)
        answer = self.recv_upto(4)
        if answer != NEGOTIATED_44:
            self.close()
            raise ProbeError(f"{self.where}: proposed 4.4, got {hex_bytes(answer) or 'nothing'}")


def read_replies(bk: Boltkit, wire: Wire, step: Step) -> list[Reply]:
    if step.kind == StepKind.SEND:
        return []
    if wire.state != WireState.OPEN:
        return [Reply("<closed>" if wire.state == WireState.CLOSED else "<skipped>")]
    if step.kind == StepKind.IDLE:
        return [wire.wait_close()]
    replies = []
    while True:
        try:
            reply = bk.decode(wire.read_payload())
        except socket.timeout:
            wire.state = WireState.DESYNC
            return replies + [Reply("<timeout>")]
        except (EOFError, OSError):
            wire.state = WireState.CLOSED
            return replies + [Reply("<closed>")]
        replies.append(reply)
        if reply.name in SUMMARIES:
            return replies


def hex_bytes(data: bytes) -> str:
    return data.hex(" ").upper()


def read_varint(wire: Wire) -> tuple[int, bytes] | None:
    """One manifest varint (7 bits per byte, low group first, high bit set
    on every byte but the last); None when the server stops short."""
    value, raw = 0, b""
    while True:
        byte = wire.recv_upto(1)
        if not byte:
            return None
        raw += byte
        value |= (byte[0] & 0x7F) << (7 * (len(raw) - 1))
        if not byte[0] & 0x80:
            return value, raw


def read_manifest(wire: Wire) -> Reply:
    """The manifest after a 00 00 01 FF answer: a varint count, one 4-byte
    [00, range, minor, major] entry per served version range, and a varint
    capability mask -- rendered raw and decoded."""
    count = read_varint(wire)
    entries = wire.recv_upto(4 * count[0]) if count else b""
    caps = read_varint(wire) if count and len(entries) == 4 * count[0] else None
    if count is None or caps is None:
        return Reply("<short manifest>")
    ranges = []
    for i in range(0, len(entries), 4):
        _, span, minor, major = entries[i : i + 4]
        ranges.append(f"{major}.{minor - span}-{major}.{minor}" if span else f"{major}.{minor}")
    raw = hex_bytes(count[1] + entries + caps[1])
    return Reply(f"MANIFEST {raw} = [{', '.join(ranges)}] capabilities {caps[0]}")


def run_handshake(
    bk: Boltkit, probe: Probe, address: tuple[str, int], timing: Timing
) -> Transcript:
    """Raw handshake: the 4-byte answer, then the connection state. A
    manifest answer is read in full and answered with a choice of 4.4 and
    no capabilities, as a Bolt 5.7+ driver would."""
    wire = Wire(address, timing)
    try:
        wire.send(bk.magic + probe.handshake)
        answer = wire.recv_upto(4)
        replies = [Reply(f"BYTES {hex_bytes(answer)}" if answer else "<no bytes>")]
        if answer == MANIFEST_V1 and wire.state == WireState.OPEN:
            replies.append(read_manifest(wire))
            wire.send(NEGOTIATED_44 + b"\x00")
        replies.append(
            Reply("<closed>") if wire.state == WireState.CLOSED else wire.wait_close()
        )
        return [(probe.steps[0], replies)]
    finally:
        wire.close()


def open_session(bk: Boltkit, address: tuple[str, int], hello: dict, timing: Timing) -> Wire:
    wire = Wire(address, timing)
    wire.negotiate_44(bk.magic, bk.proposal_44)
    step = bk.message("HELLO", hello)
    wire.send(step.data)
    replies = read_replies(bk, wire, step)
    if replies[-1].name != "SUCCESS":
        wire.close()
        raise ProbeError(f"{wire.where}: HELLO failed: {render_replies(replies)}")
    wire.hello = replies[-1].fields[0] if replies[-1].fields else {}
    return wire


def server_agent(bk: Boltkit, address: tuple[str, int], timing: Timing) -> str:
    """The server agent a default HELLO reports (`server`, e.g. Neo4j/2026.09.0)."""
    wire = open_session(bk, address, bk.hello_extra(), timing)
    wire.close()
    return str(wire.hello.get("server", "<no server agent>"))


def prepare(bk: Boltkit, probe: Probe, address: tuple[str, int], timing: Timing) -> None:
    """Run the probe's setup statements (auto-commit RUN + PULL each) on a
    connection of their own; they are not part of the transcript."""
    if not probe.setup:
        return
    wire = open_session(bk, address, bk.hello_extra(), timing)
    try:
        for cypher in probe.setup:
            for step in (bk.message("RUN", cypher, {}, {}), bk.message("PULL", {"n": -1})):
                wire.send(step.data)
                replies = read_replies(bk, wire, step)
                if replies[-1].name != "SUCCESS":
                    raise ProbeError(f"{wire.where}: setup {cypher!r}: {render_replies(replies)}")
    finally:
        wire.close()


def run_probe(bk: Boltkit, probe: Probe, address: tuple[str, int], timing: Timing) -> Transcript:
    if probe.handshake:
        return run_handshake(bk, probe, address, timing)
    prepare(bk, probe, address, timing)
    if probe.hello is None:
        wire = Wire(address, timing)
        wire.negotiate_44(bk.magic, bk.proposal_44)
    else:
        wire = open_session(bk, address, probe.hello, timing)
    try:
        if probe.pipelined:
            wire.send(b"".join(step.data for step in probe.steps))
            return [(step, read_replies(bk, wire, step)) for step in probe.steps]
        transcript = []
        for step in probe.steps:
            if wire.state == WireState.OPEN:
                wire.send(step.data)
            transcript.append((step, read_replies(bk, wire, step)))
        return transcript
    finally:
        wire.close()
