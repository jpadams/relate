//! Bolt 4.4 `patch_bolt` negotiation (ADR-0329).
//!
//! A HELLO may list protocol patches in its extra's `patch_bolt`. The
//! server applies the ones it knows, echoes exactly those in HELLO SUCCESS,
//! and the connection's [`BoltPatches`] then steers result encoding and
//! parameter decoding. `utc` is the only patch: DateTime values use the
//! Bolt 5.0 UTC structures (`0x49`/`0x69`) in both directions.
//!
//! This module is the negotiation vocabulary alone — patch names in, patch
//! names out — so the codec it steers (`packstream`, `param_structs`) can
//! import it without an edge back: the HELLO handler extracts the requested
//! names (`fields::patch_names`) and `summary::patch_bolt` wraps the
//! applied ones in Pack.

/// One protocol patch the server applies, named on the wire by
/// [`BoltPatch::name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoltPatch {
    /// DateTimeUTC `0x49` / DateTimeZoneIdUTC `0x69` replace the legacy
    /// `0x46` / `0x66` structures.
    Utc,
}

impl BoltPatch {
    /// This patch's wire name: the one wire vocabulary both the HELLO parse
    /// ([`BoltPatch::from_name`]) and the HELLO echo read.
    fn name(self) -> &'static str {
        match self {
            BoltPatch::Utc => "utc",
        }
    }

    /// The patch named `name` on the wire, or `None` for a name the server
    /// does not apply. With one patch this is the one comparison; a lookup
    /// table returns when a second patch exists.
    fn from_name(name: &str) -> Option<Self> {
        (name == Self::Utc.name()).then_some(Self::Utc)
    }
}

/// The patches one connection negotiated (ADR-0329). The default is none:
/// plain Bolt 4.4 encodings. One patch exists, so the applied set is at
/// most one patch — stored as the patch itself; a second patch turns this
/// into a real set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BoltPatches {
    applied: Option<BoltPatch>,
}

impl BoltPatches {
    /// The `utc` patch on: the shape a HELLO with `patch_bolt: ["utc"]`
    /// negotiates (tests build it directly).
    #[cfg(test)]
    pub(crate) fn utc() -> Self {
        Self {
            applied: Some(BoltPatch::Utc),
        }
    }

    /// The patches a HELLO's `patch_bolt` asks for, by the requested names
    /// (the HELLO handler reads the extra map): a name the server applies
    /// is set, every other name is ignored (ADR-0329).
    pub(crate) fn from_hello<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut patches = Self::default();
        for patch in names.into_iter().filter_map(BoltPatch::from_name) {
            patches.applied = Some(patch);
        }
        patches
    }

    /// The applied patches' wire names, in the enum's order — the elements
    /// of the HELLO SUCCESS `patch_bolt` list; `None` when no patch applies
    /// (the key is then absent). `summary::patch_bolt` wraps these in Pack.
    pub(crate) fn hello_metadata(self) -> Option<Vec<&'static str>> {
        self.applied.map(|patch| vec![patch.name()])
    }

    /// Whether `patch` was negotiated on this connection.
    pub(crate) fn applies(self, patch: BoltPatch) -> bool {
        self.applied == Some(patch)
    }
}

#[cfg(test)]
#[path = "patches_tests.rs"]
mod tests;
