//! The durability-mode vocabulary (ADR-0077, ADR-0470) as the one test
//! table: every [`relate::DurabilityMode`] with the spelling the CLI
//! flag, the Bolt flag and `RELATE_DURABILITY_MODE` all take. The
//! engine crate exposes no public variant list, so the suites that pin
//! the mode's wiring (the engine's setting parser, the CLI's serve
//! parse and store options, the Bolt flag, the CLI's e2e durability
//! suites) iterate this table: a mode added to the engine lands here
//! once and every pin picks it up.

use relate::DurabilityMode;

/// Every durability mode with its canonical spelling.
pub const DURABILITY_MODES: &[(&str, DurabilityMode)] = &[
    ("full", DurabilityMode::Full),
    ("everysecond", DurabilityMode::EverySecond),
    ("snapshot", DurabilityMode::Snapshot),
];
