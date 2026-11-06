#!/usr/bin/env python3
# FROZEN ITERATION RECORD — NOT BUILT, NOT EXECUTED, NOT NORMATIVE.
# This is the Python child the Rust port in ../src was ported from,
# statement-for-statement (LABS-884). Editing it changes nothing the
# `dungeon` extension runs; the shipped child is the Rust binary. Kept
# verbatim as the iteration artifact — the engine friction it surfaced
# (LABS-887/888/891) is cited from the Rust sources' comments, not
# from here. Nothing in this tree should be split or "improved"; it is
# history, and git owns history.
"""dungeon — a Relate extension (MCP stdio client).

Scaffolded by `relate extension init dungeon`. The package is
`extension.json` (the manifest the host parses — mount name, MCP revision,
flag table, commit policy) plus this executable (the file the host spawns);
`README.md` is the quickstart and is not part of the install.

Glue: the transport contract lives in docs/api/extensions/spec.md, "The Transport contract". The transport and
the minimal client below are generated inline — no published client
dependency to install or manage: re-scaffold with `relate extension init`
or copy that section when the contract changes.

Edit `walk()`; everything above it is the generated glue. Standard library
only: python3 is the whole runtime.
"""

from __future__ import annotations

import json
import sys
from typing import Any

# The MCP revision this package speaks — the manifest's `protocol` and the
# handshake's `protocolVersion`. A host that answers another revision fails
# the handshake instead of downgrading.
PROTOCOL_VERSION = "2025-06-18"
NAME = "dungeon"
VERSION = "0.1.0"


def read_envelope() -> dict[str, Any]:
    """The host's request envelope: the manifest's resolved flag values, one
    JSON line on stdin, written before the session starts."""
    line = sys.stdin.readline()
    if not line:
        raise SystemExit("mcp: the host wrote no request envelope")
    return json.loads(line)


class Client:
    """The minimal MCP client: one JSON-RPC line per message, ids echoed.

    The host is the MCP server — it alone owns the store — so every call
    below is a `tools/call` on its five-tool table. The handshake publishes
    the mount's caps and the store's capacity; both stay on the client after
    `start()` so a walk can size its work against them.
    """

    def __init__(self) -> None:
        self._next_id = 0
        self.caps: dict[str, Any] = {}
        self.capacity: dict[str, Any] = {}

    def _send(self, message: dict[str, Any]) -> None:
        sys.stdout.write(json.dumps(message, separators=(",", ":")) + "\n")
        sys.stdout.flush()

    def request(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        self._next_id += 1
        request_id = self._next_id
        self._send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        while True:
            line = sys.stdin.readline()
            if not line:
                raise SystemExit(f"mcp: the host closed the session before answering {method}")
            message = json.loads(line)
            if message.get("id") != request_id:
                continue  # a notification, or an answer nobody asked for
            if "error" in message:
                raise SystemExit(f"mcp: {method} refused: {json.dumps(message['error'])}")
            return message.get("result", {})

    def start(self) -> dict[str, Any]:
        """Read the envelope, run the initialize handshake, confirm it."""
        envelope = read_envelope()
        result = self.request(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": NAME, "version": VERSION},
            },
        )
        answered = result.get("protocolVersion")
        if answered != PROTOCOL_VERSION:
            raise SystemExit(f"mcp: the host speaks {answered!r}, not {PROTOCOL_VERSION!r}")
        self.caps = result.get("caps", {})
        self.capacity = result.get("capacity", {})
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        return envelope

    def call(self, tool: str, arguments: dict[str, Any]) -> dict[str, Any]:
        """One `tools/call`; returns the tool's JSON result."""
        result = self.request("tools/call", {"name": tool, "arguments": arguments})
        for block in result.get("content", []):
            if block.get("type") == "text":
                return json.loads(block["text"])
        raise SystemExit(f"mcp: '{tool}' answered no text content")


# The closed compass vocabulary: rel types, validated before any
# statement is composed from them.
# Newlines cannot survive report_output (host flattens them); the shell
# wrapper turns this back into a newline before printing.
NEWLINE_PLACEHOLDER = "%%NL%%"

DIRECTIONS = (
    "NORTH", "NORTH_EAST", "EAST", "SOUTH_EAST",
    "SOUTH", "SOUTH_WEST", "WEST", "NORTH_WEST",
)

# The glyph grid's one vocabulary table (LABS-884): room glyphs keyed by
# what occupies the room, edge glyphs keyed by the direction type.
PLAYER_GLYPH = "@"
LIT_GLYPH = "O"        # a visited, lit room
ROOM_GLYPH = "#"       # a visited, dark room
VOID_GLYPH = "·"       # unexplored / no room there
EDGE_GLYPH = {
    "EAST": "-", "WEST": "-",
    "NORTH": "|", "SOUTH": "|",
    "NORTH_EAST": "/", "SOUTH_WEST": "/",
    "NORTH_WEST": "\\", "SOUTH_EAST": "\\",
}
LEGEND = (
    f"{PLAYER_GLYPH} you   {LIT_GLYPH} room   {ROOM_GLYPH} dark room   "
    "A-Z occupant   " + "".join(sorted(set(EDGE_GLYPH.values())))
    + " passage   " + VOID_GLYPH + " unexplored"
)

# Offsets for grid math: every direction's neighbour cell.
DELTA = {
    "NORTH": (0, -1), "SOUTH": (0, 1), "EAST": (1, 0), "WEST": (-1, 0),
    "NORTH_EAST": (1, -1), "SOUTH_WEST": (-1, 1),
    "NORTH_WEST": (-1, -1), "SOUTH_EAST": (1, 1),
}

def encode_pairs(pairs: list) -> str:
    """[k, v, k, v, ...] -> 'k=v|k=v' — LABS-887's workaround: LIST-typed
    graph-type properties cannot be ALTERed current yet, so the pair-list
    travels as one delimited STRING. Values must not contain '|' or '='
    (true for the starter set; the limitation is deliberate)."""
    return "|".join(f"{k}={v}" for k, v in zip(pairs[0::2], pairs[1::2]))


