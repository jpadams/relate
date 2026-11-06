//! The keep's content tables (LABS-884): 22 rooms, 42 passage legs,
//! three characters with their in-fiction chat, four items with their
//! per-holder addenda, and the placements and locks. Ported verbatim
//! from the Python iteration child (`reference/dungeon.py`).

use crate::data::Direction;

/// A room of the keep. The long desc never mentions items — the
/// "on the floor:" line is the live inventory; static text would go
/// stale the moment an item moves.
pub struct Room {
    pub name: &'static str,
    pub short: &'static str,
    pub long: &'static str,
    pub lit: bool,
    pub zone: &'static str,
    pub x: i32,
    pub y: i32,
}

/// One leg of a passage; every passage is declared both ways.
pub struct Exit {
    pub from: &'static str,
    pub dir: Direction,
    pub to: &'static str,
}

/// A character: chat is the topic/answer pairs (the LABS-887 pair-list
/// encoding), `unknown` the in-voice deflection for a topic they do not
/// care about.
pub struct Npc {
    pub name: &'static str,
    pub short: &'static str,
    pub long: &'static str,
    pub race: &'static str,
    pub hp: i64,
    pub xp: i64,
    pub chat: &'static [(&'static str, &'static str)],
}

/// An item: `unlocks` names the room its key opens; `here` carries the
/// per-holder addendum ("on the bench, deliberately placed") joined
/// onto the holder's look — the phrase travels with the item, so the
/// static descs never go stale.
pub struct Item {
    pub name: &'static str,
    pub short: &'static str,
    pub long: &'static str,
    pub takeable: bool,
    pub unlocks: Option<&'static str>,
    pub weight: i64,
    pub here: &'static [(&'static str, &'static str)],
}

pub const START_ROOM: &str = "Entrance";

