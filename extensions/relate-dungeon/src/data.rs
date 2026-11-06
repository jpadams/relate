//! The dungeon's one vocabulary (LABS-884): the closed compass (one
//! table, everything derived), the glyph legend, the one-time intro,
//! the `play` wrapper, the pair codec, and the graph type. The keep's
//! content tables live in [`crate::keep`].

/// The player's name in the store — the one `:Player` node the verbs
/// key on.
pub const PLAYER: &str = "You";

/// Newlines cannot survive `report_output` (the host's summary
/// sanitizer flattens them, ADR-0430 §7); the shell wrapper the `play`
/// verb emits turns this back into a newline before printing.
pub const NEWLINE_PLACEHOLDER: &str = "%%NL%%";

/// The glyph grid's legend, printed above the first map of a session.
pub const LEGEND: &str = concat!(
    "@ you   O room   # dark room   A-Z occupant   -/\\|",
    " passage   · unexplored",
);

/// The one-time intro, stored on the start room at seed and read back
/// from the graph on the first open of a fresh store — the text is room
/// data, not a code constant at show time.
pub const INTRO: &str = concat!(
    "The road ends at a flight of stone steps. Above you, a keep no ",
    "lord has claimed in a hundred years; below, a taproom fire still ",
    "burns — someone is keeping it lit. Somewhere in the dark north of ",
    "the keep, they say, an old dragon guards a chest she is too proud ",
    "to carry and too stubborn to abandon. You have come for the trade.",
    "\n\n(The way is compass-wise: n, ne, e, se, s, sw, w, nw. Look at ",
    "things. Talk to whoever you meet — they answer what they care ",
    "about.)",
);

/// The TinyFugue wrapper the `play` verb emits — ONE physical line on
/// purpose: `report_output`'s sanitizer flattens newlines, so the
/// emitted script must not contain any. Consumed via process
/// substitution (`bash <(relate run dungeon --command play)
/// <store.rl>`) so stdin stays free for the `read -e` loop; a plain
/// pipe would collide with bash's own script reading. Behavior is the
/// shipped `dungeon-shell` file's: newline restore, arrow-up history
/// (toggled on only per accepted line — `set -o history` in a script
/// records the script's own lines), one `run` per turn.
pub const SHELL_SCRIPT: &str = concat!(
    r#"STORE="${1:-dungeon.rl}"; "#,
    r#"HIST="${DUNGEON_HISTFILE:-$HOME/.dungeon_history}"; "#,
    r#"run(){ out="$(relate run dungeon -d "$STORE" --command "$1")"; "#,
    r#"printf '%s\n' "${out//%%NL%%/$'\n'}"; }; "#,
    r#"[ -f "$HIST" ] && { set -o history; history -r "$HIST"; set +o history; }; "#,
    "run start; ",
    r#"while read -e -p "dungeon> " line; do "#,
    r#"[ -z "$line" ] && continue; "#,
    "set -o history; history -s \"$line\"; set +o history; ",
    r#"printf '%s\n' "$line" >> "$HIST"; "#,
    "run \"$line\"; done",
);

/// The graph type's fixed head and tail: the node types and the
/// non-compass relationships. The eight compass rel signatures are
/// generated from [`Direction::ALL`] by [`graph_type`] so the type can
/// never disagree with the vocabulary the verbs compose (LABS-884).
/// CREATE alone makes the type current; `ALTER CURRENT GRAPH TYPE SET`
/// is LABS-887's broken path and declares nothing CREATE did not. The
/// pair-list encodings (`chat`, `here`) are the same ticket's
/// workaround: LIST-typed properties cannot be ALTERed current yet, so
/// the pairs travel as one delimited STRING.
const GRAPH_TYPE_HEAD: &str = r#"CREATE GRAPH TYPE DungeonGraph AS {
  (:Room => {name :: STRING NOT NULL IS KEY, short :: STRING,
             long :: STRING, lit :: BOOLEAN, zone :: STRING,
             x :: INTEGER, y :: INTEGER, visited :: BOOLEAN,
             intro :: STRING}),
  (:Npc => {name :: STRING NOT NULL IS KEY, short :: STRING,
            long :: STRING, race :: STRING, hp :: INTEGER, xp :: INTEGER,
            chat :: STRING}),
  (:Player => {name :: STRING NOT NULL IS KEY, hp :: INTEGER,
               xp :: INTEGER}),
  (:Item => {name :: STRING NOT NULL IS KEY, short :: STRING,
             long :: STRING, takeable :: BOOLEAN, unlocks :: STRING,
             weight :: INTEGER, here :: STRING}),"#;

const GRAPH_TYPE_TAIL: &str = r#"
  (:Item)-[:IN]->(:Room),
  (:Player)-[:LOCATION]->(:Room),
  (:Item)-[:UNLOCKS]->(:Room)
}"#;