def decode_pairs(text: str) -> dict:
    return dict(pair.split("=", 1) for pair in (text or "").split("|") if pair)


def here_phrase(here: str | None, holder: str) -> str:
    """The item's per-holder addendum (", on the bench, ..."), joined
    onto its short/long wherever the holder's look lists it — the
    static desc stays valid because the phrase travels with the item,
    not the room."""
    phrase = decode_pairs(here or "").get(holder, "")
    return f", {phrase}" if phrase else ""


PLAYER = "You"


SHELL_SCRIPT = (
    # One line on purpose: report_output flattens newlines, so the
    # emitted wrapper must not contain any. Same behavior as the
    # shipped dungeon-shell file.
    "STORE=\"${1:-dungeon.rl}\"; "
    "HIST=\"${DUNGEON_HISTFILE:-$HOME/.dungeon_history}\"; "
    "run(){ out=\"$(relate run dungeon -d \"$STORE\" --command \"$1\")\"; "
    "printf '%s\\n' \"${out//%%NL%%/$'\\n'}\"; }; "
    "[ -f \"$HIST\" ] && { set -o history; history -r \"$HIST\"; set +o history; }; "
    "run start; "
    "while read -e -p \"dungeon> \" line; do "
    "[ -z \"$line\" ] && continue; "
    "set -o history; history -s \"$line\"; set +o history; "
    "printf '%s\\n' \"$line\" >> \"$HIST\"; "
    "run \"$line\"; done"
)

# The graph type, applied on seed (LABS-884's sketch; the pair-list
# encoding for chat/commands).
GRAPH_TYPE = """CREATE GRAPH TYPE DungeonGraph AS {
  (:Room => {name :: STRING NOT NULL IS KEY, short :: STRING,
             long :: STRING, lit :: BOOLEAN, zone :: STRING,
             x :: INTEGER, y :: INTEGER, visited :: BOOLEAN,
             intro :: STRING}),
  (:Npc => {name :: STRING NOT NULL IS KEY, short :: STRING,
            long :: STRING, race :: STRING, hp :: INTEGER, xp :: INTEGER,
            commands :: STRING, chat :: STRING}),
  (:Player => {name :: STRING NOT NULL IS KEY, hp :: INTEGER,
               xp :: INTEGER}),
  (:Item => {name :: STRING NOT NULL IS KEY, short :: STRING,
             long :: STRING, takeable :: BOOLEAN, unlocks :: STRING,
             weight :: INTEGER, here :: STRING}),
  (:Room)-[:NORTH]->(:Room),
  (:Room)-[:SOUTH]->(:Room),
  (:Room)-[:EAST]->(:Room),
  (:Room)-[:WEST]->(:Room),
  (:Room)-[:NORTH_EAST]->(:Room),
  (:Room)-[:NORTH_WEST]->(:Room),
  (:Room)-[:SOUTH_EAST]->(:Room),
  (:Room)-[:SOUTH_WEST]->(:Room),
  (:Item)-[:IN]->(:Room),
  (:Player)-[:LOCATION]->(:Room),
  (:Item)-[:UNLOCKS]->(:Room)
}"""
# LABS-887 workaround: repeated rel-type signatures (IN to Room/Npc/
# Player, two LOCATION rows) break the ALTER round-trip, and capability
# labels do not parse yet — the type declares the shapes that round-trip;
# the data carries the fuller edges regardless (the current type does not
# refuse undeclared shapes). Restore the full shape when LABS-887 lands.

