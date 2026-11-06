//! Unit tests: the pair codec, the compass table, the here-addendum
//! join, the renderer's lattice math, the layout's own consistency (the
//! sweep the Python iteration ran as a script, made a test so the keep
//! can never drift out of sync with its exits), and — through the
//! [`HostSession`] seam — the verbs themselves, driven by a recording
//! fake that scripts the host's read answers and records every call.

use crate::data::{
    decode_pairs, encode_pairs, graph_type, here_phrase, pair_value, Direction, LEGEND,
    NEWLINE_PLACEHOLDER, SHELL_SCRIPT,
};
use crate::game::HostSession;
use crate::keep::{EXITS, ITEMS, ITEM_LOCATION, NPCS, NPC_LOCATION, ROOMS, START_ROOM};
use crate::render::{render_map, MapEdge, Occupant, SeenRoom, NO_MAP};
use serde_json::{json, Value};
use std::collections::VecDeque;

// -- the recording fake -------------------------------------------------

/// A scripted host: `read_cypher` calls pop the queued results (a
/// read's rows, or a failure); every other tool answers `{}` without
/// consuming — the writes' counters and the report's acknowledgement
/// are not what the tests assert. Every call is recorded with its
/// arguments.
struct FakeHost {
    queued: VecDeque<Result<Value, String>>,
    calls: Vec<(String, Value)>,
}

impl FakeHost {
    fn new(queued: Vec<Result<Value, String>>) -> FakeHost {
        FakeHost {
            queued: queued.into(),
            calls: Vec::new(),
        }
    }

    /// Queue one read's rows as the `read_cypher` result.
    fn reads(rows: Vec<Value>) -> Result<Value, String> {
        Ok(json!({"rows": rows}))
    }

    fn tool(&self, tool: &str) -> Vec<&Value> {
        self.calls
            .iter()
            .filter(|(t, _)| t == tool)
            .map(|(_, a)| a)
            .collect()
    }
}

const READ_CYPHER: &str = "read_cypher";

impl HostSession for FakeHost {
    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, String> {
        self.calls.push((tool.to_string(), arguments.clone()));
        if tool != READ_CYPHER {
            return Ok(json!({}));
        }
        self.queued
            .pop_front()
            .unwrap_or_else(|| panic!("no scripted read for {arguments}"))
    }
}

fn report_text(host: &FakeHost) -> String {
    host.tool("report_output")[0]["text"]
        .as_str()
        .expect("the turn's text")
        .to_string()
}

// -- the vocabulary ------------------------------------------------------

#[test]
fn pair_codec_round_trips() {
    let encoded = encode_pairs(&[("play", "a tune"), ("unknown", "no answer")]);
    assert_eq!(encoded, "play=a tune|unknown=no answer");
    assert_eq!(
        decode_pairs(&encoded),
        vec![
            ("play".to_string(), "a tune".to_string()),
            ("unknown".to_string(), "no answer".to_string()),
        ]
    );
    // Absent key, empty text: the lookups the verbs rely on.
    assert_eq!(pair_value(&encoded, "hint"), None);
    assert!(decode_pairs("").is_empty());
    assert_eq!(pair_value("", "anything"), None);
}

#[test]
fn here_phrase_joins_only_for_its_holder() {
    let here = encode_pairs(&[("Armory", "on the bench, deliberately placed")]);
    assert_eq!(
        here_phrase(&here, "Armory"),
        ", on the bench, deliberately placed"
    );
    assert_eq!(here_phrase(&here, "Kitchen"), "");
}

#[test]
fn compass_one_table_derivations_agree() {
    // The rose is inverse-symmetric: the slot four around.
    for (i, dir) in Direction::ALL.iter().enumerate() {
        let opposite = Direction::ALL[(i + 4) % 8];
        let (dx, dy) = dir.delta();
        let (bx, by) = opposite.delta();
        assert_eq!((dx + bx, dy + by), (0, 0), "{dir:?}");
        // The word is the name's prose spelling.
        assert_eq!(
            dir.word(),
            dir.name().to_ascii_lowercase().replace('_', "-")
        );
        // The name resolves back through the closed vocabulary, from
        // both spellings and any case — the move verbs' word gate.
        assert_eq!(Direction::parse(dir.name()), Some(*dir));
        assert_eq!(Direction::parse(dir.short()), Some(*dir));
        assert_eq!(
            Direction::parse(&dir.name().to_ascii_lowercase()),
            Some(*dir)
        );
        assert_eq!(
            Direction::parse(&dir.short().to_ascii_uppercase()),
            Some(*dir)
        );
    }
    // The glyph is the delta's axis.
    assert_eq!(Direction::North.glyph(), '|');
    assert_eq!(Direction::East.glyph(), '-');
    assert_eq!(Direction::NorthEast.glyph(), '/');
    assert_eq!(Direction::NorthWest.glyph(), '\\');
    assert_eq!(Direction::parse("up"), None);
    assert_eq!(Direction::parse(""), None);
}