pub const ROOMS: &[Room] = &[
    // y=0 — the entrance row (south); both neighbours are dead ends.
    Room {
        name: "Entrance",
        short: "the bottom of the steps",
        long: concat!(
            "A cool draft blows down from the steps above. Torchlight ",
            "flickers on worn stone."
        ),
        lit: true,
        zone: "outer",
        x: 0,
        y: 0,
    },
    Room {
        name: "Guard Room",
        short: "the guard room",
        long: concat!(
            "An empty chair leans against the wall beside a cold brazier. ",
            "Nobody guarded this door for a long time."
        ),
        lit: false,
        zone: "outer",
        x: 1,
        y: 0,
    },
    Room {
        name: "Rubbish Heap",
        short: "the rubbish heap",
        long: "Broken pottery and old bones, piled against the outer wall.",
        lit: false,
        zone: "outer",
        x: -1,
        y: 0,
    },
    // y=-1 — the climb into the keep.
    Room {
        name: "Steps",
        short: "the worn steps",
        long: concat!(
            "Steps worn to shallow curves climb north into the keep. ",
            "Torchlight gutters somewhere above."
        ),
        lit: true,
        zone: "keep",
        x: 0,
        y: -1,
    },
    // y=-2 — the hall and its two wings.
    Room {
        name: "Hall",
        short: "the taproom",
        long: concat!(
            "The taproom smells of torch smoke. Doors lead off in every ",
            "direction; the corridors north are darker than the rest."
        ),
        lit: true,
        zone: "keep",
        x: 0,
        y: -2,
    },
    Room {
        name: "Corridor West",
        short: "the west corridor",
        long: concat!(
            "A dark corridor towards the armory. The dust here is old and ",
            "undisturbed."
        ),
        lit: false,
        zone: "keep",
        x: -1,
        y: -2,
    },
    Room {
        name: "Armory",
        short: "the armory",
        long: concat!(
            "Racks of rusted practice weapons. A workbench sits by the far ",
            "wall."
        ),
        lit: true,
        zone: "keep",
        x: -2,
        y: -2,
    },
    Room {
        name: "Kitchen",
        short: "the kitchen",
        long: concat!(
            "A cold hearth, hooks, and a cutting block scarred by decades ",
            "of knives."
        ),
        lit: true,
        zone: "keep",
        x: 1,
        y: -2,
    },
    Room {
        name: "Barracks",
        short: "the barracks",
        long: "Rows of bunks, rotted to splinters. The air is still and dry.",
        lit: true,
        zone: "keep",
        x: 2,
        y: -2,
    },
    // y=-3 — the west wing's water and the kitchen's pantry; the north
    // corridor begins.
    Room {
        name: "Cistern",
        short: "the cistern",
        long: concat!(
            "Black water, still as glass, reflects whatever light you ",
            "carry. The drip of water echoes from somewhere deeper."
        ),
        lit: false,
        zone: "undercroft",
        x: -1,
        y: -3,
    },
    Room {
        name: "Pantry",
        short: "the pantry",
        long: "Shelves of empty jars. The labels have long since faded.",
        lit: false,
        zone: "undercroft",
        x: 1,
        y: -3,
    },
    Room {
        name: "Chapel",
        short: "the chapel",
        long: "A cracked altar and tattered banners. Candles melted to puddles.",
        lit: true,
        zone: "undercroft",
        x: 2,
        y: -3,
    },
    Room {
        name: "Corridor North",
        short: "the north corridor",
        long: concat!(
            "A long corridor running north. Your light reaches only a few ",
            "paces ahead, and the keep gets quieter the further you go."
        ),
        lit: false,
        zone: "undercroft",
        x: 0,
        y: -3,
    },
    // y=-4 — the gallery and its side rooms; the deep west begins.
    Room {
        name: "Wine Cellar",
        short: "the wine cellar",
        long: concat!(
            "Racks of dust-caked bottles. One bottle looks newer than the ",
            "rest."
        ),
        lit: true,
        zone: "undercroft",
        x: -2,
        y: -4,
    },
    Room {
        name: "Gallery",
        short: "the gallery",
        long: concat!(
            "Faded portraits of forgotten lords line a long corridor. Their ",
            "eyes have all been scratched out."
        ),
        lit: true,
        zone: "undercroft",
        x: 0,
        y: -4,
    },
    Room {
        name: "Scriptorium",
        short: "the scriptorium",
        long: "Scattered pages, interrupted ink. The scribe left in a hurry.",
        lit: true,
        zone: "undercroft",
        x: 1,
        y: -4,
    },
    Room {
        name: "Ossuary",
        short: "the ossuary",
        long: "Bones sorted by type, stacked with terrible patience.",
        lit: false,
        zone: "undercroft",
        x: 3,
        y: -4,
    },
    // y=-5 — the furnace dead end, the treasury dead end, the
    // antechamber.
    Room {
        name: "Furnace",
        short: "the old furnace",
        long: "A cold furnace big enough to walk into. Soot shadows the walls.",
        lit: false,
        zone: "deep",
        x: -2,
        y: -5,
    },
    Room {
        name: "Treasury",
        short: "the treasury",
        long: concat!(
            "Empty plinth after empty plinth. Whatever was here was taken ",
            "long ago — almost all of it."
        ),
        lit: true,
        zone: "deep",
        x: 2,
        y: -5,
    },
    Room {
        name: "Antechamber",
        short: "the antechamber",
        long: concat!(
            "A cold antechamber before the throne room. The doors ahead are ",
            "taller than they need to be."
        ),
        lit: false,
        zone: "deep",
        x: 0,
        y: -5,
    },
    // y=-6 — the throne.
    Room {
        name: "Throne Room",
        short: "the throne room",
        long: concat!(
            "A sagging throne on a cracked dais. The floor is swept clean — ",
            "recently."
        ),
        lit: true,
        zone: "deep",
        x: 0,
        y: -6,
    },
    // y=-7 — the lair: the locked cellar, the quest's end.
    Room {
        name: "Cellar",
        short: "the locked cellar",
        long: concat!(
            "Behind the barrels, grey scales shift in the dark. One golden ",
            "eye regards you, unhurried."
        ),
        lit: false,
        zone: "lair",
        x: 0,
        y: -7,
    },
];