# The starter dungeon (LABS-884 tier 1): a hand-drawn keep — a south
# entrance, two wings off the hall (the armory dead end, the kitchen
# cluster), a long corridor run north through gallery and antechamber
# to the throne room, and the locked cellar beyond. Corridors are rooms
# (short descs, mostly dark); dead ends carry the treasure-hunting
# texture; four diagonals are shortcuts. Room long descs never mention
# items — the "on the floor:" line is the live inventory; static text
# would go stale the moment an item moves.
ROOMS = [
    # y=0 — the entrance row (south); both neighbours are dead ends.
    ("Entrance", "the bottom of the steps",
     "A cool draft blows down from the steps above. Torchlight "
     "flickers on worn stone.", True, "outer", 0, 0),
    ("Guard Room", "the guard room",
     "An empty chair leans against the wall beside a cold brazier. "
     "Nobody guarded this door for a long time.", False, "outer", 1, 0),
    ("Rubbish Heap", "the rubbish heap",
     "Broken pottery and old bones, piled against the outer wall.",
     False, "outer", -1, 0),
    # y=-1 — the climb into the keep.
    ("Steps", "the worn steps",
     "Steps worn to shallow curves climb north into the keep. "
     "Torchlight gutters somewhere above.", True, "keep", 0, -1),
    # y=-2 — the hall and its two wings.
    ("Hall", "the taproom",
     "The taproom smells of torch smoke. Doors lead off in every "
     "direction; the corridors north are darker than the rest.",
     True, "keep", 0, -2),
    ("Corridor West", "the west corridor",
     "A dark corridor towards the armory. The dust here is old and "
     "undisturbed.", False, "keep", -1, -2),
    ("Armory", "the armory",
     "Racks of rusted practice weapons. A workbench sits by the far "
     "wall.", True, "keep", -2, -2),
    ("Kitchen", "the kitchen",
     "A cold hearth, hooks, and a cutting block scarred by decades of "
     "knives.", True, "keep", 1, -2),
    ("Barracks", "the barracks",
     "Rows of bunks, rotted to splinters. The air is still and dry.",
     True, "keep", 2, -2),
    # y=-3 — the west wing's water and the kitchen's pantry; the north
    # corridor begins.
    ("Cistern", "the cistern",
     "Black water, still as glass, reflects whatever light you "
     "carry. The drip of water echoes from somewhere deeper.",
     False, "undercroft", -1, -3),
    ("Pantry", "the pantry",
     "Shelves of empty jars. The labels have long since faded.",
     False, "undercroft", 1, -3),
    ("Chapel", "the chapel",
     "A cracked altar and tattered banners. Candles melted to puddles.",
     True, "undercroft", 2, -3),
    ("Corridor North", "the north corridor",
     "A long corridor running north. Your light reaches only a few "
     "paces ahead, and the keep gets quieter the further you go.",
     False, "undercroft", 0, -3),
    # y=-4 — the gallery and its side rooms; the deep west begins.
    ("Wine Cellar", "the wine cellar",
     "Racks of dust-caked bottles. One bottle looks newer than the "
     "rest.", True, "undercroft", -2, -4),
    ("Gallery", "the gallery",
     "Faded portraits of forgotten lords line a long corridor. Their "
     "eyes have all been scratched out.", True, "undercroft", 0, -4),
    ("Scriptorium", "the scriptorium",
     "Scattered pages, interrupted ink. The scribe left in a hurry.",
     True, "undercroft", 1, -4),
    ("Ossuary", "the ossuary",
     "Bones sorted by type, stacked with terrible patience.",
     False, "undercroft", 3, -4),
    # y=-5 — the furnace dead end, the treasury dead end, the
    # antechamber.
    ("Furnace", "the old furnace",
     "A cold furnace big enough to walk into. Soot shadows the walls.",
     False, "deep", -2, -5),
    ("Treasury", "the treasury",
     "Empty plinth after empty plinth. Whatever was here was taken "
     "long ago — almost all of it.", True, "deep", 2, -5),
    ("Antechamber", "the antechamber",
     "A cold antechamber before the throne room. The doors ahead are "
     "taller than they need to be.", False, "deep", 0, -5),
    # y=-6 — the throne.
    ("Throne Room", "the throne room",
     "A sagging throne on a cracked dais. The floor is swept clean — "
     "recently.", True, "deep", 0, -6),
    # y=-7 — the lair: the locked cellar, the quest's end.
    ("Cellar", "the locked cellar",
     "Behind the barrels, grey scales shift in the dark. One golden "
     "eye regards you, unhurried.", False, "lair", 0, -7),
]
EXITS = [
    ("Entrance", "NORTH", "Steps"), ("Steps", "SOUTH", "Entrance"),
    ("Entrance", "EAST", "Guard Room"),
    ("Guard Room", "WEST", "Entrance"),
    ("Entrance", "WEST", "Rubbish Heap"),
    ("Rubbish Heap", "EAST", "Entrance"),
    ("Steps", "NORTH", "Hall"), ("Hall", "SOUTH", "Steps"),
    ("Hall", "WEST", "Corridor West"),
    ("Corridor West", "EAST", "Hall"),
    ("Corridor West", "WEST", "Armory"),
    ("Armory", "EAST", "Corridor West"),
    ("Corridor West", "NORTH", "Cistern"),
    ("Cistern", "SOUTH", "Corridor West"),
    ("Cistern", "NORTH_WEST", "Wine Cellar"),
    ("Wine Cellar", "SOUTH_EAST", "Cistern"),
    ("Wine Cellar", "NORTH", "Furnace"),
    ("Furnace", "SOUTH", "Wine Cellar"),
    ("Hall", "EAST", "Kitchen"), ("Kitchen", "WEST", "Hall"),
    ("Kitchen", "NORTH", "Pantry"), ("Pantry", "SOUTH", "Kitchen"),
    ("Kitchen", "EAST", "Barracks"), ("Barracks", "WEST", "Kitchen"),
    ("Kitchen", "NORTH_EAST", "Chapel"),
    ("Chapel", "SOUTH_WEST", "Kitchen"),
    ("Chapel", "NORTH_EAST", "Ossuary"),
    ("Ossuary", "SOUTH_WEST", "Chapel"),
    ("Hall", "NORTH", "Corridor North"),
    ("Corridor North", "SOUTH", "Hall"),
    ("Corridor North", "NORTH", "Gallery"),
    ("Gallery", "SOUTH", "Corridor North"),
    ("Gallery", "EAST", "Scriptorium"),
    ("Scriptorium", "WEST", "Gallery"),
    ("Scriptorium", "NORTH_EAST", "Treasury"),
    ("Treasury", "SOUTH_WEST", "Scriptorium"),
    ("Gallery", "NORTH", "Antechamber"),
    ("Antechamber", "SOUTH", "Gallery"),
    ("Antechamber", "NORTH", "Throne Room"),
    ("Throne Room", "SOUTH", "Antechamber"),
    ("Throne Room", "NORTH", "Cellar"),
    ("Cellar", "SOUTH", "Throne Room"),
]
START_ROOM = "Entrance"
INTRO = (
    "The road ends at a flight of stone steps. Above you, a keep no "
    "lord has claimed in a hundred years; below, a taproom fire still "
    "burns — someone is keeping it lit. Somewhere in the dark north of "
    "the keep, they say, an old dragon guards a chest she is too proud "
    "to carry and too stubborn to abandon. You have come for the trade.\n"
    "\n"
    "(The way is compass-wise: n, ne, e, se, s, sw, w, nw. Look at "
    "things. Talk to whoever you meet — they answer what they care "
    "about.)"
)
NPCS = [
    ("Bard", "a road-worn bard",
     "A road-worn bard leans in the corner, tuning a lute with cracked "
     "nails. Road dust still on his boots, a half-finished song about "
     "a cellar on his lips. He nods when you look over. He will play "
     "if you ask, and rumour is his second instrument.",
     "Human", 8, 20,
     ["play", "ask", "hint", "name", "key", "cellar", "dragon", "song",
      "rumour", "lute", "road", "dungeon", "world"],
     ["play", "The bard plays a tune about a key that opens a cellar.",
      "ask", "Ask the armorer in the armory, west of the hall.",
      "hint", "Take the key from the armory west of the hall, then "
      "follow the corridors north to the throne room. The cellar lies "
      "beyond it.",
      "name", "Fenwick. I would say at your service, but the pay "
      "stopped centuries ago.",
      "key", "The armorer left the iron key on her bench, west of "
      "the hall past the dark corridor. She says the dragon earns it "
      "honestly.",
      "cellar", "Past the throne room, at the north end of the keep. "
      "The door is iron and the lock is older than the keep.",
      "dragon", "Old Grey? She settled in the cellar before the last "
      "lord died. Pay her a courtesy and she pays it back.",
      "song", "It is about a minstrel who traded a song for a chest. "
      "It ends well, unusually.",
      "rumour", "The treasury is not as empty as it looks. Look up, "
      "not down.",
      "lute", "Six strings, five intact. The lute has outlived three "
      "lords and every one of my audiences.",
      "road", "South, down the steps, and it winds to the coast "
      "eventually. I walked up it once and never found a reason to "
      "walk back.",
      "dungeon", "Corridors and dead ends, they say — wings off this "
      "hall, and a long dark run north to the throne. The acoustics "
      "improve the deeper you go.",
      "unknown", 'The bard keeps tuning. "The lute, the dragon," '
      'he says. "Nothing else pays the drinks."',
      "world", "The road south, the coast at its end, and a few "
      "villages that still remember this keep. That is most of my "
      "world these days."]),
    ("Armorer", "the armorer's ghost",
     "A translucent figure in a scorched apron drifts between the "
     "racks, straightening weapons that are no longer there. Being a "
     "ghost has not dulled her interest in keys, dragons, or the "
     "unlit rooms below.", "Ghost", 1, 5,
     ["ask", "hint", "name", "key", "dragon", "weapons", "ghost",
      "dungeon", "world"],
     ["ask", "I left the key on the bench. Take it — the dragon keeps "
      "worse things than rust.",
      "hint", "The cellar lies north of the throne room. Take a light.",
      "name", "Marta, armorer of this keep, in death as in life. The "
      "apron still fits.",
      "key", "Iron, cellar-door, on the bench by the racks. It was "
      "mine. Now it is yours.",
      "dragon", "She guards the chest because she promised to. Trade "
      "her the key and she trades you the chest.",
      "weapons", "All rust and memories. Take none of them — the "
      "racks remember their owners.",
      "ghost", "The cold does not bother me. Mind the unlit rooms — "
      "that is where the living lose their way.",
      "dungeon", "I hammered in every room of this keep — the wings, "
      "the deep corridors, all of them — and I remember where each "
      "nail went.",
      "unknown", 'The ghost dims a little. "The key, the dragon. '
      'My trade was always narrow."',
      "world", "The world kept moving when I stopped. Take the road "
      "south if you miss it."]),
    ("Dragon", "an old cellar dragon",
     "An old dragon, grey-scaled and cool to the touch, keeps her post "
     "behind the cellar barrels. She has outlasted every lock in the "
     "keep and remembers every bard who ever passed her door.",
     "Dragon", 40, 200,
     ["roar", "ask", "name", "treasure", "key", "bard", "dungeon",
      "world"],
     ["roar", "The dragon roars politely. Cellar dust settles in the "
      "still air.",
      "ask", 'The dragon says: "The chest is heavy and the keeping '
      'heavier. Bring my key and the carrying is yours."',
      "name", 'The dragon says: "Old Grey, the cellar dragon. I '
      'prefer the title I gave myself: the Curator."',
      "treasure", 'The dragon says: "One chest, one key. Marta forged '
      'the second and I am owed it. Fair trades keep the peace."',
      "key", 'The dragon says: "Iron, cellar-door. Marta forged it. '
      'Bring it and we will settle accounts."',
      "bard", 'The dragon says: "Fenwick? He still owes me the third '
      'verse of that trade-song."',
      "dungeon", 'The dragon says: "A keep of corridors above my '
      'cellar. The deep ones are mine to keep an eye on."',
      "world", 'The dragon says: "Mostly roads, the world. They all '
      'used to pass my door — now the door passes to you."',
      "unknown", 'The dragon says: "The key. Bring the key, or talk '
      'to the door."']),
]
NPC_LOCATION = [("Bard", "Hall"), ("Armorer", "Armory"),
                ("Dragon", "Cellar")]