#[test]
fn graph_type_declares_every_compass_rel() {
    let text = graph_type();
    for dir in Direction::ALL {
        assert!(
            text.contains(&format!("(:Room)-[:{}]->(:Room)", dir.name())),
            "the graph type is missing the {} signature",
            dir.name()
        );
    }
    assert!(text.contains("(:Item)-[:IN]->(:Room)"));
}

/// The emitted `play` script and the shipped `dungeon-shell` wrapper
/// are two spellings of one protocol — the drift guard asserts the
/// load-bearing fragments appear in both, so a change to one without
/// the other fails here.
#[test]
fn shell_script_and_shipped_wrapper_agree() {
    let shipped = include_str!("../reference/dungeon-shell");
    for fragment in [
        "${out//%%NL%%/$'\\n'}",
        "relate run dungeon -d \"$STORE\" --command \"$1\"",
        "history -r \"$HIST\"",
        "history -s \"$line\"",
        "set +o history",
    ] {
        assert!(
            SHELL_SCRIPT.contains(fragment),
            "the emitted script lost the fragment: {fragment}"
        );
        assert!(
            shipped.contains(fragment),
            "the shipped wrapper lost the fragment: {fragment}"
        );
    }
}

#[test]
fn shell_script_is_one_line_and_carries_the_placeholder_restore() {
    assert!(!SHELL_SCRIPT.contains('\n'), "the sanitizer flattens it");
    assert!(SHELL_SCRIPT.contains(NEWLINE_PLACEHOLDER));
    assert!(SHELL_SCRIPT.contains("relate run dungeon"));
}

#[test]
fn legend_names_every_glyph_the_renderer_draws() {
    for glyph in ['@', 'O', '#', '·', '-', '|', '/', '\\'] {
        assert!(LEGEND.contains(glyph), "legend is missing {glyph:?}");
    }
}

/// The layout sweep: every exit joins adjacent cells, every passage is
/// declared both ways, coordinates are unique, every placement resolves
/// to a real room or character, and the keep is fully reachable from
/// the start room.
#[test]
fn layout_is_consistent() {
    use std::collections::HashMap;
    let mut by_name: HashMap<&str, (i32, i32)> = HashMap::new();
    let mut by_xy: HashMap<(i32, i32), &str> = HashMap::new();
    for r in ROOMS {
        assert!(
            by_xy.insert((r.x, r.y), r.name).is_none(),
            "two rooms share ({}, {})",
            r.x,
            r.y
        );
        by_name.insert(r.name, (r.x, r.y));
    }
    let legs: Vec<(&str, Direction, &str)> = EXITS.iter().map(|e| (e.from, e.dir, e.to)).collect();
    for exit in EXITS {
        let (ax, ay) = by_name[exit.from];
        let (bx, by) = by_name[exit.to];
        let (dx, dy) = exit.dir.delta();
        assert_eq!(
            (ax + dx, ay + dy),
            (bx, by),
            "{} -{}-> {} is not adjacent",
            exit.from,
            exit.dir.name(),
            exit.to
        );
        let opposite =
            Direction::ALL[(Direction::ALL.iter().position(|d| *d == exit.dir).unwrap() + 4) % 8];
        assert!(
            legs.contains(&(exit.to, opposite, exit.from)),
            "missing the inverse leg {} -{}-> {}",
            exit.to,
            opposite.name(),
            exit.from
        );
    }
    for (npc, room_name) in NPC_LOCATION {
        assert!(NPCS.iter().any(|n| n.name == *npc), "unknown npc {npc}");
        assert!(by_name.contains_key(*room_name), "unknown room {room_name}");
    }
    for (item, holder) in ITEM_LOCATION {
        assert!(ITEMS.iter().any(|i| i.name == *item), "unknown item {item}");
        assert!(
            by_name.contains_key(*holder) || NPCS.iter().any(|n| n.name == *holder),
            "item {item} held by unknown {holder}"
        );
    }
    for item in ITEMS {
        for (holder, _) in item.here {
            assert!(
                by_name.contains_key(*holder) || NPCS.iter().any(|n| n.name == *holder),
                "{}'s here-phrase holder {holder} is unknown",
                item.name
            );
        }
    }
    // Reachable from the start room: the keep has no orphan wing.
    let mut seen: Vec<&str> = vec![START_ROOM];
    let mut frontier = vec![START_ROOM];
    while let Some(name) = frontier.pop() {
        for exit in EXITS {
            if exit.from == name && !seen.contains(&exit.to) {
                seen.push(exit.to);
                frontier.push(exit.to);
            }
        }
    }
    for r in ROOMS {
        assert!(seen.contains(&r.name), "{} is unreachable", r.name);
    }
}

