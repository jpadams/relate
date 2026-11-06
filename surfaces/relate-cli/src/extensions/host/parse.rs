//! The `run <name>` argv grammar: parsing the action's tail from its
//! manifest flag table (ADR-0429 §5), the manifest well-formedness checks the
//! parse discovers, and the did-you-mean candidate list the verb renders.

use serde_json::{Map, Value};

use crate::extensions::manifest::{CommandManifest, FlagSpec};
use crate::extensions::registry::ExtensionEntry;
use crate::parse_error::{database_flag, flag_value, ParseError, REMOVED_DB_SPELLING};
use relate_wire::mcp::MCP_PROTOCOL_VERSION;

/// The host's own store-flag spellings, stated once and consulted at
/// every use site: the argv arms the host parses itself (never a
/// manifest flag) and the did-you-mean candidates (ADR-0383). The bare
/// names a manifest flag must not collide with are derived from this
/// table plus the removed `--db` spelling — never a second hand-written
/// list.
const HOST_STORE_FLAGS: &[&str] = &["-d", "--database"];

/// The bare flag names the host reserves: each accepted spelling's bare
/// form, plus the removed spelling's — a manifest flag of these names
/// would shadow the host's own store flag.
fn reserved_store_flag_names() -> Vec<&'static str> {
    HOST_STORE_FLAGS
        .iter()
        .map(|flag| flag.trim_start_matches('-'))
        .chain([REMOVED_DB_SPELLING.trim_start_matches('-')])
        .collect()
}

// ---------------------------------------------------------------------
// The argv parse (ADR-0429 §5)
// ---------------------------------------------------------------------

/// One parsed `run <name>` invocation.
///
/// [`Self::envelope`] is the child's whole input: its resolved flag values
/// — every flag's typed default when absent, its given value when present
/// — and its positional when one was given, keyed by the names the
/// manifest declares. It never carries the store path or any other host
/// flag: the host resolves `-d`/`--database` itself (ADR-0383), and the
/// child cannot even see the flag (ADR-0430 §6).
///
/// [`Self::db`] is the host-resolved store target: `None` is the
/// `:memory:` default whose run hands the graph to the interactive shell
/// (ADR-0286); an explicit target exits after the summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionRun {
    /// The child's request envelope, one JSON object.
    pub(crate) envelope: Map<String, Value>,
    /// The store target: `None` means `-d`/`--database` was omitted and
    /// the run uses `:memory:`.
    pub(crate) db: Option<String>,
}

/// Parse the argument tail after `run <name>` (ADR-0429 §5) against the
/// resolved action's manifest flag table: every flag parses as its kind
/// with its manifest default when absent, the manifest's optional
/// positional takes at most one value, and the host's own `-d`/
/// `--database` resolves the store target the child never sees. A flag
/// whose value does not parse as its kind is a usage error naming the
/// flag and the value; `--db` keeps its removed-spelling rejection; an
/// unknown flag is the unexpected-argument refusal in the action's scope
/// (`run <name>`).
///
/// The caller help-scans the tail first — the `-h`/`--help` pre-scan
/// every compiled-in command's tail gets (`flags::lifecycle::help_or`),
/// which prints the action's page and exits 0 — so this grammar never
/// sees a help token; handed one directly it refuses it like any
/// unknown flag.
///
/// The manifest problems the parse discovers are usage errors too
/// (ADR-0430 §5's manifest class): a declared protocol this host does not
/// speak, a flag table with duplicate names, a flag that spells the
/// host's own store flag, or a positional name colliding with a flag
/// name — refused here, before anything runs, so the verb's usage-error
/// path renders them exit 2.
pub(crate) fn parse_action_args(
    entry: &ExtensionEntry,
    rest: &[String],
) -> Result<ExtensionRun, ParseError> {
    let command = entry.command();
    let name = entry.mount_name();
    check_manifest(name, command)?;

    // The envelope starts as the flag table's resolved defaults; every
    // given occurrence overwrites (or appends, for a repeatable) its
    // flag's value.
    let mut envelope = default_envelope(command);
    let mut positional: Option<String> = None;
    let mut db: Option<String> = None;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // The host's own store flag: resolved here, never in the
            // envelope (ADR-0383, ADR-0430 §6).
            spelling if HOST_STORE_FLAGS.contains(&spelling) => {
                db = Some(database_flag(&mut args)?.clone());
            }
            // The removed spelling keeps its no-hint rejection (ADR-0383).
            spelling if spelling == REMOVED_DB_SPELLING => {
                return Err(ParseError::removed(&scope(name), spelling));
            }
            // A manifest flag: `--` plus the entry's name, its kind's
            // value grammar. Single-dash spellings are legacy forms this
            // grammar does not take (ADR-0069).
            other if other.starts_with('-') => {
                let Some(spec) = other
                    .strip_prefix("--")
                    .and_then(|bare| command.flags.iter().find(|spec| spec.name() == bare))
                else {
                    return Err(ParseError::unexpected(&scope(name), other));
                };
                parse_flag_occurrence(spec, &mut args, &mut envelope)?;
            }
            // The optional positional: at most one, and only when the
            // manifest declares one.
            other => match (&command.positional, positional.is_some()) {
                (Some(_), false) => positional = Some(other.to_string()),
                _ => return Err(ParseError::unexpected(&scope(name), other)),
            },
        }
    }
    if let (Some(declared), Some(value)) = (&command.positional, positional) {
        envelope.insert(declared.name.clone(), Value::String(value));
    }
    Ok(ExtensionRun { envelope, db })
}