pub const EXITS: &[Exit] = &[
    Exit {
        from: "Entrance",
        dir: Direction::North,
        to: "Steps",
    },
    Exit {
        from: "Steps",
        dir: Direction::South,
        to: "Entrance",
    },
    Exit {
        from: "Entrance",
        dir: Direction::East,
        to: "Guard Room",
    },
    Exit {
        from: "Guard Room",
        dir: Direction::West,
        to: "Entrance",
    },
    Exit {
        from: "Entrance",
        dir: Direction::West,
        to: "Rubbish Heap",
    },
    Exit {
        from: "Rubbish Heap",
        dir: Direction::East,
        to: "Entrance",
    },
    Exit {
        from: "Steps",
        dir: Direction::North,
        to: "Hall",
    },
    Exit {
        from: "Hall",
        dir: Direction::South,
        to: "Steps",
    },
    Exit {
        from: "Hall",
        dir: Direction::West,
        to: "Corridor West",
    },
    Exit {
        from: "Corridor West",
        dir: Direction::East,
        to: "Hall",
    },
    Exit {
        from: "Corridor West",
        dir: Direction::West,
        to: "Armory",
    },
    Exit {
        from: "Armory",
        dir: Direction::East,
        to: "Corridor West",
    },
    Exit {
        from: "Corridor West",
        dir: Direction::North,
        to: "Cistern",
    },
    Exit {
        from: "Cistern",
        dir: Direction::South,
        to: "Corridor West",
    },
    Exit {
        from: "Cistern",
        dir: Direction::NorthWest,
        to: "Wine Cellar",
    },
    Exit {
        from: "Wine Cellar",
        dir: Direction::SouthEast,
        to: "Cistern",
    },
    Exit {
        from: "Wine Cellar",
        dir: Direction::North,
        to: "Furnace",
    },
    Exit {
        from: "Furnace",
        dir: Direction::South,
        to: "Wine Cellar",
    },
    Exit {
        from: "Hall",
        dir: Direction::East,
        to: "Kitchen",
    },
    Exit {
        from: "Kitchen",
        dir: Direction::West,
        to: "Hall",
    },
    Exit {
        from: "Kitchen",
        dir: Direction::North,
        to: "Pantry",
    },
    Exit {
        from: "Pantry",
        dir: Direction::South,
        to: "Kitchen",
    },
    Exit {
        from: "Kitchen",
        dir: Direction::East,
        to: "Barracks",
    },
    Exit {
        from: "Barracks",
        dir: Direction::West,
        to: "Kitchen",
    },
    Exit {
        from: "Kitchen",
        dir: Direction::NorthEast,
        to: "Chapel",
    },
    Exit {
        from: "Chapel",
        dir: Direction::SouthWest,
        to: "Kitchen",
    },
    Exit {
        from: "Chapel",
        dir: Direction::NorthEast,
        to: "Ossuary",
    },
    Exit {
        from: "Ossuary",
        dir: Direction::SouthWest,
        to: "Chapel",
    },
    Exit {
        from: "Hall",
        dir: Direction::North,
        to: "Corridor North",
    },
    Exit {
        from: "Corridor North",
        dir: Direction::South,
        to: "Hall",
    },
    Exit {
        from: "Corridor North",
        dir: Direction::North,
        to: "Gallery",
    },
    Exit {
        from: "Gallery",
        dir: Direction::South,
        to: "Corridor North",
    },
    Exit {
        from: "Gallery",
        dir: Direction::East,
        to: "Scriptorium",
    },
    Exit {
        from: "Scriptorium",
        dir: Direction::West,
        to: "Gallery",
    },
    Exit {
        from: "Scriptorium",
        dir: Direction::NorthEast,
        to: "Treasury",
    },
    Exit {
        from: "Treasury",
        dir: Direction::SouthWest,
        to: "Scriptorium",
    },
    Exit {
        from: "Gallery",
        dir: Direction::North,
        to: "Antechamber",
    },
    Exit {
        from: "Antechamber",
        dir: Direction::South,
        to: "Gallery",
    },
    Exit {
        from: "Antechamber",
        dir: Direction::North,
        to: "Throne Room",
    },
    Exit {
        from: "Throne Room",
        dir: Direction::South,
        to: "Antechamber",
    },
    Exit {
        from: "Throne Room",
        dir: Direction::North,
        to: "Cellar",
    },
    Exit {
        from: "Cellar",
        dir: Direction::South,
        to: "Throne Room",
    },
];