/// The whole graph type, the compass signatures generated from the one
/// table.
pub fn graph_type() -> String {
    let mut text = GRAPH_TYPE_HEAD.to_string();
    for dir in Direction::ALL {
        text.push_str(&format!("\n  (:Room)-[:{}]->(:Room),", dir.name()));
    }
    text.push_str(GRAPH_TYPE_TAIL);
    text
}

/// The closed compass vocabulary: one enum, one table. The rel type is
/// the statement text the verbs compose (the name is spliced only after
/// the word resolved against this table — never a raw player word into
/// statement text), the short form is what the player types, the delta
/// is the grid math; the glyph and the inverse are derived, and the
/// prose spelling ([`Direction::word`]) comes off the name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Direction {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

/// The compass's ONE table: the variant itself, its rel type, player
/// token and grid delta — rose order starting at north, so the inverse
/// sits four slots away. `ALL` derives from it; the row lookup is by
/// the variant, not an index, so adding or reordering a direction is
/// one edit here and the compiler enforces the alignment.
const COMPASS: [(Direction, &str, &str, i32, i32); 8] = [
    (Direction::North, "NORTH", "n", 0, -1),
    (Direction::NorthEast, "NORTH_EAST", "ne", 1, -1),
    (Direction::East, "EAST", "e", 1, 0),
    (Direction::SouthEast, "SOUTH_EAST", "se", 1, 1),
    (Direction::South, "SOUTH", "s", 0, 1),
    (Direction::SouthWest, "SOUTH_WEST", "sw", -1, 1),
    (Direction::West, "WEST", "w", -1, 0),
    (Direction::NorthWest, "NORTH_WEST", "nw", -1, -1),
];

impl Direction {
    /// The compass in the table's own rose order — the order the exits
    /// line reports and the inverse is derived from.
    pub const ALL: [Direction; 8] = [
        COMPASS[0].0,
        COMPASS[1].0,
        COMPASS[2].0,
        COMPASS[3].0,
        COMPASS[4].0,
        COMPASS[5].0,
        COMPASS[6].0,
        COMPASS[7].0,
    ];

    fn at(self) -> (&'static str, &'static str, i32, i32) {
        let (_, name, short, dx, dy) = COMPASS
            .into_iter()
            .find(|(variant, _, _, _, _)| *variant == self)
            .expect("the compass table names every variant");
        (name, short, dx, dy)
    }

    /// The rel type — the statement vocabulary.
    pub fn name(self) -> &'static str {
        self.at().0
    }

    /// What the player types: `n`, `nw`, …
    pub fn short(self) -> &'static str {
        self.at().1
    }

    /// The prose spelling for messages: `north-west`.
    pub fn word(self) -> String {
        self.name().to_ascii_lowercase().replace('_', "-")
    }

    /// The neighbour cell's offset — every direction's grid delta.
    pub fn delta(self) -> (i32, i32) {
        let (_, _, dx, dy) = self.at();
        (dx, dy)
    }

    /// The map connector between two rooms this direction joins,
    /// derived from the delta's axis: vertical, horizontal, or the two
    /// diagonals.
    pub fn glyph(self) -> char {
        let (dx, dy) = self.delta();
        match (dx, dy) {
            (_, 0) => '-',
            (0, _) => '|',
            (dx, dy) if dx == dy => '\\',
            _ => '/',
        }
    }

    /// Decode a rel type read back from the store — the exact `name()`
    /// vocabulary, nothing else, so a hand-edited or foreign store's
    /// `[:n]`/`[:north]` is refused instead of silently reinterpreted
    /// as a passage.
    pub fn from_rel_type(text: &str) -> Option<Direction> {
        Direction::ALL.into_iter().find(|d| d.name() == text)
    }

    /// Resolve a player word — short or long form, any case — to the
    /// direction it names. `None` is the caller's "not a direction".
    pub fn parse(word: &str) -> Option<Direction> {
        let lowered = word.to_ascii_lowercase();
        Direction::ALL
            .into_iter()
            .find(|d| d.short() == lowered || d.name().to_ascii_lowercase() == lowered)
    }
}

/// `[k, v, k, v, ...]` pairs -> `k=v|k=v` — the LABS-887 pair-list
/// encoding. Values must not contain `|` or `=` (true for the starter
/// set; the limitation is deliberate).
pub fn encode_pairs(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("|")
}

/// The encoding's other half.
pub fn decode_pairs(text: &str) -> Vec<(String, String)> {
    text.split('|')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p
                .split_once('=')
                .expect("every encoded pair carries '=' (see encode_pairs)");
            (k.to_string(), v.to_string())
        })
        .collect()
}

/// The encoded pair for one key, if the text carries it.
pub fn pair_value(text: &str, key: &str) -> Option<String> {
    decode_pairs(text)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// The item's per-holder addendum (`", on the bench, ..."`), joined onto
/// its short/long wherever the holder's look lists it.
pub fn here_phrase(here: &str, holder: &str) -> String {
    match pair_value(here, holder) {
        Some(phrase) => format!(", {phrase}"),
        None => String::new(),
    }
}