ITEMS = [
    ("Key", "iron key", "a small iron key", True, "Cellar", 1,
     [("Armory", "on the bench, deliberately placed")]),
    ("Torch", "pitch torch", "a dry pitch torch", True, None, 2,
     [("Guard Room", "in a cold bracket by the brazier")]),
    ("Gem", "faceted gem", "a fist-sized gem that catches even weak "
     "light", True, None, 5,
     [("Treasury", "on the highest plinth, in a nest of dust")]),
    ("Treasure", "dragon's chest", "the chest of the cellar dragon",
     True, None, 20,
     [("Dragon", "coiled under one claw")]),
]
ITEM_LOCATION = [("Key", "Armory"), ("Torch", "Guard Room"),
                 ("Gem", "Treasury"), ("Treasure", "Dragon")]
UNLOCKS = [("Key", "Cellar")]


class Verbs:
    """One method per verb; each returns the turn's text via the client's
    report_output. All state is the store's."""

    def __init__(self, client: Any) -> None:
        self.client = client

    # -- host-call helpers ------------------------------------------------
    def write(self, statement: str, parameters: dict | None = None) -> dict:
        return self.client.call(
            "write_cypher",
            {"statement": statement, "parameters": parameters or {}},
        )

    def read(self, statement: str, parameters: dict | None = None) -> list:
        result = self.client.call(
            "read_cypher",
            {"statement": statement, "parameters": parameters or {}},
        )
        return result.get("rows", [])

    def report(self, text: str) -> None:
        # The host's print_summary flattens newlines (ADR-0430 §7's
        # one-line result, sanitize_error_text). Multi-line output
        # carries a placeholder the dungeon-shell wrapper restores
        # before printing — extension-surface friction on LABS-884
        # until the summary printer grows a multi-line form.
        self.client.call(
            "report_output",
            {"text": text.replace("\n", NEWLINE_PLACEHOLDER)},
        )

    # -- the verbs --------------------------------------------------------
    def seed(self, announce: bool = True) -> bool:
        """Seed or refresh; returns whether the store was fresh, so the
        caller (start) can show the one-time intro. Idempotent by
        construction: the catalog writes are MERGE-based and the
        placement writes only touch unplaced items/NPCs, so the
        wrapper can re-run seed on every start. On a store that already
        has rooms the catalog still refreshes (descs and chat improve
        between versions) — only the player's fresh start
        (hp/xp/location reset) is fresh-store-only."""
        fresh = not self.read(
            "MATCH (r:Room) RETURN r.name AS name LIMIT 1"
        )
        if fresh:
            # CREATE alone: it makes the type current; ALTER is
            # LABS-887's broken path and declares nothing CREATE did
            # not.
            self.write(GRAPH_TYPE)
        self.write(
            """UNWIND $rooms AS r
               MERGE (x:Room {name: r.name})
               SET x.short = r.short, x.long = r.long, x.lit = r.lit,
                   x.zone = r.zone, x.x = r.x, x.y = r.y""",
            {"rooms": [
                {"name": n, "short": s, "long": lo, "lit": li, "zone": z,
                 "x": x, "y": y}
                for (n, s, lo, li, z, x, y) in ROOMS
            ]},
        )
        self.write(
            "MATCH (r:Room {name: $room}) SET r.intro = $intro",
            {"room": START_ROOM, "intro": INTRO},
        )
        # One statement per direction type: the type is the statement's
        # text (the closed vocabulary), the endpoints are parameters.
        # MERGE, never CREATE — a re-seeded store must not gain
        # parallel exit edges.
        for direction in DIRECTIONS:
            pairs = [
                {"from": a, "to": b}
                for (a, d, b) in EXITS if d == direction
            ]
            if pairs:
                self.write(
                    f"UNWIND $exits AS e "
                    f"MATCH (a:Room {{name: e.from}}), (b:Room {{name: e.to}}) "
                    f"MERGE (a)-[:{direction}]->(b)",
                    {"exits": pairs},
                )
        self.write(
            """UNWIND $npcs AS n
               MERGE (x:Npc {name: n.name})
               SET x.short = n.short, x.long = n.long, x.race = n.race,
                   x.hp = n.hp, x.xp = n.xp, x.commands = n.commands,
                   x.chat = n.chat""",
            {"npcs": [
                {"name": nm, "short": s, "long": lo, "race": race,
                 "hp": hp, "xp": xp, "commands": encode_pairs(cmds),
                 "chat": encode_pairs(chat)}
                for (nm, s, lo, race, hp, xp, cmds, chat) in NPCS
            ]},
        )
        self.write(
            """UNWIND $loc AS l
               MATCH (n:Npc {name: l.npc})
               WHERE NOT (n)-[:LOCATION]->()
               MATCH (r:Room {name: l.room})
               MERGE (n)-[:LOCATION]->(r)""",
            {"loc": [{"npc": n, "room": r} for (n, r) in NPC_LOCATION]},
        )
        self.write(
            """UNWIND $items AS i
               MERGE (x:Item {name: i.name})
               SET x.short = i.short, x.long = i.long,
                   x.takeable = i.takeable, x.unlocks = i.unlocks,
                   x.weight = i.weight, x.here = i.here""",
            {"items": [
                {"name": nm, "short": s, "long": lo, "takeable": t,
                 "unlocks": u, "weight": w,
                 "here": encode_pairs([k for pair in h for k in pair])}
                for (nm, s, lo, t, u, w, h) in ITEMS
            ]},
        )
        self.write(
            """UNWIND $loc AS l
               MATCH (i:Item {name: l.item})
               WHERE NOT (i)-[:IN]->()
               MATCH (holder {name: l.holder})
               MERGE (i)-[:IN]->(holder)""",
            {"loc": [{"item": i, "holder": h} for (i, h) in ITEM_LOCATION]},
        )
        self.write(
            """UNWIND $unlocks AS u
               MATCH (i:Item {name: u.item}), (r:Room {name: u.room})
               MERGE (i)-[:UNLOCKS]->(r)""",
            {"unlocks": [{"item": i, "room": r} for (i, r) in UNLOCKS]},
        )
        if fresh:
            self.write(
                f"MERGE (p:Player {{name: $name}}) "
                f"SET p.hp = 10, p.xp = 0 "
                f"WITH p MATCH (r:Room {{name: $room}}) "
                f"MERGE (p)-[:LOCATION]->(r) SET r.visited = true",
                {"name": PLAYER, "room": START_ROOM},
            )
            if announce:
                self.report(
                    f"Seeded the starter dungeon ({len(ROOMS)} rooms, "
                    f"{len(NPCS)} NPCs, {len(ITEMS)} items).\n"
                    + self.render() + "\n" + self.room_text()
                )
        elif announce:
            self.report(
                "The dungeon is already seeded — descs and chat "
                "refreshed to the current version.\n"
                + self.render() + "\n" + self.room_text()
            )
        return fresh

    def room_text(self) -> str:
        # Long desc, then the room's contents with short descs —
        # NPCs first, then items (plain reads; OPTIONAL MATCH is
        # read-gated, LABS-888).
        rows = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               RETURN r.name AS room, r.long AS long""",
            {"player": PLAYER},
        )
        if not rows:
            return "No player in the store — run `seed` first."
        row = rows[0]
        lines = [f"[{row['room']}] {row['long'] or ''}"]
        npcs = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (n:Npc)-[:LOCATION]->(r)
               RETURN n.name AS name""",
            {"player": PLAYER},
        )
        if npcs:
            lines.append("here: " + ", ".join(n["name"] for n in npcs))
        items = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (i:Item)-[:IN]->(r)
               RETURN i.name AS name, i.short AS short, i.here AS here""",
            {"player": PLAYER},
        )
        if items:
            lines.append(
                "on the floor: "
                + ", ".join(
                    f"{i['name']} ({i['short']}"
                    + here_phrase(i["here"], row["room"]) + ")"
                    for i in items
                )
            )
        exits = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (r)-[rel]->(next:Room)
               RETURN DISTINCT type(rel) AS dir""",
            {"player": PLAYER},
        )
        if exits:
            # Inverse of SHORT_DIRS' short forms (the ≤2-char keys); the
            # long-name entries would otherwise win the reverse mapping.
            short = {d: s for s, d in SHORT_DIRS.items() if len(s) <= 2}
            # Compass-rose order starting at n (DIRECTIONS' own order).
            rose = sorted(
                (e["dir"] for e in exits), key=DIRECTIONS.index
            )
            lines.append(
                "exits: "
                + ", ".join(short.get(d, d.lower()) for d in rose)
            )
        return "\n".join(lines)

    # The explored-map window: rooms the player has already entered,
    # drawn as a lattice centred on the player's room (±2 cells), so a
    # big dungeon reveals itself room by room instead of spoiling the
    # layout up front.
    MAP_RADIUS = 2

    def render(self) -> str:
        player = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               RETURN r.x AS x, r.y AS y""",
            {"player": PLAYER},
        )
        if not player:
            return "(no map — run `seed` first)"
        px, py = player[0]["x"], player[0]["y"]
        rad = self.MAP_RADIUS
        # Only visited rooms inside the window are drawn; everything
        # else stays the void glyph.
        seen = {
            (row["x"], row["y"]): row
            for row in self.read(
                "MATCH (r:Room) WHERE r.visited "
                "RETURN r.name AS name, r.x AS x, r.y AS y, r.lit AS lit"
            )
            if abs(row["x"] - px) <= rad and abs(row["y"] - py) <= rad
        }
        if not seen:
            return "(no map — run `seed` first)"
        occupants = {
            (row["x"], row["y"]): row["npc"]
            for row in self.read(
                """MATCH (n:Npc)-[:LOCATION]->(room)
                   WHERE room.visited
                   RETURN room.x AS x, room.y AS y, n.name AS npc"""
            )
        }
        edges = self.read(
            f"MATCH (a:Room)-[rel]->(b:Room) "
            f"WHERE type(rel) IN {list(DIRECTIONS)!r} "
            f"RETURN a.x AS ax, a.y AS ay, type(rel) AS t, "
            f"b.x AS bx, b.y AS by"
        )
        # A lattice: even offsets are cells, odd offsets are connectors;
        # the window's origin is the player's room minus the radius.
        x0, y0 = px - rad, py - rad
        span = 2 * rad + 1
        grid = [
            [VOID_GLYPH for _ in range(2 * span - 1)]
            for _ in range(2 * span - 1)
        ]
        for (x, y), room in seen.items():
            cx, cy = 2 * (x - x0), 2 * (y - y0)
            if (x, y) == (px, py):
                grid[cy][cx] = PLAYER_GLYPH
            elif (x, y) in occupants:
                grid[cy][cx] = occupants[(x, y)][:1].upper()
            else:
                grid[cy][cx] = LIT_GLYPH if room["lit"] else ROOM_GLYPH
        # Passages: drawn between two visited rooms, and — the important
        # half — from a visited room into the void too, so an exit the
        # player can see from here is indicated on the map even before
        # the room behind it is entered.
        for edge in edges:
            ax, ay, bx, by = (edge["ax"], edge["ay"],
                              edge["bx"], edge["by"])
            if (ax, ay) not in seen and (bx, by) not in seen:
                continue
            mx = 2 * (ax - x0) + (bx - ax)
            my = 2 * (ay - y0) + (by - ay)
            if not (0 <= mx < 2 * span - 1 and 0 <= my < 2 * span - 1):
                continue
            grid[my][mx] = EDGE_GLYPH[edge["t"]]
        return "\n".join(" ".join(row) for row in grid)

    def status_text(self) -> str:
        stats = self.read(
            "MATCH (p:Player {name: $player}) "
            "OPTIONAL MATCH (i:Item)-[:IN]->(p) "
            "RETURN p.name AS name, p.hp AS hp, p.xp AS xp, "
            "collect(i.name) AS inventory",
            {"player": PLAYER},
        )
        if not stats:
            return "No player — run `seed` first."
        s = stats[0]
        inv = ", ".join(i for i in s["inventory"] if i) or "nothing"
        return (
            f"{s['name']} — hp {s['hp']}, xp {s['xp']}, carrying: {inv}\n"
            + self.render() + "\n"
            + self.room_text()
        )

    def status(self) -> None:
        self.report(self.status_text())

    def start(self) -> None:
        """The wrapper's one startup call: seed-or-refresh quietly
        (the catalog MERGEs are idempotent), then the status turn with
        the legend above the map — so the shell owns no game
        mechanics. A fresh store first gets the intro, read back from
        the start room where seed stored it: the text is room data."""
        fresh = self.seed(announce=False)
        text = LEGEND + "\n" + self.status_text()
        if fresh:
            row = self.read(
                "MATCH (r:Room {name: $room}) RETURN r.intro AS intro",
                {"room": START_ROOM},
            )
            if row and row[0]["intro"]:
                text = row[0]["intro"] + "\n\n" + text
        self.report(text)

    def move(self, direction: str) -> None:
        if direction not in DIRECTIONS:
            self.report(
                f"'{direction}' is not a compass direction. "
                f"Try one of: {' '.join(d.lower() for d in DIRECTIONS)}"
            )
            return
        # LABS-888 (OPTIONAL MATCH misclassified non-Read) forces three
        # plain reads instead of one OPTIONAL MATCH: the exit, whether
        # the target is locked, and whether the key is held. Collapse to
        # the single OPTIONAL MATCH statement when it lands.
        target = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(here)
               MATCH (here)-[:%s]->(next)
               RETURN next.name AS next, next.long AS long""" % direction,
            {"player": PLAYER},
        )
        if not target:
            self.report("There is no exit that way.")
            return
        room = target[0]["next"]
        lock = self.read(
            """MATCH (k:Item)-[:UNLOCKS]->(locked:Room {name: $room})
               RETURN count(k) AS locks""",
            {"room": room},
        )
        held = self.read(
            """MATCH (k:Item)-[:UNLOCKS]->(:Room {name: $room})
               MATCH (k)-[:IN]->(p:Player {name: $player})
               RETURN count(k) AS held""",
            {"room": room, "player": PLAYER},
        )
        locked = bool(lock and lock[0]["locks"]) and not (
            held and held[0]["held"]
        )
        if locked:
            self.report(
                f"The way {direction.lower()} is locked ({room}). "
                "You need the key."
            )
            return
        # The move is a write: one statement, one transaction — the
        # location edge is rewritten atomically. The room text and map
        # come from the reads below.
        self.write(
            """MATCH (p:Player {name: $player})-[l:LOCATION]->(here)
               MATCH (here)-[:%s]->(next)
               DELETE l CREATE (p)-[:LOCATION]->(next)
               SET next.visited = true""" % direction,
            {"player": PLAYER},
        )
        self.report(
            f"You go {direction.lower().replace('_', '-')}.\n"
            + self.render() + "\n"
            + self.room_text()
        )

    def take(self, item: str) -> None:
        if not item:
            self.report("Take what?")
            return
        row = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (i:Item) WHERE toLower(i.name) = toLower($item)
               MATCH (i)-[old:IN]->(holder)
               WHERE holder = r OR holder = p
                     OR (holder)-[:LOCATION]->(r)
               RETURN i.takeable AS takeable, i.name AS name""",
            {"player": PLAYER, "item": item},
        )
        if not row:
            self.report(f"There is no {item} here.")
            return
        if not row[0]["takeable"]:
            self.report(f"The {item} cannot be taken.")
            return
        self.write(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (i:Item) WHERE toLower(i.name) = toLower($item)
               MATCH (i)-[old:IN]->(holder)
               WHERE holder = r OR holder = p
                     OR (holder)-[:LOCATION]->(r)
               DELETE old
               WITH DISTINCT i, p
               CREATE (i)-[:IN]->(p)""",
            {"player": PLAYER, "item": item},
        )
        self.report(f"You take the {item}.\n" + self.render())

    def say(self, npc: str, text: str) -> None:
        if not npc:
            self.report("Say to whom?")
            return
        rows = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (n:Npc)-[:LOCATION]->(r)
               WHERE toLower(n.name) = toLower($npc)
               RETURN n.name AS name, n.chat AS chat""",
            {"player": PLAYER, "npc": npc},
        )
        if not rows:
            self.report(f"There is no {npc} here to talk to.")
            return
        pairs = decode_pairs(rows[0]["chat"])
        # No topic given -> the NPC's general "ask" answer; an unknown
        # topic is refused with the topics it does know (never a
        # generic fallback answer pretending the topic landed).
        if not text:
            answer = pairs.get("ask")
            self.report(
                f"{rows[0]['name']}: {answer}" if answer
                else f"{rows[0]['name']} waits for a specific question."
            )
            return
        keyword = text.split()[0].lower()
        answer = pairs.get(keyword) or pairs.get("unknown")
        if answer:
            self.report(f"{rows[0]['name']}: {answer}")
        else:
            self.report(
                f"{rows[0]['name']} has nothing to say about "
                f"'{keyword}'."
            )

    def drop(self, item: str) -> None:
        if not item:
            self.report("Drop what?")
            return
        held = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (i:Item) WHERE toLower(i.name) = toLower($item)
               MATCH (i)-[:IN]->(p)
               RETURN i.name AS name""",
            {"player": PLAYER, "item": item},
        )
        if not held:
            self.report(f"You are not carrying {item}.")
            return
        self.write(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (i:Item) WHERE toLower(i.name) = toLower($item)
               MATCH (i)-[old:IN]->(p)
               DELETE old
               WITH DISTINCT i, r
               CREATE (i)-[:IN]->(r)""",
            {"player": PLAYER, "item": item},
        )
        self.report(f"You drop the {held[0]['name']}.")

    def look_at(self, target: str) -> None:
        """look <npc|item> — the room's occupant or an item here or
        carried; no parameter is the room itself (status)."""
        if not target:
            self.status()
            return
        npc = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (n:Npc)-[:LOCATION]->(r)
               WHERE toLower(n.name) = toLower($target)
               RETURN n.name AS name, n.long AS long, n.chat AS chat""",
            {"player": PLAYER, "target": target},
        )
        if npc:
            # No mechanical topic list — the long desc names the themes
            # in-fiction, and an unknown topic gets the NPC's own
            # in-voice deflection (the "unknown" chat pair).
            lines = [f"{npc[0]['name']}: {npc[0]['long']}"]
            held = self.read(
                """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
                   MATCH (n:Npc)-[:LOCATION]->(r)
                   MATCH (i:Item)-[:IN]->(n)
                   WHERE toLower(n.name) = toLower($target)
                   RETURN i.name AS name, i.short AS short,
                          i.here AS here""",
                {"player": PLAYER, "target": target},
            )
            if held:
                lines.append(
                    "carrying: "
                    + ", ".join(
                        f"{h['name']} ({h['short']}"
                        + here_phrase(h["here"], npc[0]["name"]) + ")"
                        for h in held
                    )
                )
            self.report("\n".join(lines))
            return
        item = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (i:Item)
               WHERE toLower(i.name) = toLower($target)
                 AND ((i)-[:IN]->(r) OR (i)-[:IN]->(p)
                      OR (i)-[:IN]->(:Npc)-[:LOCATION]->(r))
               MATCH (i)-[:IN]->(holder)
               RETURN i.name AS name, i.long AS long,
                      i.here AS here, holder.name AS holder""",
            {"player": PLAYER, "target": target},
        )
        if item:
            self.report(
                f"{item[0]['name']}: {item[0]['long']}"
                + here_phrase(item[0]["here"], item[0]["holder"])
            )
            return
        self.report(f"You see no {target} here.")

    def ask_for(self, npc: str, item: str) -> None:
        """ask <npc> for <item> — the NPC hands over an item it holds,
        if it stands in your room."""
        if not npc or not item:
            self.report("Ask whom for what?")
            return
        offer = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (n:Npc)-[:LOCATION]->(r)
               WHERE toLower(n.name) = toLower($npc)
               MATCH (i:Item) WHERE toLower(i.name) = toLower($item)
               MATCH (i)-[:IN]->(n)
               RETURN i.takeable AS takeable, i.name AS name, n.name AS npc""",
            {"player": PLAYER, "npc": npc, "item": item},
        )
        if not offer:
            self.report(f"{npc} has no {item} to give.")
            return
        if not offer[0]["takeable"]:
            self.report(f"{npc} will not part with the {item}.")
            return
        self.write(
            """MATCH (p:Player {name: $player})
               MATCH (n:Npc) WHERE toLower(n.name) = toLower($npc)
               MATCH (i:Item) WHERE toLower(i.name) = toLower($item)
               MATCH (i)-[old:IN]->(n)
               DELETE old
               WITH DISTINCT i, p
               CREATE (i)-[:IN]->(p)""",
            {"player": PLAYER, "npc": npc, "item": item},
        )
        self.report(f"{offer[0]['npc']} hands you the {offer[0]['name']}.")

    def give(self, npc: str, item: str) -> None:
        if not npc or not item:
            self.report("Give what to whom?")
            return
        here = self.read(
            """MATCH (p:Player {name: $player})-[:LOCATION]->(r)
               MATCH (n:Npc)-[:LOCATION]->(r)
               WHERE toLower(n.name) = toLower($npc)
               MATCH (i:Item) WHERE toLower(i.name) = toLower($item)
               MATCH (i)-[:IN]->(p)
               RETURN i.name AS name, n.name AS npc""",
            {"player": PLAYER, "npc": npc, "item": item},
        )
        if not here:
            self.report(f"You cannot give {item or 'that'} to {npc or 'anyone'} here.")
            return
        self.write(
            """MATCH (p:Player {name: $player})
               MATCH (n:Npc) WHERE toLower(n.name) = toLower($npc)
               MATCH (i:Item) WHERE toLower(i.name) = toLower($item)
               MATCH (i)-[old:IN]->(p)
               DELETE old
               WITH DISTINCT i, n
               CREATE (i)-[:IN]->(n)""",
            {"player": PLAYER, "npc": npc, "item": item},
        )
        self.report(f"You give the {here[0]['name']} to {here[0]['npc']}.")

    def inventory(self) -> None:
        rows = self.read(
            """MATCH (p:Player {name: $player})
               MATCH (i:Item)-[:IN]->(p)
               RETURN i.name AS name, i.short AS short""",
            {"player": PLAYER},
        )
        if not rows:
            self.report("You are carrying nothing.")
            return
        self.report(
            "You are carrying:\n"
            + "\n".join(f"  {r['name']} — {r['short']}" for r in rows)
        )

    def play(self) -> None:
        """Emit the TinyFugue wrapper as a one-line bash program — the
        self-contained entry: `bash <(relate run dungeon
        --command play) <store.rl>` (process substitution keeps stdin
        free for the read loop; a plain pipe would collide with bash's
        own script reading). The report is the script and nothing
        else. Named `play` — what the player is doing — not
        `init`/`install`, which collide with the extension scaffold
        and package-install verbs."""
        self.report(SHELL_SCRIPT)

    def help(self) -> None:
        self.report(
            "dungeon — verbs: start (seed or resume + status), seed, "
            "status, inv, look [<target>], map, "
            "n/e/s/w/ne/nw/se/sw, move <dir>, take <item>, "
            "drop <item>, ask <npc> <question> | ask <npc> for "
            "<item>, give <item> to <npc>, give <npc> <item>, "
            "say <npc> <text>, help, play (print the bash runner). "
            "Directions: n, ne, e, se, s, sw, w, nw "
            "(or the full compass words)."
        )