/// The action's error scope: `run <name>`, composed from the manifest's
/// own mount name once.
fn scope(name: &str) -> String {
    format!("run {name}")
}

/// Every manifest problem the mount refuses before anything runs
/// (ADR-0430 §5's usage class): the declared protocol must be the MCP
/// revision this host serves, and the flag table must be a grammar the
/// host can parse. One helper for the two consumers — the `run` grammar
/// (which parses argv against the table) and the `extension check` verb
/// (which reports the same refusal as its first stage) — so a package the
/// check passes is a package `run` accepts.
pub(crate) fn check_manifest(name: &str, command: &CommandManifest) -> Result<(), ParseError> {
    check_protocol(name, command)?;
    check_grammar(name, command)
}

/// The request envelope's initial state: every flag's typed default
/// (ADR-0429 §5), keyed by the names the manifest declares. The one
/// construction — the `run` grammar starts its parse from it and the
/// `extension check` verb writes it to the child it spawns, since a check
/// passes no flags.
pub(crate) fn default_envelope(command: &CommandManifest) -> Map<String, Value> {
    let mut envelope = Map::new();
    for spec in &command.flags {
        envelope.insert(spec.name().to_string(), default_value(spec));
    }
    envelope
}

/// The manifest's declared protocol must be the MCP revision this host
/// serves (ADR-0447): the manifest declares, the host enforces — a manifest
/// naming another revision is a usage refusal naming both, never an
/// attempted run (the handshake would fail anyway; refusing before the
/// spawn is the exit-2 class ADR-0430 §5 gives manifest problems).
fn check_protocol(name: &str, command: &CommandManifest) -> Result<(), ParseError> {
    if command.protocol != MCP_PROTOCOL_VERSION {
        return Err(format!(
            "extension '{name}' declares protocol '{}'; this host speaks '{MCP_PROTOCOL_VERSION}'",
            command.protocol
        )
        .into());
    }
    Ok(())
}

/// The grammar's well-formedness as a grammar the host can parse
/// (ADR-0429 §5): no duplicate flag names — the first would silently
/// shadow the second — no name that spells the host's own store flag,
/// which the host resolves itself and whose removed `--db` spelling
/// nothing accepts (ADR-0383), and no positional name colliding with a
/// flag name — the envelope keys the positional and the flags by name,
/// so a shared name would shadow one with the other.
fn check_grammar(name: &str, command: &CommandManifest) -> Result<(), ParseError> {
    let mut seen: Vec<&str> = Vec::new();
    for spec in &command.flags {
        let flag_name = spec.name();
        if reserved_store_flag_names().contains(&flag_name) {
            return Err(format!(
                "extension '{name}' declares the flag '{flag_name}', which collides with the host's own store flag (-d/--database)"
            )
            .into());
        }
        if let Some(declared) = &command.positional {
            if declared.name == flag_name {
                return Err(format!(
                    "extension '{name}' declares '{}' twice (as its positional and as a flag)",
                    declared.name
                )
                .into());
            }
        }
        if seen.contains(&flag_name) {
            return Err(format!("extension '{name}' declares the flag '{flag_name}' twice").into());
        }
        seen.push(flag_name);
    }
    Ok(())
}