pub const NPCS: &[Npc] = &[
    Npc {
        name: "Bard",
        short: "a road-worn bard",
        long: concat!(
            "A road-worn bard leans in the corner, tuning a lute with ",
            "cracked nails. Road dust still on his boots, a half-finished ",
            "song about a cellar on his lips. He nods when you look over. ",
            "He will play if you ask, and rumour is his second instrument."
        ),
        race: "Human",
        hp: 8,
        xp: 20,
        chat: &[
            (
                "play",
                "The bard plays a tune about a key that opens a cellar.",
            ),
            ("ask", "Ask the armorer in the armory, west of the hall."),
            (
                "hint",
                concat!(
                    "Take the key from the armory west of the hall, then ",
                    "follow the corridors north to the throne room. The ",
                    "cellar lies beyond it."
                ),
            ),
            (
                "name",
                "Fenwick. I would say at your service, but the pay stopped centuries ago.",
            ),
            (
                "key",
                concat!(
                    "The armorer left the iron key on her bench, west of ",
                    "the hall past the dark corridor. She says the dragon ",
                    "earns it honestly."
                ),
            ),
            (
                "cellar",
                concat!(
                    "Past the throne room, at the north end of the keep. ",
                    "The door is iron and the lock is older than the keep."
                ),
            ),
            (
                "dragon",
                concat!(
                    "Old Grey? She settled in the cellar before the last ",
                    "lord died. Pay her a courtesy and she pays it back."
                ),
            ),
            (
                "song",
                "It is about a minstrel who traded a song for a chest. It ends well, unusually.",
            ),
            (
                "rumour",
                "The treasury is not as empty as it looks. Look up, not down.",
            ),
            (
                "lute",
                concat!(
                    "Six strings, five intact. The lute has outlived three ",
                    "lords and every one of my audiences."
                ),
            ),
            (
                "road",
                concat!(
                    "South, down the steps, and it winds to the coast ",
                    "eventually. I walked up it once and never found a ",
                    "reason to walk back."
                ),
            ),
            (
                "dungeon",
                concat!(
                    "Corridors and dead ends, they say — wings off this ",
                    "hall, and a long dark run north to the throne. The ",
                    "acoustics improve the deeper you go."
                ),
            ),
            (
                "unknown",
                concat!(
                    r#"The bard keeps tuning. "The lute, the dragon," he "#,
                    r#"says. "Nothing else pays the drinks.""#,
                ),
            ),
            (
                "world",
                concat!(
                    "The road south, the coast at its end, and a few ",
                    "villages that still remember this keep. That is most ",
                    "of my world these days."
                ),
            ),
        ],
    },
    Npc {
        name: "Armorer",
        short: "the armorer's ghost",
        long: concat!(
            "A translucent figure in a scorched apron drifts between the ",
            "racks, straightening weapons that are no longer there. Being ",
            "a ghost has not dulled her interest in keys, dragons, or the ",
            "unlit rooms below."
        ),
        race: "Ghost",
        hp: 1,
        xp: 5,
        chat: &[
            (
                "ask",
                concat!(
                    "I left the key on the bench. Take it — the dragon ",
                    "keeps worse things than rust."
                ),
            ),
            (
                "hint",
                "The cellar lies north of the throne room. Take a light.",
            ),
            (
                "name",
                "Marta, armorer of this keep, in death as in life. The apron still fits.",
            ),
            (
                "key",
                "Iron, cellar-door, on the bench by the racks. It was mine. Now it is yours.",
            ),
            (
                "dragon",
                concat!(
                    "She guards the chest because she promised to. Trade ",
                    "her the key and she trades you the chest."
                ),
            ),
            (
                "weapons",
                concat!(
                    "All rust and memories. Take none of them — the racks ",
                    "remember their owners."
                ),
            ),
            (
                "ghost",
                concat!(
                    "The cold does not bother me. Mind the unlit rooms — ",
                    "that is where the living lose their way."
                ),
            ),
            (
                "dungeon",
                concat!(
                    "I hammered in every room of this keep — the wings, ",
                    "the deep corridors, all of them — and I remember ",
                    "where each nail went."
                ),
            ),
            (
                "unknown",
                concat!(
                    r#"The ghost dims a little. "The key, the dragon. My "#,
                    r#"trade was always narrow.""#,
                ),
            ),
            (
                "world",
                "The world kept moving when I stopped. Take the road south if you miss it.",
            ),
        ],
    },
    Npc {
        name: "Dragon",
        short: "an old cellar dragon",
        long: concat!(
            "An old dragon, grey-scaled and cool to the touch, keeps her ",
            "post behind the cellar barrels. She has outlasted every lock ",
            "in the keep and remembers every bard who ever passed her door."
        ),
        race: "Dragon",
        hp: 40,
        xp: 200,
        chat: &[
            (
                "roar",
                "The dragon roars politely. Cellar dust settles in the still air.",
            ),
            (
                "ask",
                concat!(
                    r#"The dragon says: "The chest is heavy and the keeping "#,
                    r#"heavier. Bring my key and the carrying is yours.""#,
                ),
            ),
            (
                "name",
                concat!(
                    r#"The dragon says: "Old Grey, the cellar dragon. I "#,
                    r#"prefer the title I gave myself: the Curator.""#,
                ),
            ),
            (
                "treasure",
                concat!(
                    r#"The dragon says: "One chest, one key. Marta forged "#,
                    r#"the second and I am owed it. Fair trades keep the "#,
                    r#"peace.""#,
                ),
            ),
            (
                "key",
                concat!(
                    r#"The dragon says: "Iron, cellar-door. Marta forged "#,
                    r#"it. Bring it and we will settle accounts.""#,
                ),
            ),
            (
                "bard",
                concat!(
                    r#"The dragon says: "Fenwick? He still owes me the "#,
                    r#"third verse of that trade-song.""#,
                ),
            ),
            (
                "dungeon",
                concat!(
                    r#"The dragon says: "A keep of corridors above my "#,
                    r#"cellar. The deep ones are mine to keep an eye on.""#,
                ),
            ),
            (
                "world",
                concat!(
                    r#"The dragon says: "Mostly roads, the world. They all "#,
                    r#"used to pass my door — now the door passes to you.""#,
                ),
            ),
            (
                "unknown",
                concat!(
                    r#"The dragon says: "The key. Bring the key, or talk "#,
                    r#"to the door.""#,
                ),
            ),
        ],
    },
];

