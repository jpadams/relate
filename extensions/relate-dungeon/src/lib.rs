//! The dungeon game as a library (LABS-884): the vocabulary and codecs
//! ([`data`]), the keep's content tables ([`keep`]), the pure explored-
//! map renderer ([`render`]), the verbs over the host-session seam
//! ([`game`], the typed row decoders in [`rows`]), and the command
//! dispatch ([`dispatch`]). The binary (`src/main.rs`) is the MCP child
//! the `relate run dungeon` host spawns; everything but the seam's
//! real implementation is host-free and unit-testable.

mod data;
mod dispatch;
mod game;
mod keep;
mod render;
mod rows;

/// The crate's one public item: the child's whole turn. The modules are
/// internal; their items are `pub(crate)`-shaped (a `pub` inside a
/// private module is not public API) until a second crate needs them.
pub use dispatch::run;

#[cfg(test)]
mod tests;