// -- the renderer ---------------------------------------------------------

#[test]
fn render_map_windows_and_grows_with_exploration() {
    let player = (0, 0);
    let seen = vec![SeenRoom {
        x: 0,
        y: 0,
        lit: true,
    }];
    let occupants = vec![Occupant {
        x: 0,
        y: -1,
        name: "Bard".to_string(),
    }];
    // The player's own room plus an unvisited neighbour in every
    // direction: the connectors draw, the cells stay void.
    let edges = Direction::ALL
        .iter()
        .map(|d| {
            let (dx, dy) = d.delta();
            MapEdge {
                ax: 0,
                ay: 0,
                bx: dx,
                by: dy,
                dir: *d,
            }
        })
        .collect::<Vec<_>>();
    let drawn = render_map(player, &seen, &occupants, &edges);
    let rows: Vec<&str> = drawn.split('\n').collect();
    // A ±2-cell window is a 5x5 lattice: 2*5-1 rows and columns.
    assert_eq!(rows.len(), 9, "a ±2 window is a 9-row lattice");
    assert_eq!(
        rows[4], "· · · - @ - · · ·",
        "west/east connectors into the void, player at centre"
    );
    assert_eq!(
        rows[3], "· · · \\ | / · · ·",
        "north and both diagonals reach into the void"
    );
    // The occupant one cell north of the player, once visited.
    let seen = vec![
        SeenRoom {
            x: 0,
            y: 0,
            lit: true,
        },
        SeenRoom {
            x: 0,
            y: -1,
            lit: true,
        },
    ];
    let drawn = render_map(player, &seen, &occupants, &edges);
    let rows: Vec<&str> = drawn.split('\n').collect();
    assert_eq!(
        rows[2], "· · · · B · · · ·",
        "an occupant draws its initial"
    );
    // A dark room draws the dark glyph (not at the player's own cell —
    // the player glyph overrides); empty exploration is the no-map turn.
    let seen = vec![
        SeenRoom {
            x: 0,
            y: 0,
            lit: true,
        },
        SeenRoom {
            x: 1,
            y: 0,
            lit: false,
        },
    ];
    let drawn = render_map(player, &seen, &occupants, &edges);
    assert!(drawn.contains('#'));
    assert_eq!(render_map(player, &[], &occupants, &edges), NO_MAP);
    // A room outside the window is not drawn even when visited.
    let far = vec![SeenRoom {
        x: 9,
        y: 9,
        lit: true,
    }];
    assert_eq!(render_map(player, &far, &occupants, &[]), NO_MAP);
}

// -- the verbs, over the fake host ---------------------------------------