/// Where each character stands at seed (rooms; the placement write only
/// touches unplaced NPCs, so a re-seed never teleports a moved one).
pub const NPC_LOCATION: &[(&str, &str)] = &[
    ("Bard", "Hall"),
    ("Armorer", "Armory"),
    ("Dragon", "Cellar"),
];

pub const ITEMS: &[Item] = &[
    Item {
        name: "Key",
        short: "iron key",
        long: "a small iron key",
        takeable: true,
        unlocks: Some("Cellar"),
        weight: 1,
        here: &[("Armory", "on the bench, deliberately placed")],
    },
    Item {
        name: "Torch",
        short: "pitch torch",
        long: "a dry pitch torch",
        takeable: true,
        unlocks: None,
        weight: 2,
        here: &[("Guard Room", "in a cold bracket by the brazier")],
    },
    Item {
        name: "Gem",
        short: "faceted gem",
        long: "a fist-sized gem that catches even weak light",
        takeable: true,
        unlocks: None,
        weight: 5,
        here: &[("Treasury", "on the highest plinth, in a nest of dust")],
    },
    Item {
        name: "Treasure",
        short: "dragon's chest",
        long: "the chest of the cellar dragon",
        takeable: true,
        unlocks: None,
        weight: 20,
        here: &[("Dragon", "coiled under one claw")],
    },
];

/// Where each item starts (rooms or characters; guarded the same way as
/// the NPC placement — only unplaced items are placed).
pub const ITEM_LOCATION: &[(&str, &str)] = &[
    ("Key", "Armory"),
    ("Torch", "Guard Room"),
    ("Gem", "Treasury"),
    ("Treasure", "Dragon"),
];

// The lock fact lives on Item.unlocks alone: the seed writes it as the
// `unlocks` property and derives the :UNLOCKS edges from the same
// field, so the property and the edge the move verb checks cannot drift
// apart.
