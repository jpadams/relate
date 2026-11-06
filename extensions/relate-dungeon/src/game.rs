//! The verbs: one method per player verb, each a handful of
//! `read_cypher`/`write_cypher`/`batch_cypher` calls — all game state
//! is the host's store, the child holds none. The verbs run over the
//! [`HostSession`] seam (the three tool calls they make), so
//! `src/tests.rs` drives them with a recording fake; [`StdioChild`]
//! is the real implementation. Every statement is ported verbatim from
//! the Python iteration child (`reference/dungeon.py`), which was
//! tested statement-for-statement against the live engine — including
//! the shapes that only empirically pass the read gate (LABS-888).

use crate::data::{
    encode_pairs, graph_type, here_phrase, pair_value, Direction, INTRO, LEGEND,
    NEWLINE_PLACEHOLDER, PLAYER, SHELL_SCRIPT,
};
use crate::keep::{
    Exit, Item, Npc, Room, EXITS, ITEMS, ITEM_LOCATION, NPCS, NPC_LOCATION, ROOMS, START_ROOM,
};
use crate::render::{render_map, MapEdge, Occupant, SeenRoom, NO_MAP};
use crate::rows::{bool_cell, cell, coords, direction_cell, int_cell};
use relate_wire::mcp::{
    McpChild, StdioChild, BATCH_CYPHER, READ_CYPHER, REPORT_OUTPUT, WRITE_CYPHER,
};
use serde_json::{json, Value};

/// The four tool calls the verbs make — `read_cypher`, `write_cypher`,
/// `batch_cypher`, `report_output` — the seam that keeps the verb logic
/// drivable from tests by a recording fake. The real implementation is
/// [`StdioChild`]; the fake lives in `src/tests.rs`.
pub trait HostSession {
    /// One `tools/call`; returns the tool's result JSON.
    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, String>;
}

impl HostSession for StdioChild {
    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, String> {
        // The inherent `McpChild::call` — spelled as such because the
        // trait method shares the name. The wire kit's
        // `From<McpChildError> for String` carries the failure out; no
        // per-call-site adapter.
        let result = McpChild::call(self, tool, arguments)?;
        Ok(result)
    }
}

/// The `tools/call` argument envelope every statement-carrying call
/// builds — spelled once.
fn statement_args(statement: &str, parameters: Value) -> Value {
    json!({"statement": statement, "parameters": parameters})
}

/// The game over a host session: every verb's reads and writes go
/// through the session; the report is the turn's text.
pub(crate) struct Game<'a, S: HostSession> {
    session: &'a mut S,
}

