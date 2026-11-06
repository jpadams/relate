//! The command dispatch — the one placement for the missing-argument
//! policy: the turns that answer a missing argument are decided here,
//! and the verb methods take only real values.

use crate::data::Direction;
use crate::game::{Game, HostSession};

/// First word and the rest of the line — the argv split the
/// conversational verbs share.
fn head_tail<'a>(args: &[&'a str]) -> Option<(&'a str, String)> {
    args.split_first()
        .map(|(head, tail)| (*head, tail.join(" ")))
}

/// The one parser for the prepositional forms — `<target> <prep>
/// <payload>` (`give key to dragon`, `ask bard for a song`): the
/// preposition must sit second. A present preposition with an empty
/// payload is its own answer, distinct from "not that form" — folding
/// it into the fallback would parse `give key to` as `give key to`
/// (npc "to").
enum PrepForm<'a> {
    /// The form, with target and non-empty payload.
    Form(&'a str, String),
    /// The preposition sits second but its payload is empty.
    EmptyPayload,
    /// Not the form at all.
    Absent,
}

fn prepositional<'a>(args: &[&'a str], prep: &str) -> PrepForm<'a> {
    let mut it = args.iter();
    let Some(target) = it.next() else {
        return PrepForm::Absent;
    };
    let Some(second) = it.next() else {
        return PrepForm::Absent;
    };
    if !second.eq_ignore_ascii_case(prep) {
        return PrepForm::Absent;
    }
    let payload = args[2..].join(" ");
    if payload.is_empty() {
        PrepForm::EmptyPayload
    } else {
        PrepForm::Form(target, payload)
    }
}

/// The not-a-compass answer, the one spelling both move forms use.
fn not_a_compass(word: &str) -> String {
    format!(
        "'{word}' is not a compass direction. Try one of: {}",
        Direction::ALL
            .iter()
            .map(|d| d.short())
            .collect::<Vec<_>>()
            .join(" ")
    )
}

/// Dispatch one command string to one verb — the child's whole turn.
/// The missing-argument turns are decided here (one message policy for
/// the verb family); the verbs take only real values.
pub fn run(session: &mut impl HostSession, command: &str) -> Result<(), String> {
    let mut game = Game::new(session);
    let mut words = command.split_whitespace();
    let Some(first) = words.next() else {
        return game.status();
    };
    let verb = first.to_ascii_lowercase();
    let args: Vec<&str> = words.collect();
    let rest = args.join(" ");
    // One direction site: the verb itself may be the compass word
    // (`n`, `nw`, `north` …); `move <word>` names it explicitly, with
    // the one not-a-compass answer.
    if let Some(dir) = Direction::parse(&verb) {
        return game.mv(dir);
    }
    match verb.as_str() {
        "start" => game.start(),
        "play" => game.play(),
        "seed" => game.seed(),
        "status" => game.status(),
        "map" => game.map(),
        "move" => {
            // The missing-argument turn is this arm's own, never a
            // quoted empty word in the compass error.
            let Some(word) = args.first().copied() else {
                return game.report("Move where?");
            };
            match Direction::parse(word) {
                Some(dir) => game.mv(dir),
                None => game.report(&not_a_compass(word)),
            }
        }
        "take" | "get" if !rest.is_empty() => game.take(&rest),
        "take" | "get" => game.report("Take what?"),
        "drop" if !rest.is_empty() => game.drop_item(&rest),
        "drop" => game.report("Drop what?"),
        "look" | "l" | "examine" | "x" => game.look_at(&rest),
        "ask" => {
            // ask <npc> <question> | ask <npc> for <item>
            match head_tail(&args) {
                None => game.report("Ask whom?"),
                Some((npc, text)) => match prepositional(&args, "for") {
                    PrepForm::Form(_, item) => game.ask_for(npc, &item),
                    PrepForm::EmptyPayload => game.report("Ask for what?"),
                    PrepForm::Absent => game.say(npc, &text),
                },
            }
        }
        "say" | "tell" => match head_tail(&args) {
            None => game.report("Say to whom?"),
            Some((npc, text)) => game.say(npc, &text),
        },
        "give" => {
            // give <npc> <item> | give <item> to <npc> — the `to` form
            // swaps the same way `ask`'s `for` form redirects.
            match prepositional(&args, "to") {
                PrepForm::Form(item, npc) => game.give(&npc, item),
                PrepForm::EmptyPayload => game.report("Give to whom?"),
                PrepForm::Absent => match head_tail(&args) {
                    Some((npc, item)) => game.give(npc, &item),
                    None => game.report("Give what to whom?"),
                },
            }
        }
        "inv" | "inventory" | "i" => game.inventory(),
        "help" => game.help(),
        _ => game.report(&format!("'{command}' is not a dungeon verb — try `help`.")),
    }
}