/// The seed is ONE batch: the graph type first on a fresh store, exits
/// MERGEd (never CREATE), the lock edges derived from Item.unlocks, and
/// the player MERGEd unconditionally.
#[test]
fn seed_is_one_atomic_batch() {
    let mut host = FakeHost::new(vec![
        // the freshness probe: no rooms
        FakeHost::reads(vec![]),
        // render()'s reads (player, seen, occupants, edges)
        FakeHost::reads(vec![json!({"x": 0, "y": 0})]),
        FakeHost::reads(vec![
            json!({"x": 0, "y": 0, "lit": true, "name": "Entrance"}),
        ]),
        FakeHost::reads(vec![]),
        FakeHost::reads(vec![]),
        // room_text's reads (room, npcs, items, exits)
        FakeHost::reads(vec![json!({"room": "Entrance", "long": "the steps"})]),
        FakeHost::reads(vec![]),
        FakeHost::reads(vec![]),
        FakeHost::reads(vec![json!({"dir": "NORTH"})]),
    ]);
    crate::run(&mut host, "seed").expect("the seed turn");
    let batches = host.tool("batch_cypher");
    assert_eq!(batches.len(), 1, "the catalog is one batch");
    let statements = batches[0]["statements"]
        .as_array()
        .expect("the batch's statements");
    let texts: Vec<&str> = statements
        .iter()
        .map(|s| {
            s["statement"]
                .as_str()
                .expect("every batch statement carries its text")
        })
        .collect();
    assert!(texts[0].contains("CREATE GRAPH TYPE"));
    for text in &texts {
        if text.contains("MERGE (a)-[:") {
            assert!(!text.contains("CREATE (a)-[:"), "exits are MERGEd");
        }
    }
    // The lock edges derive from the one field, and the player MERGE is
    // present on a fresh store.
    assert!(texts
        .iter()
        .any(|t| t.contains("MERGE (i)-[:UNLOCKS]->(r)")));
    let player_write = texts
        .iter()
        .find(|t| t.contains("MERGE (p:Player"))
        .expect("the player write");
    // The LOCATION placement is guarded — an unguarded MERGE would hand
    // a resumed player (standing anywhere) a second LOCATION edge to
    // the start room on every session start.
    assert!(
        player_write.contains("WHERE NOT (p)-[:LOCATION]->()"),
        "the player placement must be guarded: {player_write}"
    );
}

/// A locked way with the key not held: the three plain reads (LABS-888
/// workaround), no write, the locked answer naming the direction and
/// the room.
#[test]
fn locked_way_refuses_without_the_key() {
    let mut host = FakeHost::new(vec![
        // the exit read: the Cellar north of the Throne Room
        FakeHost::reads(vec![json!({"next": "Cellar", "long": "the lair"})]),
        // the lock count: one key unlocks it
        FakeHost::reads(vec![json!({"locks": 1})]),
        // the held count: not carried
        FakeHost::reads(vec![json!({"held": 0})]),
    ]);
    crate::run(&mut host, "n").expect("the move turn");
    assert!(host.tool("write_cypher").is_empty(), "no move happened");
    let text = report_text(&host);
    assert_eq!(text, "The way north is locked (Cellar). You need the key.");
}

/// The report is the one newline-encoding boundary: the turn's text is
/// built with ordinary newlines and leaves as the placeholder.
#[test]
fn report_is_the_single_newline_boundary() {
    let mut host = FakeHost::new(vec![
        // the exit read: an open room
        FakeHost::reads(vec![json!({"next": "Steps", "long": "the climb"})]),
        // the lock count: nothing locks it
        FakeHost::reads(vec![json!({"locks": 0})]),
        // the held count
        FakeHost::reads(vec![json!({"held": 0})]),
        // render()'s reads
        FakeHost::reads(vec![json!({"x": 0, "y": -1})]),
        FakeHost::reads(vec![json!({"x": 0, "y": -1, "lit": true})]),
        FakeHost::reads(vec![]),
        FakeHost::reads(vec![]),
        // room_text's reads
        FakeHost::reads(vec![json!({"room": "Steps", "long": "the climb"})]),
        FakeHost::reads(vec![]),
        FakeHost::reads(vec![]),
        FakeHost::reads(vec![json!({"dir": "SOUTH"})]),
    ]);
    crate::run(&mut host, "n").expect("the move turn");
    // The move write happened...
    assert_eq!(host.tool("write_cypher").len(), 1);
    // ...and the reported text carries placeholders, never raw
    // newlines.
    let text = report_text(&host);
    assert!(!text.contains('\n'), "the host flattens newlines");
    assert!(text.contains(NEWLINE_PLACEHOLDER));
    assert!(text.contains("You go north."));
    assert!(text.contains("[Steps]"));
    // The direction word is the prose spelling.
    assert!(text.contains("exits: s"));
}