/// Parse one occurrence of a manifest flag against its kind (ADR-0429
/// §5): a string takes a value, an integer takes a number — a value that
/// does not parse as its kind is a usage error naming the flag and the
/// value — a boolean is bare, and a repeatable appends one value per
/// occurrence to its default-initialized list.
fn parse_flag_occurrence<'a>(
    spec: &FlagSpec,
    args: &mut impl Iterator<Item = &'a String>,
    envelope: &mut Map<String, Value>,
) -> Result<(), ParseError> {
    let flag = format!("--{}", spec.name());
    match spec {
        FlagSpec::String(declared) => {
            let value = flag_value(args, &flag, "a value")?;
            envelope.insert(declared.name.clone(), Value::String(value.clone()));
        }
        FlagSpec::Integer(declared) => {
            let value = flag_value(args, &flag, "a number")?;
            let parsed = value
                .parse::<i64>()
                .map_err(|_| format!("invalid {flag}: '{value}' is not an integer"))?;
            envelope.insert(declared.name.clone(), Value::from(parsed));
        }
        FlagSpec::Boolean(declared) => {
            envelope.insert(declared.name.clone(), Value::Bool(true));
        }
        FlagSpec::Repeatable(declared) => {
            let value = flag_value(args, &flag, "a value")?;
            // The envelope's slot holds this flag's default-initialized
            // list — the parse initialized every table entry's default —
            // so the append lands in the array the kind promised.
            let list = envelope
                .get_mut(&declared.name)
                .expect("the flag table's defaults initialized the envelope");
            match list {
                Value::Array(list) => list.push(Value::String(value.clone())),
                _ => unreachable!("a repeatable flag's default is an array"),
            }
        }
    }
    Ok(())
}

/// One flag-table entry's typed default as the envelope's JSON value —
/// the value the host resolves when the flag is absent (ADR-0429 §2).
fn default_value(spec: &FlagSpec) -> Value {
    match spec {
        FlagSpec::String(declared) => Value::String(declared.default.clone()),
        FlagSpec::Integer(declared) => Value::from(declared.default),
        FlagSpec::Boolean(declared) => Value::Bool(declared.default),
        FlagSpec::Repeatable(declared) => Value::Array(
            declared
                .default
                .iter()
                .map(|value| Value::String(value.clone()))
                .collect(),
        ),
    }
}

/// The flag vocabulary the action's grammar accepts, as the did-you-mean
/// candidate list (ADR-0192's derivation rule, ADR-0230's
/// derive-from-the-one-table principle): every manifest flag's `--`
/// spelling — a hidden extension's flags never reach it (ADR-0429 §2,
/// ADR-0279's visibility ported to `run`) — plus the host's own
/// `-d`/`--database`, which every action's grammar takes. The same list
/// the parser matches against; a suggestion can never name a flag the
/// invocation's grammar would reject.
///
/// Borrowed spellings, not `&'static` flag-table text: manifest flag
/// names are package data, so the verb renders a suggestion drawn from
/// here through the sanitizer like every other manifest-authored
/// fragment.
pub(crate) fn flag_candidates(entry: &ExtensionEntry) -> Vec<String> {
    let command = entry.command();
    let mut candidates = Vec::new();
    if !command.hidden {
        for spec in &command.flags {
            candidates.push(format!("--{}", spec.name()));
        }
    }
    candidates.extend(HOST_STORE_FLAGS.iter().map(|flag| flag.to_string()));
    candidates
}
