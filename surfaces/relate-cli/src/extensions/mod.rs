//! The extension system: userland command-kind extensions that mount as
//! `relate run <name>` (ADR-0429) and speak MCP stdio to a host that
//! alone owns the store (ADR-0442, ADR-0447).
//!
//! The module tree lands in slices. `protocol` — the mount's wire:
//! spawning the child over std pipes, the JSON-RPC framing, the host-side
//! caps, and the runtime-failure exit mapping — the registry (`manifest`:
//! the `extension.json` model, strict and fail-closed; `registry`: the XDG
//! discovery walk, the load-time collision refusals, and the
//! installed-action enumeration) — `host` — the host that parses `run
//! <name>` argv from the manifest's flag table and drives the child's tool
//! calls against the host-owned store through the run lifecycle (ADR-0442
//! §3) — and `run` — the verb mount (the reserved word, the dispatch, the
//! `run --help` page, the pinned discovery error, the telemetry) — have
//! all landed. The registry's one source is the XDG walk: the first-party
//! importers are installed packages like any other. The distribution slice
//! (ADR-0431) lands here too: `provenance` (the install marker,
//! `extensions`-feature) and, behind `extension-install`, the signed
//! channel (`channel`), the closed-contract unpacker (`unpack`), and the
//! install flow (`install`, its typed failure in the `install::error` leaf
//! the channel and unpack peers import downward). The host's write layer
//! (`write`) serves the mount's `write_cypher` and `batch_cypher` calls
//! and nothing else.
//!
//! The tree's visibility policy: modules and items are `pub(crate)`, the
//! CLI binary is the only consumer.

/// The `extension check <dir>` diagnostic (`extensions::check`): the six
/// stages a package passes from its directory to the mount's tool table,
/// over a hostless session that opens no store — the author's pre-install
/// gate, sharing the registry's loader, the `run` grammar's manifest
/// checks, the mount's handshake facts and the wire kit's tool table.
pub(crate) mod check;
pub(crate) mod host;
/// The canonical bash MCP client kit a bash package inlines (ADR-0447 §4):
/// one reader of `docs/api/extensions/examples/mcp-client.sh`, shared by the
/// `extension init` scaffold and the fixture kit.
pub(crate) mod kit;
pub(crate) mod manifest;
pub(crate) mod mcp;
pub(crate) mod protocol;
pub(crate) mod provenance;
pub(crate) mod registry;
pub(crate) mod run;
pub(crate) mod source;
/// The shared fixture kit (a package installed under a tempdir XDG, its
/// manifest text, its POSIX `sh` child), used by the host and run-mount
/// suites — one kit, not a copy per test module.
#[cfg(test)]
pub(crate) mod testkit;
/// The closed contract's test-only tarball builder (§2's encoder),
/// shared by the unpacker's own tests and the install flow's
/// fixture-index driver — one builder beside the one reader.
#[cfg(all(test, feature = "extension-install"))]
pub(crate) mod testtar;
/// The per-user trust store (ADR-0449): the recorded GitHub pins a
/// `--github` reinstall is compared against. Dependency-free, so
/// `list --trust` and `remove --forget-pin` carry it in every
/// `extensions` build.
pub(crate) mod trust;
/// The host's write layer (ADR-0442 §3): the commit policies and the
/// freshness read the mount's writing calls run through, framed by the
/// manifest's declared policy.
pub(crate) mod write;
// The distribution half (ADR-0431): the network fetch, the signature and
// checksum verification, and the tarball unpacker gate behind
// `extension-install` — network dependencies gate the install verbs only
// (§3); `list` and `remove` stay dependency-free.
#[cfg(feature = "extension-install")]
pub(crate) mod channel;
#[cfg(feature = "extension-install")]
pub(crate) mod github;
#[cfg(feature = "extension-install")]
pub(crate) mod install;
#[cfg(feature = "extension-install")]
pub(crate) mod unpack;

/// Whether `name` is a package name (ADR-0429 §2, ADR-0431 §3): a single
/// directory segment — non-empty, not `.` or `..`, no path separator, no
/// NUL.
///
/// The one predicate: the install flow's pre-flight, the registry's walk
/// and `extension init`'s scaffold all consult it, so `init` can never
/// create a directory the installer would then refuse. Deliberately the
/// *looser* rule — it is the one the install contract already states, and a
/// stricter scaffold-only rule (ASCII-only, say) would refuse names the
/// installer and the registry happily accept.
pub(crate) fn is_package_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

/// Whether a package-declared path escapes the package directory
/// (ADR-0429 §2): an absolute path, a rooted or prefixed path, or any
/// parent-directory component. Only a relative path that stays under the
/// package dir resolves.
///
/// The one canonical package-dir containment predicate, consulted at
/// every site the rule applies: the registry's load-time check of a
/// manifest's executable (ADR-0429 §2) and the installer's unpack check
/// of every tarball member name (ADR-0431 §2 — the rule extended to
/// every member). Consumer-neutral by design — the paths it judges are
/// executables, tarball members, whatever a package declares — so it
/// lives at the extensions root, not in one consumer's module.
///
/// A lexical, load-time check of the declared path — deliberately not
/// a runtime containment check: such a policy canonicalizes an
/// existing root for file IO (symlink-aware, the file must exist);
/// this one validates a manifest declaration before anything exists
/// to canonicalize. The run mount's spawn is where existence checks
/// belong.
pub(crate) fn escapes_package_dir(path: &std::path::Path) -> bool {
    path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
            )
        })
}