impl<S: HostSession> Game<'_, S> {
    pub(crate) fn new(session: &mut S) -> Game<'_, S> {
        Game { session }
    }

    fn read(&mut self, statement: &str, parameters: Value) -> Result<Vec<Value>, String> {
        let result = self
            .session
            .call(READ_CYPHER, statement_args(statement, parameters))?;
        result
            .get("rows")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| "read_cypher answered no rows".to_string())
    }

    fn write(&mut self, statement: &str, parameters: Value) -> Result<(), String> {
        self.session
            .call(WRITE_CYPHER, statement_args(statement, parameters))?;
        Ok(())
    }

    /// The turn's text — the ONE newline-encoding boundary: the host's
    /// print_summary flattens newlines (ADR-0430 §7), so everything the
    /// verbs build uses ordinary `\n` and this method alone carries the
    /// placeholder the `play` wrapper restores (LABS-900). The `play`
    /// verb's script needs no encoding: it is one physical line.
    pub(crate) fn report(&mut self, text: &str) -> Result<(), String> {
        let flat = text.replace('\n', NEWLINE_PLACEHOLDER);
        self.session
            .call(REPORT_OUTPUT, json!({"text": flat, "starters": []}))?;
        Ok(())
    }

    /// Detach `item` from `from` and attach it to `to` — the one
    /// relocation statement behind drop, ask_for and give; each verb
    /// keeps its own guard and message. `WITH DISTINCT i, <to>` carries
    /// the endpoint past the DELETE (an endpoint that falls out of
    /// scope is silently accepted by CREATE and builds a null edge,
    /// LABS-891) and collapses duplicated legacy IN edges so no
    /// parallel edge appears. `from`/`to` are internal variable names
    /// bound by `context`, never player text.
    fn reattach(
        &mut self,
        context: &str,
        from: &str,
        to: &str,
        parameters: Value,
    ) -> Result<(), String> {
        self.write(
            &format!(
                "{context} \
                 MATCH (i:Item) WHERE toLower(i.name) = toLower($item) \
                 MATCH (i)-[old:IN]->({from}) \
                 DELETE old WITH DISTINCT i, {to} CREATE (i)-[:IN]->({to})"
            ),
            parameters,
        )
    }

    // -- the seed and the session start ---------------------------------

    /// The catalog writes, ONE `batch_cypher` call: under the
    /// manifest's per-batch commit policy each `write_cypher` is its
    /// own transaction, so a mid-seed failure would commit a partial
    /// catalog — the batch is all-or-nothing. Returns whether the
    /// store was fresh (no rooms yet), so the caller can show the
    /// one-time intro.
    fn seed_catalog(&mut self) -> Result<bool, String> {
        let fresh = self
            .read("MATCH (r:Room) RETURN r.name AS name LIMIT 1", json!({}))?
            .is_empty();
        let mut statements: Vec<Value> = Vec::new();
        let mut push = |statement: String, parameters: Value| {
            statements.push(statement_args(&statement, parameters));
        };
        if fresh {
            // CREATE alone: it makes the type current; ALTER is
            // LABS-887's broken path and declares nothing CREATE did
            // not. The compass signatures come from the one table.
            push(graph_type(), json!({}));
        }
        push(
            r#"UNWIND $rooms AS r
               MERGE (x:Room {name: r.name})
               SET x.short = r.short, x.long = r.long, x.lit = r.lit,
                   x.zone = r.zone, x.x = r.x, x.y = r.y"#
                .to_string(),
            json!({"rooms": ROOMS.iter().map(|r: &Room| {
                json!({
                    "name": r.name, "short": r.short, "long": r.long,
                    "lit": r.lit, "zone": r.zone, "x": r.x, "y": r.y,
                })
            }).collect::<Vec<_>>()}),
        );
        // The one-time intro is room data: stored at seed, read back by
        // `start` on the first open.
        push(
            "MATCH (r:Room {name: $room}) SET r.intro = $intro".to_string(),
            json!({"room": START_ROOM, "intro": INTRO}),
        );
        // One statement per direction type: the type is the statement's
        // text (the closed vocabulary), the endpoints are parameters.
        // MERGE, never CREATE — a re-seeded store must not gain
        // parallel exit edges.
        for dir in Direction::ALL {
            let legs: Vec<Value> = EXITS
                .iter()
                .filter(|e: &&Exit| e.dir == dir)
                .map(|e| json!({"from": e.from, "to": e.to}))
                .collect();
            if legs.is_empty() {
                continue;
            }
            push(
                format!(
                    "UNWIND $exits AS e MATCH (a:Room {{name: e.from}}), \
                     (b:Room {{name: e.to}}) MERGE (a)-[:{}]->(b)",
                    dir.name()
                ),
                json!({"exits": legs}),
            );
        }
        push(
            r#"UNWIND $npcs AS n
               MERGE (x:Npc {name: n.name})
               SET x.short = n.short, x.long = n.long, x.race = n.race,
                   x.hp = n.hp, x.xp = n.xp, x.chat = n.chat"#
                .to_string(),
            json!({"npcs": NPCS.iter().map(|n: &Npc| {
                json!({
                    "name": n.name, "short": n.short, "long": n.long,
                    "race": n.race, "hp": n.hp, "xp": n.xp,
                    // The topics are the `chat` pair keys — one
                    // encoding (the crate's pair codec), no second
                    // `commands` spelling to drift against it.
                    "chat": encode_pairs(n.chat),
                })
            }).collect::<Vec<_>>()}),
        );
        // Placements are guarded (only unplaced NPCs/items) so a
        // re-seed never teleports a moved one back to its origin.
        push(
            r#"UNWIND $loc AS l
               MATCH (n:Npc {name: l.npc})
               WHERE NOT (n)-[:LOCATION]->()
               MATCH (r:Room {name: l.room})
               MERGE (n)-[:LOCATION]->(r)"#
                .to_string(),
            json!({"loc": NPC_LOCATION.iter().map(|(npc, room)| {
                json!({"npc": npc, "room": room})
            }).collect::<Vec<_>>()}),
        );
        push(
            r#"UNWIND $items AS i
               MERGE (x:Item {name: i.name})
               SET x.short = i.short, x.long = i.long,
                   x.takeable = i.takeable, x.unlocks = i.unlocks,
                   x.weight = i.weight, x.here = i.here"#
                .to_string(),
            json!({"items": ITEMS.iter().map(|i: &Item| {
                json!({
                    "name": i.name, "short": i.short, "long": i.long,
                    "takeable": i.takeable, "unlocks": i.unlocks,
                    "weight": i.weight, "here": encode_pairs(i.here),
                })
            }).collect::<Vec<_>>()}),
        );
        push(
            r#"UNWIND $loc AS l
               MATCH (i:Item {name: l.item})
               WHERE NOT (i)-[:IN]->()
               MATCH (holder {name: l.holder})
               MERGE (i)-[:IN]->(holder)"#
                .to_string(),
            json!({"loc": ITEM_LOCATION.iter().map(|(item, holder)| {
                json!({"item": item, "holder": holder})
            }).collect::<Vec<_>>()}),
        );
        // The lock edges derive from Item.unlocks — the one place the
        // lock fact lives (the property the same field writes and the
        // edge the move verb checks cannot drift).
        let locks: Vec<Value> = ITEMS
            .iter()
            .filter_map(|i| i.unlocks.map(|room| json!({"item": i.name, "room": room})))
            .collect();
        push(
            r#"UNWIND $unlocks AS u
               MATCH (i:Item {name: u.item}), (r:Room {name: u.room})
               MERGE (i)-[:UNLOCKS]->(r)"#
                .to_string(),
            json!({"unlocks": locks}),
        );
        // The player is MERGEd unconditionally (idempotent): a store
        // whose seed was interrupted before the player write still
        // gains one — and the hp/xp reset keys on the player's own
        // creation (ON CREATE), not on room emptiness, so the healed
        // player starts fresh too. The LOCATION placement is guarded
        // like every other placement — only when unplaced — because an
        // unguarded MERGE would hand a resumed player a second
        // LOCATION edge to the start room.
        push(
            "MERGE (p:Player {name: $name}) \
             ON CREATE SET p.hp = 10, p.xp = 0 \
             WITH p MATCH (r:Room {name: $room}) \
             SET r.visited = true \
             WITH p, r WHERE NOT (p)-[:LOCATION]->() \
             MERGE (p)-[:LOCATION]->(r)"
                .to_string(),
            json!({"name": PLAYER, "room": START_ROOM}),
        );
        self.session
            .call(BATCH_CYPHER, json!({"statements": statements}))?;
        Ok(fresh)
    }

    /// The seed verb: one catalog write, then the turn's text.
    pub(crate) fn seed(&mut self) -> Result<(), String> {
        let fresh = self.seed_catalog()?;
        let text = if fresh {
            format!(
                "Seeded the starter dungeon ({} rooms, {} NPCs, {} items).\n{}\n{}",
                ROOMS.len(),
                NPCS.len(),
                ITEMS.len(),
                self.render()?,
                self.room_text()?
            )
        } else {
            format!(
                "The dungeon is already seeded — descs and chat refreshed to \
                 the current version.\n{}\n{}",
                self.render()?,
                self.room_text()?
            )
        };
        self.report(&text)
    }

    /// The wrapper's one startup call: seed-or-refresh quietly, then
    /// the status turn with the legend above the map. A fresh store
    /// first gets the intro, read back from the start room where the
    /// seed stored it: the text is room data.
    pub(crate) fn start(&mut self) -> Result<(), String> {
        let fresh = self.seed_catalog()?;
        let mut text = format!("{LEGEND}\n{}", self.status_text()?);
        if fresh {
            let rows = self.read(
                "MATCH (r:Room {name: $room}) RETURN r.intro AS intro",
                json!({"room": START_ROOM}),
            )?;
            if let Some(intro) = rows.first().map(|r| cell(r, "intro")).transpose()? {
                text = format!("{intro}\n\n{text}");
            }
        }
        self.report(&text)
    }

    // -- the status turn -------------------------------------------------

    fn status_text(&mut self) -> Result<String, String> {
        let stats = self.read(
            "MATCH (p:Player {name: $player}) \
             OPTIONAL MATCH (i:Item)-[:IN]->(p) \
             RETURN p.name AS name, p.hp AS hp, p.xp AS xp, \
             collect(i.name) AS inventory",
            json!({"player": PLAYER}),
        )?;
        let Some(s) = stats.first() else {
            return Ok("No player — run `seed` first.".to_string());
        };
        let carried = s["inventory"]
            .as_array()
            .ok_or_else(|| format!("the status row answered no inventory array: {s}"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("a non-string inventory entry: {v}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let inv = if carried.is_empty() {
            "nothing".to_string()
        } else {
            carried.join(", ")
        };
        Ok(format!(
            "{} — hp {}, xp {}, carrying: {inv}\n{}\n{}",
            cell(s, "name")?,
            int_cell(s, "hp")?,
            int_cell(s, "xp")?,
            self.render()?,
            self.room_text()?
        ))
    }

    pub(crate) fn status(&mut self) -> Result<(), String> {
        let text = self.status_text()?;
        self.report(&text)
    }

    /// The room's long desc, then its contents with short descs —
    /// NPCs first, then items (plain reads; OPTIONAL MATCH is
    /// read-gated in the shapes the lock check needs, LABS-888).
    fn room_text(&mut self) -> Result<String, String> {
        let rows = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             RETURN r.name AS room, r.long AS long",
            json!({"player": PLAYER}),
        )?;
        let Some(row) = rows.first() else {
            return Ok("No player in the store — run `seed` first.".to_string());
        };
        let room = cell(row, "room")?.to_string();
        let mut lines = vec![format!("[{room}] {}", cell(row, "long")?)];
        let npcs = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             MATCH (n:Npc)-[:LOCATION]->(r) RETURN n.name AS name",
            json!({"player": PLAYER}),
        )?;
        if !npcs.is_empty() {
            let names: Vec<&str> = npcs
                .iter()
                .map(|n| cell(n, "name"))
                .collect::<Result<_, _>>()?;
            lines.push(format!("here: {}", names.join(", ")));
        }
        let items = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             MATCH (i:Item)-[:IN]->(r) \
             RETURN i.name AS name, i.short AS short, i.here AS here",
            json!({"player": PLAYER}),
        )?;
        if !items.is_empty() {
            let floor: Vec<String> = items
                .iter()
                .map(|i| {
                    Ok(format!(
                        "{} ({}){}",
                        cell(i, "name")?,
                        cell(i, "short")?,
                        here_phrase(cell(i, "here")?, &room)
                    ))
                })
                .collect::<Result<_, String>>()?;
            lines.push(format!("on the floor: {}", floor.join(", ")));
        }
        let exits = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             MATCH (r)-[rel]->(next:Room) \
             RETURN DISTINCT type(rel) AS dir",
            json!({"player": PLAYER}),
        )?;
        if !exits.is_empty() {
            // Compass-rose order starting at n (Direction::ALL's order).
            let mut rose: Vec<Direction> = exits
                .iter()
                .map(|e| direction_cell(e, "dir"))
                .collect::<Result<_, _>>()?;
            rose.sort_by_key(|d| Direction::ALL.iter().position(|a| a == d));
            let words: Vec<&str> = rose.iter().map(|d| d.short()).collect();
            lines.push(format!("exits: {}", words.join(", ")));
        }
        Ok(lines.join("\n"))
    }

    /// The explored map: gather the window's inputs with reads, draw
    /// with the pure renderer. An unseeded store is the renderer's own
    /// turn text.
    pub(crate) fn render(&mut self) -> Result<String, String> {
        let player = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             RETURN r.x AS x, r.y AS y",
            json!({"player": PLAYER}),
        )?;
        let Some(p) = player.first() else {
            return Ok(NO_MAP.to_string());
        };
        let (px, py) = coords(p, "x", "y")?;
        let seen: Vec<SeenRoom> = self
            .read(
                "MATCH (r:Room) WHERE r.visited \
                 RETURN r.name AS name, r.x AS x, r.y AS y, r.lit AS lit",
                json!({}),
            )?
            .iter()
            .map(|r| {
                let (x, y) = coords(r, "x", "y")?;
                Ok(SeenRoom {
                    x,
                    y,
                    lit: bool_cell(r, "lit")?,
                })
            })
            .collect::<Result<_, String>>()?;
        let occupants: Vec<Occupant> = self
            .read(
                "MATCH (n:Npc)-[:LOCATION]->(room) WHERE room.visited \
                 RETURN room.x AS x, room.y AS y, n.name AS npc",
                json!({}),
            )?
            .iter()
            .map(|r| {
                let name = cell(r, "npc")?;
                if name.is_empty() {
                    return Err(format!("an occupant row with no name: {r}"));
                }
                let (x, y) = coords(r, "x", "y")?;
                Ok(Occupant {
                    x,
                    y,
                    name: name.to_string(),
                })
            })
            .collect::<Result<_, String>>()?;
        let edges: Vec<MapEdge> = self
            .read(
                "MATCH (a:Room)-[rel]->(b:Room) RETURN a.x AS ax, \
                 a.y AS ay, type(rel) AS t, b.x AS bx, b.y AS by",
                json!({}),
            )?
            .iter()
            .map(|e| {
                let (ax, ay) = coords(e, "ax", "ay")?;
                let (bx, by) = coords(e, "bx", "by")?;
                Ok(MapEdge {
                    ax,
                    ay,
                    bx,
                    by,
                    dir: direction_cell(e, "t")?,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(render_map((px, py), &seen, &occupants, &edges))
    }

    // -- the play --------------------------------------------------------

    pub(crate) fn mv(&mut self, dir: Direction) -> Result<(), String> {
        // LABS-888 (OPTIONAL MATCH misclassified non-Read in this
        // shape) forces three plain reads instead of one OPTIONAL
        // MATCH: the exit, whether the target is locked, and whether
        // the key is held. Collapse to the single OPTIONAL MATCH
        // statement when it lands.
        let target = self.read(
            &format!(
                "MATCH (p:Player {{name: $player}})-[:LOCATION]->(here) \
                 MATCH (here)-[:{}]->(next) \
                 RETURN next.name AS next, next.long AS long",
                dir.name()
            ),
            json!({"player": PLAYER}),
        )?;
        let Some(t) = target.first() else {
            return self.report("There is no exit that way.");
        };
        let room = cell(t, "next")?.to_string();
        let lock = self.read(
            "MATCH (k:Item)-[:UNLOCKS]->(locked:Room {name: $room}) \
             RETURN count(k) AS locks",
            json!({"room": room}),
        )?;
        let held = self.read(
            "MATCH (k:Item)-[:UNLOCKS]->(:Room {name: $room}) \
             MATCH (k)-[:IN]->(p:Player {name: $player}) \
             RETURN count(k) AS held",
            json!({"room": room, "player": PLAYER}),
        )?;
        // count() always answers exactly one row: an absent row is a
        // malformed host answer and fails the turn — never read
        // absence as permission to move.
        let locks = int_cell(
            lock.first().ok_or("the lock read answered no rows")?,
            "locks",
        )?;
        let held = int_cell(
            held.first().ok_or("the held read answered no rows")?,
            "held",
        )?;
        if locks > 0 && held == 0 {
            return self.report(&format!(
                "The way {} is locked ({room}). You need the key.",
                dir.word()
            ));
        }
        // The move is a write: one statement, one transaction — the
        // location edge is rewritten atomically and the arrival room
        // marked visited.
        self.write(
            &format!(
                "MATCH (p:Player {{name: $player}})-[l:LOCATION]->(here) \
                 MATCH (here)-[:{}]->(next) \
                 DELETE l CREATE (p)-[:LOCATION]->(next) \
                 SET next.visited = true",
                dir.name()
            ),
            json!({"player": PLAYER}),
        )?;
        let text = format!(
            "You go {}.\n{}\n{}",
            dir.word(),
            self.render()?,
            self.room_text()?
        );
        self.report(&text)
    }

    pub(crate) fn take(&mut self, item: &str) -> Result<(), String> {
        if item.is_empty() {
            return self.report("Take what?");
        }
        let rows = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             MATCH (i:Item) WHERE toLower(i.name) = toLower($item) \
             MATCH (i)-[old:IN]->(holder) \
             WHERE holder = r OR holder = p OR (holder)-[:LOCATION]->(r) \
             RETURN i.takeable AS takeable, i.name AS name",
            json!({"player": PLAYER, "item": item}),
        )?;
        let Some(row) = rows.first() else {
            return self.report(&format!("There is no {item} here."));
        };
        if !bool_cell(row, "takeable")? {
            return self.report(&format!("The {item} cannot be taken."));
        }
        // The correlated multi-holder relocation (the item may sit in
        // the room, the pack, or an occupant's claw) — the same WITH
        // DISTINCT rationale as `reattach`, spelled inline because the
        // endpoint guard is this verb's own.
        self.write(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             MATCH (i:Item) WHERE toLower(i.name) = toLower($item) \
             MATCH (i)-[old:IN]->(holder) \
             WHERE holder = r OR holder = p OR (holder)-[:LOCATION]->(r) \
             DELETE old WITH DISTINCT i, p CREATE (i)-[:IN]->(p)",
            json!({"player": PLAYER, "item": item}),
        )?;
        let text = format!("You take the {}.\n{}", cell(row, "name")?, self.render()?);
        self.report(&text)
    }

    pub(crate) fn drop_item(&mut self, item: &str) -> Result<(), String> {
        if item.is_empty() {
            return self.report("Drop what?");
        }
        let held = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             MATCH (i:Item) WHERE toLower(i.name) = toLower($item) \
             MATCH (i)-[:IN]->(p) RETURN i.name AS name",
            json!({"player": PLAYER, "item": item}),
        )?;
        let Some(row) = held.first() else {
            return self.report(&format!("You are not carrying {item}."));
        };
        self.reattach(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r)",
            "p",
            "r",
            json!({"player": PLAYER, "item": item}),
        )?;
        let text = format!("You drop the {}.\n{}", cell(row, "name")?, self.render()?);
        self.report(&text)
    }

    /// `say`/`ask`: no topic given is the NPC's general answer; an
    /// unknown topic is its in-voice deflection (the `unknown` pair),
    /// never a generic answer pretending the topic landed. Empty text
    /// is legitimate here (the general question), so only the caller
    /// can decide the missing-npc turn.
    pub(crate) fn say(&mut self, npc: &str, text: &str) -> Result<(), String> {
        let rows = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             MATCH (n:Npc)-[:LOCATION]->(r) \
             WHERE toLower(n.name) = toLower($npc) \
             RETURN n.name AS name, n.chat AS chat",
            json!({"player": PLAYER, "npc": npc}),
        )?;
        let Some(row) = rows.first() else {
            return self.report(&format!("There is no {npc} here to talk to."));
        };
        let name = cell(row, "name")?.to_string();
        let chat = cell(row, "chat")?;
        let answer = if text.is_empty() {
            pair_value(chat, "ask")
        } else {
            // Lowercased like every other player-word match (the
            // reference's behavior): `ask bard Hint` is the hint
            // answer, not a deflection.
            let keyword = text
                .split_whitespace()
                .next()
                .expect("non-empty text carries a first word")
                .to_ascii_lowercase();
            pair_value(chat, &keyword).or_else(|| pair_value(chat, "unknown"))
        };
        match answer {
            Some(answer) => self.report(&format!("{name}: {answer}")),
            None => self.report(&format!("{name} waits for a specific question.")),
        }
    }

    /// `ask <npc> for <item>` — the NPC hands over an item it holds, if
    /// it stands in your room. A present-but-not-takeable item is its
    /// own answer, the same distinction `take` keeps.
    pub(crate) fn ask_for(&mut self, npc: &str, item: &str) -> Result<(), String> {
        let offer = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             MATCH (n:Npc)-[:LOCATION]->(r) \
             WHERE toLower(n.name) = toLower($npc) \
             MATCH (i:Item)-[:IN]->(n) \
             WHERE toLower(i.name) = toLower($item) \
             RETURN n.name AS npc, i.name AS name, i.takeable AS takeable",
            json!({"player": PLAYER, "npc": npc, "item": item}),
        )?;
        let Some(row) = offer.first() else {
            return self.report(&format!("{npc} has no {item} to give."));
        };
        if !bool_cell(row, "takeable")? {
            let name = cell(row, "npc")?;
            return self.report(&format!("{name} will not part with the {item}."));
        }
        self.reattach(
            "MATCH (p:Player {name: $player}) \
             MATCH (n:Npc) WHERE toLower(n.name) = toLower($npc)",
            "n",
            "p",
            json!({"player": PLAYER, "npc": npc, "item": item}),
        )?;
        self.report(&format!(
            "{} hands you the {}.",
            cell(row, "npc")?,
            cell(row, "name")?
        ))
    }

    /// `give <npc> <item>` — hand an NPC something you carry, if it
    /// stands in your room.
    pub(crate) fn give(&mut self, npc: &str, item: &str) -> Result<(), String> {
        let here = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             MATCH (n:Npc)-[:LOCATION]->(r) \
             WHERE toLower(n.name) = toLower($npc) \
             MATCH (i:Item)-[:IN]->(p) \
             WHERE toLower(i.name) = toLower($item) \
             RETURN n.name AS npc, i.name AS name",
            json!({"player": PLAYER, "npc": npc, "item": item}),
        )?;
        let Some(row) = here.first() else {
            return self.report(&format!("You cannot give {item} to {npc} here."));
        };
        self.reattach(
            "MATCH (p:Player {name: $player}) \
             MATCH (n:Npc) WHERE toLower(n.name) = toLower($npc)",
            "p",
            "n",
            json!({"player": PLAYER, "npc": npc, "item": item}),
        )?;
        self.report(&format!(
            "You give the {} to the {}.",
            cell(row, "name")?,
            cell(row, "npc")?
        ))
    }

    pub(crate) fn inventory(&mut self) -> Result<(), String> {
        let rows = self.read(
            "MATCH (p:Player {name: $player}) \
             MATCH (i:Item)-[:IN]->(p) \
             RETURN i.name AS name, i.short AS short",
            json!({"player": PLAYER}),
        )?;
        if rows.is_empty() {
            return self.report("You are carrying nothing.");
        }
        let lines: Vec<String> = rows
            .iter()
            .map(|r| Ok(format!("  {} — {}", cell(r, "name")?, cell(r, "short")?)))
            .collect::<Result<_, String>>()?;
        self.report(&format!("You are carrying:\n{}", lines.join("\n")))
    }

    /// `look <npc|item>` — the room's occupant or an item here or
    /// carried; no target is the room itself (status).
    pub(crate) fn look_at(&mut self, target: &str) -> Result<(), String> {
        if target.is_empty() {
            return self.status();
        }
        let npc = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             MATCH (n:Npc)-[:LOCATION]->(r) \
             WHERE toLower(n.name) = toLower($target) \
             RETURN n.name AS name, n.long AS long, n.chat AS chat",
            json!({"player": PLAYER, "target": target}),
        )?;
        if let Some(row) = npc.first() {
            // No mechanical topic list — the long desc names the themes
            // in-fiction, and an unknown topic gets the NPC's own
            // in-voice deflection.
            let mut lines = vec![format!("{}: {}", cell(row, "name")?, cell(row, "long")?)];
            let held = self.read(
                "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
                 MATCH (n:Npc)-[:LOCATION]->(r) \
                 MATCH (i:Item)-[:IN]->(n) \
                 WHERE toLower(n.name) = toLower($target) \
                 RETURN i.name AS name, i.short AS short, i.here AS here",
                json!({"player": PLAYER, "target": target}),
            )?;
            if !held.is_empty() {
                let npc_name = cell(row, "name")?;
                let carrying: Vec<String> = held
                    .iter()
                    .map(|h| {
                        Ok(format!(
                            "{} ({}){}",
                            cell(h, "name")?,
                            cell(h, "short")?,
                            here_phrase(cell(h, "here")?, npc_name)
                        ))
                    })
                    .collect::<Result<_, String>>()?;
                lines.push(format!("carrying: {}", carrying.join(", ")));
            }
            return self.report(&lines.join("\n"));
        }
        let item = self.read(
            "MATCH (p:Player {name: $player})-[:LOCATION]->(r) \
             MATCH (i:Item) WHERE toLower(i.name) = toLower($target) \
             AND ((i)-[:IN]->(r) OR (i)-[:IN]->(p) \
                  OR (i)-[:IN]->(:Npc)-[:LOCATION]->(r)) \
             MATCH (i)-[:IN]->(holder) \
             RETURN i.name AS name, i.long AS long, i.here AS here, \
             holder.name AS holder",
            json!({"player": PLAYER, "target": target}),
        )?;
        if let Some(row) = item.first() {
            let holder = cell(row, "holder")?;
            return self.report(&format!(
                "{}: {}{}",
                cell(row, "name")?,
                cell(row, "long")?,
                here_phrase(cell(row, "here")?, holder)
            ));
        }
        self.report(&format!("You see no {target} here."))
    }

    pub(crate) fn help(&mut self) -> Result<(), String> {
        self.report(concat!(
            "dungeon — verbs: start (seed or resume + status), seed, ",
            "status, inv, look [<target>], map, ",
            "n/e/s/w/ne/nw/se/sw, move <dir>, take <item>, ",
            "drop <item>, ask <npc> <question> | ask <npc> for ",
            "<item>, give <item> to <npc>, give <npc> <item>, ",
            "say <npc> <text>, help, play (print the bash runner). ",
            "Directions: n, ne, e, se, s, sw, w, nw ",
            "(or the full compass words)."
        ))
    }

    /// Emit the TinyFugue wrapper as a one-line bash program — the
    /// self-contained entry: `bash <(relate run dungeon --command
    /// play) <store.rl>` (process substitution keeps stdin free for the
    /// read loop; a plain pipe would collide with bash's own script
    /// reading). The report is the script and nothing else. Named
    /// `play` — what the player is doing — not `init`/`install`, which
    /// collide with the extension scaffold and package-install verbs.
    pub(crate) fn play(&mut self) -> Result<(), String> {
        self.report(SHELL_SCRIPT)
    }

    pub(crate) fn map(&mut self) -> Result<(), String> {
        let text = self.render()?;
        self.report(&text)
    }
}
