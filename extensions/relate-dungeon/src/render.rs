//! The explored-map renderer: a glyph lattice of the rooms the player
//! has already entered, centred on the player's room, so a big dungeon
//! reveals itself room by room instead of spoiling the layout up front.
//! Pure — the verbs gather the inputs with reads and call
//! [`render_map`], which makes the lattice testable without a host.

use crate::data::Direction;

/// The turn's text for an unseeded store — owned here, the one
/// spelling the map paths use.
pub(crate) const NO_MAP: &str = "(no map — run `seed` first)";

const PLAYER_GLYPH: char = '@';
const LIT_GLYPH: char = 'O';
const ROOM_GLYPH: char = '#';
const VOID_GLYPH: char = '·';

/// The map window: rooms within this many cells of the player are drawn.
pub const MAP_RADIUS: i32 = 2;

/// One room the map may draw: its cell and whether it is lit.
#[derive(Clone, Copy)]
pub struct SeenRoom {
    pub x: i32,
    pub y: i32,
    pub lit: bool,
}

/// An occupant whose room is drawn: the cell and the character's name.
pub struct Occupant {
    pub x: i32,
    pub y: i32,
    pub name: String,
}

/// One passage leg to draw between two cells.
pub struct MapEdge {
    pub ax: i32,
    pub ay: i32,
    pub bx: i32,
    pub by: i32,
    pub dir: Direction,
}

/// Draw the map: a `(2·radius+1)`-cell window centred on the player, a
/// lattice where even offsets are cells and odd offsets are connectors.
/// Only visited rooms inside the window are drawn — everything else
/// stays the void glyph — and a passage is drawn when EITHER end is
/// seen, so an exit the player can see from here is indicated on the
/// map even before the room behind it is entered.
pub fn render_map(
    player: (i32, i32),
    seen: &[SeenRoom],
    occupants: &[Occupant],
    edges: &[MapEdge],
) -> String {
    let (px, py) = player;
    let rad = MAP_RADIUS;
    let span = 2 * rad + 1;
    let in_window = |x: i32, y: i32| (x - px).abs() <= rad && (y - py).abs() <= rad;
    // The window's membership, derived once: the cell pass draws from
    // it and the edge pass consults it for both endpoints.
    let windowed: Vec<&SeenRoom> = seen.iter().filter(|r| in_window(r.x, r.y)).collect();
    let window_coords: std::collections::HashSet<(i32, i32)> =
        windowed.iter().map(|r| (r.x, r.y)).collect();
    if windowed.is_empty() {
        return NO_MAP.to_string();
    }
    // The window's origin is the player's room minus the radius.
    let (x0, y0) = (px - rad, py - rad);
    // The lattice's side: cells at even offsets, connectors at odd.
    let lattice = (2 * span - 1) as usize;
    let mut grid = vec![vec![VOID_GLYPH; lattice]; lattice];
    let occupant_at = |x: i32, y: i32| -> Option<&str> {
        occupants
            .iter()
            .find_map(|o| (o.x == x && o.y == y).then_some(o.name.as_str()))
    };
    for room in windowed {
        let (cx, cy) = (2 * (room.x - x0), 2 * (room.y - y0));
        grid[cy as usize][cx as usize] = if (room.x, room.y) == (px, py) {
            PLAYER_GLYPH
        } else if let Some(name) = occupant_at(room.x, room.y) {
            // The occupants' names are ASCII and non-empty — the verbs
            // reject empty rows, and this layer states the same
            // invariant instead of drawing a NUL glyph.
            let initial = name.chars().next().expect("occupant names are non-empty");
            initial.to_ascii_uppercase()
        } else if room.lit {
            LIT_GLYPH
        } else {
            ROOM_GLYPH
        };
    }
    for edge in edges {
        let (ax, ay, bx, by) = (edge.ax, edge.ay, edge.bx, edge.by);
        let a_seen = window_coords.contains(&(ax, ay));
        let b_seen = window_coords.contains(&(bx, by));
        if !a_seen && !b_seen {
            continue;
        }
        let (mx, my) = (2 * (ax - x0) + (bx - ax), 2 * (ay - y0) + (by - ay));
        if mx < 0 || my < 0 || mx >= lattice as i32 || my >= lattice as i32 {
            continue;
        }
        grid[my as usize][mx as usize] = edge.dir.glyph();
    }
    grid.into_iter()
        .map(|row| {
            row.into_iter()
                .map(|g| g.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