/// An unknown topic gets the NPC's in-voice deflection, never a topic
/// list and never the general answer pretending the topic landed.
#[test]
fn unknown_topic_gets_the_in_voice_deflection() {
    let chat = crate::data::encode_pairs(&[
        ("ask", "the general answer"),
        ("weather", "the forecast"),
        ("unknown", "The deflection."),
    ]);
    let mut host = FakeHost::new(vec![FakeHost::reads(vec![
        json!({"name": "Bard", "chat": chat}),
    ])]);
    crate::run(&mut host, "ask bard drizzle").expect("the ask turn");
    assert_eq!(report_text(&host), "Bard: The deflection.");
    // No topic at all: the general answer.
    let mut host = FakeHost::new(vec![FakeHost::reads(vec![
        json!({"name": "Bard", "chat": chat}),
    ])]);
    crate::run(&mut host, "ask bard").expect("the ask turn");
    assert_eq!(report_text(&host), "Bard: the general answer");
    // A capitalized topic reaches the answer — the keyword is
    // lowercased like every other player-word match.
    let mut host = FakeHost::new(vec![FakeHost::reads(vec![
        json!({"name": "Bard", "chat": chat}),
    ])]);
    crate::run(&mut host, "ask bard Weather").expect("the ask turn");
    assert_eq!(report_text(&host), "Bard: the forecast");
}

/// The give dispatcher owns both prepositional forms and the
/// missing-argument turn: `give key to dragon` swaps, `give dragon key`
/// does not, and a bare `give` asks.
#[test]
fn give_parsing_owns_both_forms() {
    let scripted = || {
        FakeHost::new(vec![FakeHost::reads(vec![
            json!({"npc": "Dragon", "name": "Key"}),
        ])])
    };
    let mut host = scripted();
    crate::run(&mut host, "give key to dragon").expect("the give turn");
    let give_args = &host.tool("write_cypher")[0]["parameters"];
    assert_eq!(give_args["npc"], "dragon");
    assert_eq!(give_args["item"], "key");

    let mut host = scripted();
    crate::run(&mut host, "give dragon key").expect("the give turn");
    let give_args = &host.tool("write_cypher")[0]["parameters"];
    assert_eq!(give_args["npc"], "dragon");
    assert_eq!(give_args["item"], "key");

    // The relocation statement MATCHes both endpoints before the
    // DELETE — an endpoint that first appears in the WITH is undefined
    // (the engine rejects it; the ask_for shape would bind a pattern
    // variable by accident instead).
    for call in host.tool("write_cypher") {
        let stmt = call["statement"].as_str().expect("the statement");
        assert!(
            stmt.contains("MATCH (n:Npc) WHERE toLower(n.name)"),
            "the relocation must MATCH the NPC: {stmt}"
        );
    }

    let mut host = FakeHost::new(vec![]);
    crate::run(&mut host, "give").expect("the missing-arg turn");
    assert_eq!(report_text(&host), "Give what to whom?");
    assert!(host.tool("write_cypher").is_empty());
}

/// `move <word>` with an off-compass word is the one not-a-compass
/// answer; a bare direction word is the same move.
#[test]
fn move_forms_share_one_policy() {
    let mut host = FakeHost::new(vec![]);
    crate::run(&mut host, "move up").expect("the compass-error turn");
    assert_eq!(
        report_text(&host),
        "'up' is not a compass direction. Try one of: n ne e se s sw w nw"
    );
    assert!(host.tool("write_cypher").is_empty());
}

/// A malformed row fails the turn loudly — the typed cells never
/// surface as a default the player reads as content.
#[test]
fn a_malformed_row_fails_loudly() {
    let mut host = FakeHost::new(vec![
        // the exit read: the target row without its name
        FakeHost::reads(vec![json!({"long": "a room with no name"})]),
    ]);
    let err = crate::run(&mut host, "n").expect_err("the malformed row");
    assert!(err.contains("no string 'next'"), "{err}");
}

/// A present-but-not-takeable item is its own answer in ask_for — the
/// distinction take keeps, restored from the reference child.
#[test]
fn ask_for_keeps_the_takeable_distinction() {
    let mut host = FakeHost::new(vec![FakeHost::reads(vec![
        json!({"npc": "Dragon", "name": "Chest", "takeable": false}),
    ])]);
    crate::run(&mut host, "ask dragon for chest").expect("the ask_for turn");
    assert_eq!(report_text(&host), "Dragon will not part with the chest.");
    assert!(host.tool("write_cypher").is_empty());
}