# The short forms a player types to the full rel-type vocabulary.
SHORT_DIRS = {
    "n": "NORTH", "ne": "NORTH_EAST", "e": "EAST", "se": "SOUTH_EAST",
    "s": "SOUTH", "sw": "SOUTH_WEST", "w": "WEST", "nw": "NORTH_WEST",
    **{d.lower(): d for d in DIRECTIONS},
}


def run(client: Any, command: str) -> None:
    """Dispatch one command string to one verb."""
    parts = command.strip().split()
    verbs = Verbs(client)
    if not parts:
        verbs.status()
        return
    verb, args = parts[0].lower(), parts[1:]
    if verb == "start":
        verbs.start()
    elif verb == "play":
        verbs.play()
    elif verb == "seed":
        verbs.seed()
    elif verb == "status":
        verbs.status()
    elif verb == "map":
        verbs.report(verbs.render())
    elif verb == "move" and args:
        verbs.move(SHORT_DIRS.get(args[0].lower(), args[0].upper()))
    elif verb in SHORT_DIRS:
        # A bare direction is a move: `n`, `nw`, `north` …
        verbs.move(SHORT_DIRS[verb])
    elif verb == "take" or verb == "get":
        verbs.take(" ".join(args))
    elif verb == "drop":
        verbs.drop(" ".join(args))
    elif verb == "look" or verb == "l" or verb == "examine" or verb == "x":
        verbs.look_at(" ".join(args))
    elif verb == "ask":
        # ask <npc> <question> | ask <npc> for <item>
        if len(args) >= 2 and args[1].lower() == "for":
            verbs.ask_for(args[0], " ".join(args[2:]))
        elif args:
            verbs.say(args[0], " ".join(args[1:]))
        else:
            verbs.say("", "")
    elif verb == "say" or verb == "tell":
        if args:
            verbs.say(args[0], " ".join(args[1:]))
        else:
            verbs.say("", "")
    elif verb == "give":
        if len(args) >= 2:
            verbs.give(args[0], " ".join(args[1:]))
        else:
            verbs.give("", "")
    elif verb in ("inv", "inventory", "i"):
        verbs.inventory()
    elif verb == "help":
        verbs.help()
    else:
        verbs.report(
            f"'{command}' is not a dungeon verb — try `help`."
        )


def walk(client: Client, envelope: dict[str, Any]) -> None:
    """One command, one turn — the host resolved every manifest flag
    (here: `command`, default `status`) into the envelope."""
    run(client, envelope["command"])


def main() -> None:
    client = Client()
    walk(client, client.start())


if __name__ == "__main__":
    main()
